use anyhow::Result;
use clap::Parser;
use minecraft_client_rs::Client;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{Validator, ValidationContext, ValidationResult};
use rustyline::history::DefaultHistory;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::{Config, CompletionType, Editor, Helper, Context as RustyContext};
use rpassword::prompt_password;
use std::borrow::Cow;

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

#[derive(Debug, Clone)]
struct CommandInfo {
    name: String,
    args: Vec<Argument>,
}

#[derive(Debug, Clone)]
enum Argument {
    Required(String),         // <arg>
    Optional(String),         // [<arg>]
    Choice(Vec<String>),      // [a|b|c] or (a|b|c)
    OptionalChoice(Vec<String>), // [(a|b|c)] or [a|b|c]
}


struct MinecraftCompleter {
    commands: Vec<CommandInfo>,
}

impl Completer for MinecraftCompleter {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _ctx: &RustyContext<'_>) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let input = &line[..pos];
        let words: Vec<&str> = input.trim_start().split_whitespace().collect();
        match words.len() {
            // No suggestions on empty input
            0 => Ok((0, Vec::new())),
            // Complete command name
            1 => {
                let candidates = self.commands.iter()
                .filter(|cmd| cmd.name.starts_with(line))
                .map(|cmd| Pair {
                    display: cmd.name.clone(),
                    replacement: cmd.name.clone(),
                })
                .collect();
                Ok((0, candidates))
            },
            // Try to match command
            _ => {
                match self.commands.iter().find(|cmd| words[0] == cmd.name) {
                    Some(cmd) => {
                        // Complete argument
                        let mut pairs = Vec::new();
                        let input_argument_count = words.len() - 1; // -1 for command name
                        // If there are too many input arguments, return no suggestions
                        if cmd.args.len() < input_argument_count {
                            return Ok((0, Vec::new()));
                        }
                        if let Some(arg) = cmd.args.get(input_argument_count - 1) {
                            match arg {
                                Argument::Choice(choices) | Argument::OptionalChoice(choices) => {
                                    for choice in choices {
                                        if choice.starts_with(words.last().unwrap()) {
                                            pairs.push(Pair {
                                                display: choice.clone(),
                                                replacement: choice.clone() + " ",
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok((line.len() - words.last().unwrap().len(), pairs))
                    },
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
        if let Some(cmd) = self.commands.iter().find(|cmd| cmd.name.starts_with(line) && cmd.name != line) {
            return Some(cmd.name[line.len()..].to_string());
        }
        None
    }
} 

impl Highlighter for MinecraftCompleter {
    fn highlight_candidate<'c>(
        &self,
        candidate: &'c str,
        _completion: rustyline::CompletionType,
    ) -> Cow<'c, str> {
        Cow::Owned(highlight_command( self, candidate, true))
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        Cow::Owned(highlight_command(self, line, false))
    }
}

fn highlight_command(completer: &MinecraftCompleter, s: &str, is_suggestion: bool) -> String {
    let mut colored = String::new();

    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() == 0 {
        return s.to_string();
    }
    let command_found = completer.commands.iter()
                .any(|cmd| cmd.name == words[0]);

    if command_found {
        if is_suggestion {
            colored.push_str("\x1b[33m"); // yellow
        } else {
            colored.push_str("\x1b[32m"); // green
        }
        colored.push_str(&words[0]);
        colored.push_str("\x1b[0m"); // reset
    } else {
        colored.push_str(&words[0]);
    }
    colored.push_str(&s[words[0].len()..]); 
    colored
}

impl Validator for MinecraftCompleter {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> Result<ValidationResult, ReadlineError> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Helper for MinecraftCompleter {}

fn format_help_response(body: &str) -> String {
    let mut fixed = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    let mut prev = None;
    while let Some(c) = chars.next() {
        if c == '/' && prev != Some('\n') && prev.is_some() {
            fixed.push('\n');
        }
        fixed.push(c);
        prev = Some(c);
    }
    fixed.trim().to_string()
}

fn parse_help_output(help: String) -> Vec<CommandInfo> {
    use regex::Regex;
    let re_cmd = Regex::new(r"^(?P<cmd>/\w+)(?P<args>.*)").unwrap();
    let re_required = Regex::new(r"<([^>]+)>").unwrap();
    let re_optional = Regex::new(r"\[<([^>]+)>\]").unwrap();
    let re_choice = Regex::new(r"\(([^)]+)\)").unwrap();
    let re_optional_choice = Regex::new(r"\[([^\]]+\|[^\]]+)\]").unwrap();

    help.lines()
        .filter_map(|line| {
            let line = line.trim();
            if let Some(cap) = re_cmd.captures(line) {
                let name = cap["cmd"].to_string();
                let mut args = Vec::new();
                let args_str = cap.name("args").map(|m| m.as_str()).unwrap_or("");
                // Parse required args
                for cap in re_required.captures_iter(args_str) {
                    args.push(Argument::Required(cap[1].to_string()));
                }
                // Parse optional args
                for cap in re_optional.captures_iter(args_str) {
                    args.push(Argument::Optional(cap[1].to_string()));
                }
                // Parse choices (parentheses or brackets)
                for cap in re_choice.captures_iter(args_str) {
                    let opts = cap[1].split('|').map(|s| s.trim().to_string()).collect();
                    args.push(Argument::Choice(opts));
                }
                for cap in re_optional_choice.captures_iter(args_str) {
                    let opts = cap[1].split('|').map(|s| s.trim().to_string()).collect();
                    args.push(Argument::OptionalChoice(opts));
                }
                Some(CommandInfo { name, args })
            } else {
                None
            }
        })
        .collect()
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
        None => {
            prompt_password("Enter RCON password: ").expect("Failed to read password")
        }
    };

    let mut client = Client::new(addr.clone()).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    client.authenticate(password.clone()).map_err(|e| anyhow::anyhow!(e.to_string()))?;

    // Fetch and parse /help for dynamic completion
    let help_response = client.send_command("/help".to_string()).map_err(|e| anyhow::anyhow!(e.to_string()))?.body;
    let commands = parse_help_output(format_help_response(&help_response));
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
                            println!("{}", format_help_response(&response.body));
                        } else {
                            println!("{}", response.body);
                        }
                    },
                    Err(e) => eprintln!("Error: {}", e),
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}
