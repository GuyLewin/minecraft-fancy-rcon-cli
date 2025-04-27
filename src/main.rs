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

mod mc_commands;
use mc_commands::MC_COMMANDS;

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

struct MinecraftCompleter;

impl Completer for MinecraftCompleter {
    type Candidate = Pair;

    fn complete(&self, line: &str, _pos: usize, _ctx: &RustyContext<'_>) -> Result<(usize, Vec<Pair>), ReadlineError> {
        let candidates = MC_COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(line))
            .map(|&cmd| Pair {
                display: cmd.to_string(),
                replacement: cmd.to_string(),
            })
            .collect::<Vec<_>>();
        Ok((0, candidates))
    }
}

impl Hinter for MinecraftCompleter {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &RustyContext<'_>) -> Option<String> {
        None
    }
}

impl Highlighter for MinecraftCompleter {}
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

fn main() -> Result<()> {
    let mut rl = Editor::<MinecraftCompleter, DefaultHistory>::new().unwrap();
    rl.set_helper(Some(MinecraftCompleter));

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
