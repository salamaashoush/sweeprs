use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel};

// AVDs and system images are emulator artifacts, not build output -- they live
// in `simulator.rs` alongside the iOS runtimes.

cache_rule!(
    AndroidGradleWrapperRule,
    "Gradle wrapper distributions",
    Category::BuildArtifact,
    SafetyLevel::Safe,
    ".gradle/wrapper/dists"
);

cache_rule!(
    AndroidGradleDaemonRule,
    "Gradle daemon logs",
    Category::LogFile,
    SafetyLevel::Safe,
    ".gradle/daemon"
);

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(AndroidGradleWrapperRule),
        Box::new(AndroidGradleDaemonRule),
    ]
}
