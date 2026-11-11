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
    if let Ok(mut cache) = SIZE_CACHE.write()
        && let Some(cache) = cache.as_mut()
    {
        cache.insert(path.to_path_buf(), measured);
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

/// What deleting `path` would free, in allocated bytes.
pub fn dir_size(path: &Path) -> u64 {
    dir_size_and_count(path).0
}

/// Measure a directory without consulting or updating any cache.
///
/// Used after a delete, where the whole point is to see what is left on disk.
pub fn dir_size_uncached(path: &Path) -> u64 {
    super::tree::measure(path, false).0
}

/// What deleting `path` would free, and how many entries it holds directly.
///
/// Within a scan each path is measured once; across scans, unchanged large
/// directories are read from the listings remembered last time.
pub fn dir_size_and_count(path: &Path) -> (u64, usize) {
    if let Some(measured) = cached(path) {
        return measured;
    }
    let measured = super::tree::measure(path, true);
    remember(path, measured);
    measured
}

/// Keep what this scan listed for the next one.
pub fn persist_listings() {
    super::tree::persist();
}
