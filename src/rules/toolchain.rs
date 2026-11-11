use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::cli_cache;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index;
use crate::scanner::walker;

pub struct RustToolchainRule;

impl CleanupRule for RustToolchainRule {
    fn name(&self) -> &'static str {
        "Old Rust toolchains"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let rustup_dir = home.join(".rustup");
        let toolchains_dir = rustup_dir.join("toolchains");

        if !toolchains_dir.exists() {
            return Vec::new();
        }

        // Collect ALL toolchains that are in use, not just the current directory's override.
        let active = collect_active_rust_toolchains(&home);

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&toolchains_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let path = entry.path();

                if !path.is_dir() || active.contains(&name_str) {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Rust toolchain: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        // Stale rustup downloads and tmp dirs
        for subdir in ["downloads", "tmp"] {
            let dir = rustup_dir.join(subdir);
            if dir.exists() {
                let size = walker::dir_size(&dir);
                if size > 1024 {
                    entries.push(ScannedEntry {
                        path: dir,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Safe,
                        description: format!("Rustup {subdir} cache"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

/// Collect all Rust toolchains that are actively referenced.
///
/// Sources checked:
/// 1. `rustup toolchain list` - all installed toolchains marked as default or active
/// 2. `rust-toolchain.toml` / `rust-toolchain` files in known project roots
/// 3. The global default toolchain
fn collect_active_rust_toolchains(home: &std::path::Path) -> FxHashSet<String> {
    let mut active = FxHashSet::default();

    // Parse `rustup toolchain list` to find the default and any active ones.
    // Output format: "stable-aarch64-apple-darwin (default)"
    //                "nightly-aarch64-apple-darwin (active)"
    if let Some(result) = cli_cache::get("rustup_toolchain_list") {
        for line in result.stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            // The toolchain name is everything before the first '(' or whitespace-paren
            let name = trimmed
                .split_once(" (")
                .map_or(trimmed, |(name, _)| name)
                .trim();
            if trimmed.contains("(default)") || trimmed.contains("(active)") {
                active.insert(name.to_owned());
            }
        }
    }

    // Also check the active-toolchain output (covers directory overrides)
    if let Some(result) = cli_cache::get("rustup_active_toolchain")
        && let Some(name) = result.stdout.split_whitespace().next()
    {
        active.insert(name.to_owned());
    }

    // Scan known project roots for rust-toolchain.toml files that pin specific channels.
    // This prevents us from suggesting removal of a toolchain a project depends on.
    let search_roots = ["Workspace", "Projects", "Developer", "Code", "src", "dev"];
    for root in &search_roots {
        let root_dir = home.join(root);
        if !root_dir.exists() {
            continue;
        }
        scan_project_toolchains(&root_dir, 0, &mut active);
    }

    active
}

/// Recursively scan for `rust-toolchain.toml` or `rust-toolchain` files
/// up to a limited depth, extracting the channel they pin.
fn scan_project_toolchains(dir: &std::path::Path, depth: u8, active: &mut FxHashSet<String>) {
    if depth > 4 {
        return;
    }

    // Check for rust-toolchain.toml or rust-toolchain in this dir
    for filename in ["rust-toolchain.toml", "rust-toolchain"] {
        let path = dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&path)
            && let Some(channel) = extract_toolchain_channel(&content)
        {
            // Resolve channel to installed toolchain name.
            // "nightly" -> "nightly-aarch64-apple-darwin" on this host.
            active.insert(channel.clone());
            // Also insert with host triple appended
            let host = current_host_triple();
            if !host.is_empty() {
                active.insert(format!("{channel}-{host}"));
            }
        }
    }

    // Recurse into subdirs, skipping heavy/irrelevant ones
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        match name_str.as_ref() {
            "node_modules" | "target" | ".git" | ".build" | "vendor" | "build" | "__pycache__"
            | ".venv" | "venv" | ".tox" | "_build" | ".dart_tool" => continue,
            _ => {}
        }
        scan_project_toolchains(&path, depth + 1, active);
    }
}

/// Extract the toolchain channel from a rust-toolchain.toml or rust-toolchain file.
///
/// Handles both formats:
/// - TOML: `[toolchain]\nchannel = "nightly"`
/// - Plain: just `nightly` or `1.91.0`
fn extract_toolchain_channel(content: &str) -> Option<String> {
    // Try TOML format first
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("channel") {
            // channel = "nightly" or channel = '1.91.0'
            let value = trimmed
                .split_once('=')
                .map(|(_, v)| v.trim().trim_matches(|c| c == '"' || c == '\''))?;
            if !value.is_empty() {
                return Some(value.to_owned());
            }
        }
    }

    // Plain format: entire file is the channel name
    let trimmed = content.trim();
    if !trimmed.is_empty() && !trimmed.contains('[') && !trimmed.contains('=') {
        return Some(trimmed.to_owned());
    }

    None
}

/// Get the current host triple (e.g. "aarch64-apple-darwin").
fn current_host_triple() -> String {
    // rustc -vV prints: host: aarch64-apple-darwin
    if let Some(result) = cli_cache::get("rustup_active_toolchain") {
        // The active-toolchain output is like "nightly-aarch64-apple-darwin (overridden...)"
        // Extract the triple from the toolchain name
        let name = result.stdout.split_whitespace().next().unwrap_or("");
        // Strip channel prefix: "nightly-aarch64-apple-darwin" -> "aarch64-apple-darwin"
        // "stable-aarch64-apple-darwin" -> "aarch64-apple-darwin"
        // "1.91-aarch64-apple-darwin" -> "aarch64-apple-darwin"
        for prefix in ["nightly-", "stable-", "beta-"] {
            if let Some(rest) = name.strip_prefix(prefix) {
                return rest.to_owned();
            }
        }
        // Version-pinned: "1.91-aarch64-apple-darwin" or "1.91.0-aarch64-apple-darwin"
        if let Some(idx) = name.find('-') {
            let after = &name[idx + 1..];
            // Check it looks like a triple (contains at least one more hyphen)
            if after.contains('-') {
                return after.to_owned();
            }
        }
    }
    String::new()
}

pub struct MiseToolchainRule;

impl CleanupRule for MiseToolchainRule {
    fn name(&self) -> &'static str {
        "Old mise/rtx tool versions"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let installs_dir = home.join(".local/share/mise/installs");

        if !installs_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        // Which versions are unused is mise's call: it tracks every config file,
        // tool stub and running process that pins a version. Without its answer
        // nothing is offered.
        let prunable = cli_cache::get("mise_prunable")
            .map(|r| prunable_mise_installs(&r.stdout, &installs_dir))
            .unwrap_or_default();

        for (tool_name, ver_name, ver_path) in prunable {
            // Skip symlinks (mise uses them for aliases like "latest", "lts", etc.)
            if ver_path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
                continue;
            }

            if !ver_path.is_dir() {
                continue;
            }

            let size = walker::dir_size(&ver_path);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: ver_path,
                    size,
                    category: Category::Toolchain,
                    safety: SafetyLevel::Caution,
                    description: format!("mise {tool_name}: {ver_name}"),
                    item_count: None,
                });
            }
        }

        // mise cache and downloads
        for subdir in ["cache", "downloads"] {
            let dir = home.join(format!(".local/share/mise/{subdir}"));
            if dir.exists() {
                let size = walker::dir_size(&dir);
                if size > 1024 {
                    entries.push(ScannedEntry {
                        path: dir,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Safe,
                        description: format!("mise {subdir}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

/// `(tool, version, install path)` for each version `mise ls --prunable --json` lists.
///
/// Paths outside the installs directory are dropped, so a surprise in mise's
/// output can never point a delete somewhere else.
fn prunable_mise_installs(json: &str, installs_dir: &Path) -> Vec<(String, String, PathBuf)> {
    let Ok(serde_json::Value::Object(tools)) = serde_json::from_str(json) else {
        return Vec::new();
    };
    let mut prunable = Vec::new();
    for (tool, versions) in &tools {
        for version in versions.as_array().into_iter().flatten() {
            let (Some(name), Some(path)) = (
                version.get("version").and_then(serde_json::Value::as_str),
                version
                    .get("install_path")
                    .and_then(serde_json::Value::as_str),
            ) else {
                continue;
            };
            let path = PathBuf::from(path);
            if path.starts_with(installs_dir) && path != installs_dir {
                prunable.push((tool.clone(), name.to_owned(), path));
            }
        }
    }
    prunable
}

/// Installed versions under a version manager that nothing selects any more.
///
/// A version is still selected when the manager's global default names it, a
/// version file in any indexed project names it, or it is the one running now.
/// A pin such as `20` or `3.12` selects the newest matching install. When a pin
/// cannot be resolved here (`lts/*`, a named alias), or nothing names any
/// version at all, nothing is offered: a guess at what is unused is how a
/// cleaner removes the toolchain a project builds with.
struct VersionManager<'a> {
    versions_dir: PathBuf,
    global_file: PathBuf,
    project_files: &'a [&'a str],
    active: Option<String>,
    label: &'a str,
}

impl VersionManager<'_> {
    fn unused_versions(&self) -> Vec<ScannedEntry> {
        let Ok(read_dir) = std::fs::read_dir(&self.versions_dir) else {
            return Vec::new();
        };
        let installed: Vec<(String, PathBuf)> = read_dir
            .flatten()
            .filter(|e| e.path().symlink_metadata().is_ok_and(|m| m.is_dir()))
            .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
            .collect();
        let names: Vec<&str> = installed.iter().map(|(name, _)| name.as_str()).collect();

        let mut pins = read_pins(&self.global_file);
        for file in self.project_files {
            for path in project_index::PROJECT_INDEX.files_named(file) {
                pins.extend(read_pins(&path));
            }
        }
        pins.extend(self.active.clone());

        let Some(kept) = selected_versions(&names, &pins) else {
            return Vec::new();
        };
        let kept: FxHashSet<String> = kept.into_iter().map(str::to_owned).collect();

        installed
            .into_iter()
            .filter(|(name, _)| !kept.contains(name))
            .filter_map(|(name, path)| {
                let size = walker::dir_size(&path);
                (size > 0).then(|| ScannedEntry {
                    path,
                    size,
                    category: Category::Toolchain,
                    safety: SafetyLevel::Caution,
                    description: format!("{}: {name}", self.label),
                    item_count: None,
                })
            })
            .collect()
    }
}

fn read_pins(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|text| {
            text.lines()
                .map(|line| line.split('#').next().unwrap_or_default().trim())
                .filter(|pin| !pin.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn bare(version: &str) -> &str {
    version.trim().trim_start_matches('v')
}

fn version_key(version: &str) -> Vec<u64> {
    bare(version)
        .split(['.', '-'])
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// The installed versions `pins` select, or `None` if a pin cannot be resolved.
fn selected_versions<'a>(installed: &[&'a str], pins: &[String]) -> Option<FxHashSet<&'a str>> {
    if pins.is_empty() {
        return None;
    }
    let mut kept = FxHashSet::default();
    for pin in pins {
        let pin = bare(pin);
        if pin == "system" {
            continue;
        }
        if let Some(exact) = installed.iter().find(|v| bare(v) == pin) {
            kept.insert(*exact);
            continue;
        }
        if !pin.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return None;
        }
        let prefix = format!("{pin}.");
        if let Some(newest) = installed
            .iter()
            .filter(|v| bare(v).starts_with(&prefix))
            .max_by_key(|v| version_key(v))
        {
            kept.insert(*newest);
        }
    }
    Some(kept)
}

pub struct NodeVersionsRule;

impl CleanupRule for NodeVersionsRule {
    fn name(&self) -> &'static str {
        "Old Node.js versions (nvm)"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        VersionManager {
            versions_dir: home.join(".nvm/versions/node"),
            global_file: home.join(".nvm/alias/default"),
            project_files: &[".nvmrc", ".node-version"],
            active: cli_cache::get("node_version").map(|r| r.stdout.trim().to_owned()),
            label: "Node.js",
        }
        .unused_versions()
    }
}

pub struct PythonVersionsRule;

impl CleanupRule for PythonVersionsRule {
    fn name(&self) -> &'static str {
        "Old Python versions"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        VersionManager {
            versions_dir: home.join(".pyenv/versions"),
            global_file: home.join(".pyenv/version"),
            project_files: &[".python-version"],
            // "Python 3.x.y"
            active: cli_cache::get("python3_version")
                .and_then(|r| r.stdout.split_whitespace().nth(1).map(str::to_owned)),
            label: "Python",
        }
        .unused_versions()
    }
}

pub struct RubyVersionsRule;

impl CleanupRule for RubyVersionsRule {
    fn name(&self) -> &'static str {
        "Old Ruby versions"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        VersionManager {
            versions_dir: home.join(".rbenv/versions"),
            global_file: home.join(".rbenv/version"),
            project_files: &[".ruby-version"],
            // "ruby 3.x.yp123 ...", without the patch suffix
            active: cli_cache::get("ruby_version").and_then(|r| {
                r.stdout
                    .split_whitespace()
                    .nth(1)
                    .and_then(|v| v.split('p').next())
                    .map(str::to_owned)
            }),
            label: "Ruby",
        }
        .unused_versions()
    }
}

pub struct JavaVersionsRule;

impl CleanupRule for JavaVersionsRule {
    fn name(&self) -> &'static str {
        "Old Java versions"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let jvm_dir = if cfg!(target_os = "macos") {
            PathBuf::from("/Library/Java/JavaVirtualMachines")
        } else {
            PathBuf::from("/usr/lib/jvm")
        };

        if !jvm_dir.exists() {
            return Vec::new();
        }

        // java --version outputs to stderr on some versions, stdout on others
        let active_version = cli_cache::get_raw("java_version")
            .and_then(|r| {
                let output = if r.stdout.is_empty() {
                    &r.stderr
                } else {
                    &r.stdout
                };
                // First line is like "openjdk 21.0.1 2023-10-17"
                output
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .map(str::to_owned)
            })
            .unwrap_or_default();

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&jvm_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let path = entry.path();

                if !path.is_dir() {
                    continue;
                }

                // Skip if the dir name contains the active version
                if !active_version.is_empty() && name_str.contains(&active_version) {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Java: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(RustToolchainRule),
        Box::new(MiseToolchainRule),
        Box::new(NodeVersionsRule),
        Box::new(PythonVersionsRule),
        Box::new(RubyVersionsRule),
        Box::new(JavaVersionsRule),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_versions_mise_calls_prunable_are_offered() {
        let installs = Path::new("/home/sashoush/.local/share/mise/installs");
        let json = r#"{
          "rust": [
            {"version": "1.99.0", "install_path": "/home/sashoush/.local/share/mise/installs/rust/1.99.0", "active": false}
          ],
          "github:acme/tool": [
            {"version": "0.1.0", "install_path": "/home/sashoush/.local/share/mise/installs/github-acme-tool/0.1.0"}
          ],
          "evil": [
            {"version": "1", "install_path": "/home/sashoush"},
            {"version": "2", "install_path": "/home/sashoush/.local/share/mise/installs"},
            {"version": "3"}
          ]
        }"#;

        assert_eq!(
            prunable_mise_installs(json, installs),
            vec![
                (
                    "github:acme/tool".to_owned(),
                    "0.1.0".to_owned(),
                    installs.join("github-acme-tool/0.1.0")
                ),
                (
                    "rust".to_owned(),
                    "1.99.0".to_owned(),
                    installs.join("rust/1.99.0")
                ),
            ]
        );
    }

    fn kept(installed: &[&'static str], pins: &[&str]) -> Option<Vec<&'static str>> {
        let pins: Vec<String> = pins.iter().map(|p| (*p).to_owned()).collect();
        selected_versions(installed, &pins).map(|kept| {
            let mut kept: Vec<_> = kept.into_iter().collect();
            kept.sort_unstable();
            kept
        })
    }

    #[test]
    fn exact_and_partial_pins_keep_the_versions_they_select() {
        let installed = ["v18.20.4", "v20.11.0", "v20.18.1", "v22.3.0"];
        assert_eq!(
            kept(&installed, &["v18.20.4", "20", "system"]),
            Some(vec!["v18.20.4", "v20.18.1"])
        );
        assert_eq!(
            kept(&["3.11.9", "3.12.1", "3.12.10"], &["3.12"]),
            Some(vec!["3.12.10"])
        );
        assert_eq!(kept(&["3.11.9"], &["3.13"]), Some(vec![]));
    }

    #[test]
    fn nothing_is_offered_without_pins_or_with_an_alias_it_cannot_resolve() {
        let installed = ["v18.20.4", "v20.18.1"];
        assert_eq!(kept(&installed, &[]), None);
        assert_eq!(kept(&installed, &["lts/iron"]), None);
        assert_eq!(kept(&installed, &["20", "node"]), None);
    }

    #[test]
    fn version_files_are_read_line_by_line_without_comments() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let file = tmp.path().join(".python-version");
        std::fs::write(&file, "3.12.1\n# the old one too\n3.11.9  # legacy\n\n").expect("write");
        assert_eq!(read_pins(&file), vec!["3.12.1", "3.11.9"]);
        assert_eq!(read_pins(&tmp.path().join("missing")), Vec::<String>::new());
    }

    #[test]
    fn nothing_is_offered_when_mise_gives_no_answer() {
        let installs = Path::new("/home/sashoush/.local/share/mise/installs");
        assert_eq!(prunable_mise_installs("", installs), Vec::new());
        assert_eq!(prunable_mise_installs("{}", installs), Vec::new());
        assert_eq!(
            prunable_mise_installs("mise ERROR something", installs),
            Vec::new()
        );
    }
}
