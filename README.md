[![CI](https://github.com/GuyLewin/minecraft-fancy-rcon-cli/actions/workflows/ci.yml/badge.svg)](https://github.com/GuyLewin/minecraft-fancy-rcon-cli/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/minecraft-fancy-rcon-cli.svg)](https://crates.io/crates/minecraft-fancy-rcon-cli)

# Fancy Minecraft RCON CLI

A powerful, user-friendly, and interactive command-line interface for sending RCON commands to a Minecraft server, written in Rust.

## Features
- Connect to a Minecraft server using the RCON protocol
- Tab completion for commands, subcommands and arguments: online players and target selectors, game modes, game rules, items, blocks, entities, effects, enchantments, biomes, structures, sounds, advancements, recipes, scoreboard objectives, teams and more
- Works through `/execute ... run` and aliases such as `/tp` and `/xp`
- Hints as you type: the rest of the word when it is unambiguous, otherwise the arguments that come next
- Syntax highlighting for commands and subcommands; input that can't be valid turns red
- Interactive shell with persistent command history (`~/.minecraft_rcon_history`)
- Readable responses: line breaks RCON strips are restored for `/help` and `/version`, and errors point at the offending part of the command
- Handles responses longer than one RCON packet (such as `/help` on recent versions)
- Reconnects automatically after the server restarts
- One-shot mode for scripts

Commands may be typed with or without the leading `/`. Press Tab twice to list all candidates.

## Usage

### Build
```sh
cargo build --release
```

### Run
```sh
cargo run -- [--address <host:port>] [--password <rcon_password>] [command...]
```
- `--address` / `-a`: The address of your Minecraft server. Defaults to `127.0.0.1:25575`.
- `--password` / `-p`: RCON password. Can also be set through the `RCON_PASSWORD` environment variable. If omitted, you will be securely prompted.
- `--minecraft-version` / `--data-file`: See [Supported Minecraft versions](#supported-minecraft-versions).
- `command`: Run a single command and exit instead of starting the shell. The exit code is non-zero if the server rejects the command.

Examples:
```sh
cargo run -- --address 127.0.0.1:25575
RCON_PASSWORD=secret cargo run -- list
```

## Supported Minecraft versions
Completion adapts to the server's version instead of being tied to the one this tool was built for:

- The list of commands comes from the server's own `/help`, so it always matches the server, including commands added by datapacks.
- What their arguments look like comes from the command tree and registries of the server's exact version. Minecraft only produces those by running the server jar's data generator, so they are downloaded (about 1 MB, using `curl`) from [misode/mcmeta](https://github.com/misode/mcmeta), which publishes them for every version since 1.14, and cached in `~/.cache/minecraft-fancy-rcon-cli`.
- Data for Minecraft Java Edition 26.3 is built in. It is used for 26.3 servers without downloading anything, and as the fallback when offline.

Servers report their version through the `version` command, which only recent ones have. For older servers, pass it yourself:
```sh
cargo run -- --minecraft-version 1.21.4
```

To not rely on the download, generate the data from your own server jar and point to it. This needs Python 3 and a `java` that can run that server version:
```sh
scripts/generate_data.py path/to/server.jar data.json
cargo run -- --data-file data.json
```
Without an output path, the script refreshes the built-in data in `data/minecraft.json`.

## TODOs
- Completion inside target selectors (`@e[type=...]`), item components and NBT
- Completion of resource tags (`#minecraft:...`)

## Dependencies
- [rustyline](https://crates.io/crates/rustyline)
- [clap](https://crates.io/crates/clap)
- [anyhow](https://crates.io/crates/anyhow)
- [rpassword](https://crates.io/crates/rpassword)
- [serde](https://crates.io/crates/serde) / [serde_json](https://crates.io/crates/serde_json)

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
