use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// -- Simple cache_rule! rules --

cache_rule!(
    QuickLookCacheRule,
    "QuickLook thumbnails",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.QuickLook.thumbnailcache"
);

cache_rule!(
    MailAttachmentsRule,
    "Mail attachments",
    Category::SystemJunk,
    SafetyLevel::Caution,
    "Library/Containers/com.apple.mail/Data/Library/Mail Downloads"
);

cache_rule!(
    MailDataRule,
    "Mail data cache",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.mail"
);

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

        // Get the user's TMPDIR as the primary temp location
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

        // Also scan /private/var/folders for user-owned temp dirs
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
                            // Skip if this is the same as TMPDIR (already scanned)
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
                            if t_dir.exists() && t_dir.is_dir() {
                                // Only scan dirs we can read (i.e., our own)
                                if std::fs::read_dir(&t_dir).is_ok() {
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

        entries
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

pub struct FontCacheRule;

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

pub struct SpotlightIndexRule;

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

pub struct DmgInstallerRule;

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
    vec![
        Box::new(SystemTempRule),
        Box::new(QuickLookCacheRule),
        Box::new(FontCacheRule),
        Box::new(SpotlightIndexRule),
        Box::new(MailAttachmentsRule),
        Box::new(MailDataRule),
        Box::new(DmgInstallerRule),
        Box::new(UpdateCacheRule),
    ]
}
