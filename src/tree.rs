use crate::help_parser::{self, HelpEntry};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Command tree and registries of one Minecraft version, as written by
/// `scripts/generate_data.py`. Used when the server's version is unknown or
/// its data can't be downloaded.
const EMBEDDED: &str = include_str!("../data/minecraft.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Root,
    Literal,
    Argument,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Properties {
    pub registry: Option<String>,
    /// `greedy` for strings that swallow the rest of the line, `players` for
    /// entity arguments that only take players.
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// A node of the command tree, in the format of Minecraft's `commands.json`
/// data report.
#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(default)]
    pub children: BTreeMap<String, Node>,
    #[serde(default)]
    pub executable: bool,
    /// Missing on arguments only known from `/help`, whose type is anyone's guess.
    pub parser: Option<String>,
    #[serde(default)]
    pub properties: Properties,
    pub redirect: Option<Vec<String>>,
}

impl Node {
    pub fn new(kind: Kind) -> Self {
        Node {
            kind,
            children: BTreeMap::new(),
            executable: false,
            parser: None,
            properties: Properties::default(),
            redirect: None,
        }
    }
}

#[derive(Deserialize)]
pub struct Tree {
    pub version: String,
    #[serde(rename = "commands")]
    pub root: Node,
    pub registries: HashMap<String, Vec<String>>,
    #[serde(skip)]
    no_children: BTreeMap<String, Node>,
}

impl Tree {
    pub fn embedded() -> Self {
        Self::from_json(EMBEDDED).expect("embedded data/minecraft.json is valid")
    }

    pub fn from_json(json: &str) -> serde_json::Result<Self> {
        serde_json::from_str(json)
    }

    /// What can follow `node`, looking through redirects such as `tp` to
    /// `teleport` or `execute as <targets>` back to `execute`.
    pub fn children<'a>(&'a self, node: &'a Node) -> &'a BTreeMap<String, Node> {
        let mut node = node;
        for _ in 0..8 {
            match &node.redirect {
                Some(path) => match self.lookup(path) {
                    Some(target) => node = target,
                    None => return &self.no_children,
                },
                // The report leaves out redirects to the root, so a dead end
                // such as `execute run` is one.
                None if node.children.is_empty()
                    && !node.executable
                    && node.kind == Kind::Literal =>
                {
                    return &self.root.children
                }
                None => return &node.children,
            }
        }
        &self.no_children
    }

    fn lookup(&self, path: &[String]) -> Option<&Node> {
        path.iter()
            .try_fold(&self.root, |node, name| node.children.get(name))
    }

    /// Aligns the top-level commands with what the server's `/help` lists:
    /// drops the ones it lacks and adds the ones the embedded data lacks, as
    /// far as their usage line describes them.
    pub fn apply_help(&mut self, entries: &[HelpEntry]) {
        if entries.is_empty() {
            return;
        }
        let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        self.root
            .children
            .retain(|name, _| names.contains(name.as_str()));
        for entry in entries {
            if self.root.children.contains_key(&entry.name) {
                continue;
            }
            let node = match &entry.alias_of {
                Some(target) => Node {
                    redirect: Some(vec![target.clone()]),
                    ..Node::new(Kind::Literal)
                },
                None => help_parser::usage_to_node(&entry.usage),
            };
            self.root.children.insert(entry.name.clone(), node);
        }
    }
}
