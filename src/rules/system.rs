use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

#[cfg(target_os = "macos")]
use crate::rules::cache_rule;

// -- macOS-only cache_rule! rules --

#[cfg(target_os = "macos")]
cache_rule!(
    QuickLookCacheRule,
    "QuickLook thumbnails",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.QuickLook.thumbnailcache"
);

#[cfg(target_os = "macos")]
cache_rule!(
    MailAttachmentsRule,
    "Mail attachments",
    Category::SystemJunk,
    SafetyLevel::Caution,
    "Library/Containers/com.apple.mail/Data/Library/Mail Downloads"
);

#[cfg(target_os = "macos")]
cache_rule!(
    MailDataRule,
    "Mail data cache",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.mail"
);

#[cfg(target_os = "macos")]
cache_rule!(
    UpdateCacheRule,
    "macOS Update cache",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.SoftwareUpdate"
);

// -- Custom rules --

pub struct SystemTempRule;

impl CleanupRule for SystemTempRule {
    fn name(&self) -> &'static str {
        "System temp files"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();
        let one_hour_ago = SystemTime::now()
            .checked_sub(Duration::from_secs(3600))
            .unwrap_or(SystemTime::UNIX_EPOCH);

        // Scan user's TMPDIR (cross-platform)
        if let Ok(tmpdir) = std::env::var("TMPDIR") {
            let tmpdir_path = PathBuf::from(&tmpdir);
            if tmpdir_path.exists() {
                let size = scan_old_files(&tmpdir_path, one_hour_ago);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: tmpdir_path,
                        size,
                        category: Category::SystemJunk,
                        safety: SafetyLevel::Caution,
                        description: "User temp files".to_owned(),
                        item_count: None,
                    });
                }
            }
        }

        if cfg!(target_os = "macos") {
            scan_macos_temp(&mut entries, one_hour_ago);
        } else {
            scan_linux_temp(&mut entries, one_hour_ago);
        }

        entries
    }
}

fn scan_macos_temp(entries: &mut Vec<ScannedEntry>, one_hour_ago: SystemTime) {
    // /private/tmp (aka /tmp on macOS)
    let private_tmp = PathBuf::from("/private/tmp");
    if private_tmp.exists() && private_tmp.is_dir() {
        let private_tmp_canonical = private_tmp
            .canonicalize()
            .unwrap_or_else(|_| private_tmp.clone());
        let already_scanned = std::env::var("TMPDIR")
            .ok()
            .and_then(|t| PathBuf::from(&t).canonicalize().ok())
            .is_some_and(|c| c == private_tmp_canonical);

        if !already_scanned && std::fs::read_dir(&private_tmp).is_ok() {
            let size = scan_old_files(&private_tmp, one_hour_ago);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: private_tmp,
                    size,
                    category: Category::SystemJunk,
                    safety: SafetyLevel::Caution,
                    description: "System temp files (/private/tmp)".to_owned(),
                    item_count: None,
                });
            }
        }
    }

    // /private/var/folders - macOS user temp dirs
    let var_folders = PathBuf::from("/private/var/folders");
    if var_folders.exists() {
        if let Ok(top_dirs) = std::fs::read_dir(&var_folders) {
            for top in top_dirs.flatten() {
                if !top.path().is_dir() {
                    continue;
                }
                if let Ok(sub_dirs) = std::fs::read_dir(top.path()) {
                    for sub in sub_dirs.flatten() {
                        let t_dir = sub.path().join("T");
                        if let Ok(tmpdir) = std::env::var("TMPDIR") {
                            let tmpdir_path = PathBuf::from(&tmpdir);
                            let tmpdir_canonical = tmpdir_path
                                .canonicalize()
                                .unwrap_or_else(|_| tmpdir_path.clone());
                            let t_canonical =
                                t_dir.canonicalize().unwrap_or_else(|_| t_dir.clone());
                            if tmpdir_canonical == t_canonical {
                                continue;
                            }
                        }
                        if t_dir.exists() && t_dir.is_dir() && std::fs::read_dir(&t_dir).is_ok() {
                            let size = scan_old_files(&t_dir, one_hour_ago);
                            if size > 0 {
                                entries.push(ScannedEntry {
                                    path: t_dir,
                                    size,
                                    category: Category::SystemJunk,
                                    safety: SafetyLevel::Caution,
                                    description: "Temp files in /var/folders".to_owned(),
                                    item_count: None,
                                });
                            }
                        }
                    }
                }
            }
        }
    }
}

fn scan_linux_temp(entries: &mut Vec<ScannedEntry>, one_hour_ago: SystemTime) {
    // /tmp - shared temp (may be tmpfs, but not always)
    let tmp = PathBuf::from("/tmp");
    if tmp.exists() && tmp.is_dir() {
        let tmp_canonical = tmp.canonicalize().unwrap_or_else(|_| tmp.clone());
        let already_scanned = std::env::var("TMPDIR")
            .ok()
            .and_then(|t| PathBuf::from(&t).canonicalize().ok())
            .is_some_and(|c| c == tmp_canonical);

        if !already_scanned && std::fs::read_dir(&tmp).is_ok() {
            let size = scan_old_files(&tmp, one_hour_ago);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: tmp,
                    size,
                    category: Category::SystemJunk,
                    safety: SafetyLevel::Caution,
                    description: "System temp files (/tmp)".to_owned(),
                    item_count: None,
                });
            }
        }
    }

    // /var/tmp - persistent temp (survives reboots)
    let var_tmp = PathBuf::from("/var/tmp");
    if var_tmp.exists() && var_tmp.is_dir() && std::fs::read_dir(&var_tmp).is_ok() {
        let size = scan_old_files(&var_tmp, one_hour_ago);
        if size > 0 {
            entries.push(ScannedEntry {
                path: var_tmp,
                size,
                category: Category::SystemJunk,
                safety: SafetyLevel::Caution,
                description: "Persistent temp files (/var/tmp)".to_owned(),
                item_count: None,
            });
        }
    }
}

fn scan_old_files(dir: &std::path::Path, older_than: SystemTime) -> u64 {
    let mut total = 0u64;
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if let Ok(meta) = path.symlink_metadata() {
                let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if modified < older_than {
                    if meta.is_dir() {
                        total += walker::dir_size(&path);
                    } else {
                        total += meta.len();
                    }
                }
            }
        }
    }
    total
}

#[cfg(target_os = "macos")]
pub struct FontCacheRule;

#[cfg(target_os = "macos")]
impl CleanupRule for FontCacheRule {
    fn name(&self) -> &'static str {
        "Font caches"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();
        let var_folders = PathBuf::from("/private/var/folders");

        if !var_folders.exists() {
            return entries;
        }

        if let Ok(top_dirs) = std::fs::read_dir(&var_folders) {
            for top in top_dirs.flatten() {
                if !top.path().is_dir() {
                    continue;
                }
                if let Ok(sub_dirs) = std::fs::read_dir(top.path()) {
                    for sub in sub_dirs.flatten() {
                        let c_dir = sub.path().join("C");
                        if !c_dir.exists() || !c_dir.is_dir() {
                            continue;
                        }
                        if let Ok(c_entries) = std::fs::read_dir(&c_dir) {
                            for c_entry in c_entries.flatten() {
                                let name = c_entry.file_name();
                                let name_str = name.to_string_lossy();
                                if name_str.contains("com.apple.FontRegistry") {
                                    let path = c_entry.path();
                                    if path.is_dir() {
                                        let size = walker::dir_size(&path);
                                        if size > 0 {
                                            entries.push(ScannedEntry {
                                                path,
                                                size,
                                                category: Category::SystemJunk,
                                                safety: SafetyLevel::Safe,
                                                description: "Font cache".to_owned(),
                                                item_count: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        entries
    }
}

#[cfg(target_os = "macos")]
pub struct SpotlightIndexRule;

#[cfg(target_os = "macos")]
impl CleanupRule for SpotlightIndexRule {
    fn name(&self) -> &'static str {
        "Spotlight index"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let spotlight_dir = PathBuf::from("/.Spotlight-V100");

        if !spotlight_dir.exists() {
            return Vec::new();
        }

        let size = walker::dir_size(&spotlight_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: spotlight_dir,
            size,
            category: Category::SystemJunk,
            safety: SafetyLevel::Caution,
            description: "Spotlight index (requires sudo to delete)".to_owned(),
            item_count: None,
        }]
    }
}

#[cfg(target_os = "macos")]
pub struct DmgInstallerRule;

#[cfg(target_os = "macos")]
impl CleanupRule for DmgInstallerRule {
    fn name(&self) -> &'static str {
        "DMG/PKG installers"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let downloads_dir = home.join("Downloads");

        if !downloads_dir.exists() {
            return Vec::new();
        }

        let extensions = [".dmg", ".pkg", ".iso"];
        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&downloads_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                if extensions.iter().any(|ext| name.ends_with(ext)) {
                    if let Ok(meta) = path.metadata() {
                        let size = meta.len();
                        if size > 0 {
                            let file_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            entries.push(ScannedEntry {
                                path,
                                size,
                                category: Category::SystemJunk,
                                safety: SafetyLevel::Caution,
                                description: format!("Installer: {file_name}"),
                                item_count: None,
                            });
                        }
                    }
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    #[allow(unused_mut)]
    let mut rules: Vec<Box<dyn CleanupRule>> = vec![Box::new(SystemTempRule)];

    #[cfg(target_os = "macos")]
    {
        rules.push(Box::new(QuickLookCacheRule));
        rules.push(Box::new(FontCacheRule));
        rules.push(Box::new(SpotlightIndexRule));
        rules.push(Box::new(MailAttachmentsRule));
        rules.push(Box::new(MailDataRule));
        rules.push(Box::new(DmgInstallerRule));
        rules.push(Box::new(UpdateCacheRule));
    }

    rules
}
