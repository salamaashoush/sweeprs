use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// Android SDK caches and build intermediates
cache_rule!(
    AndroidSdkCacheRule,
    "Android SDK cache",
    Category::BuildArtifact,
    SafetyLevel::Caution,
    "Library/Android/sdk/system-images"
);

cache_rule!(
    AndroidGradleWrapperRule,
    "Gradle wrapper distributions",
    Category::BuildArtifact,
    SafetyLevel::Safe,
    ".gradle/wrapper/dists"
);

cache_rule!(
    AndroidGradleDaemonRule,
    "Gradle daemon logs",
    Category::LogFile,
    SafetyLevel::Safe,
    ".gradle/daemon"
);

/// Scan Android AVD (emulator) images.
pub struct AndroidAvdRule;

impl CleanupRule for AndroidAvdRule {
    fn name(&self) -> &'static str {
        "Android AVDs"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let avd_dir = home.join(".android/avd");

        if !avd_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&avd_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                // AVDs are stored as .avd directories
                if path.is_dir() && path.extension().is_some_and(|ext| ext == "avd") {
                    let name = path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let size = walker::dir_size(&path);
                    if size > 0 {
                        entries.push(ScannedEntry {
                            path,
                            size,
                            category: Category::BuildArtifact,
                            safety: SafetyLevel::Caution,
                            description: format!("AVD: {name}"),
                            item_count: None,
                        });
                    }
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(AndroidSdkCacheRule),
        Box::new(AndroidGradleWrapperRule),
        Box::new(AndroidGradleDaemonRule),
        Box::new(AndroidAvdRule),
    ]
}
