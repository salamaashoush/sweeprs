//! Directories coding agents created in cache and temp locations and left behind.
//!
//! An agent builds into `~/.cache/<name>-target`, clones a reference repository
//! into `/tmp`, writes a scratch tree into `$TMPDIR`, and the session ends.
//! Nothing records that the directory belonged to that session except the
//! session's own record, so this reads them: Claude Code, Codex, Gemini CLI,
//! Qwen Code, opencode, Cline and Roo Code each keep one, in formats of their
//! own (`agents.rs`). From each it takes every command that creates a path
//! (`mkdir`, `git clone`, `git worktree add`, `--target-dir`,
//! `CARGO_TARGET_DIR=`, a copy or a redirect) and every file the agent wrote.
//!
//! A directory is offered only when a record proves a session made it and the
//! session is over:
//! - it lives directly in a cache or temp location, never in a project or in
//!   the agent's own configuration (its memory files live there);
//! - it was born after the command that names it, so a cache an agent merely
//!   `mkdir -p`'d into is not claimed;
//! - every session that created it has been quiet for an hour, and nothing
//!   inside it has changed in that hour;
//! - it holds no socket or FIFO, and the user owns all of it.

mod agents;
mod shell;

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::rules::contents;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

pub use agents::Agent;

const QUIET_FOR: Duration = Duration::from_hours(1);
/// Birth times and record timestamps come from different clocks' rounding.
const CLOCK_SLACK_SECS: u64 = 120;
const CACHE_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone)]
struct Session {
    agent: Agent,
    id: String,
    title: Option<String>,
    /// Seconds since the epoch at which the session last did anything.
    last_active: u64,
    /// Directories in a disposable location the session created things in.
    created: Vec<Created>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Created {
    path: PathBuf,
    at: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    version: u32,
    units: FxHashMap<PathBuf, Cached>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    stamp: (u64, u64),
    sessions: Vec<Session>,
}

pub struct AgentLeftoversRule;

impl CleanupRule for AgentLeftoversRule {
    fn name(&self) -> &'static str {
        "Agent leftovers"
    }

    fn category(&self) -> Category {
        Category::AgentLeftovers
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let bases = disposable_bases();
        let sessions = load_sessions(&bases);
        leftovers(&sessions, &bases, secs(SystemTime::now()), QUIET_FOR)
    }
}

/// What one session left behind, for `clean --session` run as the session ends.
///
/// The session is over by definition, so it need not have been quiet for an
/// hour; everything else still has to hold. Its subagents' records count as
/// part of it.
pub fn session_leftovers(session_id: &str) -> Vec<ScannedEntry> {
    let bases = disposable_bases();
    let sessions: Vec<Session> = load_sessions(&bases)
        .into_iter()
        .filter(|s| s.id == session_id)
        .collect();
    leftovers(&sessions, &bases, secs(SystemTime::now()), Duration::ZERO)
}

fn secs(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Places whose direct children are disposable by convention.
fn disposable_bases() -> Vec<PathBuf> {
    let mut bases = vec![
        PathBuf::from("/tmp"),
        PathBuf::from("/private/tmp"),
        PathBuf::from("/var/tmp"),
        PathBuf::from("/private/var/tmp"),
    ];
    if let Some(home) = dirs::home_dir() {
        bases.push(home.join(".cache"));
    }
    if let Some(tmpdir) = std::env::var_os("TMPDIR").map(PathBuf::from) {
        bases.push(tmpdir.canonicalize().unwrap_or(tmpdir));
    }
    bases
}

/// The child of a base that `path` lies in, if it lies in one.
fn top_level(path: &Path, bases: &[PathBuf]) -> Option<PathBuf> {
    bases.iter().find_map(|base| {
        let rest = path.strip_prefix(base).ok()?;
        match rest.components().next()? {
            Component::Normal(name) => {
                let name_str = name.to_string_lossy();
                // Per-session scratch directories have rules of their own.
                (!name_str.starts_with("claude-")).then(|| base.join(name))
            }
            _ => None,
        }
    })
}

fn leftovers(
    sessions: &[Session],
    bases: &[PathBuf],
    now: u64,
    quiet_for: Duration,
) -> Vec<ScannedEntry> {
    struct Claim<'a> {
        first_at: u64,
        last_active: u64,
        makers: Vec<(Agent, Option<&'a str>)>,
    }

    let mut claims: FxHashMap<PathBuf, Claim<'_>> = FxHashMap::default();
    for session in sessions {
        for created in &session.created {
            let Some(root) = top_level(&created.path, bases) else {
                continue;
            };
            let claim = claims.entry(root).or_insert(Claim {
                first_at: created.at,
                last_active: 0,
                makers: Vec::new(),
            });
            claim.first_at = claim.first_at.min(created.at);
            claim.last_active = claim.last_active.max(session.last_active);
            let maker = (session.agent, session.title.as_deref());
            if !claim.makers.contains(&maker) {
                claim.makers.push(maker);
            }
        }
    }

    let uid = nix::unistd::getuid().as_raw();
    let quiet_since = UNIX_EPOCH + Duration::from_secs(now.saturating_sub(quiet_for.as_secs()));
    let mut found: Vec<ScannedEntry> = claims
        .into_par_iter()
        .filter_map(|(path, claim)| {
            if now.saturating_sub(claim.last_active) < quiet_for.as_secs() {
                return None;
            }
            let meta = path.symlink_metadata().ok()?;
            if meta.is_symlink() {
                return None;
            }
            let born = meta.created().ok().map(secs)?;
            if born + CLOCK_SLACK_SECS < claim.first_at {
                return None;
            }
            let size = contents::removable_size(&path, Some(quiet_since), uid)?;
            let when = chrono::DateTime::from_timestamp(i64::try_from(claim.first_at).ok()?, 0)?
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d");
            let who = |(agent, title): &(Agent, Option<&str>)| match title {
                Some(title) => format!("{} \"{title}\"", agent.label()),
                None => format!("a {} session", agent.label()),
            };
            let who = match claim.makers.as_slice() {
                [] => "an agent session".to_owned(),
                [one] => who(one),
                [first, rest @ ..] => format!("{} and {} more", who(first), rest.len()),
            };
            Some(ScannedEntry {
                path,
                size,
                category: Category::AgentLeftovers,
                safety: SafetyLevel::Caution,
                description: format!("Left by {who} ({when})"),
                item_count: None,
            })
        })
        .collect();
    found.sort_by_key(|e| std::cmp::Reverse(e.size));
    found
}

fn cache_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("sweeprs")
            .join("agent_provenance.json"),
    )
}

/// Every agent's sessions, each record parsed once and remembered by its stamp.
///
/// Records run to gigabytes in total, almost all of it request bodies; only
/// the records of a running session change between scans.
fn load_sessions(bases: &[PathBuf]) -> Vec<Session> {
    let units = agents::units();

    let cache_file = cache_path();
    let previous: Cache = cache_file
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .filter(|c: &Cache| c.version == CACHE_VERSION)
        .unwrap_or_default();

    let current: FxHashMap<PathBuf, Cached> = units
        .into_par_iter()
        .map(|unit| {
            let stamp = unit.stamp();
            let sessions = previous
                .units
                .get(&unit.path)
                .filter(|c| c.stamp == stamp)
                .map_or_else(|| unit.parse(bases), |c| c.sessions.clone());
            (unit.path, Cached { stamp, sessions })
        })
        .collect();

    let mut sessions: Vec<Session> = current
        .values()
        .flat_map(|c| c.sessions.iter().cloned())
        .collect();
    agents::name_untitled(&mut sessions);

    if let Some(cache_file) = cache_file {
        let cache = Cache {
            version: CACHE_VERSION,
            units: current,
        };
        if let (Some(parent), Ok(json)) = (cache_file.parent(), serde_json::to_vec(&cache)) {
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::write(&cache_file, json);
        }
    }
    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(title: &str, last_active: u64, created: &[(&Path, u64)]) -> Session {
        Session {
            agent: Agent::Claude,
            id: "s".to_owned(),
            title: Some(title.to_owned()),
            last_active,
            created: created
                .iter()
                .map(|(path, at)| Created {
                    path: path.to_path_buf(),
                    at: *at,
                })
                .collect(),
        }
    }

    #[test]
    fn a_directory_an_ended_session_created_is_offered_once_with_its_title() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().canonicalize().expect("canonical");
        let made = base.join("acme-target");
        std::fs::create_dir_all(made.join("debug")).expect("dir");
        std::fs::write(made.join("debug/bin"), vec![0u8; 4096]).expect("file");

        let born = secs(
            made.symlink_metadata()
                .expect("meta")
                .created()
                .expect("birth"),
        );
        let later = born + 10 * 3600;
        let mut codex = session("Port the parser", born + 600, &[(&made, born)]);
        codex.agent = Agent::Codex;
        let sessions = [
            session(
                "Build the acme engine",
                born + 600,
                &[(&made.join("debug"), born), (&made, born)],
            ),
            codex,
        ];

        let found = leftovers(&sessions, std::slice::from_ref(&base), later, QUIET_FOR);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, made);
        assert!(
            found[0]
                .description
                .starts_with("Left by Claude Code \"Build the acme engine\" and 1 more"),
            "{}",
            found[0].description
        );
    }

    #[test]
    fn what_existed_before_the_session_or_is_still_in_use_is_not_offered() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().canonicalize().expect("canonical");
        let older = base.join("shared-cache");
        std::fs::create_dir(&older).expect("dir");
        std::fs::write(older.join("blob"), vec![0u8; 4096]).expect("file");
        let born = secs(
            older
                .symlink_metadata()
                .expect("meta")
                .created()
                .expect("birth"),
        );

        // `mkdir -p` into a cache that was already there long before.
        let claimed_later = [session("Late", born + 7200, &[(&older, born + 3600)])];
        assert!(
            leftovers(
                &claimed_later,
                std::slice::from_ref(&base),
                born + 20_000,
                QUIET_FOR
            )
            .is_empty()
        );

        // A session active within the hour may still be using it.
        let active = [session("Active", born + 19_900, &[(&older, born)])];
        assert!(
            leftovers(
                &active,
                std::slice::from_ref(&base),
                born + 20_000,
                QUIET_FOR
            )
            .is_empty()
        );
    }

    #[test]
    fn only_children_of_disposable_bases_count_and_never_session_scratch() {
        let bases = [
            PathBuf::from("/tmp"),
            PathBuf::from("/home/sashoush/.cache"),
        ];
        assert_eq!(
            top_level(Path::new("/tmp/acme/deep/file"), &bases),
            Some(PathBuf::from("/tmp/acme"))
        );
        assert_eq!(top_level(Path::new("/tmp/claude-501/x"), &bases), None);
        assert_eq!(
            top_level(Path::new("/home/sashoush/.claude/memory/x.md"), &bases),
            None
        );
        assert_eq!(top_level(Path::new("/tmp"), &bases), None);
    }
}
