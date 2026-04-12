use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

cache_rule!(
    ChromeCacheRule,
    "Chrome cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/Google/Chrome",
    ".cache/google-chrome",
    ".cache/chromium"
);

cache_rule!(
    FirefoxCacheRule,
    "Firefox cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/Firefox",
    ".cache/mozilla/firefox"
);

cache_rule!(
    SafariCacheRule,
    "Safari cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/com.apple.Safari"
);

cache_rule!(
    ArcCacheRule,
    "Arc cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/company.thebrowser.Browser"
);

cache_rule!(
    BraveCacheRule,
    "Brave cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/BraveSoftware/Brave-Browser",
    ".cache/BraveSoftware/Brave-Browser"
);

cache_rule!(
    EdgeCacheRule,
    "Microsoft Edge cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/Microsoft Edge",
    ".cache/microsoft-edge"
);

cache_rule!(
    OperaCacheRule,
    "Opera cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/com.operasoftware.Opera",
    ".cache/opera"
);

cache_rule!(
    VivaldiCacheRule,
    "Vivaldi cache",
    Category::BrowserCache,
    SafetyLevel::Safe,
    "Library/Caches/Vivaldi",
    ".cache/vivaldi"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(ChromeCacheRule),
        Box::new(FirefoxCacheRule),
        Box::new(SafariCacheRule),
        Box::new(ArcCacheRule),
        Box::new(BraveCacheRule),
        Box::new(EdgeCacheRule),
        Box::new(OperaCacheRule),
        Box::new(VivaldiCacheRule),
    ]
}
