use std::path::Path;

/// Compute the total size of all files under `path` recursively.
///
/// On macOS, uses `getattrlistbulk` to batch file attribute reads (name + type + size)
/// in a single syscall per directory, eliminating per-file `stat()` overhead.
/// Falls back to `ignore::WalkBuilder` on other platforms.
pub fn dir_size(path: &Path) -> u64 {
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
    #[cfg(target_os = "macos")]
    {
        super::bulk_stat::dir_size_and_count_bulk(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        dir_size_and_count_fallback(path)
    }
}

/// Fallback implementation using `ignore::WalkBuilder` for non-macOS platforms.
/// Uses physical size (blocks * 512) and deduplicates hard links by inode.
#[cfg(not(target_os = "macos"))]
fn dir_size_fallback(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;

    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return path.metadata().map(|m| m.blocks() * 512).unwrap_or(0);
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
                if nlink > 1 {
                    if !seen_inodes.insert(meta.ino()) {
                        continue;
                    }
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

    if !path.exists() {
        return (0, 0);
    }

    if path.is_file() {
        let size = path.metadata().map(|m| m.blocks() * 512).unwrap_or(0);
        return (size, 1);
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
                if nlink > 1 {
                    if !seen_inodes.insert(meta.ino()) {
                        continue;
                    }
                }
                total += meta.blocks() * 512;
            }
        }
    }

    (total, count)
}
