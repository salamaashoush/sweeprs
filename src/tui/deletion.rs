//! Deleting selected entries off the UI thread.
//!
//! Removing a VM image or running `docker prune` takes minutes. Done inline in
//! the event loop, the screen stops redrawing for all of it and looks hung, so
//! the work runs on its own thread and reports each item as it starts and ends.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::cleaner;
use crate::config::Config;
use crate::scanner::entry::{SafetyLevel, ScannedEntry};
use crate::virtual_entry;
use crate::{guard, in_use, journal};

enum Update {
    Started(usize),
    Finished {
        index: usize,
        freed: u64,
        error: Option<String>,
    },
    Done,
}

/// How one entry ended.
pub struct Outcome {
    pub label: String,
    pub size: u64,
    pub freed: u64,
    pub error: Option<String>,
}

pub struct DeletionJob {
    pub entries: Vec<ScannedEntry>,
    pub outcomes: Vec<Outcome>,
    /// The entry being worked on right now.
    pub current: Option<usize>,
    pub started: Instant,
    /// Set once the worker has stopped, with how long the whole run took.
    pub finished: Option<Duration>,
    stop: Arc<AtomicBool>,
    /// How far through the current item the worker is, in thousandths.
    item_progress: Arc<AtomicU64>,
    rx: Receiver<Update>,
}

impl DeletionJob {
    pub fn start(entries: Vec<ScannedEntry>, config: Config) -> Self {
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let item_progress = Arc::new(AtomicU64::new(0));
        let worker_entries = entries.clone();
        let worker_stop = Arc::clone(&stop);
        let worker_progress = Arc::clone(&item_progress);
        thread::spawn(move || {
            run(
                &worker_entries,
                &config,
                &worker_stop,
                &worker_progress,
                &tx,
            );
        });

        Self {
            entries,
            outcomes: Vec::new(),
            current: None,
            started: Instant::now(),
            finished: None,
            stop,
            item_progress,
            rx,
        }
    }

    /// Take whatever the worker has reported. Returns whether anything changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(update) = self.rx.try_recv() {
            changed = true;
            match update {
                Update::Started(index) => self.current = Some(index),
                Update::Finished {
                    index,
                    freed,
                    error,
                } => {
                    let entry = &self.entries[index];
                    self.outcomes.push(Outcome {
                        label: virtual_entry::display(&entry.path),
                        size: entry.size,
                        freed,
                        error,
                    });
                    self.current = None;
                }
                Update::Done => {
                    self.current = None;
                    self.finished = Some(self.started.elapsed());
                }
            }
        }
        changed
    }

    /// Finish the item in progress, then start no more.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopping(&self) -> bool {
        self.stop.load(Ordering::Relaxed) && self.finished.is_none()
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_some()
    }

    pub fn freed(&self) -> u64 {
        self.outcomes.iter().map(|o| o.freed).sum()
    }

    /// Bytes processed so far, for the progress gauge: every finished item in
    /// full plus the estimated share of the one in progress.
    pub fn processed(&self) -> u64 {
        let finished: u64 = self.outcomes.iter().map(|o| o.size).sum();
        let current = self.current.map_or(0, |index| {
            let permille = self.item_progress.load(Ordering::Relaxed).min(1000);
            self.entries[index].size / 1000 * permille
        });
        finished + current
    }

    pub fn planned(&self) -> u64 {
        self.entries.iter().map(|e| e.size).sum()
    }

    /// Paths that are gone and should leave the tree.
    pub fn removed_paths(&self) -> Vec<PathBuf> {
        self.outcomes
            .iter()
            .zip(&self.entries)
            .filter(|(outcome, _)| outcome.error.is_none())
            .map(|(_, entry)| entry.path.clone())
            .collect()
    }
}

fn run(
    entries: &[ScannedEntry],
    config: &Config,
    stop: &AtomicBool,
    progress: &AtomicU64,
    tx: &Sender<Update>,
) {
    let open_files = in_use::OpenFiles::snapshot();
    for (index, entry) in entries.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        progress.store(0, Ordering::Relaxed);
        if tx.send(Update::Started(index)).is_err() {
            return;
        }
        let (freed, error) = delete_one(entry, config, open_files.as_ref(), progress);
        if tx
            .send(Update::Finished {
                index,
                freed,
                error,
            })
            .is_err()
        {
            return;
        }
    }
    let _ = tx.send(Update::Done);
}

fn delete_one(
    entry: &ScannedEntry,
    config: &Config,
    open_files: Option<&in_use::OpenFiles>,
    progress: &AtomicU64,
) -> (u64, Option<String>) {
    if entry.safety == SafetyLevel::Error {
        return (0, Some("kept by its rule".to_owned()));
    }

    if virtual_entry::is_virtual(&entry.path) {
        let path = entry.path.display().to_string();
        return match virtual_entry::clean(&path, config, config.git_gc_timeout()) {
            Ok(freed) => {
                let freed = freed.unwrap_or(entry.size);
                journal::record(entry, freed);
                (freed, None)
            }
            Err(e) => (0, Some(e.to_string())),
        };
    }

    if cleaner::needs_root(&entry.path) {
        return (0, Some("requires sudo".to_owned()));
    }
    if !entry.path.exists() {
        return (0, None);
    }
    // Checked before any part goes: the parts are deleted ahead of the root.
    if let Err(refusal) = guard::check(&entry.path) {
        return (0, Some(refusal.to_string()));
    }
    if let Some(open) = open_files.and_then(|files| files.held_under(&entry.path)) {
        return (0, Some(format!("in use: {} is open", open.display())));
    }

    let result = delete_in_parts(&entry.path, progress);
    let freed = cleaner::measure_freed(entry, &result);
    if freed > 0 {
        journal::record(entry, freed);
    }
    (freed, result.err().map(|e| e.to_string()))
}

/// Parts a directory is split into so its deletion can report progress.
const TARGET_PARTS: usize = 32;
const MAX_SPLIT_DEPTH: usize = 3;

/// The pieces of `root` to delete one by one: its children, or their
/// children when there are only a few, down to `MAX_SPLIT_DEPTH` levels.
///
/// Symlinks are parts, never split: following one would reach outside the
/// tree being deleted.
fn split_into_parts(root: &std::path::Path) -> Vec<PathBuf> {
    let is_real_dir = |p: &std::path::Path| p.symlink_metadata().is_ok_and(|m| m.is_dir());
    let mut parts = vec![root.to_path_buf()];
    for _ in 0..MAX_SPLIT_DEPTH {
        if parts.len() >= TARGET_PARTS {
            break;
        }
        let mut next = Vec::new();
        for part in &parts {
            match std::fs::read_dir(part) {
                Ok(read_dir) if is_real_dir(part) => {
                    next.extend(read_dir.flatten().map(|e| e.path()));
                }
                _ => next.push(part.clone()),
            }
        }
        if next.is_empty() {
            break;
        }
        parts = next;
    }
    parts
}

/// Delete `path` a part at a time, publishing how far through it is.
///
/// The final `delete_path` removes whatever is left, empty directories and
/// any part that failed the first time, with its permission-fixing retry and
/// the AVD handling that goes with removing the whole entry.
fn delete_in_parts(path: &std::path::Path, progress: &AtomicU64) -> std::io::Result<()> {
    if path.symlink_metadata().is_ok_and(|m| m.is_dir()) {
        let parts = split_into_parts(path);
        let count = parts.len() as u64;
        for (done, part) in parts.iter().enumerate() {
            if part != path {
                let _ = cleaner::delete_path(part);
            }
            progress.store((done as u64 + 1) * 1000 / count.max(1), Ordering::Relaxed);
        }
    }
    let result = cleaner::delete_path(path);
    progress.store(1000, Ordering::Relaxed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::entry::Category;

    fn entry(path: PathBuf, size: u64) -> ScannedEntry {
        ScannedEntry {
            path,
            size,
            category: Category::PackageCache,
            safety: SafetyLevel::Safe,
            description: String::new(),
            item_count: None,
        }
    }

    fn wait(job: &mut DeletionJob) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !job.is_finished() {
            assert!(Instant::now() < deadline, "deletion never finished");
            job.poll();
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn deletes_in_the_background_and_reports_each_item() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache = tmp.path().join("cache");
        std::fs::create_dir(&cache).expect("cache");
        std::fs::write(cache.join("blob"), vec![0u8; 8192]).expect("blob");
        let privileged = PathBuf::from("/Library/Logs/acme");

        let mut job = DeletionJob::start(
            vec![entry(cache.clone(), 8192), entry(privileged.clone(), 10)],
            Config::default(),
        );
        wait(&mut job);

        assert!(!cache.exists());
        assert_eq!(job.outcomes.len(), 2);
        assert!(job.outcomes[0].error.is_none());
        assert_eq!(job.outcomes[1].error.as_deref(), Some("requires sudo"));
        assert_eq!(job.removed_paths(), [cache]);
    }

    #[test]
    fn a_stop_before_an_item_starts_leaves_it_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let kept = tmp.path().join("kept");
        std::fs::create_dir(&kept).expect("kept");

        let (tx, rx) = mpsc::channel();
        run(
            &[entry(kept.clone(), 1)],
            &Config::default(),
            &AtomicBool::new(true),
            &AtomicU64::new(0),
            &tx,
        );

        assert!(kept.exists());
        assert!(matches!(rx.try_recv(), Ok(Update::Done)));
    }

    #[test]
    fn a_tree_is_deleted_in_parts_without_following_symlinks_out_of_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let outside = tmp.path().join("outside");
        std::fs::create_dir(&outside).expect("outside");
        std::fs::write(outside.join("keep.txt"), b"keep").expect("keep");

        let root = tmp.path().join("cache");
        for d in 0..3 {
            let sub = root.join(format!("d{d}/inner"));
            std::fs::create_dir_all(&sub).expect("sub");
            std::fs::write(sub.join("f"), b"x").expect("file");
        }
        std::os::unix::fs::symlink(&outside, root.join("d0/link")).expect("symlink");

        let parts = split_into_parts(&root);
        assert!(parts.len() > 3, "a sparse top level is split further");
        assert!(parts.iter().all(|p| p.starts_with(&root)));

        let progress = AtomicU64::new(0);
        delete_in_parts(&root, &progress).expect("deleted");

        assert!(!root.exists());
        assert!(
            outside.join("keep.txt").exists(),
            "the symlink's target survives"
        );
        assert_eq!(progress.load(Ordering::Relaxed), 1000);
    }
}
