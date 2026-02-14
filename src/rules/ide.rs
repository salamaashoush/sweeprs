use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

cache_rule!(
    VsCodeCacheRule,
    "VS Code cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Application Support/Code/Cache",
    "Library/Application Support/Code/CachedData",
    "Library/Application Support/Code/CachedExtensions"
);

cache_rule!(
    XcodeCacheRule,
    "Xcode caches",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.dt.Xcode"
);

pub struct JetBrainsCacheRule;

impl CleanupRule for JetBrainsCacheRule {
    fn name(&self) -> &'static str {
        "JetBrains caches"
    }

    fn category(&self) -> Category {
        Category::IdeCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let lib_caches = home.join("Library/Caches");
        let mut entries = Vec::new();

        if !lib_caches.exists() {
            return entries;
        }

        let prefixes = [
            "JetBrains",
            "IntelliJIdea",
            "WebStorm",
            "PyCharm",
            "CLion",
            "GoLand",
            "RustRover",
            "DataGrip",
            "Rider",
        ];

        if let Ok(read_dir) = std::fs::read_dir(&lib_caches) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if prefixes.iter().any(|p| name_str.starts_with(p)) {
                    let path = entry.path();
                    if path.is_dir() {
                        let size = walker::dir_size(&path);
                        if size > 0 {
                            entries.push(ScannedEntry {
                                path,
                                size,
                                category: Category::IdeCache,
                                safety: SafetyLevel::Safe,
                                description: format!("{name_str} cache"),
                                item_count: None,
                            });
                        }
                    }
                }
            }
        }

        entries
    }
}

cache_rule!(
    CursorCacheRule,
    "Cursor cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Application Support/Cursor/Cache",
    "Library/Application Support/Cursor/CachedData"
);

cache_rule!(
    ZedCacheRule,
    "Zed cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Caches/dev.zed.Zed"
);

cache_rule!(
    SublimeCacheRule,
    "Sublime Text cache",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Caches/com.sublimetext.4"
);

cache_rule!(
    XcodeProductsRule,
    "Xcode Products",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/Products"
);

cache_rule!(
    XcodeIBSupportRule,
    "Xcode IB Support",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/UserData/IB Support"
);

cache_rule!(
    XcodeDocIndexRule,
    "Xcode DocumentationIndex",
    Category::IdeCache,
    SafetyLevel::Safe,
    "Library/Developer/Xcode/DocumentationIndex"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(VsCodeCacheRule),
        Box::new(XcodeCacheRule),
        Box::new(JetBrainsCacheRule),
        Box::new(CursorCacheRule),
        Box::new(ZedCacheRule),
        Box::new(SublimeCacheRule),
        Box::new(XcodeProductsRule),
        Box::new(XcodeIBSupportRule),
        Box::new(XcodeDocIndexRule),
    ]
}
