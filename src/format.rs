//! Makes responses readable again: RCON delivers multi-line output with the
//! line breaks removed, and without any color.

use crate::help_parser;

const RED: &str = "\x1b[31m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

const HERE: &str = "<--[HERE]";
const VERSION_HEADER: &str = "Server version info:";
const VERSION_KEYS: &[&str] = &[
    "id",
    "name",
    "data",
    "series",
    "protocol",
    "build_time",
    "pack_resource",
    "pack_data",
    "stable",
];

pub struct Response {
    pub text: String,
    pub is_error: bool,
}

pub fn format_response(command: &str, body: &str, color: bool) -> Response {
    let command = command.trim().trim_start_matches('/');
    let paint = |text: &str, style: &str| match color {
        true if !text.is_empty() => format!("{style}{text}{RESET}"),
        _ => text.to_string(),
    };

    if let Some(body) = body.strip_suffix(HERE) {
        let (message, context) = split_error(command, body);
        let text = format!(
            "{}\n{}{}",
            paint(message, RED),
            paint(context, DIM),
            paint(HERE, RED)
        );
        return Response {
            text,
            is_error: true,
        };
    }

    let name = command.split(' ').next().unwrap_or("");
    let text = if name == "help" && body.starts_with('/') {
        help_parser::format_help_response(body)
    } else if body.starts_with(VERSION_HEADER) {
        format_version(body)
    } else {
        body.to_string()
    };
    Response {
        text,
        is_error: false,
    }
}

/// Splits `<message><context>`, where the context is the command up to where
/// parsing failed, cut down to its last 10 characters, plus what follows.
fn split_error<'a>(command: &str, body: &'a str) -> (&'a str, &'a str) {
    let chars: Vec<char> = command.chars().collect();
    let mut context_len = 0;
    for cursor in 0..=chars.len() {
        let mut context = String::new();
        if cursor > 10 {
            context.push_str("...");
        }
        context.extend(&chars[cursor.saturating_sub(10)..]);
        if body.ends_with(&context) {
            context_len = context_len.max(context.len());
        }
    }
    body.split_at(body.len() - context_len)
}

fn format_version(body: &str) -> String {
    let mut text = body.to_string();
    let mut from = VERSION_HEADER.len();
    for key in VERSION_KEYS {
        let marker = format!("{key} = ");
        if let Some(i) = text[from..].find(&marker) {
            text.insert_str(from + i, "\n  ");
            from += i + 3 + marker.len();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(command: &str, body: &str) -> String {
        format_response(command, body, false).text
    }

    #[test]
    fn splits_errors_from_their_context() {
        assert_eq!(
            plain(
                "/teleport",
                "Unknown or incomplete command. See below for errorteleport<--[HERE]"
            ),
            "Unknown or incomplete command. See below for error\nteleport<--[HERE]"
        );
        assert_eq!(
            plain(
                "gamemode foo",
                "Unknown game mode: foo...memode foo<--[HERE]"
            ),
            "Unknown game mode: foo\n...memode foo<--[HERE]"
        );
        assert_eq!(
            plain(
                "data get entity @e[limit=1,type=cow] Pos x y",
                "Expected double...=cow] Pos x y<--[HERE]"
            ),
            "Expected double\n...=cow] Pos x y<--[HERE]"
        );
        assert!(format_response("x", "Unknown commandx<--[HERE]", false).is_error);
        assert!(!format_response("list", "There are 0 players", false).is_error);
    }

    #[test]
    fn restores_line_breaks() {
        assert_eq!(
            plain(
                "help teleport",
                "/teleport <location>/teleport <destination>"
            ),
            "/teleport <location>\n/teleport <destination>"
        );
        assert_eq!(
            plain(
                "version",
                "Server version info:id = 26.3name = 26.3data = 5023stable = yes"
            ),
            "Server version info:\n  id = 26.3\n  name = 26.3\n  data = 5023\n  stable = yes"
        );
        assert_eq!(plain("say a/b", "a/b"), "a/b");
    }
}
