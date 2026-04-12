use std::path::PathBuf;

use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// ---------------------------------------------------------------------------
// Simple cache_rule! based rules
// ---------------------------------------------------------------------------

// iMessage / Messages attachments -- media shared via iMessage accumulates
// silently and iCloud sync keeps local copies even with "optimize storage".
cache_rule!(
    MessagesAttachmentsRule,
    "iMessage attachments",
    Category::MacosSpecific,
    SafetyLevel::Caution,
    "Library/Messages/Attachments"
);

cache_rule!(
    MessagesCacheRule,
    "iMessage cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Messages/Caches"
);

// Saved Application State -- window positions and unsaved state for every app.
// Rebuilds automatically on app launch.
cache_rule!(
    SavedAppStateRule,
    "Saved Application State",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Saved Application State"
);

// App Store caches
cache_rule!(
    AppStoreCacheRule,
    "App Store cache",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.appstore",
    "Library/Caches/com.apple.appstoreagent"
);

// Apple Maps / GeoServices cache
cache_rule!(
    MapsCacheRule,
    "Apple Maps cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Caches/GeoServices",
    "Library/Caches/com.apple.Maps"
);

// Speech / Siri / Dictation caches
cache_rule!(
    SpeechCacheRule,
    "Speech & Siri cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.SpeechRecognitionCore",
    "Library/Caches/VoiceServices",
    "Library/Caches/com.apple.parsec",
    "Library/Caches/com.apple.parsecd"
);

// NuGet / .NET package cache
cache_rule!(
    NugetCacheRule,
    "NuGet package cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".nuget/packages"
);

cache_rule!(
    DotnetCacheRule,
    ".NET SDK cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".dotnet"
);

// Unity editor GI cache -- can be 5-30 GB, rebuilds when projects open.
cache_rule!(
    UnityCacheRule,
    "Unity editor cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.unity3d.UnityEditor",
    "Library/Unity"
);

// Steam caches
cache_rule!(
    SteamCacheRule,
    "Steam cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.valvesoftware.steam"
);

cache_rule!(
    SteamShaderCacheRule,
    "Steam shader cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Steam/steamapps/shadercache"
);

// Epic Games Launcher cache
cache_rule!(
    EpicGamesCacheRule,
    "Epic Games Launcher cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.epicgames.EpicGamesLauncher"
);

// GarageBand / Logic Pro sound libraries and loops.
// These persist even after uninstalling the app.
cache_rule!(
    GarageBandCacheRule,
    "GarageBand data",
    Category::MacosSpecific,
    SafetyLevel::Caution,
    "Library/Application Support/GarageBand",
    "Library/Caches/com.apple.garageband10"
);

cache_rule!(
    LogicProCacheRule,
    "Logic Pro data",
    Category::MacosSpecific,
    SafetyLevel::Caution,
    "Library/Application Support/Logic",
    "Library/Caches/com.apple.logic10"
);

// Shared Apple audio loops (GarageBand + Logic Pro)
cache_rule!(
    AppleLoopsRule,
    "Apple audio loops",
    Category::MacosSpecific,
    SafetyLevel::Caution,
    "Library/Audio/Apple Loops"
);

// Video transcoding cache from Photos
cache_rule!(
    PhotosTranscodeCacheRule,
    "Photos transcode cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Containers/com.apple.Photos/Data/Library/Caches/com.apple.Transcode"
);

// QuickLook daemon cache (beyond the thumbnail cache already covered in system.rs)
cache_rule!(
    QuickLookDaemonCacheRule,
    "QuickLook daemon cache",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.quicklookd"
);

// NSURLSession / network caches
cache_rule!(
    NsurlSessionCacheRule,
    "Network session cache",
    Category::SystemJunk,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.nsurlsessiond"
);

// iCloud container caches (bird is the iCloud sync daemon)
cache_rule!(
    ICloudCacheRule,
    "iCloud sync cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.bird",
    "Library/Caches/CloudKit"
);

// ---------------------------------------------------------------------------
// Orphaned Containers -- sandboxed app data that persists after app removal
// ---------------------------------------------------------------------------

pub struct OrphanedContainersRule;

impl CleanupRule for OrphanedContainersRule {
    fn name(&self) -> &'static str {
        "Orphaned app containers"
    }

    fn category(&self) -> Category {
        Category::MacosSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let containers_dir = home.join("Library/Containers");

        if !containers_dir.exists() {
            return Vec::new();
        }

        // Build a set of installed app bundle IDs by scanning /Applications
        // and ~/Applications for .app bundles and reading their Info.plist.
        let installed_bundles = collect_installed_bundle_ids();

        let mut entries = Vec::new();
        let Ok(read_dir) = std::fs::read_dir(&containers_dir) else {
            return entries;
        };

        for entry_result in read_dir.flatten() {
            let path = entry_result.path();
            if !path.is_dir() {
                continue;
            }

            let dirname = match path.file_name() {
                Some(n) => n.to_string_lossy().into_owned(),
                None => continue,
            };

            // Skip Apple system containers (com.apple.*)
            if dirname.starts_with("com.apple.") {
                continue;
            }

            // Check if the corresponding app is still installed
            if installed_bundles.contains(&dirname) {
                continue;
            }

            let size = walker::dir_size(&path);
            // Only report containers > 10 MiB to reduce noise
            if size < 10_000_000 {
                continue;
            }

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::MacosSpecific,
                safety: SafetyLevel::Caution,
                description: format!("Orphaned container: {dirname} (app not installed)"),
                item_count: None,
            });
        }

        entries
    }
}

/// Collect bundle identifiers of all installed applications.
fn collect_installed_bundle_ids() -> std::collections::HashSet<String> {
    let mut bundles = std::collections::HashSet::new();

    let app_dirs = [
        PathBuf::from("/Applications"),
        dirs::home_dir()
            .unwrap_or_default()
            .join("Applications"),
    ];

    for app_dir in &app_dirs {
        collect_bundles_from(app_dir, &mut bundles, 2);
    }

    bundles
}

fn collect_bundles_from(
    dir: &std::path::Path,
    bundles: &mut std::collections::HashSet<String>,
    depth: u8,
) {
    if depth == 0 || !dir.exists() {
        return;
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        if name.ends_with(".app") {
            // Try to read the bundle ID from Info.plist
            let plist = path.join("Contents/Info.plist");
            if let Some(bundle_id) = read_bundle_id(&plist) {
                bundles.insert(bundle_id);
            }
        } else {
            // Recurse into subdirectories (e.g., /Applications/Utilities)
            collect_bundles_from(&path, bundles, depth - 1);
        }
    }
}

fn read_bundle_id(plist_path: &std::path::Path) -> Option<String> {
    if !plist_path.exists() {
        return None;
    }
    // Use /usr/libexec/PlistBuddy to read the CFBundleIdentifier
    let output = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier", &plist_path.to_string_lossy()])
        .output()
        .ok()?;

    if output.status.success() {
        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !id.is_empty() {
            return Some(id);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// macOS Sonoma+ dynamic wallpapers / aerial screen savers
// ---------------------------------------------------------------------------

pub struct DynamicWallpaperRule;

impl CleanupRule for DynamicWallpaperRule {
    fn name(&self) -> &'static str {
        "Dynamic wallpapers & screen savers"
    }

    fn category(&self) -> Category {
        Category::MacosSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // Sonoma+ stores downloaded aerial wallpapers here
        let wallpaper_paths = [
            home.join("Library/Application Support/com.apple.wallpaper"),
            home.join("Library/Caches/com.apple.wallpaper"),
        ];

        for path in &wallpaper_paths {
            if path.exists() {
                let size = walker::dir_size(path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: path.clone(),
                        size,
                        category: Category::MacosSpecific,
                        safety: SafetyLevel::Safe,
                        description: "Dynamic wallpapers (re-downloadable)".to_owned(),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

// ---------------------------------------------------------------------------
// Final Cut Pro render files and proxy media
// ---------------------------------------------------------------------------

pub struct FinalCutProRule;

impl CleanupRule for FinalCutProRule {
    fn name(&self) -> &'static str {
        "Final Cut Pro cache"
    }

    fn category(&self) -> Category {
        Category::MacosSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        let paths = [
            (
                home.join("Library/Caches/com.apple.FinalCut"),
                "Final Cut Pro cache",
            ),
            (
                home.join("Library/Application Support/Final Cut Pro"),
                "Final Cut Pro support data",
            ),
            (
                home.join("Movies/Motion Templates.localized"),
                "Motion Templates",
            ),
        ];

        for (path, desc) in &paths {
            if path.exists() {
                let size = walker::dir_size(path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: path.clone(),
                        size,
                        category: Category::MacosSpecific,
                        safety: SafetyLevel::Caution,
                        description: desc.to_string(),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

// ---------------------------------------------------------------------------
// Printer drivers for printers no longer configured
// ---------------------------------------------------------------------------

pub struct PrinterDriversRule;

impl CleanupRule for PrinterDriversRule {
    fn name(&self) -> &'static str {
        "Printer drivers"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let printers_dir = PathBuf::from("/Library/Printers");

        if !printers_dir.exists() {
            return Vec::new();
        }

        let size = walker::dir_size(&printers_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: printers_dir,
            size,
            category: Category::SystemJunk,
            safety: SafetyLevel::Caution,
            description: "Printer drivers (/Library/Printers)".to_owned(),
            item_count: None,
        }]
    }
}

// Note: Diagnostic reports are already covered by logs.rs (LogFilesRule + SystemLogsRule)
// which scan ~/Library/Logs and /Library/Logs as parent directories. No separate rule needed.

// ---------------------------------------------------------------------------
// Xcode additional caches not covered by existing rules
// ---------------------------------------------------------------------------

cache_rule!(
    XcodeDocIndexRule,
    "Xcode documentation index",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/DocumentationIndex"
);

cache_rule!(
    XcodeProductsRule,
    "Xcode Products",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/Products"
);

cache_rule!(
    CoreSimulatorCachesRule,
    "CoreSimulator caches",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Developer/CoreSimulator/Caches"
);

// ---------------------------------------------------------------------------
// iMovie / Apple video editing render caches
// ---------------------------------------------------------------------------

cache_rule!(
    IMovieCacheRule,
    "iMovie cache",
    Category::MacosSpecific,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.iMovieApp",
    "Library/Containers/com.apple.iMovieApp/Data/Library/Caches"
);

// ---------------------------------------------------------------------------
// pub fn rules()
// ---------------------------------------------------------------------------

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        // iMessage
        Box::new(MessagesAttachmentsRule),
        Box::new(MessagesCacheRule),
        // System
        Box::new(SavedAppStateRule),
        Box::new(AppStoreCacheRule),
        Box::new(QuickLookDaemonCacheRule),
        Box::new(NsurlSessionCacheRule),
        Box::new(PrinterDriversRule),
        // macOS-specific
        Box::new(MapsCacheRule),
        Box::new(SpeechCacheRule),
        Box::new(ICloudCacheRule),
        Box::new(OrphanedContainersRule),
        Box::new(DynamicWallpaperRule),
        Box::new(FinalCutProRule),
        Box::new(GarageBandCacheRule),
        Box::new(LogicProCacheRule),
        Box::new(AppleLoopsRule),
        Box::new(PhotosTranscodeCacheRule),
        Box::new(XcodeDocIndexRule),
        Box::new(XcodeProductsRule),
        Box::new(CoreSimulatorCachesRule),
        Box::new(IMovieCacheRule),
        // Gaming
        Box::new(UnityCacheRule),
        Box::new(SteamCacheRule),
        Box::new(SteamShaderCacheRule),
        Box::new(EpicGamesCacheRule),
        // Dev caches
        Box::new(NugetCacheRule),
        Box::new(DotnetCacheRule),
    ]
}
