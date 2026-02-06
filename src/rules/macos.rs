use crate::config::Config;
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
        let Some(result) = cli_cache::get("tmutil_snapshots") else {
            return Vec::new();
        };

        let snapshot_count = result
            .stdout
            .lines()
            .filter(|l| l.contains("com.apple."))
            .count();

        if snapshot_count == 0 {
            return Vec::new();
        }

        let size = parse_snapshot_sizes();

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

/// Parse total snapshot size from `diskutil apfs list` output.
fn parse_snapshot_sizes() -> u64 {
    let Some(result) = cli_cache::get("diskutil_apfs_list") else {
        return 0;
    };

    let mut total = 0u64;
    let lines: Vec<&str> = result.stdout.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Snapshot Name:") && line.contains("com.apple.TimeMachine") {
            for following in &lines[i + 1..] {
                if following.contains("Snapshot Disk Size:") {
                    if let Some(start) = following.find('(') {
                        if let Some(end) = following[start..].find(" Bytes)") {
                            if let Ok(bytes) =
                                following[start + 1..start + end].trim().parse::<u64>()
                            {
                                total += bytes;
                            }
                        }
                    }
                    break;
                }
                if following.contains("Snapshot Name:") {
                    break;
                }
            }
        }
    }
    total
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

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(TimeMachineSnapshotsRule),
        Box::new(XcodeSimulatorsRule),
        Box::new(XcodeArchivesRule),
        Box::new(AppleMusicCacheRule),
        Box::new(PhotosFaceCacheRule),
        Box::new(PodcastCacheRule),
    ]
}
