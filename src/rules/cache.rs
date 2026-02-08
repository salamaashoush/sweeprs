use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

cache_rule!(
    NpmCacheRule,
    "npm cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".npm/_cacache"
);

cache_rule!(
    YarnCacheRule,
    "Yarn cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/Yarn"
);

cache_rule!(
    PnpmCacheRule,
    "pnpm store",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/pnpm/store"
);

cache_rule!(
    BunCacheRule,
    "Bun cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".bun/install/cache"
);

cache_rule!(
    CargoCacheRule,
    "Cargo cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cargo/registry/cache",
    ".cargo/registry/src"
);

cache_rule!(
    PipCacheRule,
    "pip cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/pip"
);

cache_rule!(
    HomebrewCacheRule,
    "Homebrew cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/Homebrew"
);

cache_rule!(
    CocoaPodsCacheRule,
    "CocoaPods cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/CocoaPods"
);

cache_rule!(
    GoCacheRule,
    "Go module cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "go/pkg/mod",
    "Library/Caches/go-build"
);

cache_rule!(
    MavenCacheRule,
    "Maven cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".m2/repository"
);

cache_rule!(
    SpmCacheRule,
    "Swift PM cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/org.swift.swiftpm"
);

cache_rule!(
    ComposerCacheRule,
    "Composer cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".composer/cache"
);

cache_rule!(
    GemCacheRule,
    "Ruby Gem cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".gem"
);

cache_rule!(
    PoetryCacheRule,
    "Poetry cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/pypoetry"
);

cache_rule!(
    DenoCacheRule,
    "Deno cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/deno"
);

cache_rule!(
    NeovimCacheRule,
    "Neovim cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/nvim",
    ".local/share/nvim"
);

cache_rule!(
    BundlerCacheRule,
    "Bundler cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".bundle/cache"
);

cache_rule!(
    MavenWrapperRule,
    "Maven wrapper distributions",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".m2/wrapper/dists"
);

cache_rule!(
    GoDownloadCacheRule,
    "Go download cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "go/pkg/mod/download"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(NpmCacheRule),
        Box::new(YarnCacheRule),
        Box::new(PnpmCacheRule),
        Box::new(BunCacheRule),
        Box::new(CargoCacheRule),
        Box::new(PipCacheRule),
        Box::new(HomebrewCacheRule),
        Box::new(CocoaPodsCacheRule),
        Box::new(GoCacheRule),
        Box::new(MavenCacheRule),
        Box::new(SpmCacheRule),
        Box::new(ComposerCacheRule),
        Box::new(GemCacheRule),
        Box::new(PoetryCacheRule),
        Box::new(DenoCacheRule),
        Box::new(NeovimCacheRule),
        Box::new(BundlerCacheRule),
        Box::new(MavenWrapperRule),
        Box::new(GoDownloadCacheRule),
    ]
}
