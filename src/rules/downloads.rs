use std::time::SystemTime;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

pub struct OldDownloadsRule;

impl CleanupRule for OldDownloadsRule {
    fn name(&self) -> &'static str {
        "Old Downloads"
    }

    fn category(&self) -> Category {
        Category::OldDownload
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let downloads = home.join("Downloads");

        if !downloads.exists() {
            return Vec::new();
        }

        let age_threshold_secs = config.categories.download_age_days * 24 * 60 * 60;
        let now = SystemTime::now();
        let mut entries = Vec::new();

        let Ok(read_dir) = std::fs::read_dir(&downloads) else {
            return Vec::new();
        };

        for entry in read_dir.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            let modified = metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH);

            let age = now.duration_since(modified).unwrap_or_default();

            if age.as_secs() >= age_threshold_secs {
                let size = if metadata.is_dir() {
                    crate::scanner::walker::dir_size(&path)
                } else {
                    metadata.len()
                };

                if size > 0 {
                    let days_old = age.as_secs() / (24 * 60 * 60);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::OldDownload,
                        safety: SafetyLevel::Caution,
                        description: format!("{name} ({days_old} days old)"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(OldDownloadsRule)]
}
