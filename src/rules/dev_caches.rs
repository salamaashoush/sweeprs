use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

cache_rule!(
    BazelCacheRule,
    "Bazel cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/bazel"
);

cache_rule!(
    CcacheCacheRule,
    "ccache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/ccache",
    ".ccache"
);

cache_rule!(
    SccacheCacheRule,
    "sccache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/sccache"
);

cache_rule!(
    TurboCacheRule,
    "Turborepo cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/turbo"
);

cache_rule!(
    NxCacheRule,
    "Nx cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/nx"
);

cache_rule!(
    PreCommitCacheRule,
    "pre-commit cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/pre-commit"
);

cache_rule!(
    UvCacheRule,
    "uv cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/uv"
);

cache_rule!(
    PipWheelCacheRule,
    "pip wheel cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/pip/wheels"
);

// `~/.local/share/mise` is deliberately absent: `toolchain.rs` walks it
// per tool version so the active toolchain survives. Claiming the whole
// directory here would delete the toolchain the user is running on.
cache_rule!(
    MiseCacheRule,
    "mise cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/mise"
);

cache_rule!(
    TorchCacheRule,
    "PyTorch cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/torch"
);

cache_rule!(
    KerasCacheRule,
    "Keras cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".keras"
);

cache_rule!(
    TritonCacheRule,
    "Triton cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".triton/cache"
);

cache_rule!(
    GradleWrapperDistsRule,
    "Gradle wrapper distributions",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".gradle/wrapper/dists"
);

cache_rule!(
    SdkmanArchivesRule,
    "SDKMAN archives",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".sdkman/archives"
);

cache_rule!(
    AsdfDownloadsRule,
    "asdf downloads",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".asdf/downloads"
);

cache_rule!(
    CoursierCacheRule,
    "Coursier cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/coursier"
);

cache_rule!(
    HelmCacheRule,
    "Helm cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/helm"
);

cache_rule!(
    MinikubeCacheRule,
    "Minikube cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".minikube/cache"
);

cache_rule!(
    TerraformPluginsRule,
    "Terraform plugin cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".terraform.d/plugins"
);

// Browser and runtime binaries that test tooling downloads on demand. Each is
// hundreds of megabytes per version and every one of these tools re-fetches
// silently on the next run.
cache_rule!(
    PlaywrightBrowsersRule,
    "Playwright browsers",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/ms-playwright",
    ".cache/ms-playwright"
);

cache_rule!(
    PuppeteerBrowsersRule,
    "Puppeteer browsers",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/puppeteer",
    ".cache/puppeteer"
);

cache_rule!(
    CypressBinariesRule,
    "Cypress binaries",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/Cypress",
    ".cache/Cypress"
);

cache_rule!(
    ElectronDownloadsRule,
    "Electron downloads",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/electron",
    "Library/Caches/electron-builder",
    ".cache/electron",
    ".cache/electron-builder"
);

cache_rule!(
    NodeGypHeadersRule,
    "node-gyp headers",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/node-gyp",
    ".cache/node-gyp",
    ".node-gyp",
    ".electron-gyp"
);

cache_rule!(
    YarnBerryCacheRule,
    "Yarn Berry cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".yarn/berry/cache"
);

cache_rule!(
    GolangciLintCacheRule,
    "golangci-lint cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/golangci-lint",
    ".cache/golangci-lint"
);

cache_rule!(
    CarthageCacheRule,
    "Carthage cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    "Library/Caches/org.carthage.CarthageKit"
);

cache_rule!(
    GradleNativeRule,
    "Gradle native dependencies",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".gradle/native"
);

cache_rule!(
    AndroidBuildCacheRule,
    "Android build cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".android/build-cache",
    ".android/cache"
);

// Re-cloning every podspec is a multi-minute network operation, not a rebuild.
cache_rule!(
    CocoaPodsReposRule,
    "CocoaPods spec repos",
    Category::PackageCache,
    SafetyLevel::Caution,
    ".cocoapods/repos"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(BazelCacheRule),
        Box::new(CcacheCacheRule),
        Box::new(SccacheCacheRule),
        Box::new(TurboCacheRule),
        Box::new(NxCacheRule),
        Box::new(PreCommitCacheRule),
        Box::new(UvCacheRule),
        Box::new(PipWheelCacheRule),
        Box::new(MiseCacheRule),
        Box::new(TorchCacheRule),
        Box::new(KerasCacheRule),
        Box::new(TritonCacheRule),
        Box::new(GradleWrapperDistsRule),
        Box::new(SdkmanArchivesRule),
        Box::new(AsdfDownloadsRule),
        Box::new(CoursierCacheRule),
        Box::new(HelmCacheRule),
        Box::new(MinikubeCacheRule),
        Box::new(TerraformPluginsRule),
        Box::new(PlaywrightBrowsersRule),
        Box::new(PuppeteerBrowsersRule),
        Box::new(CypressBinariesRule),
        Box::new(ElectronDownloadsRule),
        Box::new(NodeGypHeadersRule),
        Box::new(YarnBerryCacheRule),
        Box::new(GolangciLintCacheRule),
        Box::new(CarthageCacheRule),
        Box::new(GradleNativeRule),
        Box::new(AndroidBuildCacheRule),
        Box::new(CocoaPodsReposRule),
    ]
}
