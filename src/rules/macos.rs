use crate::config::Config;
use crate::platform;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::cli_cache;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

pub struct TimeMachineSnapshotsRule;

impl CleanupRule for TimeMachineSnapshotsRule {
    fn name(&self) -> &'static str {
        "Time Machine local snapshots"
    }

    fn category(&self) -> Category {
        Category::MacosSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let raw = cli_cache::get_raw("tmutil_snapshots");
        let Some(result) = raw else {
            return Vec::new();
        };
        if !result.success {
            let stderr = result.stderr.trim();
            let msg =
                if stderr.contains("requires root") || stderr.contains("Operation not permitted") {
                    "tmutil failed (requires root privileges)"
                } else if stderr.is_empty() {
                    "tmutil failed (timed out or not available)"
                } else {
                    "tmutil failed (check Time Machine configuration)"
                };
            return vec![ScannedEntry {
                path: std::path::PathBuf::from("/Time Machine Snapshots"),
                size: 0,
                category: Category::MacosSpecific,
                safety: SafetyLevel::Error,
                description: msg.to_owned(),
                item_count: None,
            }];
        }

        let snapshot_count = result
            .stdout
            .lines()
            .filter(|l| l.contains("com.apple."))
            .count();

        if snapshot_count == 0 {
            return Vec::new();
        }

        let size = platform::parse_snapshot_bytes();

        vec![ScannedEntry {
            path: std::path::PathBuf::from("/Time Machine Snapshots"),
            size,
            category: Category::MacosSpecific,
            safety: SafetyLevel::Caution,
            description: format!("{snapshot_count} local Time Machine snapshot(s)"),
            item_count: Some(snapshot_count),
        }]
    }
}

pub struct XcodeSimulatorsRule;

impl CleanupRule for XcodeSimulatorsRule {
    fn name(&self) -> &'static str {
        "Xcode Simulators"
    }

    fn category(&self) -> Category {
        Category::MacosSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let sims_dir = home.join("Library/Developer/CoreSimulator/Devices");

        if !sims_dir.exists() {
            return Vec::new();
        }

        let (size, device_count) = walker::dir_size_and_count(&sims_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: sims_dir,
            size,
            category: Category::MacosSpecific,
            safety: SafetyLevel::Caution,
            description: format!("Xcode Simulators ({device_count} devices)"),
            item_count: Some(device_count),
        }]
    }
}

pub struct XcodeArchivesRule;

impl CleanupRule for XcodeArchivesRule {
    fn name(&self) -> &'static str {
        "Xcode Archives"
    }

    fn category(&self) -> Category {
        Category::MacosSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let archives_dir = home.join("Library/Developer/Xcode/Archives");

        if !archives_dir.exists() {
            return Vec::new();
        }

        let (size, count) = walker::dir_size_and_count(&archives_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: archives_dir,
            size,
            category: Category::MacosSpecific,
            safety: SafetyLevel::Caution,
            description: "Xcode Archives".to_owned(),
            item_count: Some(count),
        }]
    }
}

cache_rule!(
    AppleMusicCacheRule,
    "Apple Music cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.Music"
);

cache_rule!(
    PhotosFaceCacheRule,
    "Photos face cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Containers/com.apple.Photos/Data/Library/Caches"
);

cache_rule!(
    PodcastCacheRule,
    "Podcasts cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.podcasts"
);

cache_rule!(
    XcodePlaygroundRule,
    "Xcode Playground data",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Developer/XCPGDevices"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(TimeMachineSnapshotsRule),
        Box::new(XcodeSimulatorsRule),
        Box::new(XcodeArchivesRule),
        Box::new(AppleMusicCacheRule),
        Box::new(PhotosFaceCacheRule),
        Box::new(PodcastCacheRule),
        Box::new(XcodePlaygroundRule),
    ]
}
