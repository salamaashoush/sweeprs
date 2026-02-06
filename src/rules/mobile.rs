use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// -- cache_rule! rules --

cache_rule!(
    XcodeDeviceSupportRule,
    "Xcode Device Support",
    Category::MobileBackup,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/iOS DeviceSupport",
    "Library/Developer/Xcode/watchOS DeviceSupport"
);

cache_rule!(
    XcodeDocSetsRule,
    "Xcode Documentation",
    Category::MobileBackup,
    SafetyLevel::Safe,
    "Library/Developer/Shared/Documentation/DocSets"
);

// -- Custom rule --

pub struct IosBackupRule;

impl CleanupRule for IosBackupRule {
    fn name(&self) -> &'static str {
        "iOS/iPadOS Backups"
    }

    fn category(&self) -> Category {
        Category::MobileBackup
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let backup_dir = home.join("Library/Application Support/MobileSync/Backup");

        if !backup_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&backup_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size == 0 {
                    continue;
                }

                let dir_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                // Try to extract a useful date from the backup's Info.plist
                let description = format!("Device backup: {dir_name}");

                entries.push(ScannedEntry {
                    path,
                    size,
                    category: Category::MobileBackup,
                    safety: SafetyLevel::Caution,
                    description,
                    item_count: None,
                });
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(IosBackupRule),
        Box::new(XcodeDeviceSupportRule),
        Box::new(XcodeDocSetsRule),
    ]
}
