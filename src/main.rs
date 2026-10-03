use anyhow::{Context, Result};
use clap::Parser;
use rpassword::prompt_password;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::{Hint, Hinter};
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context as RustyContext, Editor, Helper};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::rc::Rc;
use std::time::{Duration, Instant};

mod complete;
mod data;
mod format;
mod help_parser;
mod rcon;
mod tree;

use complete::{Engine, Live, LiveList};

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// How long player names and the like are reused before asking again.
const LIVE_LIST_TTL: Duration = Duration::from_secs(10);
const HISTORY_FILE: &str = ".minecraft_rcon_history";

/// Minecraft RCON CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Server address (host:port)
    #[arg(short, long, default_value = "127.0.0.1:25575")]
    pub address: String,

    /// RCON password; prompted for when missing
    #[arg(short, long, env = "RCON_PASSWORD", hide_env_values = true)]
    pub password: Option<String>,

    /// The server's Minecraft version, e.g. 1.21.4, for servers too old to
    /// report it themselves
    #[arg(long, value_name = "VERSION")]
    pub minecraft_version: Option<String>,

    /// Take command data from this file, made by scripts/generate_data.py,
    /// instead of downloading it
    #[arg(long, value_name = "PATH", conflicts_with = "minecraft_version")]
    pub data_file: Option<PathBuf>,

    /// Run this command and exit, instead of starting the interactive shell
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub command: Vec<String>,
}

/// The server connection, shared between the shell and its completer.
struct Session {
    client: RefCell<rcon::Client>,
    lists: RefCell<HashMap<LiveList, (Instant, Vec<String>)>>,
}

impl Live for Session {
    fn list(&self, what: LiveList, fetch: bool) -> Vec<String> {
        if let Some((fetched, names)) = self.lists.borrow().get(&what) {
            if !fetch || fetched.elapsed() < LIVE_LIST_TTL {
                return names.clone();
            }
        }
        if !fetch {
            return Vec::new();
        }
        let command = match what {
            LiveList::Players => "list",
            LiveList::Whitelist => "whitelist list",
            LiveList::Objectives => "scoreboard objectives list",
            LiveList::Teams => "team list",
        };
        // Failures are cached as well, to not stall on every keypress.
        let names = match self.client.borrow_mut().command(command) {
            Ok(body) => parse_name_list(&body),
            Err(_) => Vec::new(),
        };
        self.lists
            .borrow_mut()
            .insert(what, (Instant::now(), names.clone()));
        names
    }
}

/// Extracts the names from responses like `There are 2 of a max of 20 players
/// online: a, b` or `There are 2 team(s): [a], [b]`.
fn parse_name_list(body: &str) -> Vec<String> {
    let Some((_, names)) = body.split_once(':') else {
        return Vec::new();
    };
    names
        .split(',')
        .map(|name| name.trim().trim_matches(['[', ']']).to_string())
        .filter(|name| !name.is_empty() && !name.contains(' '))
        .collect()
}

struct MinecraftHelper {
    engine: Engine,
    session: Rc<Session>,
}

struct CommandHint {
    text: String,
    completes: bool,
}

impl Hint for CommandHint {
    fn display(&self) -> &str {
        &self.text
    }

    fn completion(&self) -> Option<&str> {
        self.completes.then_some(&self.text)
    }
}

impl Completer for MinecraftHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &RustyContext<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let candidates = self
            .engine
            .candidates(&line[..pos], self.session.as_ref(), true);
        let start = candidates.first().map_or(pos, |c| c.start);
        // Don't double the space when completing in the middle of a line.
        let space_follows = line[pos..].starts_with(' ');
        let pairs = candidates
            .into_iter()
            .map(|c| Pair {
                replacement: if c.space && !space_follows {
                    format!("{} ", c.text)
                } else {
                    c.text.clone()
                },
                display: c.text,
            })
            .collect();
        Ok((start, pairs))
    }
}

impl Hinter for MinecraftHelper {
    type Hint = CommandHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &RustyContext<'_>) -> Option<CommandHint> {
        if pos < line.len() {
            return None;
        }
        let hint = self.engine.hint(line, self.session.as_ref())?;
        Some(CommandHint {
            text: hint.text,
            completes: hint.completes,
        })
    }
}

impl Highlighter for MinecraftHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned(self.engine.highlight(line))
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("{DIM}{hint}{RESET}"))
    }

    fn highlight_char(&self, _line: &str, _pos: usize, kind: CmdKind) -> bool {
        kind != CmdKind::MoveCursor
    }
}

impl Validator for MinecraftHelper {}

impl Helper for MinecraftHelper {}

fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(HISTORY_FILE))
}

/// Runs a command and prints its response. Returns whether it succeeded.
fn run_command(session: &Session, command: &str, color: bool, interactive: bool) -> bool {
    let result = session.client.borrow_mut().command(command);
    // The command may have changed any of the cached lists.
    session.lists.borrow_mut().clear();
    match result {
        Ok(body) => {
            let response = format::format_response(command, &body, color);
            if !response.text.is_empty() {
                println!("{}", response.text);
            } else if interactive {
                println!("{DIM}(no output){RESET}");
            }
            !response.is_error
        }
        Err(e) => {
            eprintln!("Error: {e:#}");
            false
        }
    }
}

fn run_shell(
    session: Rc<Session>,
    color: bool,
    version: Option<String>,
    data_file: Option<PathBuf>,
) -> Result<()> {
    let mut tree = match data_file {
        Some(path) => {
            let json = std::fs::read_to_string(&path)
                .with_context(|| format!("could not read {}", path.display()))?;
            tree::Tree::from_json(&json)
                .with_context(|| format!("{} is not a command data file", path.display()))?
        }
        None => {
            let version = version.or_else(|| {
                let body = session.client.borrow_mut().command("version").ok()?;
                data::parse_version(&body)
            });
            data::tree_for(version.as_deref())
        }
    };
    let data_version = tree.version.clone();
    // The server knows best which commands exist; the data fills in their
    // arguments.
    match session.client.borrow_mut().command("help") {
        Ok(body) => tree.apply_help(&help_parser::parse_help(&body)),
        Err(e) => eprintln!("Warning: could not fetch the command list: {e:#}"),
    }
    let command_count = tree.root.children.len();

    let config = Config::builder()
        .completion_type(CompletionType::List)
        .history_ignore_dups(true)?
        .max_history_size(10_000)?
        .build();
    let mut rl = Editor::<MinecraftHelper, DefaultHistory>::with_config(config)?;
    rl.set_helper(Some(MinecraftHelper {
        engine: Engine { tree },
        session: Rc::clone(&session),
    }));
    let history = history_path();
    if let Some(path) = &history {
        // Missing on first run.
        let _ = rl.load_history(path);
    }

    // Keep piped output down to the responses.
    if std::io::stdin().is_terminal() {
        println!(
            "Connected: {command_count} commands, completing arguments as of Minecraft {data_version}."
        );
        println!("Tab completes, 'exit' or Ctrl-D quits.");
    }

    loop {
        match rl.readline("> ") {
            Ok(line) => {
                let cmd = line.trim();
                if cmd.eq_ignore_ascii_case("exit") || cmd.eq_ignore_ascii_case("quit") {
                    break;
                }
                if cmd.is_empty() {
                    continue;
                }
                // Ignore failures in history addition
                let _ = rl.add_history_entry(cmd);
                run_command(&session, cmd, color, true);
            }
            // Ctrl-C drops the line being typed, as in a shell.
            Err(ReadlineError::Interrupted) => continue,
            Err(ReadlineError::Eof) => break,
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }
    if let Some(path) = &history {
        if let Err(e) = rl.save_history(path) {
            eprintln!("Warning: could not save history to {}: {e}", path.display());
        }
    }
    Ok(())
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let password = match cli.password {
        Some(pw) => pw,
        None => prompt_password("Enter RCON password: ")?,
    };
    let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();

    let session = Rc::new(Session {
        client: RefCell::new(rcon::Client::connect(&cli.address, &password)?),
        lists: RefCell::new(HashMap::new()),
    });

    if !cli.command.is_empty() {
        let ok = run_command(&session, &cli.command.join(" "), color, false);
        return Ok(if ok {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        });
    }
    run_shell(session, color, cli.minecraft_version, cli.data_file)?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_lists() {
        assert_eq!(
            parse_name_list("There are 2 of a max of 20 players online: GuyLewin, Steve"),
            ["GuyLewin", "Steve"]
        );
        assert!(parse_name_list("There are 0 of a max of 20 players online: ").is_empty());
        assert_eq!(
            parse_name_list("There are 2 team(s): [red], [blue]"),
            ["red", "blue"]
        );
        assert!(parse_name_list("There are no teams").is_empty());
    }
}
