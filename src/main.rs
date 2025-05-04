use anyhow::Result;
use clap::Parser;
use minecraft_client_rs::Client;
use rpassword::prompt_password;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{CompletionType, Config, Context as RustyContext, Editor, Helper};
use std::borrow::Cow;
use std::collections::HashMap;

mod help_parser;

/// Minecraft RCON CLI
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Server address (host:port)
    #[arg(short, long)]
    pub address: String,

    /// RCON password
    #[arg(short, long)]
    pub password: Option<String>,
}

// TODO: Add support for complex structures like (<respectTeams>|under)
#[derive(Debug, Clone)]
enum Argument {
    #[allow(dead_code)]
    Required(String), // <arg>
    #[allow(dead_code)]
    Optional(String), // [<arg>]
    RequiredChoice(Vec<String>), //(a|b|c)
    OptionalChoice(Vec<String>), // [(a|b|c)] or [a|b|c]
}

struct MinecraftCompleter {
    commands: HashMap<String, Vec<Argument>>,
}

const ERROR_PREFIXES: &[&str] = &[
    "Unknown or incomplete command, see below for error",
    "Incorrect argument for command",
];

impl Completer for MinecraftCompleter {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &RustyContext<'_>,
    ) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let input = &line[..pos];
        let words: Vec<&str> = input.split(' ').collect();
        match words.len() {
            // No suggestions on empty input
            0 => Ok((0, Vec::new())),
            // Complete command name
            1 => {
                let candidates = self
                    .commands
                    .keys()
                    .filter(|cmd_name| cmd_name.starts_with(line))
                    .map(|cmd_name| Pair {
                        display: cmd_name.clone(),
                        replacement: cmd_name.clone() + " ",
                    })
                    .collect();
                Ok((0, candidates))
            }
            // Try to match command
            _ => {
                match self.commands.get(words[0]) {
                    Some(args) => {
                        // Complete argument
                        let mut pairs = Vec::new();
                        let input_argument_count = words.len() - 1; // -1 for command name

                        // If there are too many input arguments, return no suggestions
                        if args.len() < input_argument_count {
                            return Ok((0, Vec::new()));
                        }
                        if let Some(
                            Argument::RequiredChoice(choices) | Argument::OptionalChoice(choices),
                        ) = args.get(input_argument_count - 1)
                        {
                            for choice in choices {
                                if choice.starts_with(words.last().unwrap()) {
                                    pairs.push(Pair {
                                        display: choice.clone(),
                                        replacement: choice.clone() + " ",
                                    });
                                }
                            }
                        }
                        Ok((line.len() - words.last().unwrap().len(), pairs))
                    }
                    None => Ok((0, Vec::new())),
                }
            }
        }
    }
}

impl Hinter for MinecraftCompleter {
    type Hint = String;
    fn hint(&self, line: &str, _pos: usize, _ctx: &RustyContext<'_>) -> Option<String> {
        if line.is_empty() || line == "/" || !line.starts_with('/') || line.contains(' ') {
            return None;
        }
        if let Some(cmd_name) = self
            .commands
            .keys()
            .find(|cmd_name| cmd_name.starts_with(line))
        {
            return Some(cmd_name[line.len()..].to_string());
        }
        // TODO: Add support for argument hinting
        None
    }
}

impl Highlighter for MinecraftCompleter {
    fn highlight_candidate<'c>(
        &self,
        candidate: &'c str,
        _completion: rustyline::CompletionType,
    ) -> Cow<'c, str> {
        Cow::Owned(highlight_command(self, candidate, true))
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned(highlight_command(self, line, false))
    }
}

fn highlight_command(completer: &MinecraftCompleter, s: &str, is_suggestion: bool) -> String {
    let mut colored = String::new();

    let words: Vec<&str> = s.split_whitespace().collect();
    if words.is_empty() {
        return s.to_string();
    }
    let command_found = completer
        .commands
        .iter()
        .any(|(cmd_name, _)| cmd_name == words[0]);

    if command_found {
        if is_suggestion {
            colored.push_str("\x1b[33m"); // yellow
        } else {
            colored.push_str("\x1b[32m"); // green
        }
        colored.push_str(words[0]);
        colored.push_str("\x1b[0m"); // reset
    } else {
        colored.push_str(words[0]);
    }
    colored.push_str(&s[words[0].len()..]);
    colored
}

impl Validator for MinecraftCompleter {
    fn validate(
        &self,
        _ctx: &mut ValidationContext<'_>,
    ) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Helper for MinecraftCompleter {}

fn format_generic_response(body: &str) -> String {
    if let Some(prefix) = ERROR_PREFIXES
        .iter()
        .find(|prefix| body.starts_with(*prefix))
    {
        let suffix = &body[prefix.len()..];
        format!("{}\n{}", prefix, suffix.trim_start())
    } else {
        body.to_string()
    }
}

fn main() -> Result<()> {
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let mut rl = Editor::<MinecraftCompleter, DefaultHistory>::with_config(config).unwrap();

    println!("Minecraft RCON CLI");
    let cli = Cli::parse();
    let addr = cli.address;
    let password = match cli.password {
        Some(pw) => pw,
        None => prompt_password("Enter RCON password: ").expect("Failed to read password"),
    };

    let mut client = Client::new(addr.clone()).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    client
        .authenticate(password.clone())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // Fetch and parse /help for dynamic completion
    let help_response = client
        .send_command("/help".to_string())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .body;
    let commands = help_parser::parse_commands(help_parser::format_help_response(&help_response));
    rl.set_helper(Some(MinecraftCompleter { commands }));
    println!("Connected. Type Minecraft commands or 'exit' to quit.");

    loop {
        let readline = rl.readline("> ");
        match readline {
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
                match client.send_command(cmd.to_string()) {
                    Ok(response) => {
                        if cmd.starts_with("help") || cmd.starts_with("/help") {
                            println!("{}", help_parser::format_help_response(&response.body));
                        } else {
                            println!("{}", format_generic_response(&response.body));
                        }
                    }
                    Err(e) => eprintln!("Error: {e}"),
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                println!("Error: {err:?}");
                break;
            }
        }
    }
    Ok(())
}
