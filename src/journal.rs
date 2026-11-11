//! A permanent record of every clean.
//!
//! One JSON object per line, appended as each item goes: what was removed,
//! when, by which rule, how much it returned, and, for a tool or toolchain,
//! the command that puts it back. Nothing is held back to make undo possible;
//! space is returned immediately, and what can be reinstalled says how.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::Serialize;

use crate::scanner::entry::ScannedEntry;

#[derive(Serialize)]
struct Record<'a> {
    time: String,
    path: String,
    freed: u64,
    category: String,
    safety: String,
    rule: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    restore: Option<String>,
}

pub fn path() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("sweeprs").join("clean-log.jsonl"))
}

fn open() -> Option<&'static Mutex<File>> {
    static LOG: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    LOG.get_or_init(|| {
        let path = path()?;
        std::fs::create_dir_all(path.parent()?).ok()?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Record that `entry` was cleaned and returned `freed` bytes.
///
/// A log that cannot be written never stops a clean: the space matters more
/// than the record of it.
pub fn record(entry: &ScannedEntry, freed: u64) {
    let record = Record {
        time: chrono::Local::now().to_rfc3339(),
        path: entry.path.display().to_string(),
        freed,
        category: format!("{:?}", entry.category),
        safety: format!("{:?}", entry.safety),
        rule: &entry.description,
        restore: restore_command(&entry.path),
    };
    let (Some(log), Ok(mut line)) = (open(), serde_json::to_string(&record)) else {
        return;
    };
    line.push('\n');
    if let Ok(mut file) = log.lock() {
        let _ = file.write_all(line.as_bytes());
    }
}

/// The command that reinstalls what lived at `path`, when it is a tool version.
pub fn restore_command(path: &Path) -> Option<String> {
    let home = dirs::home_dir()?;
    restore_command_in(path, &home)
}

fn restore_command_in(path: &Path, home: &Path) -> Option<String> {
    let version = path.file_name()?.to_string_lossy();
    let parent = path.parent()?;

    if let Ok(tool_dir) = parent.strip_prefix(home.join(".local/share/mise/installs"))
        && tool_dir.components().count() == 1
    {
        let tool = std::fs::read_to_string(parent.join(".mise.backend.toml"))
            .ok()
            .and_then(|backend| backend.parse::<toml::Table>().ok())
            .and_then(|table| table.get("short")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| tool_dir.display().to_string());
        return Some(format!("mise install {tool}@{version}"));
    }
    if parent == home.join(".rustup/toolchains") {
        return Some(format!("rustup toolchain install {version}"));
    }
    if parent == home.join(".nvm/versions/node") {
        return Some(format!("nvm install {version}"));
    }
    if parent == home.join(".pyenv/versions") {
        return Some(format!("pyenv install {version}"));
    }
    if parent == home.join(".rbenv/versions") {
        return Some(format!("rbenv install {version}"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_versions_say_how_to_reinstall_them() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = tmp.path();
        let tool = home.join(".local/share/mise/installs/github-acme-tool");
        std::fs::create_dir_all(tool.join("0.8.2")).expect("install");
        std::fs::write(
            tool.join(".mise.backend.toml"),
            "short = \"github:acme/tool\"\nfull = \"github:acme/tool\"\n",
        )
        .expect("backend");

        let cases = [
            (
                tool.join("0.8.2"),
                Some("mise install github:acme/tool@0.8.2"),
            ),
            (
                home.join(".local/share/mise/installs/node/24.22.2"),
                Some("mise install node@24.22.2"),
            ),
            (
                home.join(".rustup/toolchains/1.99.0-aarch64-apple-darwin"),
                Some("rustup toolchain install 1.99.0-aarch64-apple-darwin"),
            ),
            (
                home.join(".nvm/versions/node/v20.1.0"),
                Some("nvm install v20.1.0"),
            ),
            (
                home.join(".pyenv/versions/3.12.1"),
                Some("pyenv install 3.12.1"),
            ),
            (home.join("Library/Caches/Google/Chrome"), None),
            (
                home.join(".local/share/mise/installs/node/24.22.2/bin"),
                None,
            ),
        ];
        for (path, expected) in cases {
            assert_eq!(
                restore_command_in(&path, home).as_deref(),
                expected,
                "{}",
                path.display()
            );
        }
    }
}
