use std::path::PathBuf;

use rustc_hash::FxHashSet;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::cli_cache;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
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
    if let Some(result) = cli_cache::get("rustup_active_toolchain") {
        if let Some(name) = result.stdout.split_whitespace().next() {
            active.insert(name.to_owned());
        }
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
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Some(channel) = extract_toolchain_channel(&content) {
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

        // Get currently active versions per tool
        let active_versions = collect_active_mise_versions();

        let mut entries = Vec::new();

        // Each subdir is a tool (node, python, etc.), each subdir of that is a version
        let Ok(tools) = std::fs::read_dir(&installs_dir) else {
            return entries;
        };

        for tool_entry in tools.flatten() {
            let tool_path = tool_entry.path();
            if !tool_path.is_dir() {
                continue;
            }
            let tool_name = tool_entry.file_name().to_string_lossy().to_string();
            let active_for_tool = active_versions.get(&tool_name);

            let Ok(versions) = std::fs::read_dir(&tool_path) else {
                continue;
            };

            for version_entry in versions.flatten() {
                let ver_path = version_entry.path();
                let ver_name = version_entry.file_name().to_string_lossy().to_string();

                // Skip symlinks (mise uses them for aliases like "latest", "lts", etc.)
                if ver_path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
                    continue;
                }

                if !ver_path.is_dir() {
                    continue;
                }

                // Skip if this version is currently active
                if active_for_tool.is_some_and(|v| v.contains(&ver_name)) {
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

/// Collect active mise versions by running `mise current` or reading config files.
fn collect_active_mise_versions() -> rustc_hash::FxHashMap<String, FxHashSet<String>> {
    let mut active: rustc_hash::FxHashMap<String, FxHashSet<String>> =
        rustc_hash::FxHashMap::default();

    // Try `mise current` which outputs: "node  22.21.1  ~/.tool-versions"
    if let Ok(output) = std::process::Command::new("mise").arg("current").output() {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    active
                        .entry(parts[0].to_owned())
                        .or_default()
                        .insert(parts[1].to_owned());
                }
            }
        }
    }

    active
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
        let nvm_dir = home.join(".nvm/versions/node");

        if !nvm_dir.exists() {
            return Vec::new();
        }

        let active_version = cli_cache::get("node_version")
            .map(|r| r.stdout.trim().to_owned())
            .unwrap_or_default();

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&nvm_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let path = entry.path();

                if !path.is_dir() || name_str == active_version {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Node.js: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
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
        let pyenv_dir = home.join(".pyenv/versions");

        if !pyenv_dir.exists() {
            return Vec::new();
        }

        let active_version = cli_cache::get("python3_version")
            .and_then(|r| {
                // Output is "Python 3.x.y"
                r.stdout
                    .split_whitespace()
                    .nth(1)
                    .map(|v| v.trim().to_owned())
            })
            .unwrap_or_default();

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&pyenv_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let path = entry.path();

                // Skip symlinks (pyenv uses them for aliases)
                if path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
                    continue;
                }

                if !path.is_dir() || name_str == active_version {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Python: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
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
        let rbenv_dir = home.join(".rbenv/versions");

        if !rbenv_dir.exists() {
            return Vec::new();
        }

        let active_version = cli_cache::get("ruby_version")
            .and_then(|r| {
                // Output is "ruby 3.x.yp123 ..."
                r.stdout.split_whitespace().nth(1).map(|v| {
                    // Strip patch suffix like "p123"
                    v.split('p').next().unwrap_or(v).trim().to_owned()
                })
            })
            .unwrap_or_default();

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&rbenv_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let path = entry.path();

                if !path.is_dir() || name_str == active_version {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Ruby: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
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
