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
];

const KNOWN_APP_SUPPORT_DIRS: &[&str] = &[
    "Slack",
    "discord",
    "Code",
    "Cursor",
    "Microsoft",
    "Spotify",
    "Signal",
    "LM Studio",
    "nomic.ai",
    "Jan",
    "MobileSync",
    "com.docker.docker",
];

const MIN_CACHE_SIZE: u64 = 50_000_000; // 50 MiB
const MIN_APP_SUPPORT_SIZE: u64 = 100_000_000; // 100 MiB

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

pub struct GenericAppSupportRule;

impl CleanupRule for GenericAppSupportRule {
    fn name(&self) -> &'static str {
        "Generic Application Support"
    }

    fn category(&self) -> Category {
        Category::AppCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();
        let home = dirs::home_dir().unwrap_or_default();
        let app_support = home.join("Library/Application Support");
        if !app_support.exists() {
            return entries;
        }

        let Ok(read_dir) = std::fs::read_dir(&app_support) else {
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

            if KNOWN_APP_SUPPORT_DIRS.contains(&dirname.as_str()) {
                continue;
            }

            let size = walker::dir_size(&path);
            if size < MIN_APP_SUPPORT_SIZE {
                continue;
            }

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::AppCache,
                safety: SafetyLevel::Caution,
                description: format!("~/Library/Application Support/{dirname}"),
                item_count: None,
            });
        }

        entries
    }
}

const MIN_GROUP_CONTAINER_SIZE: u64 = 100_000_000; // 100 MiB

pub struct GenericGroupContainersRule;

impl CleanupRule for GenericGroupContainersRule {
    fn name(&self) -> &'static str {
        "Group Containers"
    }

    fn category(&self) -> Category {
        Category::AppCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();
        let home = dirs::home_dir().unwrap_or_default();
        let group_containers = home.join("Library/Group Containers");
        if !group_containers.exists() {
            return entries;
        }

        let Ok(read_dir) = std::fs::read_dir(&group_containers) else {
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

            let size = walker::dir_size(&path);
            if size < MIN_GROUP_CONTAINER_SIZE {
                continue;
            }

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::AppCache,
                safety: SafetyLevel::Caution,
                description: format!("~/Library/Group Containers/{dirname}"),
                item_count: None,
            });
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(GenericCacheDirsRule),
        Box::new(GenericAppSupportRule),
        Box::new(GenericGroupContainersRule),
    ]
}
