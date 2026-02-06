use std::io::Read;
use std::path::PathBuf;

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::Xxh3;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util;

pub struct DuplicatesRule;

impl CleanupRule for DuplicatesRule {
    fn name(&self) -> &'static str {
        "Duplicates"
    }

    fn category(&self) -> Category {
        Category::Duplicate
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        if !config.categories.enable_duplicates {
            return Vec::new();
        }

        let min_size = config.categories.duplicate_min_size;
        let dirs: Vec<PathBuf> = config
            .categories
            .duplicate_dirs
            .iter()
            .map(|d| Config::expand_path(d))
            .filter(|d| d.exists())
            .collect();

        if dirs.is_empty() {
            return Vec::new();
        }

        // Phase 1: group files by size
        let mut size_groups: FxHashMap<u64, Vec<PathBuf>> = FxHashMap::default();

        for dir in &dirs {
            let walker = ignore::WalkBuilder::new(dir)
                .hidden(false)
                .ignore(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .follow_links(false)
                .build();

            for entry in walker.flatten() {
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if size >= min_size {
                    size_groups
                        .entry(size)
                        .or_default()
                        .push(entry.path().to_path_buf());
                }
            }
        }

        // Phase 2: hash files that share sizes (parallel across size groups)
        let candidate_groups: Vec<_> = size_groups
            .into_iter()
            .filter(|(_, paths)| paths.len() >= 2)
            .collect();

        let entries: Vec<ScannedEntry> = candidate_groups
            .par_iter()
            .flat_map(|(size, paths)| {
                let mut hash_groups: FxHashMap<u64, Vec<&PathBuf>> = FxHashMap::default();

                for path in paths {
                    if let Some(hash) = hash_file_streaming(path) {
                        hash_groups.entry(hash).or_default().push(path);
                    }
                }

                let mut group_entries = Vec::new();
                for dups in hash_groups.values() {
                    if dups.len() < 2 {
                        continue;
                    }

                    // Keep the first, mark rest as duplicates
                    for dup in dups.iter().skip(1) {
                        let name = dup
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        group_entries.push(ScannedEntry {
                            path: (*dup).clone(),
                            size: *size,
                            category: Category::Duplicate,
                            safety: SafetyLevel::Danger,
                            description: format!(
                                "Duplicate: {name} ({}, {n} copies)",
                                util::human_size(*size),
                                n = dups.len()
                            ),
                            item_count: Some(dups.len()),
                        });
                    }
                }
                group_entries
            })
            .collect();

        entries
    }
}

/// Stream-hash a file in 64 KiB chunks using Xxh3, avoiding loading the entire file into RAM.
fn hash_file_streaming(path: &PathBuf) -> Option<u64> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Xxh3::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Some(hasher.digest())
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(DuplicatesRule)]
}
