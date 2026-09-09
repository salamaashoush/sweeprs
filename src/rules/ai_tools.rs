use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

const MIN_SIZE: u64 = 1_048_576;

pub struct ClaudeCodeVersionsRule;
pub struct ClaudeDesktopVmRule;
pub struct ClaudeCachesRule;
pub struct AgentCliDataRule;

/// Resolve the version that `~/.local/bin/claude` currently points at.
///
/// The launcher symlinks straight into `versions/<semver>`, so the link target's
/// file name is the active version. Anything else under `versions/` is a
/// superseded install kept only for rollback.
fn active_claude_version(versions_dir: &Path) -> Option<String> {
    let launcher = dirs::home_dir()?.join(".local/bin/claude");
    let target = std::fs::read_link(&launcher).ok()?;

    let target = if target.is_absolute() {
        target
    } else {
        launcher.parent()?.join(target)
    };

    // Only trust the target if it really lives under the versions directory --
    // a Homebrew or npm install would point somewhere else entirely, and then
    // no version here is safe to call inactive.
    let canonical_versions = versions_dir.canonicalize().ok()?;
    let canonical_target = target.canonicalize().ok()?;
    if !canonical_target.starts_with(&canonical_versions) {
        return None;
    }

    canonical_target
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
}

impl CleanupRule for ClaudeCodeVersionsRule {
    fn name(&self) -> &'static str {
        "Claude Code versions"
    }

    fn category(&self) -> Category {
        Category::AiTools
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let versions = home.join(".local/share/claude/versions");

        // Without a resolvable active version every install here looks
        // inactive, and cleaning would delete the running CLI. Bail instead.
        let Some(active) = active_claude_version(&versions) else {
            return Vec::new();
        };

        let Ok(read_dir) = std::fs::read_dir(&versions) else {
            return Vec::new();
        };

        let mut entries = Vec::new();

        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == active {
                continue;
            }

            // A native install is a single self-contained binary per version;
            // the npm install is a directory. Both appear here.
            let path = entry.path();
            let size = if path.is_dir() {
                walker::dir_size(&path)
            } else {
                walker::file_size(&path)
            };
            if size == 0 {
                continue;
            }

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::AiTools,
                safety: SafetyLevel::Safe,
                description: format!("Claude Code {name} (superseded by {active})"),
                item_count: None,
            });
        }

        entries
    }
}

impl CleanupRule for ClaudeDesktopVmRule {
    fn name(&self) -> &'static str {
        "Claude Desktop VM"
    }

    fn category(&self) -> Category {
        Category::AiTools
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let support = home.join("Library/Application Support/Claude");

        let mut entries = Vec::new();

        // The Linux guest the desktop app runs its sandboxed sessions in. The
        // rootfs is a sparse image that only ever grows, so it stays large even
        // after the guest frees the space internally.
        let bundles = support.join("vm_bundles");
        if bundles.exists() {
            let size = walker::dir_size(&bundles);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: bundles,
                    size,
                    category: Category::AiTools,
                    safety: SafetyLevel::Caution,
                    description:
                        "Claude Desktop VM image (re-downloaded on next sandboxed session)"
                            .to_owned(),
                    item_count: None,
                });
            }
        }

        let vm_sdk = support.join("claude-code-vm");
        if vm_sdk.exists() {
            let size = walker::dir_size(&vm_sdk);
            if size > MIN_SIZE {
                entries.push(ScannedEntry {
                    path: vm_sdk,
                    size,
                    category: Category::AiTools,
                    safety: SafetyLevel::Caution,
                    description: "Claude Desktop VM guest SDK".to_owned(),
                    item_count: None,
                });
            }
        }

        entries
    }
}

impl CleanupRule for ClaudeCachesRule {
    fn name(&self) -> &'static str {
        "Claude caches"
    }

    fn category(&self) -> Category {
        Category::AiTools
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let support = home.join("Library/Application Support/Claude");

        let candidates = [
            (
                home.join("Library/Caches/claude-cli-nodejs"),
                "Claude Code CLI cache",
            ),
            (support.join("Cache"), "Claude Desktop cache"),
            (support.join("Code Cache"), "Claude Desktop code cache"),
            (support.join("GPUCache"), "Claude Desktop GPU cache"),
        ];

        let mut entries = Vec::new();

        for (path, description) in candidates {
            if !path.exists() {
                continue;
            }
            let size = walker::dir_size(&path);
            if size < MIN_SIZE {
                continue;
            }
            entries.push(ScannedEntry {
                path,
                size,
                category: Category::AiTools,
                safety: SafetyLevel::Safe,
                description: description.to_owned(),
                item_count: None,
            });
        }

        entries
    }
}

/// Re-downloadable payloads inside the agent CLI and editor dotfile dirs.
///
/// These directories are named individually rather than by their parent: a
/// dotfile root such as `~/.codex` or `~/.cursor` also holds `auth.json`,
/// `config.toml`, `mcp.json`, memories, rules and session history, none of
/// which can be recovered once deleted. Only the subdirectories the tool
/// re-fetches on demand belong here.
const AGENT_PAYLOAD_DIRS: &[(&str, &str)] = &[
    (".cursor/extensions", "Cursor extensions"),
    (".antigravity/extensions", "Antigravity extensions"),
    (".copilot/pkg", "Copilot CLI downloaded packages"),
    (".codex/cache", "Codex CLI cache"),
    (".codex/log", "Codex CLI logs"),
    (".codex/shell_snapshots", "Codex CLI shell snapshots"),
];

impl CleanupRule for AgentCliDataRule {
    fn name(&self) -> &'static str {
        "Agent CLI payloads"
    }

    fn category(&self) -> Category {
        Category::AiTools
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        for &(dir, description) in AGENT_PAYLOAD_DIRS {
            let path: PathBuf = home.join(dir);
            if !path.is_dir() {
                continue;
            }

            let size = walker::dir_size(&path);
            if size < MIN_SIZE {
                continue;
            }

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::AiTools,
                safety: SafetyLevel::Caution,
                description: description.to_owned(),
                item_count: None,
            });
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(ClaudeCodeVersionsRule),
        Box::new(ClaudeDesktopVmRule),
        Box::new(ClaudeCachesRule),
        Box::new(AgentCliDataRule),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_version_requires_link_inside_versions_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let versions = tmp.path().join("versions");
        std::fs::create_dir_all(versions.join("1.0.0")).expect("create version dir");

        // No launcher symlink resolvable in the test home -- must refuse to
        // guess rather than mark every version inactive.
        assert!(active_claude_version(&versions).is_none());
    }

    #[test]
    fn payload_dirs_never_name_a_dotfile_root() {
        // A bare `.codex` or `.cursor` would take auth.json, mcp.json, memories,
        // rules and session history with it.
        for (dir, _) in AGENT_PAYLOAD_DIRS {
            assert!(
                Path::new(dir).components().count() > 1,
                "{dir} is a dotfile root, not a re-downloadable payload"
            );
        }
    }
}
