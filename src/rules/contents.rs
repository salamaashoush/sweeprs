//! Offer what is inside a shared directory, one item at a time.
//!
//! Temp, log and trash directories are themselves part of the system: other
//! users' files, live IPC endpoints and the directory's own permissions all
//! live there. Rules that clean them offer each item inside, never the root.

use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;
use std::time::SystemTime;

use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

pub struct Contents<'a> {
    pub category: Category,
    pub safety: SafetyLevel,
    pub description: &'a str,
    /// Offer only items nothing inside has been modified since; `None` takes any age.
    pub untouched_since: Option<SystemTime>,
}

impl Contents<'_> {
    pub fn push(&self, entries: &mut Vec<ScannedEntry>, dir: &Path) {
        self.push_as(entries, dir, nix::unistd::getuid().as_raw());
    }

    fn push_as(&self, entries: &mut Vec<ScannedEntry>, dir: &Path, uid: u32) {
        let Ok(read_dir) = std::fs::read_dir(dir) else {
            return;
        };
        let mut found: Vec<ScannedEntry> = read_dir
            .flatten()
            .filter_map(|item| {
                let path = item.path();
                let size = removable_size(&path, self.untouched_since, uid)?;
                Some(ScannedEntry {
                    path,
                    size,
                    category: self.category,
                    safety: self.safety,
                    description: self.description.to_owned(),
                    item_count: None,
                })
            })
            .collect();
        self.name_repeat_producers(&mut found);
        entries.append(&mut found);
    }

    /// Name the producer of items that share a stem.
    ///
    /// Thousands of `tool-run-x8Kq2Z` directories are one tool that does not
    /// clean up after itself; saying so is more useful than listing them.
    fn name_repeat_producers(&self, found: &mut [ScannedEntry]) {
        const REPEATS: usize = 3;
        let mut counts: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();
        for entry in found.iter() {
            if let Some(stem) = entry
                .path
                .file_name()
                .and_then(|n| random_suffix_stem(&n.to_string_lossy()))
            {
                *counts.entry(stem).or_default() += 1;
            }
        }
        for entry in found.iter_mut() {
            let Some(stem) = entry
                .path
                .file_name()
                .and_then(|n| random_suffix_stem(&n.to_string_lossy()))
            else {
                continue;
            };
            if let Some(&count) = counts.get(&stem)
                && count >= REPEATS
            {
                entry.description = format!("{}: {stem}* ({count} left behind)", self.description);
            }
        }
    }
}

/// The part of a name before a random-looking suffix, if it has one.
///
/// A suffix is random when it follows a separator, is at least six letters or
/// digits long, and mixes in a digit or both cases, which `mktemp` and
/// `tempfile` names do and words do not.
fn random_suffix_stem(name: &str) -> Option<String> {
    let cut = name.rfind(['-', '_', '.', ','])?;
    let (stem, suffix) = (&name[..cut], &name[cut + 1..]);
    let random = suffix.len() >= 6
        && suffix.chars().all(|c| c.is_ascii_alphanumeric())
        && (suffix.chars().any(|c| c.is_ascii_digit())
            || (suffix.chars().any(|c| c.is_ascii_uppercase())
                && suffix.chars().any(|c| c.is_ascii_lowercase())));
    (random && !stem.is_empty()).then(|| format!("{stem}{}", &name[cut..=cut]))
}

/// Allocated size of an item that is safe to remove, or `None` if it is not.
///
/// A directory's own modification time says nothing about the files inside it,
/// so every descendant is checked. A socket or FIFO anywhere in the tree means a
/// process may still be listening on it, however old it looks: a daemon's
/// socket directory keeps the timestamp of the day the daemon started. An item
/// owned by someone else cannot be removed whole, and a partial removal is
/// worse than none.
pub fn removable_size(path: &Path, untouched_since: Option<SystemTime>, uid: u32) -> Option<u64> {
    let mut total = 0u64;
    for entry in ignore::WalkBuilder::new(path)
        .standard_filters(false)
        .build()
    {
        let meta = entry.ok()?.path().symlink_metadata().ok()?;
        let kind = meta.file_type();
        if kind.is_socket() || kind.is_fifo() || kind.is_char_device() || kind.is_block_device() {
            return None;
        }
        if meta.uid() != uid {
            return None;
        }
        if let Some(cutoff) = untouched_since
            && meta.modified().ok()? >= cutoff
        {
            return None;
        }
        total += meta.blocks() * 512;
    }
    (total > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    fn uid() -> u32 {
        nix::unistd::getuid().as_raw()
    }

    fn age(path: &Path, when: SystemTime) {
        std::fs::File::open(path)
            .expect("open")
            .set_modified(when)
            .expect("set mtime");
    }

    fn scan(dir: &Path, untouched_since: Option<SystemTime>, uid: u32) -> Vec<PathBuf> {
        let contents = Contents {
            category: Category::SystemJunk,
            safety: SafetyLevel::Caution,
            description: "temp",
            untouched_since,
        };
        let mut entries = Vec::new();
        contents.push_as(&mut entries, dir, uid);
        let mut paths: Vec<PathBuf> = entries.into_iter().map(|e| e.path).collect();
        paths.sort();
        paths
    }

    #[test]
    fn items_are_listed_one_by_one_and_never_the_directory_itself() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        std::fs::write(dir.join("a.log"), b"old").expect("file");
        std::fs::create_dir(dir.join("build")).expect("dir");
        std::fs::write(dir.join("build/out.o"), b"old").expect("nested");

        assert_eq!(
            scan(dir, None, uid()),
            vec![dir.join("a.log"), dir.join("build")]
        );
    }

    #[test]
    fn a_directory_holding_a_socket_is_left_alone_however_old() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        std::fs::create_dir(dir.join("ipc")).expect("ipc dir");
        let _listener = std::os::unix::net::UnixListener::bind(dir.join("ipc/d")).expect("bind");
        std::fs::write(dir.join("ipc/lock"), b"1").expect("lock");

        assert_eq!(scan(dir, None, uid()), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_old_directory_with_a_recent_file_inside_is_not_offered() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();
        let two_hours_ago = SystemTime::now() - Duration::from_secs(7200);
        let one_hour_ago = SystemTime::now() - Duration::from_secs(3600);

        std::fs::create_dir(dir.join("active")).expect("dir");
        std::fs::write(dir.join("active/state"), b"fresh").expect("recent file");
        age(&dir.join("active"), two_hours_ago);

        std::fs::create_dir(dir.join("idle")).expect("dir");
        std::fs::write(dir.join("idle/state"), b"stale").expect("old file");
        age(&dir.join("idle/state"), two_hours_ago);
        age(&dir.join("idle"), two_hours_ago);

        assert_eq!(scan(dir, Some(one_hour_ago), uid()), vec![dir.join("idle")]);
    }

    #[test]
    fn random_suffixes_are_recognised_and_words_are_not() {
        assert_eq!(
            random_suffix_stem("ferridriver-integration-,3gAVce").as_deref(),
            Some("ferridriver-integration-,")
        );
        assert_eq!(random_suffix_stem(".tmpA83lWJ").as_deref(), None);
        assert_eq!(
            random_suffix_stem("tool-run_x8Kq2Z").as_deref(),
            Some("tool-run_")
        );
        assert_eq!(random_suffix_stem("com.apple.remotemanagement"), None);
        assert_eq!(random_suffix_stem("build-output"), None);
    }

    #[test]
    fn three_items_with_one_stem_name_their_producer() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for name in ["run-a1b2c3", "run-d4e5f6", "run-g7h8i9", "notes.txt"] {
            std::fs::write(tmp.path().join(name), b"x").expect("file");
        }
        let contents = Contents {
            category: Category::SystemJunk,
            safety: SafetyLevel::Caution,
            description: "User temp files",
            untouched_since: None,
        };
        let mut entries = Vec::new();
        contents.push_as(&mut entries, tmp.path(), uid());
        let mut descriptions: Vec<_> = entries.iter().map(|e| e.description.as_str()).collect();
        descriptions.sort_unstable();
        assert_eq!(
            descriptions,
            [
                "User temp files",
                "User temp files: run-* (3 left behind)",
                "User temp files: run-* (3 left behind)",
                "User temp files: run-* (3 left behind)",
            ]
        );
    }

    #[test]
    fn items_owned_by_someone_else_are_skipped() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("theirs"), b"x").expect("file");

        assert_eq!(scan(tmp.path(), None, uid() + 1), Vec::<PathBuf>::new());
    }
}
