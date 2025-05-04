use regex::Regex;
use std::collections::HashMap;

use crate::Argument;

pub fn format_help_response(body: &str) -> String {
    let mut fixed = String::with_capacity(body.len());
    let chars = body.chars().peekable();
    let mut prev = None;
    for c in chars {
        if c == '/' && prev != Some('\n') && prev.is_some() {
            fixed.push('\n');
        }
        fixed.push(c);
        prev = Some(c);
    }
    fixed.trim().to_string()
}

pub fn parse_commands(help: String) -> HashMap<String, Vec<Argument>> {
    let re_cmd = Regex::new(r"^(?P<cmd>/\w+)(?P<args>.*)").unwrap();
    let re_required = Regex::new(r"<([^>]+)>").unwrap();
    let re_optional = Regex::new(r"\[<([^>]+)>\]").unwrap();
    let re_required_choice = Regex::new(r"\(([^)]+)\)").unwrap();
    let re_optional_choice = Regex::new(r"\[([^\]]+\|[^\]]+)\]").unwrap();
    let re_alias = Regex::new(r"^(?P<alias>/\w+)\s*->\s*(?P<target>\w+)").unwrap();

    let mut commands: HashMap<String, Vec<Argument>> = HashMap::new();
    let mut alias_map: HashMap<String, String> = HashMap::new(); // alias -> target

    for line in help.lines() {
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
            for cap in re_required_choice.captures_iter(args_str) {
                let opts = cap[1].split('|').map(|s| s.trim().to_string()).collect();
                args.push(Argument::RequiredChoice(opts));
            }
            for cap in re_optional_choice.captures_iter(args_str) {
                let opts = cap[1].split('|').map(|s| s.trim().to_string()).collect();
                args.push(Argument::OptionalChoice(opts));
            }
            commands.insert(name, args);
        }
        if let Some(cap) = re_alias.captures(line) {
            let alias = cap["alias"].to_string();
            let target = format!("/{}", &cap["target"]);
            alias_map.insert(alias, target);
        }
    }

    for (alias, target) in alias_map {
        // Replace empty alias commands with target commands
        commands.insert(alias, commands[&target].clone());
    }
    commands
}
