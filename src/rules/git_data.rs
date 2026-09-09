use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;
use crate::util::{self, CommandOutcome};

const LFS_SIZE_THRESHOLD: u64 = 10 * 1024 * 1024;
const RERERE_THRESHOLD: u64 = 1_048_576; // 1 MB

/// `git count-objects` only reads pack index headers, so it is cheap even on a
/// multi-gigabyte repo. The timeout is a guard against a wedged filesystem.
const COUNT_OBJECTS_TIMEOUT: Duration = Duration::from_secs(10);

/// Temp files younger than this may belong to a repack that is running right now.
const TEMP_FILE_MIN_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub struct GitLfsCacheRule;
pub struct GitGcRule;
pub struct GitTempObjectsRule;
pub struct GitRererecacheRule;

impl CleanupRule for GitLfsCacheRule {
    fn name(&self) -> &'static str {
        "Git LFS Cache"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .object_stores()
            .par_iter()
            .filter_map(|store| {
                let lfs_objects = store.join("lfs/objects");
                if lfs_objects.exists() {
                    let size = walker::dir_size(&lfs_objects);
                    if size > LFS_SIZE_THRESHOLD {
                        return Some(ScannedEntry {
                            path: lfs_objects,
                            size,
                            category: Category::BuildArtifact,
                            safety: SafetyLevel::Caution,
                            description: "Git LFS cache".to_owned(),
                            item_count: None,
                        });
                    }
                }
                None
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Git gc optimization rule
// ---------------------------------------------------------------------------

/// Object-store statistics from `git count-objects -v`. Sizes are in KiB, as git
/// reports them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ObjectStats {
    pub loose_count: u64,
    pub loose_kib: u64,
    pub in_pack: u64,
    pub packs: u64,
    pub prune_packable: u64,
    pub garbage_files: u64,
    pub garbage_kib: u64,
    pub pack_kib: u64,
}

impl ObjectStats {
    /// Bytes a `git gc` run can realistically hand back.
    ///
    /// Anything the repo has already packed stays packed, so a well-maintained
    /// repo scores zero here no matter how large `.git` is. Sizing `.git` and
    /// calling a tenth of it "savings" claimed hundreds of megabytes on repos
    /// where gc had nothing to do.
    pub fn reclaimable_bytes(&self) -> u64 {
        // Leftover temp packs and unreferenced files: gc drops these outright.
        let garbage = self.garbage_kib * 1024;

        // Loose objects already present in a pack are pure duplication.
        // The rest compress down, but not to nothing.
        let loose = self.loose_kib * 1024;
        let loose_reclaim = if self.loose_count == 0 {
            0
        } else {
            let duplicated = loose * self.prune_packable / self.loose_count.max(1);
            let compressible = loose.saturating_sub(duplicated) / 2;
            duplicated + compressible
        };

        // Merging many packs finds cross-pack deltas. The win is small and very
        // repo-dependent, so only claim it once the pack count is clearly untidy.
        let repack = if self.packs > 2 {
            self.pack_kib * 1024 / 50
        } else {
            0
        };

        garbage + loose_reclaim + repack
    }
}

/// Parse the `key: value` lines of `git count-objects -v`.
pub fn parse_count_objects(output: &str) -> ObjectStats {
    let mut stats = ObjectStats::default();

    for line in output.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match key {
            "count" => stats.loose_count = value,
            "size" => stats.loose_kib = value,
            "in-pack" => stats.in_pack = value,
            "packs" => stats.packs = value,
            "prune-packable" => stats.prune_packable = value,
            "garbage" => stats.garbage_files = value,
            "size-garbage" => stats.garbage_kib = value,
            "size-pack" => stats.pack_kib = value,
            _ => {}
        }
    }

    stats
}

pub fn read_object_stats(repo: &Path) -> Option<ObjectStats> {
    let repo = repo.display().to_string();
    let output = util::run_with_timeout(
        &["git", "-C", &repo, "count-objects", "-v"],
        COUNT_OBJECTS_TIMEOUT,
    )
    .success()?;
    Some(parse_count_objects(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Why a repo must be left alone, or `None` when it is idle.
///
/// Repacking under a half-finished rebase or merge rewrites the object store the
/// interrupted command is still holding references into, and `index.lock` means
/// another git process owns the repo right now.
pub fn busy_reason(git_dir: &Path) -> Option<&'static str> {
    const IN_PROGRESS: &[(&str, &str)] = &[
        ("index.lock", "another git process is running"),
        ("gc.pid", "a gc is already running"),
        ("rebase-merge", "a rebase is in progress"),
        ("rebase-apply", "a rebase or am is in progress"),
        ("MERGE_HEAD", "a merge is in progress"),
        ("CHERRY_PICK_HEAD", "a cherry-pick is in progress"),
        ("REVERT_HEAD", "a revert is in progress"),
        ("BISECT_LOG", "a bisect is in progress"),
        ("sequencer", "a sequencer operation is in progress"),
    ];

    let marker_in = |dir: &Path| {
        IN_PROGRESS
            .iter()
            .find(|(marker, _)| dir.join(marker).exists())
            .map(|(_, reason)| *reason)
    };

    if let Some(reason) = marker_in(git_dir) {
        return Some(reason);
    }

    // A rebase running in any linked worktree holds references into the shared
    // object store, so repacking it is just as unsafe as if the operation were
    // in the main tree.
    std::fs::read_dir(git_dir.join("worktrees"))
        .ok()?
        .flatten()
        .find_map(|entry| marker_in(&entry.path()))
}

/// A name for the repository behind an object store.
///
/// A worktree stack keeps its store in a bare directory such as `.bare`, whose
/// own name says nothing; the directory holding it is what the user calls the
/// project.
fn repo_label(store: &Path) -> String {
    let name = store
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    if name == ".git" || name.starts_with('.') {
        if let Some(parent) = store.parent().and_then(Path::file_name) {
            return parent.to_string_lossy().to_string();
        }
    }
    name
}

/// What a pair of `readdir` calls on `.git/objects` can tell us.
///
/// Spawning `git count-objects` for every repository on a developer's machine
/// costs more than the whole rest of the scan -- hundreds of processes, most of
/// them reporting a repository that is already packed. These two directory
/// listings answer "could a repack possibly help here?" without a subprocess,
/// and only the survivors are asked properly.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ObjectDirSurvey {
    /// Fanout directories (`objects/00` .. `objects/ff`) hold loose objects.
    pub loose_fanouts: usize,
    pub packs: usize,
    pub temp_files: bool,
}

impl ObjectDirSurvey {
    /// A repository with no loose objects, no leftovers and a tidy pack count
    /// has nothing for gc to reclaim, however large it is.
    pub fn worth_asking_git(&self) -> bool {
        self.loose_fanouts > 0 || self.temp_files || self.packs > 2
    }
}

fn is_temp_object(name: &str) -> bool {
    name.starts_with("tmp_pack_") || name.starts_with("tmp_obj_")
}

fn is_fanout_dir(name: &str) -> bool {
    name.len() == 2 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn survey_object_dir(git_dir: &Path) -> ObjectDirSurvey {
    let objects = git_dir.join("objects");
    let mut survey = ObjectDirSurvey::default();

    if let Ok(read_dir) = std::fs::read_dir(&objects) {
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if is_fanout_dir(&name) {
                survey.loose_fanouts += 1;
            } else if is_temp_object(&name) {
                survey.temp_files = true;
            }
        }
    }

    if let Ok(read_dir) = std::fs::read_dir(objects.join("pack")) {
        for entry in read_dir.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".pack") {
                survey.packs += 1;
            } else if is_temp_object(&name) {
                survey.temp_files = true;
            }
        }
    }

    survey
}

impl CleanupRule for GitGcRule {
    fn name(&self) -> &'static str {
        "Git gc optimization"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let min_reclaim = config.categories.git_gc_min_reclaim;

        PROJECT_INDEX
            .object_stores()
            .par_iter()
            .filter_map(|store| {
                if !store.is_dir() || busy_reason(store).is_some() {
                    return None;
                }

                if !survey_object_dir(store).worth_asking_git() {
                    return None;
                }

                let stats = read_object_stats(store)?;
                let reclaimable = stats.reclaimable_bytes();
                if reclaimable < min_reclaim {
                    return None;
                }

                let repo_name = repo_label(store);

                let mut detail = vec![format!(
                    "packed {}",
                    util::human_size(stats.pack_kib * 1024)
                )];
                if stats.loose_count > 0 {
                    detail.push(format!(
                        "{} loose objects ({})",
                        stats.loose_count,
                        util::human_size(stats.loose_kib * 1024)
                    ));
                }
                if stats.garbage_files > 0 {
                    detail.push(format!(
                        "{} garbage ({})",
                        stats.garbage_files,
                        util::human_size(stats.garbage_kib * 1024)
                    ));
                }
                if stats.packs > 2 {
                    detail.push(format!("{} packs", stats.packs));
                }

                Some(ScannedEntry {
                    path: PathBuf::from(format!("git-gc:{}", store.display())),
                    size: reclaimable,
                    category: Category::BuildArtifact,
                    safety: SafetyLevel::Caution,
                    description: format!("Git gc: {repo_name} ({})", detail.join(", ")),
                    item_count: None,
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Leftover git temp objects
// ---------------------------------------------------------------------------

/// Half-written packs and objects left behind when a repack is interrupted.
///
/// Git names them `tmp_pack_*` / `tmp_obj_*` and only clears them on the next
/// `prune`, so a repo that was gc'd under a Ctrl-C can sit on gigabytes of them
/// indefinitely. Nothing references them, but the age guard keeps us off the
/// temp files of a repack that is running right now.
impl CleanupRule for GitTempObjectsRule {
    fn name(&self) -> &'static str {
        "Git interrupted-repack leftovers"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .object_stores()
            .par_iter()
            .flat_map(|store| {
                if busy_reason(store).is_some() {
                    return Vec::new();
                }
                let objects = store.join("objects");
                let mut found = stale_temp_files(&objects.join("pack"));
                found.extend(stale_temp_files(&objects));
                found
            })
            .collect()
    }
}

fn stale_temp_files(dir: &Path) -> Vec<ScannedEntry> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !is_temp_object(&name) {
            continue;
        }

        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let recently_touched = meta
            .modified()
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_none_or(|age| age < TEMP_FILE_MIN_AGE);
        if recently_touched {
            continue;
        }

        entries.push(ScannedEntry {
            path: entry.path(),
            size: meta.len(),
            category: Category::BuildArtifact,
            safety: SafetyLevel::Safe,
            description: "Git leftover from an interrupted repack".to_owned(),
            item_count: None,
        });
    }
    entries
}

// ---------------------------------------------------------------------------
// Git rerere cache rule
// ---------------------------------------------------------------------------

impl CleanupRule for GitRererecacheRule {
    fn name(&self) -> &'static str {
        "Git rerere cache"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .object_stores()
            .par_iter()
            .filter_map(|store| {
                let rr_cache = store.join("rr-cache");
                if !rr_cache.exists() {
                    return None;
                }

                let size = walker::dir_size(&rr_cache);
                if size < RERERE_THRESHOLD {
                    return None;
                }

                Some(ScannedEntry {
                    path: rr_cache,
                    size,
                    category: Category::BuildArtifact,
                    safety: SafetyLevel::Safe,
                    description: "Git rerere cache".to_owned(),
                    item_count: None,
                })
            })
            .collect()
    }
}

/// Repack a repository and drop what it no longer needs.
///
/// The entry path is expected to be `git-gc:<repo_path>`. Returns the bytes the
/// object store actually shrank by, measured rather than estimated.
///
/// Deliberately plain `git gc`:
/// - `--aggressive` rebuilds every delta chain from scratch (`--window=250
///   --depth=250`). On a large repo that is tens of minutes of CPU for a
///   single-digit percentage gain, and it is what made this look wedged.
/// - `--prune=now` drops the grace period that protects objects a concurrent
///   git process has written but not yet referenced. The default two-week
///   window is the safety margin, not a missed opportunity.
/// - Reflogs are left to git's own retention (90 days reachable, 30 days
///   unreachable). Expiring them is what makes a lost commit unrecoverable.
pub fn clean_git_gc(entry_path: &str, timeout: Duration) -> Result<Option<u64>, std::io::Error> {
    let repo_path = entry_path.strip_prefix("git-gc:").unwrap_or(entry_path);
    let repo = Path::new(repo_path);

    if let Some(reason) = busy_reason(repo) {
        return Err(std::io::Error::other(format!("skipped: {reason}")));
    }

    let before = read_object_stats(repo);

    match util::run_with_timeout(&["git", "-C", repo_path, "gc"], timeout) {
        CommandOutcome::Completed(output) if output.status.success() => {}
        CommandOutcome::Completed(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.lines().last().unwrap_or("no output").trim();
            return Err(std::io::Error::other(format!(
                "git gc exited with {}: {detail}",
                output.status
            )));
        }
        CommandOutcome::TimedOut => {
            return Err(std::io::Error::other(format!(
                "git gc exceeded {}s and was stopped; leftover temp packs are reported as reclaimable on the next scan",
                timeout.as_secs()
            )));
        }
        CommandOutcome::NotSpawned(e) => return Err(e),
    }

    let after = read_object_stats(repo);
    let freed = match (before, after) {
        (Some(before), Some(after)) => {
            let before_bytes = (before.pack_kib + before.loose_kib + before.garbage_kib) * 1024;
            let after_bytes = (after.pack_kib + after.loose_kib + after.garbage_kib) * 1024;
            Some(before_bytes.saturating_sub(after_bytes))
        }
        _ => None,
    };

    Ok(freed)
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(GitLfsCacheRule),
        Box::new(GitGcRule),
        Box::new(GitTempObjectsRule),
        Box::new(GitRererecacheRule),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKED_REPO: &str = "\
count: 0
size: 0
in-pack: 217456
packs: 2
size-pack: 1980262
prune-packable: 0
garbage: 1
size-garbage: 215552
";

    #[test]
    fn parses_every_field_git_reports() {
        let stats = parse_count_objects(PACKED_REPO);
        assert_eq!(
            stats,
            ObjectStats {
                loose_count: 0,
                loose_kib: 0,
                in_pack: 217_456,
                packs: 2,
                prune_packable: 0,
                garbage_files: 1,
                garbage_kib: 215_552,
                pack_kib: 1_980_262,
            }
        );
    }

    #[test]
    fn a_fully_packed_repo_only_claims_its_garbage() {
        let stats = parse_count_objects(PACKED_REPO);
        // 2 GB of packs that gc would rewrite byte-for-byte must not be counted.
        assert_eq!(stats.reclaimable_bytes(), 215_552 * 1024);
    }

    #[test]
    fn a_clean_repo_offers_nothing() {
        let stats = parse_count_objects("count: 0\nsize: 0\npacks: 1\nsize-pack: 500000\n");
        assert_eq!(stats.reclaimable_bytes(), 0);
    }

    #[test]
    fn loose_objects_already_in_a_pack_are_counted_in_full() {
        let stats = parse_count_objects(
            "count: 100\nsize: 10000\npacks: 1\nsize-pack: 5000\nprune-packable: 100\n",
        );
        assert_eq!(stats.reclaimable_bytes(), 10_000 * 1024);
    }

    #[test]
    fn stray_output_lines_do_not_derail_the_parse() {
        let stats = parse_count_objects(
            "warning: garbage found: .git/objects/pack/tmp_pack_JS4yqz\ncount: 3\nsize: 12\n",
        );
        assert_eq!(stats.loose_count, 3);
        assert_eq!(stats.loose_kib, 12);
    }

    #[test]
    fn an_interrupted_rebase_puts_the_repo_off_limits() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git_dir = tmp.path().join(".git");
        std::fs::create_dir_all(&git_dir).expect("git dir");

        assert!(busy_reason(&git_dir).is_none());

        std::fs::create_dir(git_dir.join("rebase-merge")).expect("rebase state");
        assert_eq!(busy_reason(&git_dir), Some("a rebase is in progress"));
    }

    #[test]
    fn a_rebase_in_a_linked_worktree_also_protects_the_shared_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = tmp.path().join(".bare");
        let worktree_state = store.join("worktrees/feature");
        std::fs::create_dir_all(&worktree_state).expect("worktree state");

        assert!(busy_reason(&store).is_none());

        std::fs::write(worktree_state.join("MERGE_HEAD"), b"abc").expect("merge state");
        assert_eq!(busy_reason(&store), Some("a merge is in progress"));
    }

    #[test]
    fn a_bare_store_is_named_after_the_directory_holding_it() {
        assert_eq!(
            repo_label(Path::new("/w/app-worktrees/.bare")),
            "app-worktrees"
        );
        assert_eq!(repo_label(Path::new("/w/myproject/.git")), "myproject");
        assert_eq!(repo_label(Path::new("/w/bare-clone.git")), "bare-clone.git");
    }

    #[test]
    fn temp_packs_are_only_reclaimed_once_they_are_stale() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pack_dir = tmp.path();
        std::fs::write(pack_dir.join("tmp_pack_fresh"), b"still being written").expect("fresh");
        std::fs::write(pack_dir.join("pack-abc.pack"), b"a real pack").expect("real pack");

        // A repack running right now owns its temp files; nothing here is stale yet.
        assert!(stale_temp_files(pack_dir).is_empty());
    }

    #[test]
    fn a_tidy_object_store_is_never_worth_a_subprocess() {
        let survey = ObjectDirSurvey {
            loose_fanouts: 0,
            packs: 1,
            temp_files: false,
        };
        assert!(!survey.worth_asking_git());
    }

    #[test]
    fn loose_objects_leftovers_or_many_packs_earn_a_closer_look() {
        for survey in [
            ObjectDirSurvey {
                loose_fanouts: 1,
                packs: 1,
                temp_files: false,
            },
            ObjectDirSurvey {
                loose_fanouts: 0,
                packs: 1,
                temp_files: true,
            },
            ObjectDirSurvey {
                loose_fanouts: 0,
                packs: 7,
                temp_files: false,
            },
        ] {
            assert!(survey.worth_asking_git(), "{survey:?}");
        }
    }

    #[test]
    fn the_survey_reads_a_real_object_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let git_dir = tmp.path().join(".git");
        let objects = git_dir.join("objects");
        std::fs::create_dir_all(objects.join("pack")).expect("pack dir");
        std::fs::create_dir(objects.join("ab")).expect("fanout");
        // `info` is two-plus letters but not hex, and must not read as a fanout.
        std::fs::create_dir(objects.join("info")).expect("info");
        std::fs::write(objects.join("pack/pack-1.pack"), b"p").expect("pack");
        std::fs::write(objects.join("pack/pack-1.idx"), b"i").expect("idx");
        std::fs::write(objects.join("pack/tmp_pack_x"), b"t").expect("temp");

        assert_eq!(
            survey_object_dir(&git_dir),
            ObjectDirSurvey {
                loose_fanouts: 1,
                packs: 1,
                temp_files: true,
            }
        );
    }
}
