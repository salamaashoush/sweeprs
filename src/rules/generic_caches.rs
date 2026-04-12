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
        let mut entries = Vec::new();
        let Some(cache_dir) = dirs::cache_dir() else {
            return entries;
        };
        if !cache_dir.exists() {
            return entries;
        }

        let Ok(read_dir) = std::fs::read_dir(&cache_dir) else {
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

            if KNOWN_CACHE_DIRS.contains(&dirname.as_str()) {
                continue;
            }

            let size = walker::dir_size(&path);
            if size < MIN_CACHE_SIZE {
                continue;
            }

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::AppCache,
                safety: SafetyLevel::Safe,
                description: format!("~/.cache/{dirname}"),
                item_count: None,
            });
        }

        entries
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
