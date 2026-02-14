use crate::rules::{cache_rule, CleanupRule};
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

cache_rule!(
    MiseCacheRule,
    "mise cache",
    Category::PackageCache,
    SafetyLevel::Safe,
    ".cache/mise",
    ".local/share/mise"
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
    ]
}
