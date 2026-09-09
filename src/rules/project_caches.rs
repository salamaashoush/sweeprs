use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

/// Framework caches and build output that live inside a project.
///
/// `gitignored.rs` catches some of these, but only at a repository's top level
/// and only when the project actually lists them in `.gitignore`. In a monorepo
/// the same directories sit one per package, which is where the bulk of the
/// space is.
///
/// Every marker is a file the framework itself requires, so a directory named
/// `.output` next to a `package.json` is not mistaken for a build cache.
const CACHE_MARKERS: &[(&str, &str, &str)] = &[
    (".next", "package.json", "Next.js .next/"),
    (".nuxt", "package.json", "Nuxt .nuxt/"),
    (".output", "nuxt.config.ts", "Nuxt .output/"),
    (".output", "nuxt.config.js", "Nuxt .output/"),
    (".svelte-kit", "package.json", "SvelteKit .svelte-kit/"),
    (".astro", "package.json", "Astro .astro/"),
    (".parcel-cache", "package.json", "Parcel cache"),
    (".turbo", "package.json", "Turborepo cache"),
    (".angular", "angular.json", "Angular cache"),
    (".docusaurus", "package.json", "Docusaurus cache"),
    (".expo", "package.json", "Expo cache"),
    (".rollup.cache", "package.json", "Rollup cache"),
    (".serverless", "serverless.yml", "Serverless package"),
    (".gradle", "settings.gradle", "Gradle project cache"),
    (".gradle", "settings.gradle.kts", "Gradle project cache"),
    (".gradle", "build.gradle", "Gradle project cache"),
    (".gradle", "build.gradle.kts", "Gradle project cache"),
    (".cxx", "build.gradle", "Android native build"),
    (".cxx", "build.gradle.kts", "Android native build"),
    (".kotlin", "build.gradle.kts", "Kotlin session data"),
];

/// Dependency trees inside a project that cost a network round trip to restore,
/// rather than a local rebuild.
const DEPS_MARKERS: &[(&str, &str, &str)] = &[
    ("Pods", "Podfile", "CocoaPods Pods/"),
    (".terraform", ".terraform.lock.hcl", "Terraform providers"),
];

const MIN_SIZE: u64 = 1_048_576;

pub struct ProjectCacheRule;
pub struct ProjectVendoredDepsRule;

fn collect(
    markers: &[(&str, &str, &'static str)],
    category: Category,
    safety: SafetyLevel,
) -> Vec<ScannedEntry> {
    PROJECT_INDEX
        .find_matching_dirs(markers)
        .par_iter()
        .filter_map(|(path, label)| {
            let (size, count) = walker::dir_size_and_count(path);
            if size < MIN_SIZE {
                return None;
            }
            let project = path
                .parent()
                .and_then(|p| p.file_name())
                .map_or_else(String::new, |n| n.to_string_lossy().to_string());
            Some(ScannedEntry {
                path: path.clone(),
                size,
                category,
                safety,
                description: format!("{label} in {project}"),
                item_count: Some(count),
            })
        })
        .collect()
}

impl CleanupRule for ProjectCacheRule {
    fn name(&self) -> &'static str {
        "Project framework caches"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        collect(CACHE_MARKERS, Category::BuildArtifact, SafetyLevel::Safe)
    }
}

impl CleanupRule for ProjectVendoredDepsRule {
    fn name(&self) -> &'static str {
        "Project vendored dependencies"
    }

    fn category(&self) -> Category {
        Category::InstalledDeps
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        collect(DEPS_MARKERS, Category::InstalledDeps, SafetyLevel::Caution)
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(ProjectCacheRule),
        Box::new(ProjectVendoredDepsRule),
    ]
}
