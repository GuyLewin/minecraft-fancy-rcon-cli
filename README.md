[![CI](https://github.com/GuyLewin/minecraft-fancy-rcon-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/GuyLewin/minecraft-fancy-rcon-cli/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/minecraft-fancy-rcon-cli.svg)](https://crates.io/crates/minecraft-fancy-rcon-cli)

# Fancy Minecraft RCON CLI

A powerful, user-friendly, and interactive command-line interface for sending RCON commands to a Minecraft server, written in Rust.

## Features
- Connect to a Minecraft server using the RCON protocol
- Command autocompletion for all supported Minecraft commands (auto-derived via /help)
- Argument autocompletion and real-time hinting (shows next argument or possible values as you type)
- Interactive shell with persistent command history
- Syntax highlighting for commands
- Clean error handling and helpful error messages
- Supports aliases for commands (if defined)

## TODOs
- Argument autocompletion
- Syntax highlighting for arguments
- More complex argument parsing (e.g., `<respectTeams>|under`)

## Usage

### Build
```sh
cargo build --release
```

### Run
```sh
cargo run -- --address <host:port> [--password <rcon_password>]
```
- `--address` / `-a`: The address of your Minecraft server (e.g., `127.0.0.1:25575`).
- `--password` / `-p`: (Optional) RCON password. If omitted, you will be securely prompted.

Example:
```sh
cargo run -- --address 127.0.0.1:25575
```

## Dependencies
- [minecraft-client-rs](https://crates.io/crates/minecraft-client-rs)
- [rustyline](https://crates.io/crates/rustyline)
- [clap](https://crates.io/crates/clap)
- [anyhow](https://crates.io/crates/anyhow)
- [rpassword](https://crates.io/crates/rpassword)

## License
MIT

---

### Notes
- Make sure your Minecraft server has RCON enabled and configured in `server.properties`.
- This tool is for server operators and requires the RCON port and password.

---

## Disclaimer
This project is not affiliated with, endorsed by, or associated with Mojang, Microsoft, or Minecraft. All trademarks and copyrights are the property of their respective owners.

---

Pull requests and issues welcome!
