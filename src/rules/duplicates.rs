use std::cell::RefCell;
use std::io::Read;
use std::path::PathBuf;

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use xxhash_rust::xxh3::Xxh3;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util;

/// Size of the buffer used for streaming file hashing (64 KiB).
const HASH_BUF_SIZE: usize = 65536;

/// Size of the partial hash prefix (16 KiB).
/// Files that differ in the first 16 KiB are not duplicates.
const PARTIAL_HASH_SIZE: usize = 16384;

// Thread-local reusable buffer for file hashing.
thread_local! {
    static HASH_BUF: RefCell<Vec<u8>> = RefCell::new(vec![0u8; HASH_BUF_SIZE]);
}

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
                // Grouping key is the logical length: two identical files always
                // share it, while their allocated size can differ on a
                // copy-on-write filesystem.
                let size = entry.metadata().map_or(0, |m| m.len());
                if size >= min_size {
                    size_groups
                        .entry(size)
                        .or_default()
                        .push(entry.path().to_path_buf());
                }
            }
        }

        // Phase 2: partial hash (first 16 KiB) to cheaply eliminate non-duplicates
        let candidate_groups: Vec<_> = size_groups
            .into_iter()
            .filter(|(_, paths)| paths.len() >= 2)
            .collect();

        let entries: Vec<ScannedEntry> = candidate_groups
            .par_iter()
            .flat_map(|(_size, paths)| {
                // Phase 2a: partial hash to narrow candidates
                let mut partial_groups: FxHashMap<u64, Vec<&PathBuf>> = FxHashMap::default();
                for path in paths {
                    if let Some(hash) = hash_file_partial(path) {
                        partial_groups.entry(hash).or_default().push(path);
                    }
                }

                let mut group_entries = Vec::new();

                for partial_dups in partial_groups.values() {
                    if partial_dups.len() < 2 {
                        continue;
                    }

                    // Phase 2b: full hash only files whose partial hashes matched
                    let mut full_groups: FxHashMap<u64, Vec<&PathBuf>> = FxHashMap::default();
                    for path in partial_dups {
                        if let Some(hash) = hash_file_full(path) {
                            full_groups.entry(hash).or_default().push(path);
                        }
                    }

                    for dups in full_groups.values() {
                        if dups.len() < 2 {
                            continue;
                        }

                        // Keep the first, mark rest as duplicates
                        for dup in dups.iter().skip(1) {
                            let name = dup
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            // Report the blocks this copy occupies, not the
                            // logical length shared with the original.
                            let freed = crate::scanner::walker::file_size(dup);
                            group_entries.push(ScannedEntry {
                                path: (*dup).clone(),
                                size: freed,
                                category: Category::Duplicate,
                                safety: SafetyLevel::Danger,
                                description: format!(
                                    "Duplicate: {name} ({}, {n} copies)",
                                    util::human_size(freed),
                                    n = dups.len()
                                ),
                                item_count: Some(dups.len()),
                            });
                        }
                    }
                }
                group_entries
            })
            .collect();

        entries
    }
}

/// Hash only the first `PARTIAL_HASH_SIZE` bytes of a file using Xxh3.
/// This is a cheap pre-filter: files that differ early avoid full hashing.
fn hash_file_partial(path: &PathBuf) -> Option<u64> {
    HASH_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        let mut file = std::fs::File::open(path).ok()?;
        let mut hasher = Xxh3::new();
        let mut remaining = PARTIAL_HASH_SIZE;
        while remaining > 0 {
            let to_read = remaining.min(buf.len());
            let n = file.read(&mut buf[..to_read]).ok()?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            remaining -= n;
        }
        Some(hasher.digest())
    })
}

/// Stream-hash an entire file in 64 KiB chunks using Xxh3.
fn hash_file_full(path: &PathBuf) -> Option<u64> {
    HASH_BUF.with(|cell| {
        let mut buf = cell.borrow_mut();
        let mut file = std::fs::File::open(path).ok()?;
        let mut hasher = Xxh3::new();
        loop {
            let n = file.read(&mut buf).ok()?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Some(hasher.digest())
    })
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(DuplicatesRule)]
}
