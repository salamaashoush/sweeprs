//! Conversation records and rewind state written by coding agents.
//!
//! These are not caches. A transcript is the only copy of what an agent and a
//! person said to each other, a checkpoint is the only way to undo an edit the
//! agent made, and neither is re-downloadable. Nothing here is ever `Safe`, and
//! the category as a whole is `Danger` so the default clean and the
//! `[c]aution+safe` prompt both leave it alone.
//!
//! What makes it offerable at all is age: a session untouched for a month is
//! past the point where `--resume`, `--continue` or a rewind reaches it.
//!
//! Agent worktrees hold checked-out source with their own `.git`, so they are
//! only ever offered once git itself confirms there is nothing in them to lose:
//! no uncommitted change, no untracked file, and a HEAD that some remote branch
//! already contains.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

/// Sessions below this are noise in a listing; the aggregate is what matters.
const MIN_SESSION_SIZE: u64 = 262_144;

/// Scratch and log directories worth listing on their own.
const MIN_SCRATCH_SIZE: u64 = 1_048_576;

pub struct AgentTranscriptRule;
pub struct AgentCheckpointRule;
pub struct AgentScratchRule;
pub struct OrphanedAgentStateRule;
pub struct AgentWorktreeRule;
pub struct AgentScratchpadRule;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

/// Age of the most recently touched file under `path`.
///
/// A session directory's own mtime only tracks its last direct child change, so
/// a transcript appended to yesterday inside a directory created months ago
/// would otherwise look abandoned.
fn newest_mtime(path: &Path) -> Option<SystemTime> {
    let meta = path.symlink_metadata().ok()?;
    if !meta.is_dir() {
        return meta.modified().ok();
    }

    let mut newest = meta.modified().ok();
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return newest;
    };
    for entry in read_dir.flatten() {
        if let Some(child) = newest_mtime(&entry.path()) {
            newest = Some(newest.map_or(child, |n| n.max(child)));
        }
    }
    newest
}

fn idle_for(path: &Path) -> Option<Duration> {
    SystemTime::now().duration_since(newest_mtime(path)?).ok()
}

/// `(days_idle, size)` for a path that has been untouched longer than `cutoff`.
fn aged(path: &Path, cutoff: Duration) -> Option<(u64, u64)> {
    let idle = idle_for(path)?;
    if idle < cutoff {
        return None;
    }
    let size = if path.is_dir() {
        walker::dir_size(path)
    } else {
        walker::file_size(path)
    };
    Some((idle.as_secs() / 86_400, size))
}

fn cutoff(config: &Config) -> Duration {
    Duration::from_secs(config.categories.agent_session_days * 86_400)
}

fn entry(path: PathBuf, size: u64, safety: SafetyLevel, description: String) -> ScannedEntry {
    ScannedEntry {
        path,
        size,
        category: Category::AgentSession,
        safety,
        description,
        item_count: None,
    }
}

/// Direct children of `dir`, or nothing if it does not exist.
fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|read_dir| read_dir.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Transcripts
// ---------------------------------------------------------------------------

/// Claude Code keeps `<session>.jsonl` next to a `<session>/` directory of tool
/// results. They are one session and are reported as one entry.
fn claude_transcripts(cutoff: Duration) -> Vec<ScannedEntry> {
    let projects = home().join(".claude/projects");

    children(&projects)
        .par_iter()
        .flat_map(|slug| {
            let project = slug
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .replace('-', "/");

            children(slug)
                .into_iter()
                .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                .filter_map(|transcript| {
                    let sidecar = transcript.with_extension("");
                    let idle =
                        idle_for(&transcript)?.min(idle_for(&sidecar).unwrap_or(Duration::MAX));
                    if idle < cutoff {
                        return None;
                    }

                    let size = walker::file_size(&transcript) + walker::dir_size(&sidecar);
                    if size < MIN_SESSION_SIZE {
                        return None;
                    }
                    Some(entry(
                        transcript,
                        size,
                        SafetyLevel::Danger,
                        format!(
                            "Claude Code transcript, {} days idle ({project})",
                            idle.as_secs() / 86_400
                        ),
                    ))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Codex writes one `rollout-*.jsonl` per session under `sessions/YYYY/MM/DD`.
fn codex_transcripts(cutoff: Duration) -> Vec<ScannedEntry> {
    let mut found = Vec::new();
    let sessions = home().join(".codex/sessions");

    for year in children(&sessions) {
        for month in children(&year) {
            for day in children(&month) {
                for file in children(&day) {
                    if file.extension().is_none_or(|e| e != "jsonl") {
                        continue;
                    }
                    let Some((days, size)) = aged(&file, cutoff) else {
                        continue;
                    };
                    if size < MIN_SESSION_SIZE {
                        continue;
                    }
                    found.push(entry(
                        file,
                        size,
                        SafetyLevel::Danger,
                        format!("Codex transcript, {days} days idle"),
                    ));
                }
            }
        }
    }

    found
}

/// Cursor's agent sessions and chat threads, one directory each.
fn cursor_transcripts(cutoff: Duration) -> Vec<ScannedEntry> {
    let cursor = home().join(".cursor");
    let sources = [
        ("acp-sessions", "Cursor agent session"),
        ("chats", "Cursor chat thread"),
    ];

    sources
        .into_iter()
        .flat_map(|(dir, label)| {
            children(&cursor.join(dir))
                .into_iter()
                .filter_map(move |path| {
                    let (days, size) = aged(&path, cutoff)?;
                    (size >= MIN_SESSION_SIZE).then(|| {
                        entry(
                            path,
                            size,
                            SafetyLevel::Danger,
                            format!("{label}, {days} days idle"),
                        )
                    })
                })
        })
        .collect()
}

impl CleanupRule for AgentTranscriptRule {
    fn name(&self) -> &'static str {
        "Agent session transcripts"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);
        let mut entries = claude_transcripts(cutoff);
        entries.extend(codex_transcripts(cutoff));
        entries.extend(cursor_transcripts(cutoff));
        entries
    }
}

// ---------------------------------------------------------------------------
// Rewind checkpoints
// ---------------------------------------------------------------------------

/// Snapshots of files as they were before an agent edited them.
///
/// Claude Code keys them by session id; Cursor keeps a bare git repository per
/// workspace under `snapshots/`. Either way this is the undo history for agent
/// edits, and it is the last thing to reach for.
impl CleanupRule for AgentCheckpointRule {
    fn name(&self) -> &'static str {
        "Agent rewind checkpoints"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);
        let home = home();
        let sources = [
            (
                home.join(".claude/file-history"),
                "Claude Code file checkpoints",
            ),
            (home.join(".cursor/snapshots"), "Cursor edit snapshots"),
        ];

        sources
            .into_iter()
            .flat_map(|(dir, label)| {
                children(&dir).into_iter().filter_map(move |path| {
                    let (days, size) = aged(&path, cutoff)?;
                    (size >= MIN_SESSION_SIZE).then(|| {
                        entry(
                            path,
                            size,
                            SafetyLevel::Danger,
                            format!("{label}, {days} days idle"),
                        )
                    })
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Scratch, logs and temp
// ---------------------------------------------------------------------------

/// Per-session working files the agent regenerates rather than reads back.
///
/// Named one directory at a time on purpose. A bare `~/.codex` or `~/.claude`
/// also holds `auth.json`, `config.toml`, memories, rules and skills, none of
/// which come back.
const SCRATCH_DIRS: &[(&str, &str)] = &[
    (".claude/shell-snapshots", "Claude Code shell snapshots"),
    (".claude/session-env", "Claude Code session environments"),
    (".claude/paste-cache", "Claude Code paste cache"),
    (".claude/debug", "Claude Code debug logs"),
    (".codex/.tmp", "Codex temporary files"),
    (".codex/log", "Codex logs"),
    (".codex/shell_snapshots", "Codex shell snapshots"),
    (".gemini/tmp", "Gemini CLI session scratch"),
    (".copilot/logs", "Copilot CLI logs"),
    (
        ".copilot/history-session-state",
        "Copilot CLI session state",
    ),
    (".cursor/projects", "Cursor per-project agent state"),
    (".cursor/ai-tracking", "Cursor AI edit tracking"),
];

impl CleanupRule for AgentScratchRule {
    fn name(&self) -> &'static str {
        "Agent session scratch"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);
        let home = home();

        SCRATCH_DIRS
            .par_iter()
            .flat_map(|&(dir, label)| {
                let root = home.join(dir);
                if !root.is_dir() {
                    return Vec::new();
                }

                // Prefer per-item entries so a directory holding one live
                // session and twenty dead ones is not offered wholesale.
                let mut entries: Vec<ScannedEntry> = children(&root)
                    .into_iter()
                    .filter_map(|path| {
                        let (days, size) = aged(&path, cutoff)?;
                        (size >= MIN_SCRATCH_SIZE).then(|| {
                            entry(
                                path,
                                size,
                                SafetyLevel::Caution,
                                format!("{label}, {days} days idle"),
                            )
                        })
                    })
                    .collect();

                // Nothing individually notable: offer the directory once, still
                // only if the whole of it is stale.
                if entries.is_empty() {
                    if let Some((days, size)) = aged(&root, cutoff) {
                        if size >= MIN_SCRATCH_SIZE {
                            entries.push(entry(
                                root,
                                size,
                                SafetyLevel::Caution,
                                format!("{label}, {days} days idle"),
                            ));
                        }
                    }
                }

                entries
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// State left behind by sessions that no longer exist
// ---------------------------------------------------------------------------

/// Session ids that still have a Claude Code transcript.
fn live_claude_sessions() -> FxHashSet<String> {
    let mut live = FxHashSet::default();
    for slug in children(&home().join(".claude/projects")) {
        for file in children(&slug) {
            if file.extension().is_some_and(|e| e == "jsonl") {
                if let Some(stem) = file.file_stem() {
                    live.insert(stem.to_string_lossy().to_string());
                }
            }
        }
    }
    live
}

/// Checkpoints and environments whose session was already deleted.
///
/// Unlike everything else here this needs no age gate: with the transcript gone
/// there is no session left to resume or rewind, so the state is unreachable
/// however recent it is.
impl CleanupRule for OrphanedAgentStateRule {
    fn name(&self) -> &'static str {
        "Orphaned agent session state"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let live = live_claude_sessions();
        if live.is_empty() {
            // Either Claude Code is not installed or the transcripts could not
            // be read. Calling every checkpoint orphaned on that basis would
            // sweep away the history of live sessions.
            return Vec::new();
        }

        let home = home();
        let sources = [
            (
                home.join(".claude/file-history"),
                "Claude Code checkpoints for a deleted session",
            ),
            (
                home.join(".claude/session-env"),
                "Claude Code environment for a deleted session",
            ),
            (
                home.join(".claude/shell-snapshots"),
                "Claude Code shell snapshot for a deleted session",
            ),
        ];

        sources
            .into_iter()
            .flat_map(|(dir, label)| {
                let live = &live;
                children(&dir).into_iter().filter_map(move |path| {
                    let name = path.file_name()?.to_string_lossy().to_string();
                    // Files are named `<session-id>.<ext>`, directories bare.
                    let session_id = name.split('.').next().unwrap_or(&name);
                    if live.contains(session_id) {
                        return None;
                    }
                    let size = if path.is_dir() {
                        walker::dir_size(&path)
                    } else {
                        walker::file_size(&path)
                    };
                    (size > 0).then(|| entry(path, size, SafetyLevel::Caution, label.to_owned()))
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Agent worktrees
// ---------------------------------------------------------------------------

/// Whether the `gitdir:` a linked worktree points at is still present.
fn linked_gitdir_exists(worktree: &Path) -> bool {
    let dot_git = worktree.join(".git");
    let Ok(meta) = dot_git.symlink_metadata() else {
        return false;
    };
    if meta.is_dir() {
        return true;
    }

    std::fs::read_to_string(&dot_git)
        .ok()
        .and_then(|c| {
            c.trim()
                .strip_prefix("gitdir:")
                .map(|t| PathBuf::from(t.trim()))
        })
        .is_some_and(|target| {
            if target.is_absolute() {
                target.exists()
            } else {
                worktree.join(target).exists()
            }
        })
}

/// Timeout for the two git queries that decide whether a worktree is expendable.
const GIT_QUERY_TIMEOUT: Duration = Duration::from_secs(15);

fn git_stdout(worktree: &Path, args: &[&str]) -> Option<String> {
    let dir = worktree.display().to_string();
    let mut command = vec!["git", "-C", dir.as_str()];
    command.extend_from_slice(args);
    let output = crate::util::run_with_timeout(&command, GIT_QUERY_TIMEOUT).success()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Why a worktree must be kept, or `None` when git says nothing would be lost.
///
/// Both questions have to be answered positively. A silent git -- not a
/// repository, git missing, a query that timed out -- is not an answer, so the
/// worktree is kept.
fn worktree_keep_reason(worktree: &Path) -> Option<&'static str> {
    // A linked worktree whose administrative directory was pruned cannot be
    // questioned at all: git will not open it, so there is no way to tell
    // whether the files still sitting there were ever committed.
    if !linked_gitdir_exists(worktree) {
        return Some("its git directory is gone, so its contents cannot be checked");
    }

    let Some(status) = git_stdout(worktree, &["status", "--porcelain"]) else {
        return Some("git could not read it");
    };
    if !status.trim().is_empty() {
        return Some("it has uncommitted or untracked changes");
    }

    let Some(remote_branches) = git_stdout(worktree, &["branch", "-r", "--contains", "HEAD"])
    else {
        return Some("git could not read it");
    };
    if remote_branches.trim().is_empty() {
        return Some("its commits are not on any remote");
    }

    None
}

/// Worktrees an agent checked out and left behind.
///
/// Cursor puts these under `~/.cursor/worktrees/<project>/<branch>`. They are
/// real source trees, so unlike everything else here the age gate is not what
/// makes them safe -- git confirming the work is pushed and the tree clean is.
impl CleanupRule for AgentWorktreeRule {
    fn name(&self) -> &'static str {
        "Agent worktrees"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);

        children(&home().join(".cursor/worktrees"))
            .par_iter()
            .flat_map(|project| children(project))
            .filter_map(|worktree| {
                if !worktree.join(".git").exists() {
                    return None;
                }
                let (days, size) = aged(&worktree, cutoff)?;
                if size < MIN_SESSION_SIZE {
                    return None;
                }

                let name = worktree
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                match worktree_keep_reason(&worktree) {
                    // Surfacing the skip matters: a worktree sitting on
                    // unpushed work is exactly what a user wants to know about.
                    Some(reason) => Some(entry(
                        worktree,
                        0,
                        SafetyLevel::Error,
                        format!("Agent worktree {name} kept -- {reason}"),
                    )),
                    None => Some(entry(
                        worktree,
                        size,
                        SafetyLevel::Danger,
                        format!("Agent worktree {name}, {days} days idle, clean and pushed"),
                    )),
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Session scratchpads
// ---------------------------------------------------------------------------

/// Per-session working directories agents are handed instead of `/tmp`.
///
/// Laid out as `<tmp>/claude-<uid>/<project-slug>/<session-id>/`, so a session
/// still in progress is distinguishable from one that ended months ago.
fn scratchpad_roots() -> Vec<PathBuf> {
    let tmp = PathBuf::from(if cfg!(target_os = "macos") {
        "/private/tmp"
    } else {
        "/tmp"
    });

    children(&tmp)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("claude-"))
                && p.is_dir()
        })
        .collect()
}

impl CleanupRule for AgentScratchpadRule {
    fn name(&self) -> &'static str {
        "Agent session scratchpads"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);
        let live = live_claude_sessions();

        scratchpad_roots()
            .par_iter()
            .flat_map(|root| children(root))
            .flat_map(|project| {
                let slug = project
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                children(&project)
                    .into_iter()
                    .filter_map(|session| {
                        let session_id = session.file_name()?.to_string_lossy().to_string();
                        // A scratchpad whose session is gone is unreachable at
                        // any age; one that still has a transcript waits.
                        let orphaned = !live.is_empty() && !live.contains(&session_id);
                        let (days, size) = if orphaned {
                            (0, walker::dir_size(&session))
                        } else {
                            aged(&session, cutoff)?
                        };
                        if size < MIN_SCRATCH_SIZE {
                            return None;
                        }
                        let detail = if orphaned {
                            "session no longer exists".to_owned()
                        } else {
                            format!("{days} days idle")
                        };
                        Some(entry(
                            session,
                            size,
                            SafetyLevel::Caution,
                            format!("Agent scratchpad ({detail}) for {slug}"),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(AgentTranscriptRule),
        Box::new(AgentCheckpointRule),
        Box::new(AgentScratchRule),
        Box::new(OrphanedAgentStateRule),
        Box::new(AgentWorktreeRule),
        Box::new(AgentScratchpadRule),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scratch_entries_never_name_a_dotfile_root() {
        // A bare `.claude` or `.codex` would take auth.json, config.toml,
        // memories, rules and skills with it.
        for (dir, _) in SCRATCH_DIRS {
            assert!(
                Path::new(dir).components().count() > 1,
                "{dir} is a dotfile root, not session scratch"
            );
        }
    }

    #[test]
    fn worktrees_are_never_swept_as_plain_scratch() {
        // They hold checked-out source; only the dedicated rule, which asks git
        // first, may offer them.
        for (dir, _) in SCRATCH_DIRS {
            assert!(!dir.contains("worktrees"), "{dir} points at live source");
        }
    }

    #[test]
    fn a_worktree_git_cannot_speak_for_is_kept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // Not a repository at all, so nothing about its contents is assumed.
        assert!(worktree_keep_reason(tmp.path()).is_some());
    }

    #[test]
    fn a_worktree_whose_gitdir_was_pruned_is_kept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let worktree = tmp.path().join("nkr");
        std::fs::create_dir(&worktree).expect("worktree");
        std::fs::write(
            worktree.join(".git"),
            "gitdir: /nowhere/.git/worktrees/nkr\n",
        )
        .expect("git file");

        assert!(!linked_gitdir_exists(&worktree));
        assert_eq!(
            worktree_keep_reason(&worktree),
            Some("its git directory is gone, so its contents cannot be checked")
        );
    }

    #[test]
    fn a_directory_is_only_as_idle_as_its_newest_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let session = tmp.path().join("session");
        std::fs::create_dir_all(session.join("nested")).expect("nested");
        std::fs::write(session.join("nested/fresh.jsonl"), b"x").expect("write");

        // Everything just written, so nothing is idle by any margin.
        assert!(aged(&session, Duration::from_secs(60)).is_none());
        // With no cutoff at all it reports, which proves the walk found the file.
        assert!(aged(&session, Duration::ZERO).is_some());
    }

    #[test]
    fn a_session_with_no_transcript_is_orphaned_whatever_its_name_looks_like() {
        let live: FxHashSet<String> = ["abc".to_owned()].into_iter().collect();
        assert!(live.contains("abc.jsonl".split('.').next().unwrap()));
        assert!(!live.contains("def"));
    }
}
