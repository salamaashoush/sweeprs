//! What deleting a directory tree would free, and the memory that keeps a
//! rescan from reading a million unchanged files again.
//!
//! Every directory is listed once per walk: its files' sizes, its hard-linked
//! files, and its subdirectories. Listing is where the time goes; a cache
//! directory of a million build outputs costs seconds of kernel time per scan.
//! A large directory's listing is therefore remembered between scans, keyed by
//! the directory's modification time to the nanosecond: creating, removing or
//! renaming anything in it moves that time and forces a fresh listing. A file
//! rewritten in place does not, so a remembered listing is also only trusted
//! for a day. Measurements taken to see what a delete left behind never use it.

use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// One directory's own contents.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Listing {
    /// Allocated bytes of files with a single link.
    pub files: u64,
    /// `(inode, link count, bytes)` of files with more than one link.
    pub linked: Vec<(u64, u32, u64)>,
    pub subdirs: Vec<String>,
    /// Files, directories and anything else directly inside.
    pub entries: usize,
}

/// Below this many entries a directory is cheaper to list than to remember.
const REMEMBER_FROM: usize = 256;
const TRUST_FOR: Duration = Duration::from_hours(24);
const CACHE_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
struct Remembered {
    mtime: (i64, i64),
    listed_at: u64,
    listing: Listing,
}

#[derive(Default, Serialize, Deserialize)]
struct CacheFile {
    version: u32,
    dirs: FxHashMap<PathBuf, Remembered>,
}

static REMEMBERED: LazyLock<Mutex<FxHashMap<PathBuf, Remembered>>> =
    LazyLock::new(|| Mutex::new(load().unwrap_or_default()));

fn cache_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("sweeprs")
            .join("tree_listings.json"),
    )
}

fn load() -> Option<FxHashMap<PathBuf, Remembered>> {
    let bytes = std::fs::read(cache_path()?).ok()?;
    let file: CacheFile = serde_json::from_slice(&bytes).ok()?;
    (file.version == CACHE_VERSION).then_some(file.dirs)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Write the remembered listings back, dropping any too old to be trusted.
pub fn persist() {
    let Some(path) = cache_path() else {
        return;
    };
    let Ok(dirs) = REMEMBERED.lock() else {
        return;
    };
    let now = now();
    let file = CacheFile {
        version: CACHE_VERSION,
        dirs: dirs
            .iter()
            .filter(|(_, r)| now.saturating_sub(r.listed_at) < TRUST_FOR.as_secs())
            .map(|(p, r)| (p.clone(), r.clone()))
            .collect(),
    };
    if let (Some(parent), Ok(json)) = (path.parent(), serde_json::to_vec(&file)) {
        let _ = std::fs::create_dir_all(parent);
        let _ = std::fs::write(path, json);
    }
}

fn mtime(meta: &std::fs::Metadata) -> (i64, i64) {
    (meta.mtime(), meta.mtime_nsec())
}

fn recall(dir: &Path, meta: &std::fs::Metadata) -> Option<Listing> {
    let dirs = REMEMBERED.lock().ok()?;
    let remembered = dirs.get(dir)?;
    (remembered.mtime == mtime(meta)
        && now().saturating_sub(remembered.listed_at) < TRUST_FOR.as_secs())
    .then(|| remembered.listing.clone())
}

fn remember(dir: &Path, meta: &std::fs::Metadata, listing: &Listing) {
    if listing.entries < REMEMBER_FROM {
        return;
    }
    if let Ok(mut dirs) = REMEMBERED.lock() {
        dirs.insert(
            dir.to_path_buf(),
            Remembered {
                mtime: mtime(meta),
                listed_at: now(),
                listing: listing.clone(),
            },
        );
    }
}

/// List one directory, from memory when `use_memory` allows and it is unchanged.
fn listing(dir: &Path, root_dev: u64, use_memory: bool) -> Option<Listing> {
    let meta = dir.symlink_metadata().ok()?;
    if use_memory && let Some(listing) = recall(dir, &meta) {
        return Some(listing);
    }
    let listing = list_dir(dir, root_dev)?;
    if use_memory {
        remember(dir, &meta, &listing);
    }
    Some(listing)
}

#[cfg(target_os = "macos")]
fn list_dir(dir: &Path, root_dev: u64) -> Option<Listing> {
    super::bulk_stat::list_dir(dir, root_dev)
}

#[cfg(not(target_os = "macos"))]
fn list_dir(dir: &Path, root_dev: u64) -> Option<Listing> {
    let mut listing = Listing::default();
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        listing.entries += 1;
        let Ok(meta) = entry.path().symlink_metadata() else {
            continue;
        };
        if meta.is_dir() {
            if meta.dev() == root_dev {
                listing
                    .subdirs
                    .push(entry.file_name().to_string_lossy().into_owned());
            }
        } else if meta.is_file() {
            let size = meta.blocks() * 512;
            if meta.nlink() > 1 {
                listing.linked.push((
                    meta.ino(),
                    u32::try_from(meta.nlink()).unwrap_or(u32::MAX),
                    size,
                ));
            } else {
                listing.files += size;
            }
        }
    }
    Some(listing)
}

/// What deleting a tree returns, accumulated while walking it.
///
/// A file with several hard links frees its blocks only when its last link
/// goes, so such a file counts only once every one of its links has been seen
/// inside the tree. `node_modules` hard-linked to a package store frees almost
/// nothing, and is reported that way.
#[derive(Default)]
struct Tally {
    total: u64,
    linked: FxHashMap<u64, (u32, u32, u64)>,
}

impl Tally {
    fn add(&mut self, listing: &Listing) {
        self.total += listing.files;
        for &(inode, nlink, size) in &listing.linked {
            self.linked.entry(inode).or_insert((0, nlink, size)).0 += 1;
        }
    }

    fn merge(&mut self, other: Self) {
        self.total += other.total;
        for (inode, (seen, nlink, size)) in other.linked {
            self.linked.entry(inode).or_insert((0, nlink, size)).0 += seen;
        }
    }

    fn reclaimable(&self) -> u64 {
        self.total
            + self
                .linked
                .values()
                .filter(|(seen, nlink, _)| seen >= nlink)
                .map(|(_, _, size)| size)
                .sum::<u64>()
    }
}

fn walk(dir: &Path, tally: &mut Tally, root_dev: u64, depth: usize, use_memory: bool) {
    let Some(listing) = listing(dir, root_dev, use_memory) else {
        return;
    };
    tally.add(&listing);
    let subdirs: Vec<PathBuf> = listing.subdirs.iter().map(|name| dir.join(name)).collect();
    // A few levels in parallel spreads a lopsided tree over every core.
    if depth <= 2 && subdirs.len() >= 8 {
        let branches: Vec<Tally> = subdirs
            .par_iter()
            .map(|subdir| {
                let mut local = Tally::default();
                walk(subdir, &mut local, root_dev, depth + 1, use_memory);
                local
            })
            .collect();
        for branch in branches {
            tally.merge(branch);
        }
    } else {
        for subdir in &subdirs {
            walk(subdir, tally, root_dev, depth + 1, use_memory);
        }
    }
}

/// What deleting `path` would free, and how many entries it holds directly.
///
/// `use_memory` lets unchanged large directories be read from the listings
/// remembered by earlier scans.
pub fn measure(path: &Path, use_memory: bool) -> (u64, usize) {
    let Ok(meta) = path.metadata() else {
        return (0, 0);
    };
    if meta.is_file() {
        return (meta.blocks() * 512, 1);
    }
    let Some(top) = listing(path, meta.dev(), use_memory) else {
        return (0, 0);
    };
    let mut tally = Tally::default();
    tally.add(&top);
    let entries = top.entries;
    let subdirs: Vec<PathBuf> = top.subdirs.iter().map(|name| path.join(name)).collect();
    let branches: Vec<Tally> = subdirs
        .par_iter()
        .map(|subdir| {
            let mut local = Tally::default();
            walk(subdir, &mut local, meta.dev(), 1, use_memory);
            local
        })
        .collect();
    for branch in branches {
        tally.merge(branch);
    }
    (tally.reclaimable(), entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: usize = 1024 * 1024;

    fn write(path: &Path, bytes: usize) {
        // Distinct content per file so APFS cannot share blocks behind our back.
        let fill = u8::try_from(path.as_os_str().len() % 251).unwrap_or(7);
        std::fs::write(path, vec![fill; bytes]).expect("write");
    }

    #[test]
    fn a_hard_link_outside_the_tree_keeps_its_blocks_alive() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tree = tmp.path().join("node_modules");
        std::fs::create_dir(&tree).expect("tree");
        write(&tree.join("pkg.js"), MIB);
        std::fs::hard_link(tree.join("pkg.js"), tmp.path().join("store-copy")).expect("link");

        assert_eq!(measure(&tree, false).0, 0);

        std::fs::remove_file(tmp.path().join("store-copy")).expect("unlink");
        std::fs::hard_link(tree.join("pkg.js"), tree.join("alias.js")).expect("inner link");
        let both_inside = measure(&tree, false).0;
        assert!(
            (MIB as u64..2 * MIB as u64).contains(&both_inside),
            "{both_inside}"
        );
    }

    #[test]
    fn hard_links_split_across_parallel_branches_are_counted_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tree = tmp.path().join("tree");
        for i in 0..10 {
            std::fs::create_dir_all(tree.join(format!("b{i}"))).expect("branch");
        }
        write(&tree.join("b0/file"), MIB);
        std::fs::hard_link(tree.join("b0/file"), tree.join("b9/file")).expect("link");

        let size = measure(&tree, false).0;
        assert!((MIB as u64..2 * MIB as u64).contains(&size), "{size}");
    }

    #[test]
    fn a_remembered_listing_is_used_until_the_directory_changes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tree = tmp.path().join("cache");
        std::fs::create_dir(&tree).expect("tree");
        for i in 0..REMEMBER_FROM {
            write(&tree.join(format!("obj{i}")), 4096);
        }
        let first = measure(&tree, true).0;
        assert!(first >= (REMEMBER_FROM * 4096) as u64);

        // Pretend the remembered listing says something else: it is trusted
        // while the directory is unchanged...
        if let Ok(mut dirs) = REMEMBERED.lock()
            && let Some(remembered) = dirs.get_mut(&tree)
        {
            remembered.listing.files = 1;
        }
        assert_eq!(measure(&tree, true).0, 1);
        assert_eq!(measure(&tree, false).0, first);

        // ...and dropped once an entry is added, which moves the mtime.
        write(&tree.join("new"), 4096);
        assert_eq!(measure(&tree, true).0, first + 4096);
    }

    #[test]
    fn small_directories_are_not_remembered() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tree = tmp.path().join("small");
        std::fs::create_dir(&tree).expect("tree");
        write(&tree.join("one"), 4096);
        measure(&tree, true);
        assert!(
            REMEMBERED
                .lock()
                .is_ok_and(|dirs| !dirs.contains_key(&tree))
        );
    }
}
