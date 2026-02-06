use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

cache_rule!(
    SlackCacheRule,
    "Slack",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Slack/Cache",
    "Library/Application Support/Slack/Service Worker/CacheStorage"
);

cache_rule!(
    DiscordCacheRule,
    "Discord",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/discord/Cache",
    "Library/Application Support/discord/Code Cache"
);

cache_rule!(
    TeamsClassicCacheRule,
    "Microsoft Teams (Classic)",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Application Support/Microsoft/Teams/Cache"
);

cache_rule!(
    TeamsNewCacheRule,
    "Microsoft Teams",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.microsoft.teams2"
);

cache_rule!(
    SpotifyCacheRule,
    "Spotify",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.spotify.client"
);

cache_rule!(
    ZoomCacheRule,
    "Zoom",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/us.zoom.xos"
);

cache_rule!(
    TelegramCacheRule,
    "Telegram",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/ru.keepcoder.Telegram"
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
    "Library/Application Support/Signal/Cache"
);

cache_rule!(
    FigmaCacheRule,
    "Figma",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.figma.Desktop"
);

cache_rule!(
    NotionCacheRule,
    "Notion",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/notion.id"
);

cache_rule!(
    LinearCacheRule,
    "Linear",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.linear"
);

cache_rule!(
    OnePasswordCacheRule,
    "1Password",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.1password.1password"
);

cache_rule!(
    PostmanCacheRule,
    "Postman",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.postmanlabs.mac"
);

cache_rule!(
    DockerDesktopCacheRule,
    "Docker Desktop",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/com.docker.docker"
);

cache_rule!(
    ObsidianCacheRule,
    "Obsidian",
    Category::AppCache,
    SafetyLevel::Safe,
    "Library/Caches/md.obsidian"
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
    ]
}
