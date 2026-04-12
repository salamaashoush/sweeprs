use std::path::Path;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

const MIN_SIZE: u64 = 1_048_576; // 1 MiB

const ELECTRON_APPS: &[(&str, &str)] = &[
    ("Slack", "Slack"),
    ("Discord", "discord"),
    ("VS Code", "Code"),
    ("Cursor", "Cursor"),
    ("Teams", "Microsoft/Teams"),
];

const ELECTRON_SUBDIRS: &[&str] = &[
    "Local Storage",
    "IndexedDB",
    "GPUCache",
    "blob_storage",
    "Session Storage",
    "Service Worker",
];

pub struct ElectronAppDataRule;

impl CleanupRule for ElectronAppDataRule {
    fn name(&self) -> &'static str {
        "Electron app data"
    }

    fn category(&self) -> Category {
        Category::AppCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // macOS: ~/Library/Application Support/<app>
        let macos_dir = home.join("Library/Application Support");
        if macos_dir.exists() {
            for &(app_name, app_dir) in ELECTRON_APPS {
                let base = macos_dir.join(app_dir);
                if !base.exists() {
                    continue;
                }
                for &subdir in ELECTRON_SUBDIRS {
                    scan_subdir(&mut entries, &base, subdir, app_name);
                }
            }
        }

        // Linux: ~/.config/<app>
        let linux_dir = home.join(".config");
        if linux_dir.exists() {
            for &(app_name, app_dir) in ELECTRON_APPS {
                let base = linux_dir.join(app_dir);
                if !base.exists() {
                    continue;
                }
                for &subdir in ELECTRON_SUBDIRS {
                    scan_subdir(&mut entries, &base, subdir, app_name);
                }
            }
        }

        entries
    }
}

fn scan_subdir(entries: &mut Vec<ScannedEntry>, base: &Path, subdir: &str, app: &str) {
    let path = base.join(subdir);
    if !path.exists() {
        return;
    }

    let size = walker::dir_size(&path);
    if size < MIN_SIZE {
        return;
    }

    entries.push(ScannedEntry {
        path,
        size,
        category: Category::AppCache,
        safety: SafetyLevel::Safe,
        description: format!("{app} {subdir}"),
        item_count: None,
    });
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(ElectronAppDataRule)]
}
