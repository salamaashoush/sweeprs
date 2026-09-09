use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// Directories already covered by dedicated rules -- skip them in the generic scan
// to avoid double-reporting.
const KNOWN_CACHE_DIRS: &[&str] = &[
    "nvim",
    "pip",
    "Homebrew",
    "go-build",
    "gcloud",
    "sccache",
    "ccache",
    "bazel",
    "turbo",
    "nx",
    "pre-commit",
    "uv",
    "mise",
    "torch",
    "lm-studio",
    "gpt4all",
    "deno",
    "nix",
    "containers",
    "huggingface",
    // macOS_extra rules
    "com.unity3d.UnityEditor",
    "com.valvesoftware.steam",
    "com.epicgames.EpicGamesLauncher",
    "GeoServices",
    "com.apple.Maps",
    "com.apple.SpeechRecognitionCore",
    "VoiceServices",
    "com.apple.parsec",
    "com.apple.parsecd",
    "com.apple.bird",
    "CloudKit",
    "com.apple.quicklookd",
    "com.apple.nsurlsessiond",
    "com.apple.appstore",
    "com.apple.appstoreagent",
    "com.apple.FinalCut",
    "com.apple.garageband10",
    "com.apple.logic10",
    "com.apple.iMovieApp",
    "com.apple.wallpaper",
    // dev_caches additions
    "coursier",
    "helm",
    // Linux-specific (covered by linux.rs and other rules)
    "thumbnails",
    "fontconfig",
    "mesa_shader_cache",
    "mesa_shader_cache_db",
    "man",
    "yarn",
    "pypoetry",
    "spotify",
    "mozilla",
    "google-chrome",
    "chromium",
    "BraveSoftware",
    "microsoft-edge",
    "opera",
    "vivaldi",
    "ms-teams",
    "zoom",
    "TelegramDesktop",
    // Browser caches (covered by browser.rs)
    "Google",
    "Firefox",
    "Safari",
    "company.thebrowser.Browser",
    "BraveSoftware/Brave-Browser",
    "Microsoft Edge",
    "com.operasoftware.Opera",
    "Vivaldi",
    // App caches (covered by app_cache.rs)
    "com.spotify.client",
    "us.zoom.xos",
    "ru.keepcoder.Telegram",
    "net.whatsapp.WhatsApp",
    "com.figma.Desktop",
    "notion.id",
    "com.linear",
    "com.1password.1password",
    "com.postmanlabs.mac",
    "com.docker.docker",
    "md.obsidian",
    "com.canva.CanvaDesktop",
    "com.grammarly.ProjectLlama",
    "com.raycast.macos",
    "com.microsoft.teams2",
    // IDE caches (covered by ide.rs)
    "zed",
    "sublime-text",
    // Linux-specific (covered by linux.rs)
    "paru",
    "yay",
    "ms-playwright",
    "electron",
    "nvidia",
    "radv_builtin_shaders",
];

const MIN_CACHE_SIZE: u64 = 50_000_000; // 50 MiB

pub struct GenericCacheDirsRule;

impl CleanupRule for GenericCacheDirsRule {
    fn name(&self) -> &'static str {
        "Generic cache dirs"
    }

    fn category(&self) -> Category {
        Category::AppCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let Some(cache_dir) = dirs::cache_dir() else {
            return Vec::new();
        };
        let Ok(read_dir) = std::fs::read_dir(&cache_dir) else {
            return Vec::new();
        };

        // Sizing these one after another was the slowest single rule on a
        // developer machine: the cache root holds a long tail of directories,
        // several of them gigabytes deep.
        let candidates: Vec<_> = read_dir
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if !path.is_dir() {
                    return None;
                }
                let dirname = path.file_name()?.to_string_lossy().into_owned();
                if KNOWN_CACHE_DIRS.contains(&dirname.as_str()) {
                    return None;
                }
                Some((path, dirname))
            })
            .collect();

        candidates
            .par_iter()
            .filter_map(|(path, dirname)| {
                let size = walker::dir_size(path);
                if size < MIN_CACHE_SIZE {
                    return None;
                }
                Some(ScannedEntry {
                    path: path.clone(),
                    size,
                    category: Category::AppCache,
                    safety: SafetyLevel::Safe,
                    description: format!("~/.cache/{dirname}"),
                    item_count: None,
                })
            })
            .collect()
    }
}

// NOTE: GenericAppSupportRule and GenericGroupContainersRule were removed.
// ~/Library/Application Support/ contains app data (settings, profiles, extensions),
// NOT caches. Deleting them resets apps to factory state and loses user data.
// ~/Library/Group Containers/ is the same -- sandboxed app data, not caches.
// Only ~/Library/Caches/ and ~/.cache/ are actual cache directories.

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(GenericCacheDirsRule)]
}
