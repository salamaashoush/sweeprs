//! Directories coding agents created in cache and temp locations and left behind.
//!
//! An agent builds into `~/.cache/<name>-target`, clones a reference repository
//! into `/tmp`, writes a scratch tree into `$TMPDIR`, and the session ends.
//! Nothing records that the directory belonged to that session except the
//! session's own transcript, so this reads the transcripts: every command that
//! creates a path (`mkdir`, `git clone`, `git worktree add`, `--target-dir`,
//! `CARGO_TARGET_DIR=`, a copy or a redirect) and every file the agent wrote.
//!
//! A directory is offered only when the transcript proves the session made it
//! and the session is over:
//! - it lives directly in a cache or temp location, never in a project or in
//!   the agent's own configuration (its memory files live there);
//! - it was born after the command that names it, so a cache an agent merely
//!   `mkdir -p`'d into is not claimed;
//! - every session that created it has been quiet for an hour, and nothing
//!   inside it has changed in that hour;
//! - it holds no socket or FIFO, and the user owns all of it.

use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rayon::prelude::*;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::rules::agent_sessions::claude_home;
use crate::rules::contents;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

const QUIET_FOR: Duration = Duration::from_hours(1);
/// Birth times and transcript timestamps come from different clocks' rounding.
const CLOCK_SLACK_SECS: u64 = 120;
const CACHE_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Default)]
struct Session {
    title: Option<String>,
    /// Seconds since the epoch at which the transcript was last written.
    last_active: u64,
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
    transcripts: FxHashMap<PathBuf, Cached>,
}

#[derive(Serialize, Deserialize)]
struct Cached {
    size: u64,
    mtime: u64,
    session: Session,
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
        let sessions = load_sessions(&claude_home().join("projects"));
        let bases = disposable_bases();
        let now = secs(SystemTime::now());
        leftovers(&sessions, &bases, now, QUIET_FOR)
    }
}

/// What one session left behind, for `clean --session` run as the session ends.
///
/// The session is over by definition, so it need not have been quiet for an
/// hour; everything else still has to hold. Its subagents' transcripts count
/// as part of it.
pub fn session_leftovers(session_id: &str) -> Vec<ScannedEntry> {
    let projects = claude_home().join("projects");
    let transcript = format!("{session_id}.jsonl");
    let sessions: Vec<Session> = ignore::WalkBuilder::new(&projects)
        .standard_filters(false)
        .max_depth(Some(4))
        .build()
        .flatten()
        .filter(|e| {
            let path = e.path();
            path.file_name().is_some_and(|n| n == transcript.as_str())
                || (path.extension().is_some_and(|x| x == "jsonl")
                    && path
                        .ancestors()
                        .any(|a| a.file_name().is_some_and(|n| n == session_id)))
        })
        .map(|e| {
            let mut session = parse_transcript(e.path());
            session.last_active = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map_or(0, secs);
            session
        })
        .collect();
    leftovers(
        &sessions,
        &disposable_bases(),
        secs(SystemTime::now()),
        Duration::ZERO,
    )
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
        titles: Vec<&'a str>,
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
                titles: Vec::new(),
            });
            claim.first_at = claim.first_at.min(created.at);
            claim.last_active = claim.last_active.max(session.last_active);
            if let Some(title) = session.title.as_deref()
                && !claim.titles.contains(&title)
            {
                claim.titles.push(title);
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
            let who = match claim.titles.as_slice() {
                [] => "an agent session".to_owned(),
                [one] => format!("\"{one}\""),
                [first, rest @ ..] => format!("\"{first}\" and {} more", rest.len()),
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

/// Every session transcript, parsed once and remembered by size and mtime.
///
/// Transcripts run to gigabytes in total, almost all of it request bodies; only
/// the transcript of a running session changes between scans.
fn load_sessions(projects: &Path) -> Vec<Session> {
    let transcripts: Vec<(PathBuf, u64, u64)> = ignore::WalkBuilder::new(projects)
        .standard_filters(false)
        .max_depth(Some(4))
        .build()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            Some((e.into_path(), meta.len(), meta.modified().ok().map(secs)?))
        })
        .collect();

    let cache_file = cache_path();
    let previous: Cache = cache_file
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .filter(|c: &Cache| c.version == CACHE_VERSION)
        .unwrap_or_default();

    let current: FxHashMap<PathBuf, Cached> = transcripts
        .into_par_iter()
        .map(|(path, size, mtime)| {
            let reuse = previous
                .transcripts
                .get(&path)
                .filter(|c| c.size == size && c.mtime == mtime);
            let session = reuse.map_or_else(
                || {
                    let mut session = parse_transcript(&path);
                    session.last_active = mtime;
                    session
                },
                |c| c.session.clone(),
            );
            (
                path,
                Cached {
                    size,
                    mtime,
                    session,
                },
            )
        })
        .collect();

    let sessions = current.values().map(|c| c.session.clone()).collect();
    if let Some(cache_file) = cache_file {
        let cache = Cache {
            version: CACHE_VERSION,
            transcripts: current,
        };
        if let (Some(parent), Ok(json)) = (cache_file.parent(), serde_json::to_vec(&cache)) {
            let _ = std::fs::create_dir_all(parent);
            let _ = std::fs::write(&cache_file, json);
        }
    }
    sessions
}

fn parse_transcript(path: &Path) -> Session {
    let Ok(bytes) = std::fs::read(path) else {
        return Session::default();
    };
    let title_marker = memchr::memmem::Finder::new(b"\"ai-title\"");
    let tool_marker = memchr::memmem::Finder::new(b"\"tool_use\"");
    let mut session = Session::default();

    for line in bytes.split(|&b| b == b'\n') {
        let is_title = title_marker.find(line).is_some();
        if !is_title && tool_marker.find(line).is_none() {
            continue;
        }
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if is_title {
            if let Some(title) = record.get("aiTitle").and_then(serde_json::Value::as_str) {
                session.title = Some(title.to_owned());
            }
            continue;
        }
        let at = record
            .get("timestamp")
            .and_then(serde_json::Value::as_str)
            .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
            .and_then(|t| u64::try_from(t.timestamp()).ok())
            .unwrap_or(0);
        let cwd = record
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .map(PathBuf::from);
        let Some(blocks) = record
            .pointer("/message/content")
            .and_then(serde_json::Value::as_array)
        else {
            continue;
        };
        for block in blocks {
            if block.get("type").and_then(serde_json::Value::as_str) != Some("tool_use") {
                continue;
            }
            let input = block.get("input");
            let field = |name: &str| {
                input
                    .and_then(|i| i.get(name))
                    .and_then(serde_json::Value::as_str)
            };
            let paths = match block.get("name").and_then(serde_json::Value::as_str) {
                Some("Bash") => field("command")
                    .map(|c| created_by_command(c, cwd.as_deref()))
                    .unwrap_or_default(),
                Some("Write") => field("file_path").map(PathBuf::from).into_iter().collect(),
                _ => Vec::new(),
            };
            session
                .created
                .extend(paths.into_iter().map(|path| Created { path, at }));
        }
    }
    session
}

/// Split a command line into words, honouring quotes. Operators that end a
/// simple command become words of their own.
fn shell_words(command: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    let flush = |word: &mut String, words: &mut Vec<String>| {
        if !word.is_empty() {
            words.push(std::mem::take(word));
        }
    };
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                }
            }
            (None, c) if c.is_whitespace() => flush(&mut word, &mut words),
            (None, ';' | '|' | '&' | '(' | ')') => {
                flush(&mut word, &mut words);
                words.push(";".to_owned());
            }
            (None, '>') => {
                flush(&mut word, &mut words);
                while chars.peek() == Some(&'>') {
                    chars.next();
                }
                words.push(">".to_owned());
            }
            (_, c) => word.push(c),
        }
    }
    flush(&mut word, &mut words);
    words
}

fn expand(word: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let home = dirs::home_dir();
    let expanded = if let Some(rest) = word.strip_prefix("~/") {
        home?.join(rest)
    } else if let Some(rest) = word
        .strip_prefix("$HOME/")
        .or_else(|| word.strip_prefix("${HOME}/"))
    {
        home?.join(rest)
    } else if let Some(rest) = word
        .strip_prefix("$TMPDIR/")
        .or_else(|| word.strip_prefix("${TMPDIR}/"))
    {
        PathBuf::from(std::env::var_os("TMPDIR")?).join(rest)
    } else if word.starts_with('$') || word.contains("$(") {
        return None;
    } else {
        let path = PathBuf::from(word);
        if path.is_absolute() {
            path
        } else {
            cwd?.join(path)
        }
    };
    Some(expanded)
}

/// Options of `git clone` and `git worktree add` whose value is the next word.
const FLAGS_WITH_VALUES: &[&str] = &[
    "--depth",
    "-b",
    "-B",
    "--branch",
    "--origin",
    "--reference",
    "--filter",
    "-c",
    "--config",
    "-j",
    "--jobs",
    "--template",
    "--separate-git-dir",
    "--shallow-since",
    "--shallow-exclude",
    "--reason",
];

/// Paths a shell command creates: `mkdir` operands, clone and worktree
/// destinations, build output directories, copy targets and redirections.
fn created_by_command(command: &str, cwd: Option<&Path>) -> Vec<PathBuf> {
    const VALUE_FLAGS: &[&str] = &["--target-dir", "--out-dir", "--output", "-o", "--prefix"];
    let words = shell_words(command);
    let mut created = Vec::new();
    let mut push = |word: &str| {
        if let Some(path) = expand(word, cwd) {
            created.push(path);
        }
    };

    for simple in words.split(|w| w == ";") {
        let mut i = 0;
        while i < simple.len() {
            let word = simple[i].as_str();
            if word == ">" {
                if let Some(target) = simple.get(i + 1) {
                    push(target);
                }
                i += 2;
                continue;
            }
            if let Some(value) = word
                .strip_prefix("CARGO_TARGET_DIR=")
                .or_else(|| word.strip_prefix("--target-dir="))
                .or_else(|| word.strip_prefix("--out-dir="))
            {
                push(value);
            } else if VALUE_FLAGS.contains(&word)
                && let Some(value) = simple.get(i + 1)
            {
                push(value);
                i += 1;
            }
            i += 1;
        }

        let mut positional: Vec<&str> = Vec::new();
        let mut words = simple.iter().map(String::as_str).take_while(|w| *w != ">");
        while let Some(word) = words.next() {
            if FLAGS_WITH_VALUES.contains(&word) {
                words.next();
            } else if !word.starts_with('-') && !word.contains('=') {
                positional.push(word);
            }
        }
        match positional.as_slice() {
            ["mkdir", dirs @ ..] => dirs.iter().for_each(|d| push(d)),
            ["git", "clone", _, dest, ..]
            | ["git", "worktree", "add", dest, ..]
            | ["cp" | "rsync" | "mv" | "ln", .., _, dest] => push(dest),
            ["tee", files @ ..] => files.iter().for_each(|f| push(f)),
            _ => {}
        }
    }
    created
}

#[cfg(test)]
mod tests {
    use super::*;

    fn created(command: &str) -> Vec<String> {
        created_by_command(command, Some(Path::new("/work/proj")))
            .into_iter()
            .map(|p| p.display().to_string())
            .collect()
    }

    #[test]
    fn creating_commands_name_what_they_create() {
        assert_eq!(
            created("mkdir -p /tmp/a /tmp/b && cd /tmp/a"),
            ["/tmp/a", "/tmp/b"]
        );
        assert_eq!(
            created("git clone --depth 1 https://example.com/acme.git /tmp/acme"),
            ["/tmp/acme"]
        );
        assert_eq!(
            created("git worktree add -b feature /tmp/wt main"),
            ["/tmp/wt"]
        );
        assert_eq!(
            created("CARGO_TARGET_DIR=/tmp/t cargo build --release"),
            ["/tmp/t"]
        );
        assert_eq!(created("cargo build --target-dir /tmp/t2"), ["/tmp/t2"]);
        assert_eq!(created("cp -r src /tmp/copy"), ["/tmp/copy"]);
        assert_eq!(created("just test > /tmp/out.log 2>&1"), ["/tmp/out.log"]);
        assert_eq!(created("echo 'a > b' | tee \"/tmp/x y\""), ["/tmp/x y"]);
        assert_eq!(created("mkdir build"), ["/work/proj/build"]);
    }

    #[test]
    fn reading_commands_create_nothing() {
        assert_eq!(created("ls -la /tmp/a"), Vec::<String>::new());
        assert_eq!(created("cat /tmp/a | grep x"), Vec::<String>::new());
        assert_eq!(created("mkdir \"$(mktemp -d)\""), Vec::<String>::new());
    }

    fn session(title: &str, last_active: u64, created: &[(&Path, u64)]) -> Session {
        Session {
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
        let sessions = [session(
            "Build the acme engine",
            born + 600,
            &[(&made.join("debug"), born), (&made, born)],
        )];

        let found = leftovers(&sessions, std::slice::from_ref(&base), later, QUIET_FOR);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].path, made);
        assert!(
            found[0]
                .description
                .starts_with("Left by \"Build the acme engine\"")
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

    #[test]
    fn transcripts_yield_their_title_and_what_their_tools_created() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let transcript = tmp.path().join("s.jsonl");
        let lines = [
            r#"{"type":"ai-title","aiTitle":"Profile the engine","sessionId":"s"}"#,
            r#"{"type":"assistant","timestamp":"2026-11-01T10:00:00Z","cwd":"/work/proj","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"mkdir -p /tmp/prof && cargo build --target-dir /tmp/prof/t"}}]}}"#,
            r#"{"type":"assistant","timestamp":"2026-11-01T10:05:00Z","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/tmp/prof/notes.md"}}]}}"#,
            r#"{"type":"user","message":{"content":"no tools here"}}"#,
        ];
        std::fs::write(&transcript, lines.join("\n")).expect("write");

        let session = parse_transcript(&transcript);
        assert_eq!(session.title.as_deref(), Some("Profile the engine"));
        let paths: Vec<_> = session
            .created
            .iter()
            .map(|c| c.path.display().to_string())
            .collect();
        assert_eq!(paths, ["/tmp/prof", "/tmp/prof/t", "/tmp/prof/notes.md"]);
        assert_eq!(session.created[0].at, 1_793_527_200);
    }
}
