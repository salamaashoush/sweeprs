use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;
use std::time::SystemTime;

cache_rule!(
    VsCodeCacheRule,
    "VS Code cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Application Support/Code/Cache",
    "Library/Application Support/Code/CachedData",
    "Library/Application Support/Code/CachedExtensions",
    ".config/Code/Cache",
    ".config/Code/CachedData",
    ".config/Code/CachedExtensionVSIXs"
);

cache_rule!(
    XcodeCacheRule,
    "Xcode caches",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.dt.Xcode"
);

pub struct JetBrainsCacheRule;

impl CleanupRule for JetBrainsCacheRule {
    fn name(&self) -> &'static str {
        "JetBrains caches"
    }

    fn category(&self) -> Category {
        Category::IdeCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        let prefixes = [
            "JetBrains",
            "IntelliJIdea",
            "WebStorm",
            "PyCharm",
            "CLion",
            "GoLand",
            "RustRover",
            "DataGrip",
            "Rider",
            "PhpStorm",
            "AndroidStudio",
        ];

        // macOS: ~/Library/Caches/JetBrains*
        let macos_caches = home.join("Library/Caches");
        scan_jetbrains_dir(&macos_caches, &prefixes, &mut entries);

        // Linux: ~/.cache/JetBrains*
        let linux_caches = home.join(".cache");
        scan_jetbrains_dir(&linux_caches, &prefixes, &mut entries);

        // Linux: ~/.local/share/JetBrains* (IDE configs/indices)
        let linux_local = home.join(".local/share");
        scan_jetbrains_dir(&linux_local, &prefixes, &mut entries);

        entries
    }
}

fn scan_jetbrains_dir(dir: &std::path::Path, prefixes: &[&str], entries: &mut Vec<ScannedEntry>) {
    if !dir.exists() {
        return;
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if prefixes.iter().any(|p| name_str.starts_with(p)) {
            let path = entry.path();
            if path.is_dir() {
                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::IdeCache,
                        safety: SafetyLevel::Safe,
                        description: format!("{name_str} cache"),
                        item_count: None,
                    });
                }
            }
        }
    }
}

cache_rule!(
    CursorCacheRule,
    "Cursor cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Application Support/Cursor/Cache",
    "Library/Application Support/Cursor/CachedData",
    ".config/Cursor/Cache",
    ".config/Cursor/CachedData"
);

cache_rule!(
    ZedCacheRule,
    "Zed cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Caches/dev.zed.Zed",
    ".cache/zed",
    ".local/share/zed"
);

cache_rule!(
    SublimeCacheRule,
    "Sublime Text cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Caches/com.sublimetext.4",
    ".cache/sublime-text"
);

cache_rule!(
    XcodeProductsRule,
    "Xcode Products",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/Products"
);

cache_rule!(
    XcodeIBSupportRule,
    "Xcode IB Support",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/UserData/IB Support"
);

cache_rule!(
    XcodeDocIndexRule,
    "Xcode DocumentationIndex",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/DocumentationIndex"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(VsCodeCacheRule),
        Box::new(XcodeCacheRule),
        Box::new(JetBrainsCacheRule),
        Box::new(CursorCacheRule),
        Box::new(ZedCacheRule),
        Box::new(SublimeCacheRule),
        Box::new(XcodeProductsRule),
        Box::new(XcodeIBSupportRule),
        Box::new(XcodeDocIndexRule),
        Box::new(OrphanedWorkspaceStorageRule),
        Box::new(EditorStateBackupRule),
        Box::new(EditorLocalHistoryRule),
    ]
}

/// Per-workspace state for a folder that is no longer on disk.
///
/// VS Code and its forks keep a directory of state per workspace -- search
/// history, extension state, and for the AI forks the chat sessions scoped to
/// that folder -- keyed by a hash, and never remove it when the folder goes
/// away. On a machine where projects are cloned and deleted, most of what is
/// here belongs to folders that stopped existing months ago.
pub struct OrphanedWorkspaceStorageRule;

const EDITOR_SUPPORT_DIRS: &[&str] = &[
    "Library/Application Support/Code",
    "Library/Application Support/Cursor",
    "Library/Application Support/Windsurf",
    "Library/Application Support/Antigravity",
    ".config/Code",
    ".config/Cursor",
    ".config/Windsurf",
];

/// The folder a workspace-storage directory belongs to, if it names one.
fn workspace_folder(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let raw = std::fs::read_to_string(dir.join("workspace.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let uri = json
        .get("folder")
        .or_else(|| json.get("workspace"))
        .and_then(serde_json::Value::as_str)?;
    let encoded = uri.strip_prefix("file://")?;
    Some(std::path::PathBuf::from(percent_decode(encoded)))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Whether the folder is gone rather than merely unreachable.
///
/// An unmounted volume or a disconnected network share makes every path under
/// it vanish at once. Requiring the folder's own parent to still be there
/// distinguishes "this one folder was deleted" from "this whole disk is not
/// plugged in", and only the first is safe to act on. Any surviving ancestor is
/// not enough: `/Volumes` outlives every disk that was ever mounted under it.
fn folder_is_gone(folder: &std::path::Path) -> bool {
    if folder.exists() {
        return false;
    }
    folder.parent().is_some_and(std::path::Path::exists)
}

impl CleanupRule for OrphanedWorkspaceStorageRule {
    fn name(&self) -> &'static str {
        "Orphaned editor workspace storage"
    }

    fn category(&self) -> Category {
        Category::IdeCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();

        EDITOR_SUPPORT_DIRS
            .iter()
            .filter_map(|support| {
                let storage = home.join(support).join("User/workspaceStorage");
                let editor = std::path::Path::new(support)
                    .file_name()?
                    .to_string_lossy()
                    .to_string();
                Some((storage, editor))
            })
            .flat_map(|(storage, editor)| {
                let Ok(read_dir) = std::fs::read_dir(&storage) else {
                    return Vec::new();
                };
                read_dir
                    .flatten()
                    .filter_map(|entry| {
                        let path = entry.path();
                        let folder = workspace_folder(&path)?;
                        if !folder_is_gone(&folder) {
                            return None;
                        }
                        let size = walker::dir_size(&path);
                        if size == 0 {
                            return None;
                        }
                        Some(ScannedEntry {
                            path,
                            size,
                            category: Category::IdeCache,
                            safety: SafetyLevel::Caution,
                            description: format!(
                                "{editor} workspace state for deleted {}",
                                crate::util::tilde_path(&folder)
                            ),
                            item_count: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// The rollback copy the editor keeps of its global state database.
///
/// `state.vscdb.backup` is written before each rewrite of `state.vscdb` and is
/// only ever read if the live database fails to open. It matches the live file
/// byte for byte, so on a long-lived profile it is one of the largest single
/// files in the whole application-support tree.
pub struct EditorStateBackupRule;

impl CleanupRule for EditorStateBackupRule {
    fn name(&self) -> &'static str {
        "Editor state database backups"
    }

    fn category(&self) -> Category {
        Category::IdeCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();

        EDITOR_SUPPORT_DIRS
            .iter()
            .filter_map(|support| {
                let backup = home
                    .join(support)
                    .join("User/globalStorage/state.vscdb.backup");
                let size = walker::file_size(&backup);
                if size == 0 {
                    return None;
                }
                let editor = std::path::Path::new(support)
                    .file_name()?
                    .to_string_lossy()
                    .to_string();
                Some(ScannedEntry {
                    path: backup,
                    size,
                    category: Category::IdeCache,
                    safety: SafetyLevel::Caution,
                    description: format!(
                        "{editor} state database rollback copy (only read if the live database fails)"
                    ),
                    item_count: None,
                })
            })
            .collect()
    }
}

/// The editor's own file history, kept independently of git.
///
/// This is what the Timeline view restores from: local revisions of files you
/// edited. It is stored as one small directory per tracked file, thousands of
/// them, so it is reported as a single entry per editor rather than as a wall
/// of individually worthless items. Cleaning removes only the entries past the
/// cutoff, leaving recent history intact -- which is why the entry stands for
/// that operation instead of for the directory.
pub struct EditorLocalHistoryRule;

/// Per-file history directories under `history` untouched for longer than `cutoff`.
fn aged_history_entries(
    history: &std::path::Path,
    cutoff: std::time::Duration,
) -> (Vec<std::path::PathBuf>, usize) {
    let Ok(read_dir) = std::fs::read_dir(history) else {
        return (Vec::new(), 0);
    };

    let mut aged = Vec::new();
    let mut total = 0;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        total += 1;
        let idle = path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok());
        if idle.is_some_and(|idle| idle >= cutoff) {
            aged.push(path);
        }
    }
    (aged, total)
}

impl CleanupRule for EditorLocalHistoryRule {
    fn name(&self) -> &'static str {
        "Editor local file history"
    }

    fn category(&self) -> Category {
        Category::IdeCache
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let cutoff = std::time::Duration::from_secs(config.categories.agent_session_days * 86_400);

        EDITOR_SUPPORT_DIRS
            .iter()
            .filter_map(|support| {
                let history = home.join(support).join("User/History");
                let (aged, total) = aged_history_entries(&history, cutoff);
                if aged.is_empty() {
                    return None;
                }

                let size: u64 = aged.iter().map(|p| walker::dir_size(p)).sum();
                if size == 0 {
                    return None;
                }

                let editor = std::path::Path::new(support)
                    .file_name()?
                    .to_string_lossy()
                    .to_string();
                let days = config.categories.agent_session_days;
                Some(ScannedEntry {
                    path: std::path::PathBuf::from(format!(
                        "editor-history:{}",
                        history.display()
                    )),
                    size,
                    category: Category::IdeCache,
                    safety: SafetyLevel::Caution,
                    description: format!(
                        "{editor} local file history: {} of {total} files untouched for {days}+ days",
                        aged.len()
                    ),
                    item_count: Some(aged.len()),
                })
            })
            .collect()
    }
}

/// Remove the aged half of an editor's local history, keeping the rest.
pub fn clean_editor_history(entry_path: &str, config: &Config) -> std::io::Result<Option<u64>> {
    let history = entry_path
        .strip_prefix("editor-history:")
        .unwrap_or(entry_path);
    let cutoff = std::time::Duration::from_secs(config.categories.agent_session_days * 86_400);

    let (aged, _) = aged_history_entries(std::path::Path::new(history), cutoff);
    if aged.is_empty() {
        return Ok(Some(0));
    }

    let mut freed = 0u64;
    let mut first_error = None;
    for dir in &aged {
        let size = walker::dir_size(dir);
        match std::fs::remove_dir_all(dir) {
            Ok(()) => freed += size,
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }

    match first_error {
        Some(e) if freed == 0 => Err(e),
        _ => Ok(Some(freed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn workspace_uris_are_percent_decoded() {
        assert_eq!(
            percent_decode("/Users/me/My%20Project"),
            "/Users/me/My Project"
        );
        assert_eq!(percent_decode("/plain/path"), "/plain/path");
        // A stray percent must not swallow the rest of the path.
        assert_eq!(percent_decode("/a%zz/b"), "/a%zz/b");
    }

    #[test]
    fn an_unmounted_volume_is_not_treated_as_a_deleted_folder() {
        // Nothing under this path exists, which is what a detached disk looks
        // like -- reclaiming its workspace state would be wrong.
        assert!(!folder_is_gone(Path::new(
            "/Volumes/NotPluggedIn/code/project"
        )));
    }

    #[test]
    fn history_cleanup_spares_entries_inside_the_cutoff() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let history = tmp.path();
        std::fs::create_dir(history.join("fresh")).expect("fresh");
        std::fs::write(history.join("fresh/1.ts"), b"x").expect("write");
        std::fs::write(history.join("entries.json"), b"{}").expect("index");

        // Just-written entries are inside any real cutoff.
        let (aged, total) = aged_history_entries(history, std::time::Duration::from_secs(86_400));
        assert_eq!(aged, Vec::<std::path::PathBuf>::new());
        // Loose files at the top level are not per-file history directories.
        assert_eq!(total, 1);
    }

    #[test]
    fn a_deleted_folder_under_a_live_parent_is_gone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(folder_is_gone(&tmp.path().join("deleted-project")));
        assert!(!folder_is_gone(tmp.path()));
    }
}
