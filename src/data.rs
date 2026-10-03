//! Finds the command data matching the server's Minecraft version.
//!
//! Minecraft only produces this data by running the server jar's data
//! generator, so it is fetched from a mirror that publishes the result for
//! every version, and cached. The copy embedded in the binary is the fallback.

use crate::tree::Tree;
use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;

const MIRROR: &str = "https://raw.githubusercontent.com/misode/mcmeta";
const CACHE_DIR: &str = "minecraft-fancy-rcon-cli";
const DOWNLOAD_TIMEOUT_SECS: &str = "30";

/// Extracts the version id from the response to `version`, a command only
/// recent servers have.
pub fn parse_version(body: &str) -> Option<String> {
    let id = body.strip_prefix("Server version info:")?.trim_start();
    let id = id.strip_prefix("id = ")?;
    let id = id[..id.find("name = ")?].trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Returns the tree for `version`, or the embedded one if that's not to be had.
pub fn tree_for(version: Option<&str>) -> Tree {
    let embedded = Tree::embedded();
    let Some(version) = version else {
        eprintln!(
            "Could not tell the server's version, so arguments are completed as of Minecraft {}. \
             Pass --minecraft-version to fix that.",
            embedded.version
        );
        return embedded;
    };
    if version == embedded.version {
        return embedded;
    }
    match cached_or_downloaded(version) {
        Ok(tree) => tree,
        Err(e) => {
            eprintln!(
                "Could not get command data for Minecraft {version} ({e:#}), \
                 so arguments are completed as of Minecraft {}.",
                embedded.version
            );
            embedded
        }
    }
}

fn cached_or_downloaded(version: &str) -> Result<Tree> {
    // The version ends up in a URL and a file name.
    let safe = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    if version.is_empty() || version.starts_with('.') || !version.chars().all(safe) {
        bail!("not a version this can look up");
    }
    let cache = cache_dir().map(|dir| dir.join(format!("{version}.json")));
    if let Some(tree) = cache
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|json| Tree::from_json(&json).ok())
    {
        return Ok(tree);
    }

    eprintln!("Downloading command data for Minecraft {version}...");
    let json = download(version)?;
    let tree = Tree::from_json(&json).context("the downloaded data is not usable")?;
    if let Some(path) = cache {
        if let Err(e) = write_cache(&path, &json) {
            eprintln!("Warning: could not cache it in {}: {e}", path.display());
        }
    }
    Ok(tree)
}

fn download(version: &str) -> Result<String> {
    let fetch = |report: &str| -> Result<serde_json::Value> {
        let url = format!("{MIRROR}/{version}-summary/{report}/data.min.json");
        let json = curl(&url)?;
        serde_json::from_slice(&json).context("the download is not JSON")
    };
    let data = serde_json::json!({
        "version": version,
        "commands": fetch("commands")?,
        "registries": fetch("registries")?,
    });
    Ok(data.to_string())
}

/// Downloads with the system's `curl` rather than an HTTP library: it is
/// there on about every system, already knows the local proxies and
/// certificates, and spares the build a TLS stack.
fn curl(url: &str) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args(["--fail", "--silent", "--show-error", "--location"])
        .args(["--proto", "=https", "--max-time", DOWNLOAD_TIMEOUT_SECS])
        .arg(url)
        .output()
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => anyhow!("downloading needs curl, which is not installed"),
            _ => anyhow!(e).context("could not run curl"),
        })?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let error = String::from_utf8_lossy(&output.stderr);
    if error.contains("404") {
        bail!("none is published for that version");
    }
    bail!("{}", error.trim().trim_start_matches("curl: "))
}

fn write_cache(path: &PathBuf, json: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // Written aside first, so an interrupted run leaves no broken file.
    let partial = path.with_extension("json.partial");
    fs::write(&partial, json)?;
    fs::rename(partial, path)
}

fn cache_dir() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| var("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join(CACHE_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_response() {
        assert_eq!(
            parse_version("Server version info:id = 26.3name = 26.3data = 5023").as_deref(),
            Some("26.3")
        );
        assert_eq!(
            parse_version("Server version info:id = 26.4-snapshot-2name = 26.4 Snapshot 2")
                .as_deref(),
            Some("26.4-snapshot-2")
        );
        assert_eq!(
            parse_version("Unknown or incomplete command. See below for errorversion<--[HERE]"),
            None
        );
    }

    #[test]
    fn rejects_versions_unfit_for_a_path() {
        assert!(cached_or_downloaded("../../etc/passwd").is_err());
        assert!(cached_or_downloaded("").is_err());
        assert!(cached_or_downloaded("1.14 Pre-Release 1").is_err());
    }
}
