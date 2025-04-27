use anyhow::{Context, Result};
use minecraft_client_rs::rcon::RconClient;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::{Editor, Helper, Context as RustyContext};
use std::borrow::Cow;
use std::net::SocketAddr;

// List of common Minecraft commands for autocompletion
const MC_COMMANDS: &[&str] = &[
    "/help", "/ban", "/ban-ip", "/banlist", "/deop", "/difficulty", "/effect", "/enchant", "/gamemode", "/gamerule", "/give", "/kick", "/kill", "/list", "/me", "/op", "/pardon", "/pardon-ip", "/save-all", "/save-off", "/save-on", "/say", "/scoreboard", "/seed", "/setblock", "/setidletimeout", "/setworldspawn", "/spawnpoint", "/stop", "/summon", "/teleport", "/tell", "/time", "/tp", "/weather", "/whitelist", "/xp"
];

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

impl Helper for MinecraftCompleter {}

#[tokio::main]
async fn main() -> Result<()> {
    let mut rl = Editor::new().unwrap();
    rl.set_helper(Some(MinecraftCompleter));

    println!("Minecraft RCON CLI");
    let addr = prompt(&mut rl, "Server address (host:port): ")?;
    let password = prompt(&mut rl, "RCON password: ")?;

    let sock_addr: SocketAddr = addr.parse().context("Invalid server address")?;
    let mut client = RconClient::connect(sock_addr, &password)
        .await
        .context("Failed to connect to RCON server")?;
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
                rl.add_history_entry(cmd);
                match client.cmd(cmd).await {
                    Ok(response) => println!("{}", response),
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
    println!("Goodbye!");
    Ok(())
}

fn prompt(rl: &mut Editor<MinecraftCompleter>, msg: &str) -> Result<String> {
    let line = rl.readline(msg)?;
    Ok(line.trim().to_string())
}
