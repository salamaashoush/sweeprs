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
    "zig",
    "bazelisk",
    "cabal",
    "rebar3",
    "ferridriver",
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
    "puppeteer",
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
        let mut roots: Vec<(std::path::PathBuf, SafetyLevel)> = Vec::new();
        if let Some(cache_dir) = dirs::cache_dir() {
            roots.push((cache_dir, SafetyLevel::Safe));
        }
        // Command-line tools follow XDG on macOS too, so `~/.cache` fills up
        // beside `~/Library/Caches`. Nothing on macOS promises that what lands
        // there is disposable, so it is offered with caution.
        if cfg!(target_os = "macos")
            && let Some(home) = dirs::home_dir()
        {
            roots.push((home.join(".cache"), SafetyLevel::Caution));
        }

        // Sizing these one after another was the slowest single rule on a
        // developer machine: the cache root holds a long tail of directories,
        // several of them gigabytes deep.
        let candidates: Vec<_> = roots
            .iter()
            .filter_map(|(root, safety)| Some((std::fs::read_dir(root).ok()?, *safety)))
            .flat_map(|(read_dir, safety)| {
                read_dir.flatten().map(move |entry| (entry.path(), safety))
            })
            .filter(|(path, _)| {
                path.symlink_metadata().is_ok_and(|m| m.is_dir())
                    && path.file_name().is_some_and(|name| {
                        !KNOWN_CACHE_DIRS.contains(&name.to_string_lossy().as_ref())
                    })
            })
            .collect();

        candidates
            .par_iter()
            .filter_map(|(path, safety)| {
                let size = walker::dir_size(path);
                if size < MIN_CACHE_SIZE {
                    return None;
                }
                Some(ScannedEntry {
                    path: path.clone(),
                    size,
                    category: Category::AppCache,
                    safety: *safety,
                    description: format!(
                        "{} cache",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
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
