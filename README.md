# Minecraft RCON CLI

A simple, user-friendly command-line interface for sending RCON commands to a Minecraft server, written in Rust.

## Features
- Connect to a Minecraft server using RCON protocol
- Securely prompt for password if not provided via CLI
- Command autocompletion for common Minecraft commands
- Interactive shell with history support
- Clean error handling

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

## Features
- Autocompletes common Minecraft commands (e.g., `/op`, `/ban`, `/whitelist`, etc.)
- Command history and editing
- Graceful exit with `exit` or `quit`

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

Pull requests and issues welcome!
