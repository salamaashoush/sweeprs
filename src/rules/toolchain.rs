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
        let toolchains_dir = home.join(".rustup/toolchains");

        if !toolchains_dir.exists() {
            return Vec::new();
        }

        let active_toolchain = cli_cache::get("rustup_active_toolchain")
            .map(|r| r.stdout.split_whitespace().next().unwrap_or("").to_owned())
            .unwrap_or_default();

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&toolchains_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let path = entry.path();

                if !path.is_dir() || name_str == active_toolchain {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Toolchain: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

pub struct NodeVersionsRule;

impl CleanupRule for NodeVersionsRule {
    fn name(&self) -> &'static str {
        "Old Node.js versions"
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
        let jvm_dir = std::path::PathBuf::from("/Library/Java/JavaVirtualMachines");

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
        Box::new(NodeVersionsRule),
        Box::new(PythonVersionsRule),
        Box::new(RubyVersionsRule),
        Box::new(JavaVersionsRule),
    ]
}
