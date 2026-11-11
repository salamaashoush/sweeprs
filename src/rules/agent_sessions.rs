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
//! Agent worktrees hold checked-out source, so they are only ever offered once
//! git itself confirms there is nothing in them to lose: no uncommitted change,
//! no untracked file, no ignored file that a tool would not regenerate, and a
//! HEAD that some remote branch already contains. Cleaning one goes through
//! `git worktree remove`, which asks again.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use rayon::prelude::*;
use rustc_hash::FxHashSet;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::{self, PROJECT_INDEX};
use crate::scanner::walker;
use crate::util::{self, CommandOutcome};

/// Sessions below this are noise in a listing; the aggregate is what matters.
const MIN_SESSION_SIZE: u64 = 262_144;

/// Scratch and log directories worth listing on their own.
const MIN_SCRATCH_SIZE: u64 = 1_048_576;

/// State whose session is gone is unreachable at any age, but a session that
/// has only just started may not have written its transcript yet.
const ORPHAN_MIN_AGE: Duration = Duration::from_hours(24);

pub struct AgentTranscriptRule;
pub struct AgentCheckpointRule;
pub struct AgentScratchRule;
pub struct OrphanedAgentStateRule;
pub struct AgentWorktreeRule;
pub struct AgentScratchpadRule;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Claude Code's config directory, which `CLAUDE_CONFIG_DIR` relocates.
pub fn claude_home() -> PathBuf {
    env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home().join(".claude"))
}

/// Codex's home, which `CODEX_HOME` relocates.
pub fn codex_home() -> PathBuf {
    env_dir("CODEX_HOME").unwrap_or_else(|| home().join(".codex"))
}

/// Gemini CLI's directory. `GEMINI_CLI_HOME` stands in for `$HOME`.
pub fn gemini_home() -> PathBuf {
    env_dir("GEMINI_CLI_HOME")
        .unwrap_or_else(home)
        .join(".gemini")
}

/// Qwen Code's directory, which `QWEN_HOME` replaces outright.
pub fn qwen_home() -> PathBuf {
    env_dir("QWEN_HOME").unwrap_or_else(|| home().join(".qwen"))
}

/// Copilot CLI's directory, which `COPILOT_HOME` relocates.
pub fn copilot_home() -> PathBuf {
    env_dir("COPILOT_HOME").unwrap_or_else(|| home().join(".copilot"))
}

/// Cline's data directory: `CLINE_DATA_DIR`, else `data` under `CLINE_DIR`
/// or `~/.cline`.
pub(crate) fn cline_data() -> PathBuf {
    env_dir("CLINE_DATA_DIR").unwrap_or_else(|| {
        env_dir("CLINE_DIR")
            .unwrap_or_else(|| home().join(".cline"))
            .join("data")
    })
}

/// The XDG data directory. Node and Go CLIs use it on macOS too, where
/// `dirs::data_dir` would answer `~/Library/Application Support`.
pub(crate) fn xdg_data() -> PathBuf {
    env_dir("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local/share"))
}

fn zed_data() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join("Library/Application Support/Zed")
    } else {
        xdg_data().join("zed")
    }
}

/// Session ids are v4 UUIDs. Anything else sitting next to them belongs to
/// the tool, not to a session, and must not be judged by whether a session
/// with that name exists.
fn is_session_id(name: &str) -> bool {
    name.len() == 36
        && name.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
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

fn size_of(path: &Path) -> u64 {
    if path.is_dir() {
        walker::dir_size(path)
    } else {
        walker::file_size(path)
    }
}

/// `(days_idle, size)` for a path that has been untouched longer than `cutoff`.
fn aged(path: &Path, cutoff: Duration) -> Option<(u64, u64)> {
    let idle = idle_for(path)?;
    if idle < cutoff {
        return None;
    }
    Some((idle.as_secs() / 86_400, size_of(path)))
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

/// One entry per child of each source that has aged past `cutoff`.
fn aged_children(
    sources: Vec<(PathBuf, &'static str)>,
    cutoff: Duration,
    min_size: u64,
    safety: SafetyLevel,
) -> Vec<ScannedEntry> {
    let paths = sources
        .into_iter()
        .flat_map(|(dir, label)| children(&dir).into_iter().map(move |p| (p, label)))
        .collect();
    aged_paths(paths, cutoff, min_size, safety)
}

/// One entry per path that has aged past `cutoff`.
fn aged_paths(
    paths: Vec<(PathBuf, &'static str)>,
    cutoff: Duration,
    min_size: u64,
    safety: SafetyLevel,
) -> Vec<ScannedEntry> {
    paths
        .into_par_iter()
        .filter_map(|(path, label)| {
            let (days, size) = aged(&path, cutoff)?;
            (size >= min_size)
                .then(|| entry(path, size, safety, format!("{label}, {days} days idle")))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Transcripts
// ---------------------------------------------------------------------------

/// Claude Code keeps `<session>.jsonl` next to a `<session>/` directory of tool
/// results. They are one session and are reported as one entry.
fn claude_transcripts(cutoff: Duration) -> Vec<ScannedEntry> {
    let projects = claude_home().join("projects");

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

fn jsonl_under(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    for path in children(dir) {
        if path.is_dir() {
            if depth > 0 {
                jsonl_under(&path, depth - 1, out);
            }
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

/// Codex writes one `rollout-*.jsonl` per session under `sessions/YYYY/MM/DD`,
/// and moves archived ones to `archived_sessions`.
fn codex_transcripts(cutoff: Duration) -> Vec<ScannedEntry> {
    let codex = codex_home();
    let mut files = Vec::new();
    jsonl_under(&codex.join("sessions"), 3, &mut files);
    jsonl_under(&codex.join("archived_sessions"), 3, &mut files);

    files
        .into_par_iter()
        .filter_map(|file| {
            let (days, size) = aged(&file, cutoff)?;
            (size >= MIN_SESSION_SIZE).then(|| {
                entry(
                    file,
                    size,
                    SafetyLevel::Danger,
                    format!("Codex transcript, {days} days idle"),
                )
            })
        })
        .collect()
}

/// Directories holding one session, chat or task per child.
fn per_session_sources() -> Vec<(PathBuf, &'static str)> {
    let home = home();
    let cursor = home.join(".cursor");

    let copilot = copilot_home();
    let cline = cline_data();

    let mut sources = vec![
        (cursor.join("acp-sessions"), "Cursor agent session"),
        (cursor.join("chats"), "Cursor chat thread"),
        // `history-session-state` is where versions before 0.0.400 kept them.
        (copilot.join("history-session-state"), "Copilot CLI session"),
        (copilot.join("session-state"), "Copilot CLI session"),
        (gemini_home().join("history"), "Gemini CLI chat history"),
        (cline.join("sessions"), "Cline session"),
        (cline.join("tasks"), "Cline task"),
    ];

    for project in children(&cursor.join("projects")) {
        sources.push((project.join("agent-transcripts"), "Cursor agent transcript"));
    }
    for project in children(&qwen_home().join("projects")) {
        sources.push((project.join("chats"), "Qwen Code chat"));
    }

    for support in crate::rules::ide::EDITOR_SUPPORT_DIRS {
        let storage = home.join(support).join("User/globalStorage");
        sources.push((storage.join("saoudrizwan.claude-dev/tasks"), "Cline task"));
        sources.push((
            storage.join("rooveterinaryinc.roo-cline/tasks"),
            "Roo Code task",
        ));
    }

    sources
}

fn is_sha256_name(path: &Path) -> bool {
    path.file_name().is_some_and(|n| {
        let n = n.to_string_lossy();
        n.len() == 64 && n.bytes().all(|b| b.is_ascii_hexdigit())
    })
}

fn gemini_project_dirs(tmp: &Path) -> Vec<PathBuf> {
    children(tmp)
        .into_iter()
        .filter(|p| p.join(".project_root").is_file() || is_sha256_name(p))
        .collect()
}

/// Per-project directories under Gemini CLI's and Qwen Code's `tmp`.
///
/// Gemini names them after the project and marks each with `.project_root`,
/// migrating older sha256-named ones; Qwen still hashes, and keeps checkpoints
/// there. The same `tmp` holds downloaded tools such as `bin/`, which neither
/// test admits.
fn project_session_dirs() -> Vec<(PathBuf, &'static str)> {
    let gemini = gemini_project_dirs(&gemini_home().join("tmp"))
        .into_iter()
        .map(|p| (p, "Gemini CLI project sessions and checkpoints"));
    let qwen = children(&qwen_home().join("tmp"))
        .into_iter()
        .filter(|p| is_sha256_name(p))
        .map(|p| (p, "Qwen Code project checkpoints"));
    gemini.chain(qwen).collect()
}

/// Stores whose sessions share one database or index, offered only whole.
///
/// Zed and Goose keep every session in one SQLite file (the directory goes so
/// its WAL goes with it), Continue indexes its sessions in `sessions.json`,
/// and opencode's legacy store splits each session across `session/`,
/// `message/` and `part/`.
fn whole_session_stores() -> Vec<(PathBuf, &'static str)> {
    vec![
        (zed_data().join("threads"), "Zed agent threads, all of them"),
        (
            xdg_data().join("goose/sessions"),
            "Goose sessions, all of them",
        ),
        (
            home().join(".continue/sessions"),
            "Continue sessions, all of them",
        ),
        (
            xdg_data().join("opencode/storage"),
            "opencode legacy session store, all of it",
        ),
    ]
}

/// Aider writes its chat log into the repository it was run in.
fn aider_histories(cutoff: Duration) -> Vec<ScannedEntry> {
    PROJECT_INDEX
        .git_roots()
        .par_iter()
        .filter_map(|root| {
            let history = root.join(".aider.chat.history.md");
            let (days, size) = aged(&history, cutoff)?;
            let project = root.file_name()?.to_string_lossy().to_string();
            (size >= MIN_SESSION_SIZE).then(|| {
                entry(
                    history,
                    size,
                    SafetyLevel::Danger,
                    format!("Aider chat history, {days} days idle ({project})"),
                )
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
        entries.extend(aged_children(
            per_session_sources(),
            cutoff,
            MIN_SESSION_SIZE,
            SafetyLevel::Danger,
        ));
        entries.extend(aged_paths(
            project_session_dirs(),
            cutoff,
            MIN_SESSION_SIZE,
            SafetyLevel::Danger,
        ));
        entries.extend(aged_paths(
            whole_session_stores(),
            cutoff,
            MIN_SESSION_SIZE,
            SafetyLevel::Danger,
        ));
        entries.extend(aider_histories(cutoff));
        entries
    }
}

// ---------------------------------------------------------------------------
// Rewind checkpoints
// ---------------------------------------------------------------------------

/// Snapshots of files as they were before an agent edited them.
///
/// Claude Code keys them by session id; Cursor keeps a bare git repository per
/// workspace under `snapshots/`, and opencode one under `snapshot/`. Either way
/// this is the undo history for agent edits, and it is the last thing to reach
/// for.
impl CleanupRule for AgentCheckpointRule {
    fn name(&self) -> &'static str {
        "Agent rewind checkpoints"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let sources = vec![
            (
                claude_home().join("file-history"),
                "Claude Code file checkpoints",
            ),
            (home().join(".cursor/snapshots"), "Cursor edit snapshots"),
            (
                xdg_data().join("opencode/snapshot"),
                "opencode edit snapshots",
            ),
        ];
        aged_children(
            sources,
            cutoff(config),
            MIN_SESSION_SIZE,
            SafetyLevel::Danger,
        )
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
fn scratch_dirs() -> Vec<(PathBuf, &'static str)> {
    let home = home();
    let claude = claude_home();
    let codex = codex_home();

    vec![
        (
            claude.join("shell-snapshots"),
            "Claude Code shell snapshots",
        ),
        (
            claude.join("session-env"),
            "Claude Code session environments",
        ),
        (claude.join("paste-cache"), "Claude Code paste cache"),
        (claude.join("debug"), "Claude Code debug logs"),
        (claude.join("telemetry"), "Claude Code unsent telemetry"),
        (codex.join(".tmp"), "Codex temporary files"),
        (codex.join("log"), "Codex logs"),
        (codex.join("shell_snapshots"), "Codex shell snapshots"),
        (copilot_home().join("logs"), "Copilot CLI logs"),
        (home.join(".cursor/ai-tracking"), "Cursor AI edit tracking"),
        (xdg_data().join("opencode/log"), "opencode logs"),
        (
            xdg_data().join("opencode/tool-output"),
            "opencode tool output",
        ),
        (zed_data().join("hang_traces"), "Zed hang traces"),
    ]
}

impl CleanupRule for AgentScratchRule {
    fn name(&self) -> &'static str {
        "Agent session scratch"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);

        scratch_dirs()
            .into_par_iter()
            .flat_map(|(root, label)| {
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
                if entries.is_empty()
                    && let Some((days, size)) = aged(&root, cutoff)
                    && size >= MIN_SCRATCH_SIZE
                {
                    entries.push(entry(
                        root,
                        size,
                        SafetyLevel::Caution,
                        format!("{label}, {days} days idle"),
                    ));
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
    for slug in children(&claude_home().join("projects")) {
        for file in children(&slug) {
            if file.extension().is_some_and(|e| e == "jsonl")
                && let Some(stem) = file.file_stem()
            {
                live.insert(stem.to_string_lossy().to_string());
            }
        }
    }
    live
}

/// The session a piece of Claude Code state belongs to, if its name says.
///
/// Checkpoint, environment and tool-result directories are named after the
/// session; todo lists are `<session>-agent-<agent>.json`. Shell snapshots are
/// named by timestamp and belong to no session, so they never qualify.
fn owning_session(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    let id = name.get(..36)?;
    is_session_id(id).then(|| id.to_owned())
}

/// Checkpoints, environments, todo lists and tool results whose session was
/// already deleted.
///
/// The age gate here is a day rather than the session cutoff: with the
/// transcript gone there is nothing left to resume or rewind, and the day only
/// covers a session too new to have written its transcript.
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

        let claude = claude_home();
        let mut candidates: Vec<(PathBuf, &'static str)> = Vec::new();
        for (dir, label) in [
            (
                claude.join("file-history"),
                "Claude Code checkpoints for a deleted session",
            ),
            (
                claude.join("session-env"),
                "Claude Code environment for a deleted session",
            ),
            (
                claude.join("todos"),
                "Claude Code todo list for a deleted session",
            ),
        ] {
            candidates.extend(children(&dir).into_iter().map(|p| (p, label)));
        }
        for slug in children(&claude.join("projects")) {
            candidates.extend(
                children(&slug)
                    .into_iter()
                    .filter(|p| p.is_dir())
                    .map(|p| (p, "Claude Code tool results for a deleted session")),
            );
        }

        candidates
            .into_par_iter()
            .filter_map(|(path, label)| {
                let session = owning_session(&path)?;
                if live.contains(&session) || idle_for(&path)? < ORPHAN_MIN_AGE {
                    return None;
                }
                let size = size_of(&path);
                (size > 0).then(|| entry(path, size, SafetyLevel::Caution, label.to_owned()))
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

/// Timeout for the git queries that decide whether a worktree is expendable.
const GIT_QUERY_TIMEOUT: Duration = Duration::from_secs(15);

/// `git worktree remove` deletes the checkout, which can be large.
const GIT_REMOVE_TIMEOUT: Duration = Duration::from_secs(120);

fn git_stdout(worktree: &Path, args: &[&str]) -> Option<String> {
    let dir = worktree.display().to_string();
    let mut command = vec!["git", "-C", dir.as_str()];
    command.extend_from_slice(args);
    let output = util::run_with_timeout(&command, GIT_QUERY_TIMEOUT).success()?;
    Some(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Whether an ignored path is output a tool regenerates.
///
/// An ignored `.env` or `settings.local.json` exists nowhere else; an ignored
/// `node_modules/` comes back with an install.
fn is_regenerable_ignored(path: &str) -> bool {
    let path = path.trim().trim_matches('"').trim_end_matches('/');
    let name = path.rsplit('/').next().unwrap_or(path);
    path.split('/').any(project_index::is_disposable_dir)
        || matches!(name, ".DS_Store" | ".eslintcache")
        || [".log", ".tsbuildinfo", ".pyc"]
            .iter()
            .any(|ext| name.ends_with(ext))
}

/// Why a worktree must be kept, or `None` when git says nothing would be lost.
///
/// Every question has to be answered positively. A silent git -- not a
/// repository, git missing, a query that timed out -- is not an answer, so the
/// worktree is kept.
fn worktree_keep_reason(worktree: &Path) -> Option<&'static str> {
    // A standalone clone keeps its other branches and its stashes inside the
    // checkout. Only a linked worktree leaves those in a store that survives.
    if worktree.join(".git").is_dir() {
        return Some("it is a standalone clone, not a linked worktree");
    }

    // A linked worktree whose administrative directory was pruned cannot be
    // questioned at all: git will not open it, so there is no way to tell
    // whether the files still sitting there were ever committed.
    if !linked_gitdir_exists(worktree) {
        return Some("its git directory is gone, so its contents cannot be checked");
    }

    // Explicit flags, because `status.showUntrackedFiles=no` in the user's
    // config would otherwise hide exactly the files this is looking for.
    let Some(status) = git_stdout(
        worktree,
        &[
            "status",
            "--porcelain",
            "--untracked-files=normal",
            "--ignored=matching",
        ],
    ) else {
        return Some("git could not read it");
    };
    let mut unrecoverable_ignored = false;
    for line in status.lines().filter(|l| !l.trim().is_empty()) {
        match line.strip_prefix("!! ") {
            Some(ignored) => unrecoverable_ignored |= !is_regenerable_ignored(ignored),
            None => return Some("it has uncommitted or untracked changes"),
        }
    }
    if unrecoverable_ignored {
        return Some("it has ignored files that exist nowhere else");
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

/// Checkouts up to `depth` levels below `dir`.
fn find_checkouts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    for path in children(dir) {
        if !path.is_dir() {
            continue;
        }
        if path.join(".git").exists() {
            out.push(path);
        } else if depth > 1 {
            find_checkouts(&path, depth - 1, out);
        }
    }
}

/// Cursor checks out under `~/.cursor/worktrees/<project>/<branch>`, Codex
/// under `~/.codex/worktrees/<id>/<repo>`, opencode under its data dir's
/// `worktree/<project>/`, and Claude Code inside the repository at
/// `.claude/worktrees/<name>`.
fn worktree_candidates() -> Vec<PathBuf> {
    let mut found = Vec::new();
    find_checkouts(&home().join(".cursor/worktrees"), 2, &mut found);
    find_checkouts(&codex_home().join("worktrees"), 2, &mut found);
    find_checkouts(&xdg_data().join("opencode/worktree"), 2, &mut found);
    for repo in PROJECT_INDEX.git_roots() {
        find_checkouts(&repo.join(".claude/worktrees"), 1, &mut found);
    }
    found.sort();
    found.dedup();
    found
}

impl CleanupRule for AgentWorktreeRule {
    fn name(&self) -> &'static str {
        "Agent worktrees"
    }

    fn category(&self) -> Category {
        Category::AgentSession
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let cutoff = cutoff(config);

        worktree_candidates()
            .into_par_iter()
            .filter_map(|worktree| {
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
                        PathBuf::from(format!("agent-worktree:{}", worktree.display())),
                        size,
                        SafetyLevel::Danger,
                        format!("Agent worktree {name}, {days} days idle, clean and pushed"),
                    )),
                }
            })
            .collect()
    }
}

/// Remove a worktree named by an `agent-worktree:` entry.
///
/// Asks git again first, since the scan may be minutes old, then removes it
/// through `git worktree remove` so the store forgets it too. Deleting the
/// directory alone leaves the branch registered as checked out there.
pub fn clean_agent_worktree(entry_path: &str) -> io::Result<Option<u64>> {
    let worktree = Path::new(
        entry_path
            .strip_prefix("agent-worktree:")
            .unwrap_or(entry_path),
    );

    if let Some(reason) = worktree_keep_reason(worktree) {
        return Err(io::Error::other(format!("kept: {reason}")));
    }
    let store = project_index::resolve_git_dir(worktree)
        .ok_or_else(|| io::Error::other("kept: its git directory could not be resolved"))?;

    let size = walker::dir_size_uncached(worktree);
    let store = store.display().to_string();
    let target = worktree.display().to_string();
    match util::run_with_timeout(
        &["git", "-C", &store, "worktree", "remove", &target],
        GIT_REMOVE_TIMEOUT,
    ) {
        CommandOutcome::Completed(output) if output.status.success() => Ok(Some(size)),
        CommandOutcome::Completed(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let detail = stderr.lines().last().unwrap_or("no output").trim();
            Err(io::Error::other(format!("git worktree remove: {detail}")))
        }
        CommandOutcome::TimedOut => Err(io::Error::other(format!(
            "git worktree remove exceeded {}s",
            GIT_REMOVE_TIMEOUT.as_secs()
        ))),
        CommandOutcome::NotSpawned(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Session scratchpads
// ---------------------------------------------------------------------------

/// Per-session working directories agents are handed instead of `/tmp`.
///
/// Laid out as `<tmp>/claude-<uid>/<project-slug>/<session-id>/`, so a session
/// still in progress is distinguishable from one that ended months ago. Only
/// this user's root is considered; other users' are not ours to judge.
///
/// `CLAUDE_CODE_TMPDIR` moves the tree, and on Linux so does `$TMPDIR`. macOS
/// uses `/private/tmp` whatever `$TMPDIR` says.
fn scratchpad_roots() -> Vec<PathBuf> {
    let Some(uid) = current_uid() else {
        return Vec::new();
    };
    let mut bases: Vec<PathBuf> = env_dir("CLAUDE_CODE_TMPDIR").into_iter().collect();
    if cfg!(target_os = "macos") {
        bases.push(PathBuf::from("/private/tmp"));
    } else {
        bases.extend(env_dir("TMPDIR"));
        bases.push(PathBuf::from("/tmp"));
    }

    let mut roots: Vec<PathBuf> = bases
        .into_iter()
        .map(|base| base.join(format!("claude-{uid}")))
        .filter(|root| root.is_dir())
        .map(|root| root.canonicalize().unwrap_or(root))
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(unix)]
fn current_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    home().metadata().ok().map(|m| m.uid())
}

#[cfg(not(unix))]
fn current_uid() -> Option<u32> {
    None
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
            .iter()
            .flat_map(|root| children(root))
            .collect::<Vec<_>>()
            .par_iter()
            .flat_map(|project| {
                let slug = project
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                children(project)
                    .into_iter()
                    .filter_map(|session| {
                        let session_id = session.file_name()?.to_string_lossy().to_string();
                        if !is_session_id(&session_id) {
                            return None;
                        }
                        // A scratchpad whose session is gone is unreachable
                        // once it is a day old; one with a transcript waits.
                        let orphaned = !live.is_empty() && !live.contains(&session_id);
                        let gate = if orphaned { ORPHAN_MIN_AGE } else { cutoff };
                        let (days, size) = aged(&session, gate)?;
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
        let home = home();
        let roots = [home.clone(), claude_home(), codex_home()];
        for (dir, _) in scratch_dirs() {
            assert!(!roots.contains(&dir), "{} is a config root", dir.display());
            assert_ne!(
                dir.parent(),
                Some(home.as_path()),
                "{} is a dotfile root, not session scratch",
                dir.display()
            );
        }
    }

    #[test]
    fn worktrees_are_never_swept_as_plain_scratch() {
        // They hold checked-out source; only the dedicated rule, which asks git
        // first, may offer them.
        for (dir, _) in scratch_dirs() {
            assert!(
                !dir.to_string_lossy().contains("worktrees"),
                "{} points at live source",
                dir.display()
            );
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

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Salama Ashoush"])
            .args(["-c", "user.email=sashoush@example.com"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .output()
            .expect("git runs")
            .status;
        assert!(status.success(), "git {args:?} failed");
    }

    /// A repo with a pushed `main`, a linked worktree `wt` on a pushed branch,
    /// and `status.showUntrackedFiles=no` set the way some people run git.
    fn pushed_worktree(tmp: &Path) -> (PathBuf, PathBuf) {
        let remote = tmp.join("remote.git");
        let repo = tmp.join("repo");
        std::fs::create_dir(&repo).expect("repo");
        git(tmp, &["init", "-q", "--bare", "remote.git"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "status.showUntrackedFiles", "no"]);
        std::fs::write(repo.join(".gitignore"), "node_modules/\n.env\n").expect("ignore");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "init"]);
        git(
            &repo,
            &["remote", "add", "origin", &remote.display().to_string()],
        );
        git(&repo, &["push", "-q", "origin", "main"]);
        let worktree = tmp.join("wt");
        git(
            &repo,
            &["worktree", "add", "-q", &worktree.display().to_string()],
        );
        git(&worktree, &["push", "-q", "origin", "HEAD"]);
        (repo, worktree)
    }

    #[test]
    fn untracked_files_keep_a_worktree_even_when_git_is_told_to_hide_them() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, worktree) = pushed_worktree(tmp.path());
        assert_eq!(worktree_keep_reason(&worktree), None);

        std::fs::write(worktree.join("notes.md"), b"draft").expect("untracked");
        assert_eq!(
            worktree_keep_reason(&worktree),
            Some("it has uncommitted or untracked changes")
        );
    }

    #[test]
    fn an_ignored_env_file_keeps_a_worktree_but_node_modules_does_not() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, worktree) = pushed_worktree(tmp.path());

        std::fs::create_dir(worktree.join("node_modules")).expect("deps");
        std::fs::write(worktree.join("node_modules/x.js"), b"x").expect("dep");
        assert_eq!(worktree_keep_reason(&worktree), None);

        std::fs::write(worktree.join(".env"), b"API_TOKEN=x").expect("env");
        assert_eq!(
            worktree_keep_reason(&worktree),
            Some("it has ignored files that exist nowhere else")
        );
    }

    #[test]
    fn a_standalone_clone_is_kept() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (repo, _) = pushed_worktree(tmp.path());
        assert_eq!(
            worktree_keep_reason(&repo),
            Some("it is a standalone clone, not a linked worktree")
        );
    }

    #[test]
    fn cleaning_a_worktree_unregisters_it_from_the_store() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (repo, worktree) = pushed_worktree(tmp.path());

        clean_agent_worktree(&format!("agent-worktree:{}", worktree.display())).expect("removed");

        assert!(!worktree.exists());
        assert!(
            children(&repo.join(".git/worktrees")).is_empty(),
            "the store still lists the worktree"
        );
    }

    #[test]
    fn cleaning_asks_git_again_and_keeps_work_added_since_the_scan() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let (_, worktree) = pushed_worktree(tmp.path());
        std::fs::write(worktree.join("late.txt"), b"new").expect("late file");

        assert!(clean_agent_worktree(&format!("agent-worktree:{}", worktree.display())).is_err());
        assert!(worktree.join("late.txt").exists());
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
    fn only_session_named_state_can_be_orphaned() {
        let id = "0a77ae1b-bbae-47b7-9086-b31e943b3ef1";
        assert_eq!(owning_session(Path::new(id)).as_deref(), Some(id));
        assert_eq!(
            owning_session(Path::new(&format!("{id}-agent-{id}.json"))).as_deref(),
            Some(id)
        );
        // Shell snapshots and tool scratch are named by timestamp or hash.
        assert_eq!(
            owning_session(Path::new("snapshot-zsh-1790680563187-fc4sw2.sh")),
            None
        );
        assert_eq!(owning_session(Path::new("bash-edit-diff")), None);
        assert_eq!(owning_session(Path::new("memory")), None);
    }

    #[test]
    fn gemini_project_dirs_are_told_apart_from_its_downloaded_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let slug = root.join("sweeprs");
        let hashed = root.join("a".repeat(64));
        for dir in [&slug, &hashed, &root.join("bin"), &root.join("design-sync")] {
            std::fs::create_dir(dir).expect("dir");
        }
        std::fs::write(slug.join(".project_root"), b"/home/me/sweeprs").expect("marker");

        let mut found = gemini_project_dirs(root);
        found.sort();
        assert_eq!(found, [hashed, slug]);
    }
}
