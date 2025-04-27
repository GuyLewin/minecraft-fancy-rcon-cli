use anyhow::Result;
use clap::Parser;
use minecraft_client_rs::Client;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::{Validator, ValidationContext, ValidationResult};
use rustyline::history::DefaultHistory;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::{Editor, Helper, Context as RustyContext};
use rpassword::prompt_password;

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
}

struct MinecraftCompleter {
    commands: Vec<CommandInfo>,
}

impl Completer for MinecraftCompleter {
    type Candidate = Pair;

    fn complete(&self, line: &str, _pos: usize, _ctx: &RustyContext<'_>) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let candidates = self.commands.iter()
            .filter(|cmd| cmd.name.starts_with(line))
            .map(|cmd| Pair {
                display: cmd.name.clone(),
                replacement: cmd.name.clone(),
            })
            .collect::<Vec<_>>();
        Ok((0, candidates))
    }
}

impl Hinter for MinecraftCompleter {
    type Hint = String;
    fn hint(&self, line: &str, _pos: usize, _ctx: &RustyContext<'_>) -> Option<String> {
        self.commands.iter()
            .find(|cmd| cmd.name.starts_with(line) && cmd.name != line)
            .map(|cmd| cmd.name[line.len()..].to_string())
    }
}

impl Highlighter for MinecraftCompleter {
    fn highlight_hint<'h>(&self, hint: &'h str) -> std::borrow::Cow<'h, str> {
        use std::borrow::Cow;
        if !hint.is_empty() {
            Cow::Owned(format!("\x1b[90m{}\x1b[0m", hint)) // gray hint
        } else {
            Cow::Borrowed("")
        }
    }
}

impl Validator for MinecraftCompleter {
    fn validate(&self, ctx: &mut ValidationContext<'_>) -> Result<ValidationResult, ReadlineError> {
        let input = ctx.input();
        if self.commands.iter().any(|cmd| cmd.name == input.trim()) {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Invalid(Some("Unknown command".to_string())))
        }
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
    help.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.starts_with('/') {
                let cmd = line.split_whitespace().next().unwrap_or("");
                Some(CommandInfo { name: cmd.to_string() })
            } else {
                None
            }
        })
        .collect()
}

fn main() -> Result<()> {
    let mut rl = Editor::<MinecraftCompleter, DefaultHistory>::new().unwrap();

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
                        if cmd.eq_ignore_ascii_case("help") || cmd.eq_ignore_ascii_case("/help") {
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
