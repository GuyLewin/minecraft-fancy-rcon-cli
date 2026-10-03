//! Completion, hints and highlighting, driven by walking the command tree
//! along the typed line.

use crate::tree::{Kind, Node, Tree};
use std::collections::HashSet;

/// Lists only the running server knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveList {
    Players,
    Whitelist,
    Objectives,
    Teams,
}

pub trait Live {
    /// With `fetch` unset only what is already known is returned, for callers
    /// that can't afford a round trip to the server.
    fn list(&self, what: LiveList, fetch: bool) -> Vec<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Byte offset of the text the candidate replaces, up to the cursor.
    pub start: usize,
    pub text: String,
    /// Whether more arguments follow, so a space can be appended.
    pub space: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct HintText {
    pub text: String,
    /// Whether the hint is the rest of the word, rather than a description of
    /// what comes next.
    pub completes: bool,
}

const GREEN_BOLD: &str = "\x1b[1;32m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

const MAX_STATES: usize = 256;
const MAX_USAGE_OPTIONS: usize = 6;
const MAX_USAGE_DEPTH: usize = 6;

const SELECTORS: &[&str] = &["@a", "@e", "@n", "@p", "@r", "@s"];
const PLAYER_SELECTORS: &[&str] = &["@a", "@p", "@r", "@s"];
const DIMENSIONS: &[&str] = &["overworld", "the_end", "the_nether"];

enum Shape {
    /// Space-separated words, e.g. three for a position.
    Words(usize),
    /// Everything up to the end of the line.
    Greedy,
}

enum Consumed {
    /// The argument spans this many bytes and a space follows it.
    Complete(usize),
    /// The line ends inside the argument, in its `index`th word.
    Incomplete {
        word_start: usize,
        index: usize,
    },
    Mismatch,
}

#[derive(Clone, Copy)]
struct Token {
    start: usize,
    end: usize,
    literal: bool,
}

/// A way of reading the line: `tokens` lead to `node`, and whatever starts at
/// `pos` is still being typed.
#[derive(Clone)]
struct State<'a> {
    node: &'a Node,
    pos: usize,
    tokens: Vec<Token>,
    /// Whether what is being typed can still become something valid.
    open: bool,
}

pub struct Engine {
    pub tree: Tree,
}

impl Engine {
    pub fn candidates(&self, input: &str, live: &dyn Live, fetch: bool) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        for state in self.analyze(input) {
            let rest = &input[state.pos..];
            for (name, child) in self.tree.children(state.node) {
                let space = !self.tree.children(child).is_empty();
                if child.kind == Kind::Literal {
                    if name.starts_with(rest) && !is_hidden(name, rest) {
                        candidates.push(Candidate {
                            start: state.pos,
                            text: name.clone(),
                            space,
                        });
                    }
                    continue;
                }
                let Consumed::Incomplete { word_start, index } = consume(child, name, rest) else {
                    continue;
                };
                let word = &rest[word_start..];
                let last_word = matches!(shape(child), Shape::Words(n) if index + 1 == n);
                for text in self.suggestions(child, name, index, word, live, fetch) {
                    candidates.push(Candidate {
                        start: state.pos + word_start,
                        // Suggestions spanning several words finish the argument.
                        space: if last_word || text.contains(' ') {
                            space
                        } else {
                            true
                        },
                        text,
                    });
                }
            }
        }

        // Readings can disagree on where the current word starts, but the
        // line editor wants a single position.
        if let Some(start) = candidates.iter().map(|c| c.start).min() {
            for candidate in &mut candidates {
                candidate.text.insert_str(0, &input[start..candidate.start]);
                candidate.start = start;
            }
        }
        candidates.sort_by(|a, b| a.text.cmp(&b.text));
        candidates.dedup_by(|a, b| a.text == b.text);
        candidates
    }

    /// What to show after the cursor when it is at the end of `input`.
    pub fn hint(&self, input: &str, live: &dyn Live) -> Option<HintText> {
        let typed_start = input.rfind(' ').map_or(0, |i| i + 1);
        if input[typed_start..].trim_start_matches('/').is_empty() {
            return self.usage_hint(input);
        }
        let candidates = self.candidates(input, live, false);
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        let rest = candidate.text.strip_prefix(&input[candidate.start..])?;
        (!rest.is_empty()).then(|| HintText {
            text: rest.to_string(),
            completes: true,
        })
    }

    fn usage_hint(&self, input: &str) -> Option<HintText> {
        let states: Vec<_> = self
            .analyze(input)
            .into_iter()
            .filter(|s| s.open && !s.tokens.is_empty())
            .collect();
        let text = match states.as_slice() {
            [] => return None,
            [state] if state.pos == input.len() => self.usage(state.node, MAX_USAGE_DEPTH),
            _ => {
                let mut options = Vec::new();
                for state in &states {
                    let rest = &input[state.pos..];
                    for (name, child) in self.tree.children(state.node) {
                        // Only between words, not in the middle of a
                        // message or a selector.
                        if matches!(consume(child, name, rest), Consumed::Incomplete { word_start, .. } if word_start == rest.len())
                        {
                            options.push(label(name, child));
                        }
                    }
                }
                options.sort();
                options.dedup();
                join_options(options, states.iter().any(|s| s.node.executable))
            }
        };
        (!text.is_empty()).then_some(HintText {
            text,
            completes: false,
        })
    }

    /// Describes what follows `node`, e.g. `<targets> <item> [<count>]`.
    fn usage(&self, node: &Node, depth: usize) -> String {
        let children = self.tree.children(node);
        let mut options: Vec<String> = children
            .iter()
            .filter(|(n, _)| !is_hidden(n, ""))
            .map(|(n, c)| label(n, c))
            .collect();
        if let ([option], Some(child), true) =
            (options.as_mut_slice(), children.values().next(), depth > 0)
        {
            let rest = self.usage(child, depth - 1);
            if !rest.is_empty() {
                option.push(' ');
                option.push_str(&rest);
            }
        }
        join_options(options, node.executable)
    }

    /// Colors the command, its subcommands and whatever can't be valid.
    pub fn highlight(&self, line: &str) -> String {
        let states = self.analyze(line);
        let furthest = |open: bool| {
            states
                .iter()
                .filter(|s| s.open == open)
                .max_by_key(|s| s.pos)
        };
        let Some(state) = furthest(true).or_else(|| furthest(false)) else {
            return line.to_string();
        };

        let mut out = String::with_capacity(line.len() + 32);
        let mut pos = 0;
        let mut paint = |out: &mut String, start: usize, end: usize, color: &str| {
            out.push_str(&line[pos..start]);
            out.push_str(color);
            out.push_str(&line[start..end]);
            out.push_str(RESET);
            pos = end;
        };
        for (i, token) in state.tokens.iter().enumerate() {
            if token.literal {
                let color = if i == 0 { GREEN_BOLD } else { CYAN };
                paint(&mut out, token.start, token.end, color);
            }
        }
        let rest = &line[state.pos..];
        let children = self.tree.children(state.node);
        if !state.open {
            paint(&mut out, state.pos, line.len(), RED);
        } else if children.get(rest).is_some_and(|c| c.kind == Kind::Literal) {
            let color = if state.tokens.is_empty() {
                GREEN_BOLD
            } else {
                CYAN
            };
            paint(&mut out, state.pos, line.len(), color);
        }
        out.push_str(&line[pos..]);
        out
    }

    /// Finds every way to read `input` as a command, up to the part that is
    /// still being typed.
    fn analyze<'a>(&'a self, input: &str) -> Vec<State<'a>> {
        let start = usize::from(input.starts_with('/'));
        let mut queue = vec![State {
            node: &self.tree.root,
            pos: start,
            tokens: Vec::new(),
            open: false,
        }];
        let mut seen = HashSet::new();
        let mut frontier = Vec::new();

        while let Some(mut state) = queue.pop() {
            let rest = &input[state.pos..];
            let mut advances = Vec::new();
            let mut matched_literal = false;
            for (name, child) in self.tree.children(state.node) {
                match consume(child, name, rest) {
                    Consumed::Complete(len) => {
                        matched_literal |= child.kind == Kind::Literal;
                        advances.push((child, len));
                    }
                    Consumed::Incomplete { .. } => state.open = true,
                    Consumed::Mismatch => {}
                }
            }
            // Like the game's parser, don't consider arguments where a
            // literal fits.
            if matched_literal {
                advances.retain(|(child, _)| child.kind == Kind::Literal);
            } else if state.open || advances.is_empty() {
                // Nothing left to type after a complete command is fine too.
                state.open |= rest.is_empty() && state.node.executable;
                frontier.push(state.clone());
            }
            for (child, len) in advances {
                let pos = state.pos + len + 1;
                if seen.len() < MAX_STATES && seen.insert((child as *const Node, pos)) {
                    let mut tokens = state.tokens.clone();
                    tokens.push(Token {
                        start: state.pos,
                        end: state.pos + len,
                        literal: child.kind == Kind::Literal,
                    });
                    queue.push(State {
                        node: child,
                        pos,
                        tokens,
                        open: false,
                    });
                }
            }
        }
        frontier
    }

    fn suggestions(
        &self,
        node: &Node,
        name: &str,
        index: usize,
        word: &str,
        live: &dyn Live,
        fetch: bool,
    ) -> Vec<String> {
        let parser = node.parser.as_deref().unwrap_or("");
        let prefixed = |options: &[&str]| -> Vec<String> {
            options
                .iter()
                .filter(|o| o.starts_with(word))
                .map(|o| o.to_string())
                .collect()
        };
        let named = |lists: &[LiveList]| -> Vec<String> {
            let lower = word.to_lowercase();
            lists
                .iter()
                .flat_map(|list| live.list(*list, fetch))
                .filter(|n| n.to_lowercase().starts_with(&lower))
                .collect()
        };

        match parser {
            "minecraft:vec3"
            | "minecraft:block_pos"
            | "minecraft:vec2"
            | "minecraft:column_pos"
            | "minecraft:rotation" => {
                let Shape::Words(count) = shape(node) else {
                    unreachable!()
                };
                match (word.is_empty(), index) {
                    (true, 0) => vec![vec!["~"; count].join(" ")],
                    (true, _) => vec!["~".to_string()],
                    _ => Vec::new(),
                }
            }
            "minecraft:entity" | "minecraft:score_holder" | "minecraft:game_profile" => {
                if word.contains('[') {
                    return Vec::new();
                }
                let players_only = node.properties.kind.as_deref() == Some("players")
                    || parser == "minecraft:game_profile";
                let mut names = if parser == "minecraft:game_profile" {
                    named(&[LiveList::Players, LiveList::Whitelist])
                } else {
                    named(&[LiveList::Players])
                };
                names.extend(prefixed(if players_only {
                    PLAYER_SELECTORS
                } else {
                    SELECTORS
                }));
                if parser == "minecraft:score_holder" {
                    names.extend(prefixed(&["*"]));
                }
                names
            }
            "minecraft:objective" => named(&[LiveList::Objectives]),
            "minecraft:team" => named(&[LiveList::Teams]),
            _ => match fixed_options(parser) {
                Some((options, _)) => prefixed(options),
                None => self.registry_suggestions(node, name, word),
            },
        }
    }

    fn registry_suggestions(&self, node: &Node, name: &str, word: &str) -> Vec<String> {
        let parser = node.parser.as_deref().unwrap_or("");
        let registry = match (&node.properties.registry, parser) {
            (Some(registry), _) => registry.trim_start_matches("minecraft:"),
            (_, "minecraft:item_stack" | "minecraft:item_predicate") => "item",
            (_, "minecraft:block_state" | "minecraft:block_predicate") => "block",
            (_, "minecraft:particle") => "particle_type",
            (_, "minecraft:loot_table") => "loot_table",
            (_, "minecraft:dialog") => "dialog",
            (_, "minecraft:dimension") => "dimension",
            // Where configured features exist, `worldgen/feature` holds
            // feature types instead.
            (_, "minecraft:feature")
                if self
                    .tree
                    .registries
                    .contains_key("worldgen/configured_feature") =>
            {
                "worldgen/configured_feature"
            }
            (_, "minecraft:feature") => "worldgen/feature",
            // Parsers of older versions.
            (_, "minecraft:entity_summon") => "entity_type",
            (_, "minecraft:mob_effect") => "mob_effect",
            (_, "minecraft:item_enchantment") => "enchantment",
            (_, "minecraft:resource_location") if name == "sound" => "sound_event",
            _ => return Vec::new(),
        };
        // Tags, components and NBT are beyond what this can complete.
        if word.contains(['#', '[', '{']) {
            return Vec::new();
        }
        let (namespace, path) = match word.strip_prefix("minecraft:") {
            Some(path) => ("minecraft:", path),
            None => ("", word),
        };
        let complete = |entries: &mut dyn Iterator<Item = &str>| {
            entries
                .filter(|e| e.starts_with(path))
                .map(|e| format!("{namespace}{e}"))
                .collect()
        };
        match self.tree.registries.get(registry) {
            Some(entries) => complete(&mut entries.iter().map(String::as_str)),
            None if registry == "dimension" => complete(&mut DIMENSIONS.iter().copied()),
            None => Vec::new(),
        }
    }
}

/// Game rules exist as both `pvp` and `minecraft:pvp`; the long forms are
/// only worth listing once the namespace is being typed.
fn is_hidden(literal: &str, typed: &str) -> bool {
    const NAMESPACE: &str = "minecraft:";
    literal.starts_with(NAMESPACE)
        && !(typed.starts_with(NAMESPACE) || (typed.len() > 1 && NAMESPACE.starts_with(typed)))
}

fn label(name: &str, node: &Node) -> String {
    match node.kind {
        Kind::Argument => format!("<{name}>"),
        _ => name.to_string(),
    }
}

fn join_options(mut options: Vec<String>, optional: bool) -> String {
    if options.len() > MAX_USAGE_OPTIONS {
        options.truncate(MAX_USAGE_OPTIONS);
        options.push("…".to_string());
    }
    match (options.len(), optional) {
        (0, _) => String::new(),
        (_, true) => format!("[{}]", options.join("|")),
        (1, false) => options.remove(0),
        (_, false) => format!("({})", options.join("|")),
    }
}

fn shape(node: &Node) -> Shape {
    match node.parser.as_deref().unwrap_or("") {
        "minecraft:vec3" | "minecraft:block_pos" => Shape::Words(3),
        "minecraft:vec2" | "minecraft:column_pos" | "minecraft:rotation" => Shape::Words(2),
        "minecraft:message" => Shape::Greedy,
        "brigadier:string" if node.properties.kind.as_deref() == Some("greedy") => Shape::Greedy,
        _ => Shape::Words(1),
    }
}

/// The values a parser accepts, and whether it accepts nothing else.
fn fixed_options(parser: &str) -> Option<(&'static [&'static str], bool)> {
    Some(match parser {
        "brigadier:bool" => (&["false", "true"], true),
        "minecraft:gamemode" => (&["adventure", "creative", "spectator", "survival"], true),
        "minecraft:entity_anchor" => (&["eyes", "feet"], true),
        "minecraft:template_mirror" => (&["front_back", "left_right", "none"], true),
        "minecraft:template_rotation" => (
            &["180", "clockwise_90", "counterclockwise_90", "none"],
            true,
        ),
        "minecraft:heightmap" => (
            &[
                "motion_blocking",
                "motion_blocking_no_leaves",
                "ocean_floor",
                "world_surface",
            ],
            true,
        ),
        "minecraft:team_color" | "minecraft:color" => (
            &[
                "aqua",
                "black",
                "blue",
                "dark_aqua",
                "dark_blue",
                "dark_gray",
                "dark_green",
                "dark_purple",
                "dark_red",
                "gold",
                "gray",
                "green",
                "light_purple",
                "red",
                "reset",
                "white",
                "yellow",
            ],
            true,
        ),
        "minecraft:swizzle" => (&["x", "xy", "xyz", "xz", "y", "yz", "z"], false),
        "minecraft:operation" => (&["%=", "*=", "+=", "-=", "/=", "<", "=", ">", "><"], false),
        "minecraft:scoreboard_slot" => (&["below_name", "list", "sidebar"], false),
        "minecraft:objective_criteria" => (
            &[
                "air",
                "armor",
                "deathCount",
                "dummy",
                "food",
                "health",
                "level",
                "playerKillCount",
                "totalKillCount",
                "trigger",
                "xp",
            ],
            false,
        ),
        _ => return None,
    })
}

/// Tries to read `node` off the start of `rest`.
fn consume(node: &Node, name: &str, rest: &str) -> Consumed {
    if node.kind == Kind::Literal {
        return match rest.strip_prefix(name) {
            Some(after) if after.starts_with(' ') => Consumed::Complete(name.len()),
            _ if name.starts_with(rest) => Consumed::Incomplete {
                word_start: 0,
                index: 0,
            },
            _ => Consumed::Mismatch,
        };
    }
    let Shape::Words(count) = shape(node) else {
        return Consumed::Incomplete {
            word_start: 0,
            index: 0,
        };
    };
    let parser = node.parser.as_deref().unwrap_or("");
    let mut pos = 0;
    for index in 0..count {
        let Some(len) = word_end(&rest[pos..]) else {
            return if valid_word(parser, &rest[pos..], true) {
                Consumed::Incomplete {
                    word_start: pos,
                    index,
                }
            } else {
                Consumed::Mismatch
            };
        };
        if len == 0 || !valid_word(parser, &rest[pos..pos + len], false) {
            return Consumed::Mismatch;
        }
        pos += len;
        if index + 1 < count {
            pos += 1;
        }
    }
    Consumed::Complete(pos)
}

/// Finds the space ending the word at the start of `s`, skipping the spaces
/// inside quotes and brackets, as in `@e[nbt={a: "b c"}]`.
fn word_end(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => quote = Some(c),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// A cheap plausibility check, to tell apart alternatives such as the
/// position and the entity of `/teleport`. `partial` words may still grow.
fn valid_word(parser: &str, word: &str, partial: bool) -> bool {
    let number = |word: &str, decimal: bool| {
        let digits = word.strip_prefix('-').unwrap_or(word);
        digits
            .chars()
            .all(|c| c.is_ascii_digit() || (decimal && c == '.'))
            && (partial || digits.chars().any(|c| c.is_ascii_digit()))
    };
    match parser {
        "brigadier:integer" | "brigadier:long" => number(word, false),
        "brigadier:float" | "brigadier:double" => number(word, true),
        "minecraft:time" => {
            let value = word.strip_suffix(['d', 's', 't']).unwrap_or(word);
            number(value, true)
        }
        "minecraft:vec3"
        | "minecraft:block_pos"
        | "minecraft:vec2"
        | "minecraft:column_pos"
        | "minecraft:rotation" => {
            let relative = word.strip_prefix(['~', '^']);
            // `~` on its own is a complete coordinate.
            relative == Some("") || number(relative.unwrap_or(word), true)
        }
        _ => match fixed_options(parser) {
            Some((options, true)) if partial => options.iter().any(|o| o.starts_with(word)),
            Some((options, true)) => options.contains(&word),
            _ => true,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Live for Fake {
        fn list(&self, what: LiveList, _fetch: bool) -> Vec<String> {
            match what {
                LiveList::Players => vec!["GuyLewin".to_string(), "Steve".to_string()],
                LiveList::Whitelist => vec!["Alex".to_string()],
                LiveList::Objectives => vec!["deaths".to_string()],
                LiveList::Teams => vec!["red".to_string()],
            }
        }
    }

    fn engine() -> Engine {
        Engine {
            tree: Tree::embedded(),
        }
    }

    fn complete(input: &str) -> Vec<String> {
        engine()
            .candidates(input, &Fake, true)
            .into_iter()
            .map(|c| c.text)
            .collect()
    }

    fn hint(input: &str) -> Option<String> {
        engine().hint(input, &Fake).map(|h| h.text)
    }

    #[test]
    fn completes_command_names_with_and_without_slash() {
        assert_eq!(complete("/telep"), ["teleport"]);
        assert_eq!(complete("telep"), ["teleport"]);
        assert_eq!(
            complete("/te"),
            ["team", "teammsg", "teleport", "tell", "tellraw", "test"]
        );
        assert!(complete("/save-").contains(&"save-all".to_string()));
        assert!(complete("").len() > 80);
    }

    #[test]
    fn completes_teleport_arguments() {
        assert_eq!(
            complete("/teleport "),
            ["@a", "@e", "@n", "@p", "@r", "@s", "GuyLewin", "Steve", "~ ~ ~"]
        );
        assert_eq!(complete("/teleport g"), ["GuyLewin"]);
        assert_eq!(complete("/tp GuyLewin S"), ["Steve"]);
        assert_eq!(complete("/tp 1 2 "), ["~"]);
        assert_eq!(complete("/tp GuyLewin 1 2 3 "), ["facing", "~ ~"]);
        assert_eq!(complete("/tp @e[type=cow, limit=1] St"), ["Steve"]);
    }

    #[test]
    fn completes_literals_enums_and_registries() {
        assert_eq!(complete("/gamemode s"), ["spectator", "survival"]);
        assert_eq!(complete("/gamemode creative G"), ["GuyLewin"]);
        assert_eq!(complete("/gamerule keep"), ["keep_inventory"]);
        assert_eq!(complete("/gamerule keep_inventory t"), ["true"]);
        assert_eq!(
            complete("/gamerule spawn_w"),
            ["spawn_wandering_traders", "spawn_wardens"]
        );
        assert_eq!(complete("/gamerule minecraft:pv"), ["minecraft:pvp"]);
        assert_eq!(complete("/weather th"), ["thunder"]);
        assert_eq!(complete("/give @s diamond_sw"), ["diamond_sword"]);
        assert_eq!(
            complete("/give @s minecraft:diamond_sw"),
            ["minecraft:diamond_sword"]
        );
        assert_eq!(complete("/summon ender_d"), ["ender_dragon"]);
        assert_eq!(complete("/effect give @s night"), ["night_vision"]);
        assert_eq!(complete("/locate biome cherry"), ["cherry_grove"]);
        assert_eq!(complete("/execute in the_n"), ["the_nether"]);
        assert_eq!(complete("/whitelist remove A"), ["Alex"]);
        assert_eq!(complete("/scoreboard players set @s d"), ["deaths"]);
    }

    #[test]
    fn follows_redirects() {
        assert_eq!(
            complete("/execute as @a at @s run gamemode c"),
            ["creative"]
        );
        assert_eq!(complete("/xp ad"), ["add"]);
    }

    #[test]
    fn appends_space_only_when_more_follows() {
        let engine = engine();
        let space = |input: &str| engine.candidates(input, &Fake, true)[0].space;
        assert!(space("/telep"));
        assert!(!space("/reloa"));
        assert!(space("/gamemode creat"));
        assert!(!space("/gamemode creative Guy"));
        assert!(!space("/list uu"));
    }

    #[test]
    fn hints_rest_of_word_or_usage() {
        assert_eq!(hint("/telep").as_deref(), Some("ort"));
        assert_eq!(hint("/te"), None);
        assert_eq!(hint(""), None);
        assert_eq!(hint("/"), None);
        assert_eq!(
            hint("/give ").as_deref(),
            Some("<targets> <item> [<count>]")
        );
        assert_eq!(hint("/gamemode ").as_deref(), Some("<gamemode> [<target>]"));
        assert_eq!(hint("/gamemode creative ").as_deref(), Some("[<target>]"));
        assert_eq!(
            hint("/teleport ").as_deref(),
            Some("(<destination>|<location>|<targets>)")
        );
        assert_eq!(hint("/weather ").as_deref(), Some("(clear|rain|thunder)"));
        assert_eq!(hint("/list ").as_deref(), Some("[uuids]"));
        assert_eq!(hint("/seed "), None);
        assert_eq!(hint("/tp 1 2 ").as_deref(), Some("[<location>]"));
        assert_eq!(hint("/say hello "), None);
        assert_eq!(hint("/say ").as_deref(), Some("<message>"));
        assert_eq!(hint("/tp @e[type=cow, "), None);
    }

    #[test]
    fn highlights_commands_literals_and_errors() {
        let engine = engine();
        assert_eq!(engine.highlight("/telepo"), "/telepo");
        assert_eq!(
            engine.highlight("/list"),
            format!("/{GREEN_BOLD}list{RESET}")
        );
        assert_eq!(
            engine.highlight("weather clear"),
            format!("{GREEN_BOLD}weather{RESET} {CYAN}clear{RESET}")
        );
        assert_eq!(
            engine.highlight("/tp 1 2 3"),
            format!("/{GREEN_BOLD}tp{RESET} 1 2 3")
        );
        assert_eq!(engine.highlight("/nope x"), format!("/{RED}nope x{RESET}"));
        assert_eq!(
            engine.highlight("/gamemode foo"),
            format!("/{GREEN_BOLD}gamemode{RESET} {RED}foo{RESET}")
        );
        assert_eq!(
            engine.highlight("/seed extra"),
            format!("/{GREEN_BOLD}seed{RESET} {RED}extra{RESET}")
        );
        assert_eq!(
            engine.highlight("/say hello there"),
            format!("/{GREEN_BOLD}say{RESET} hello there")
        );
    }

    #[test]
    fn finds_word_ends() {
        assert_eq!(word_end("abc def"), Some(3));
        assert_eq!(word_end("abc"), None);
        assert_eq!(word_end("@e[a=b, c=d] x"), Some(12));
        assert_eq!(word_end(r#""a \" b" x"#), Some(8));
        assert_eq!(word_end("{a: 'b c'"), None);
    }
}
