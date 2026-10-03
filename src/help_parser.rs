use crate::tree::{Kind, Node};
use std::collections::BTreeMap;

/// One line of `/help`: `/name usage...` or `/alias -> target`.
#[derive(Debug, PartialEq, Eq)]
pub struct HelpEntry {
    pub name: String,
    pub usage: String,
    pub alias_of: Option<String>,
}

/// RCON strips the line breaks between help entries; put them back.
pub fn format_help_response(body: &str) -> String {
    let mut fixed = String::with_capacity(body.len());
    for (i, c) in body.char_indices() {
        if c == '/' && i > 0 && !fixed.ends_with('\n') {
            fixed.push('\n');
        }
        fixed.push(c);
    }
    fixed.trim().to_string()
}

pub fn parse_help(body: &str) -> Vec<HelpEntry> {
    format_help_response(body)
        .lines()
        .filter_map(|line| {
            let line = line.trim().strip_prefix('/')?;
            let (name, usage) = line.split_once(' ').unwrap_or((line, ""));
            if name.is_empty() {
                return None;
            }
            let usage = usage.trim();
            Some(HelpEntry {
                name: name.to_string(),
                usage: usage.to_string(),
                alias_of: usage.strip_prefix("-> ").map(|t| t.trim().to_string()),
            })
        })
        .collect()
}

/// Builds a command node from a usage line such as
/// `<targets> (add|remove|list) [<reason>]`.
///
/// Usage lines are abbreviated, so this only knows the first few arguments
/// and none of their types.
pub fn usage_to_node(usage: &str) -> Node {
    let mut node = Node::new(Kind::Literal);
    let mut next: BTreeMap<String, Node> = BTreeMap::new();
    let mut next_optional = true;

    for element in split_top_level(usage, ' ').into_iter().rev() {
        let (inner, optional) = match element.strip_prefix('[').and_then(|e| e.strip_suffix(']')) {
            Some(inner) => (inner, true),
            None => (element, false),
        };
        let inner = inner
            .strip_prefix('(')
            .and_then(|e| e.strip_suffix(')'))
            .unwrap_or(inner);

        let mut level = BTreeMap::new();
        for alternative in split_top_level(inner, '|') {
            let (name, kind) = match alternative
                .strip_prefix('<')
                .and_then(|a| a.strip_suffix('>'))
            {
                Some(name) => (name, Kind::Argument),
                None => (alternative, Kind::Literal),
            };
            let mut child = Node::new(kind);
            child.children = next.clone();
            child.executable = next_optional;
            level.insert(name.to_string(), child);
        }
        next = level;
        next_optional = optional;
    }

    node.children = next;
    node.executable = next_optional;
    node
}

/// Splits on `separator` outside of any brackets, skipping empty pieces.
fn split_top_level(s: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' => depth = depth.saturating_sub(1),
            c if c == separator && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts.retain(|p| !p.is_empty());
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_with_dashes_and_aliases() {
        let entries = parse_help("/ban-ip <target> [<reason>]/tp -> teleport/reload/seed");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["ban-ip", "tp", "reload", "seed"]);
        assert_eq!(entries[0].usage, "<target> [<reason>]");
        assert_eq!(entries[1].alias_of.as_deref(), Some("teleport"));
        assert_eq!(entries[2].alias_of, None);
    }

    #[test]
    fn builds_node_from_usage() {
        let node = usage_to_node("<targets> (add|remove|list) [<reason>]");
        assert!(!node.executable);
        let targets = &node.children["targets"];
        assert_eq!(targets.kind, Kind::Argument);
        assert_eq!(
            targets.children.keys().collect::<Vec<_>>(),
            ["add", "list", "remove"]
        );
        let add = &targets.children["add"];
        assert!(add.executable);
        assert_eq!(add.children["reason"].kind, Kind::Argument);
        assert!(add.children["reason"].executable);

        assert!(usage_to_node("").executable);
        let choice = usage_to_node("(<respectTeams>|under)");
        assert_eq!(choice.children["respectTeams"].kind, Kind::Argument);
        assert_eq!(choice.children["under"].kind, Kind::Literal);
    }
}
