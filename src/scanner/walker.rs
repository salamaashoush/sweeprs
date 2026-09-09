use std::path::{Path, PathBuf};
use std::sync::RwLock;

use rustc_hash::FxHashMap;

/// Sizes measured during the current scan.
///
/// Several rules legitimately look at the same directory -- a stale project's
/// `node_modules` is also an installed-dependency tree and also a gitignored
/// path -- and walking a dense tree three times is the single largest avoidable
/// cost in a scan. Cleared by `reset_size_cache` at the start of every scan so a
/// long-lived process (the monitor daemon) never reports pre-clean sizes.
static SIZE_CACHE: RwLock<Option<FxHashMap<PathBuf, (u64, usize)>>> = RwLock::new(None);

/// Start a fresh measurement generation. Call once per scan.
pub fn reset_size_cache() {
    if let Ok(mut cache) = SIZE_CACHE.write() {
        *cache = Some(FxHashMap::default());
    }
}

fn cached(path: &Path) -> Option<(u64, usize)> {
    SIZE_CACHE.read().ok()?.as_ref()?.get(path).copied()
}

fn remember(path: &Path, measured: (u64, usize)) {
    if let Ok(mut cache) = SIZE_CACHE.write() {
        if let Some(cache) = cache.as_mut() {
            cache.insert(path.to_path_buf(), measured);
        }
    }
}

/// Space a single file actually occupies, in allocated blocks.
///
/// `metadata().len()` is the logical length, which for a sparse file -- a
/// simulator runtime `.dmg`, a VM disk image, an agent's SQLite store -- can be
/// many times what deleting it would return. `dir_size` already measures
/// allocated bytes, so using the logical length for files made the same scan
/// report two different things depending on whether the entry was a file or a
/// directory.
pub fn file_size(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    path.metadata().map_or(0, |m| m.blocks() * 512)
}

/// Compute the total size of all files under `path` recursively.
///
/// On macOS, uses `getattrlistbulk` to batch file attribute reads (name + type + size)
/// in a single syscall per directory, eliminating per-file `stat()` overhead.
/// Falls back to `ignore::WalkBuilder` on other platforms.
pub fn dir_size(path: &Path) -> u64 {
    dir_size_and_count(path).0
}

/// Measure a directory without consulting or updating the scan cache.
///
/// Used after a delete, where the whole point is to see what is left on disk.
pub fn dir_size_uncached(path: &Path) -> u64 {
    #[cfg(target_os = "macos")]
    {
        super::bulk_stat::dir_size_bulk(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        dir_size_fallback(path)
    }
}

/// Compute total recursive size AND top-level item count.
///
/// Returns `(total_bytes, top_level_count)` where `top_level_count` is the number
/// of direct children (files and dirs at depth 1).
pub fn dir_size_and_count(path: &Path) -> (u64, usize) {
    if let Some(measured) = cached(path) {
        return measured;
    }

    #[cfg(target_os = "macos")]
    let measured = super::bulk_stat::dir_size_and_count_bulk(path);
    #[cfg(not(target_os = "macos"))]
    let measured = dir_size_and_count_fallback(path);

    remember(path, measured);
    measured
}

/// Fallback implementation using `ignore::WalkBuilder` for non-macOS platforms.
/// Uses physical size (blocks * 512) and deduplicates hard links by inode.
#[cfg(not(target_os = "macos"))]
fn dir_size_fallback(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;

    let Ok(meta) = path.metadata() else {
        return 0;
    };

    if meta.is_file() {
        return meta.blocks() * 512;
    }

    let walker = ignore::WalkBuilder::new(path)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true)
        .build();

    let mut total: u64 = 0;
    let mut seen_inodes = rustc_hash::FxHashSet::default();

    for entry in walker.flatten() {
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            if let Ok(meta) = entry.metadata() {
                let nlink = meta.nlink();
                if nlink > 1 && !seen_inodes.insert(meta.ino()) {
                    continue;
                }
                total += meta.blocks() * 512;
            }
        }
    }
    total
}

/// Fallback implementation for non-macOS platforms.
/// Uses physical size (blocks * 512) and deduplicates hard links by inode.
#[cfg(not(target_os = "macos"))]
fn dir_size_and_count_fallback(path: &Path) -> (u64, usize) {
    use std::os::unix::fs::MetadataExt;

    let Ok(meta) = path.metadata() else {
        return (0, 0);
    };

    if meta.is_file() {
        return (meta.blocks() * 512, 1);
    }

    let walker = ignore::WalkBuilder::new(path)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .follow_links(false)
        .same_file_system(true)
        .build();

    let mut total: u64 = 0;
    let mut count: usize = 0;
    let mut seen_inodes = rustc_hash::FxHashSet::default();

    for entry in walker.flatten() {
        let depth = entry.depth();
        if depth == 0 {
            continue;
        }
        if depth == 1 {
            count += 1;
        }
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            if let Ok(meta) = entry.metadata() {
                let nlink = meta.nlink();
                if nlink > 1 && !seen_inodes.insert(meta.ino()) {
                    continue;
                }
                total += meta.blocks() * 512;
            }
        }
    }

    (total, count)
}
