use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

cache_rule!(
    SlackCacheRule,
    "Slack",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Slack/Cache",
    "Library/Application Support/Slack/Service Worker/CacheStorage",
    ".config/Slack/Cache",
    ".config/Slack/Service Worker/CacheStorage"
);

cache_rule!(
    DiscordCacheRule,
    "Discord",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/discord/Cache",
    "Library/Application Support/discord/Code Cache",
    ".config/discord/Cache",
    ".config/discord/Code Cache"
);

cache_rule!(
    TeamsClassicCacheRule,
    "Microsoft Teams (Classic)",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Microsoft/Teams/Cache",
    ".config/Microsoft/Microsoft Teams/Cache"
);

cache_rule!(
    TeamsNewCacheRule,
    "Microsoft Teams",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.microsoft.teams2",
    ".cache/ms-teams"
);

cache_rule!(
    SpotifyCacheRule,
    "Spotify",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.spotify.client",
    ".cache/spotify"
);

cache_rule!(
    ZoomCacheRule,
    "Zoom",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/us.zoom.xos",
    ".cache/zoom"
);

cache_rule!(
    TelegramCacheRule,
    "Telegram",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/ru.keepcoder.Telegram",
    ".cache/TelegramDesktop"
);

cache_rule!(
    WhatsAppCacheRule,
    "WhatsApp",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/net.whatsapp.WhatsApp"
);

cache_rule!(
    SignalCacheRule,
    "Signal",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Signal/Cache",
    ".config/Signal/Cache"
);

cache_rule!(
    FigmaCacheRule,
    "Figma",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.figma.Desktop",
    ".config/Figma/Cache"
);

cache_rule!(
    NotionCacheRule,
    "Notion",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/notion.id",
    ".config/Notion/Cache"
);

cache_rule!(
    LinearCacheRule,
    "Linear",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.linear",
    ".config/Linear/Cache"
);

cache_rule!(
    OnePasswordCacheRule,
    "1Password",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.1password.1password",
    ".config/1Password/Cache"
);

cache_rule!(
    PostmanCacheRule,
    "Postman",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.postmanlabs.mac",
    ".config/Postman/Cache"
);

cache_rule!(
    DockerDesktopCacheRule,
    "Docker Desktop",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.docker.docker",
    ".docker/desktop/cache"
);

cache_rule!(
    ObsidianCacheRule,
    "Obsidian",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/md.obsidian",
    ".config/obsidian/Cache"
);

cache_rule!(
    CanvaCacheRule,
    "Canva",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.canva.CanvaDesktop"
);

cache_rule!(
    GrammarlyCacheRule,
    "Grammarly",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.grammarly.ProjectLlama"
);

cache_rule!(
    RaycastCacheRule,
    "Raycast",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.raycast.macos"
);

cache_rule!(
    ChromeCodeCacheRule,
    "Chrome Code Cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Google/Chrome/Default/Code Cache",
    ".config/google-chrome/Default/Code Cache"
);

cache_rule!(
    ChromeServiceWorkerRule,
    "Chrome Service Worker cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Google/Chrome/Default/Service Worker/CacheStorage",
    ".config/google-chrome/Default/Service Worker/CacheStorage"
);

cache_rule!(
    FirefoxOfflineCacheRule,
    "Firefox profile cache",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/Firefox/Profiles",
    ".cache/mozilla/firefox"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(SlackCacheRule),
        Box::new(DiscordCacheRule),
        Box::new(TeamsClassicCacheRule),
        Box::new(TeamsNewCacheRule),
        Box::new(SpotifyCacheRule),
        Box::new(ZoomCacheRule),
        Box::new(TelegramCacheRule),
        Box::new(WhatsAppCacheRule),
        Box::new(SignalCacheRule),
        Box::new(FigmaCacheRule),
        Box::new(NotionCacheRule),
        Box::new(LinearCacheRule),
        Box::new(OnePasswordCacheRule),
        Box::new(PostmanCacheRule),
        Box::new(DockerDesktopCacheRule),
        Box::new(ObsidianCacheRule),
        Box::new(CanvaCacheRule),
        Box::new(GrammarlyCacheRule),
        Box::new(RaycastCacheRule),
        Box::new(ChromeCodeCacheRule),
        Box::new(ChromeServiceWorkerRule),
        Box::new(FirefoxOfflineCacheRule),
    ]
}
