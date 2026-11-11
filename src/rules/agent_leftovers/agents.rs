//! Where each coding agent keeps its session records, and how to read them.
//!
//! Every format here was read from the agent's own source, not inferred from
//! sample files. GitHub Copilot CLI is absent on purpose: its source is not
//! public and its session format is undocumented, so there is nothing to read
//! it against.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::shell::{argv_command, created_by_command, expand, patch_targets};
use super::{Created, Session, secs, top_level};
use crate::rules::agent_sessions::{
    claude_home, cline_data, codex_home, gemini_home, qwen_home, xdg_data,
};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Agent {
    Claude,
    Codex,
    Gemini,
    Qwen,
    Opencode,
    Cline,
    Roo,
}

impl Agent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::Qwen => "Qwen Code",
            Self::Opencode => "opencode",
            Self::Cline => "Cline",
            Self::Roo => "Roo Code",
        }
    }
}

/// One file (or database, or directory) holding one or more sessions, cached
/// as a whole by the size and modification time of `stamp_paths`.
pub struct Unit {
    pub path: PathBuf,
    stamp_paths: Vec<PathBuf>,
    kind: Kind,
}

enum Kind {
    Claude,
    Codex,
    Gemini {
        project_root: Option<PathBuf>,
    },
    Qwen,
    OpencodeDb,
    OpencodeStorage,
    ClineTask {
        cwd: Option<PathBuf>,
        title: Option<String>,
    },
    ClineSession,
    RooTask,
}

impl Unit {
    fn file(path: PathBuf, kind: Kind) -> Self {
        Self {
            stamp_paths: vec![path.clone()],
            path,
            kind,
        }
    }

    /// Total size and newest modification time of what the unit is read from.
    pub fn stamp(&self) -> (u64, u64) {
        self.stamp_paths
            .iter()
            .filter_map(|p| p.metadata().ok())
            .fold((0, 0), |(size, mtime), meta| {
                let modified = meta.modified().map_or(0, secs);
                (size + meta.len(), mtime.max(modified))
            })
    }

    pub fn parse(&self, bases: &[PathBuf]) -> Vec<Session> {
        let last_active = self.stamp().1;
        let single = |agent: Agent, parsed: Parsed| {
            vec![parsed.into_session(agent, &self.path, last_active)]
        };
        match &self.kind {
            Kind::Claude => single(Agent::Claude, parse_claude(&self.path, bases)),
            Kind::Codex => single(Agent::Codex, parse_codex(&self.path, bases)),
            Kind::Gemini { project_root } => single(
                Agent::Gemini,
                parse_gemini(&self.path, project_root.as_deref(), bases),
            ),
            Kind::Qwen => single(Agent::Qwen, parse_qwen(&self.path, bases)),
            Kind::OpencodeDb => opencode_db(&self.path, bases),
            Kind::OpencodeStorage => opencode_storage(&self.path, bases),
            Kind::ClineTask { cwd, title } => {
                let mut parsed = parse_ui_messages(&self.path, cwd.as_deref(), bases);
                parsed.title.clone_from(title);
                parsed.id = self.path.parent().map(dir_name);
                single(Agent::Cline, parsed)
            }
            Kind::ClineSession => single(Agent::Cline, parse_cline_session(&self.path, bases)),
            Kind::RooTask => {
                let history = self
                    .path
                    .parent()
                    .and_then(|dir| read_json(&dir.join("history_item.json")));
                let field = |name: &str| {
                    history
                        .as_ref()
                        .and_then(|h| h.get(name))
                        .and_then(Value::as_str)
                };
                let cwd = field("workspace").map(PathBuf::from);
                let mut parsed = parse_ui_messages(&self.path, cwd.as_deref(), bases);
                parsed.title = field("task").map(short_title);
                parsed.id = self.path.parent().map(dir_name);
                single(Agent::Roo, parsed)
            }
        }
    }
}

/// One session's facts while its record is read.
#[derive(Default)]
struct Parsed {
    id: Option<String>,
    title: Option<String>,
    created: FxHashMap<PathBuf, u64>,
}

impl Parsed {
    fn into_session(self, agent: Agent, path: &Path, last_active: u64) -> Session {
        Session {
            agent,
            id: self.id.unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }),
            title: self.title,
            last_active,
            created: self
                .created
                .into_iter()
                .map(|(path, at)| Created { path, at })
                .collect(),
        }
    }
}

/// Collects what a record says was created, keeping only the disposable
/// directories it lands in and the earliest time each was named.
struct Recorder<'a> {
    bases: &'a [PathBuf],
    parsed: &'a mut Parsed,
}

impl Recorder<'_> {
    fn path(&mut self, path: &Path, at: u64) {
        if let Some(root) = top_level(path, self.bases) {
            let first = self.parsed.created.entry(root).or_insert(at);
            *first = (*first).min(at);
        }
    }

    fn shell(&mut self, command: &str, cwd: Option<&Path>, at: u64) {
        for path in created_by_command(command, cwd) {
            self.path(&path, at);
        }
    }

    fn wrote(&mut self, path: &str, cwd: Option<&Path>, at: u64) {
        if let Some(path) = expand(path, cwd) {
            self.path(&path, at);
        }
    }

    fn patch(&mut self, patch: &str, cwd: Option<&Path>, at: u64) {
        for target in patch_targets(patch) {
            self.wrote(target, cwd, at);
        }
    }
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn short_title(text: &str) -> String {
    let first = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut title: String = first.chars().take(80).collect();
    if first.chars().count() > 80 {
        title.push('\u{2026}');
    }
    title
}

fn iso_secs(value: Option<&Value>) -> u64 {
    value
        .and_then(Value::as_str)
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok())
        .and_then(|t| u64::try_from(t.timestamp()).ok())
        .unwrap_or(0)
}

fn ms_secs(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_u64).map_or(0, |ms| ms / 1000)
}

fn str_of<'a>(value: &'a Value, pointer: &str) -> Option<&'a str> {
    value.pointer(pointer).and_then(Value::as_str)
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The JSON lines of `path` that contain any of `markers`; a record runs to
/// gigabytes of request bodies, and only a few kinds of line matter.
fn json_lines(path: &Path, markers: &[&str]) -> Vec<Value> {
    let Ok(bytes) = std::fs::read(path) else {
        return Vec::new();
    };
    let finders: Vec<_> = markers
        .iter()
        .map(|m| memchr::memmem::Finder::new(m.as_bytes()))
        .collect();
    bytes
        .split(|&b| b == b'\n')
        .filter(|line| finders.iter().any(|f| f.find(line).is_some()))
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect()
}

fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|read_dir| read_dir.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

fn files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    children(dir)
        .into_iter()
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == extension))
        .collect()
}

fn walk_files(root: &Path, depth: usize, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    ignore::WalkBuilder::new(root)
        .standard_filters(false)
        .max_depth(Some(depth))
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()) && keep(e.path()))
        .map(ignore::DirEntry::into_path)
        .collect()
}

/// VS Code and its forks' extension storage, where Cline and Roo keep tasks.
fn editor_global_storage() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    crate::rules::ide::EDITOR_SUPPORT_DIRS
        .iter()
        .map(|support| home.join(support).join("User/globalStorage"))
        .collect()
}

/// Every record of every agent this machine has.
pub fn units() -> Vec<Unit> {
    let mut units = Vec::new();
    claude_units(&mut units);
    codex_units(&mut units);
    gemini_units(&mut units);
    qwen_units(&mut units);
    opencode_units(&mut units);
    cline_units(&mut units);
    cline_session_units(&mut units);
    roo_units(&mut units);
    units
}

/// Claude Code: `<config>/projects/<project>/<session>.jsonl`, subagents below.
fn claude_units(units: &mut Vec<Unit>) {
    for path in walk_files(&claude_home().join("projects"), 4, |p| {
        p.extension().is_some_and(|x| x == "jsonl")
    }) {
        units.push(Unit::file(path, Kind::Claude));
    }
}

/// Codex: `sessions/YYYY/MM/DD/rollout-*.jsonl` and `archived_sessions/`.
fn codex_units(units: &mut Vec<Unit>) {
    let rollout = |p: &Path| {
        p.extension().is_some_and(|x| x == "jsonl")
            && p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
    };
    let codex = codex_home();
    for path in walk_files(&codex.join("sessions"), 4, rollout)
        .into_iter()
        .chain(walk_files(&codex.join("archived_sessions"), 1, rollout))
    {
        units.push(Unit::file(path, Kind::Codex));
    }
}

/// Gemini CLI: `tmp/<project>/chats/*.jsonl` (older: `*.json`), subagents in
/// `chats/<parent>/`; the project's root is in `tmp/<project>/.project_root`.
fn gemini_units(units: &mut Vec<Unit>) {
    for project in children(&gemini_home().join("tmp")) {
        let project_root = std::fs::read_to_string(project.join(".project_root"))
            .ok()
            .map(|root| PathBuf::from(root.trim()));
        let chats = project.join("chats");
        let mut files = files_with_extension(&chats, "jsonl");
        files.extend(files_with_extension(&chats, "json"));
        for sub in children(&chats).into_iter().filter(|p| p.is_dir()) {
            files.extend(files_with_extension(&sub, "jsonl"));
        }
        for path in files {
            units.push(Unit::file(
                path,
                Kind::Gemini {
                    project_root: project_root.clone(),
                },
            ));
        }
    }
}

/// Qwen Code: `<runtime>/projects/<cwd>/chats/<id>.jsonl`, `chats/archive/`,
/// and `subagents/<id>/agent-*.jsonl`.
fn qwen_units(units: &mut Vec<Unit>) {
    let qwen_runtime = std::env::var_os("QWEN_RUNTIME_DIR")
        .filter(|v| !v.is_empty())
        .map_or_else(qwen_home, PathBuf::from);
    for project in children(&qwen_runtime.join("projects")) {
        let mut files = files_with_extension(&project.join("chats"), "jsonl");
        files.extend(files_with_extension(
            &project.join("chats/archive"),
            "jsonl",
        ));
        for sub in children(&project.join("subagents")) {
            files.extend(files_with_extension(&sub, "jsonl"));
        }
        for path in files {
            units.push(Unit::file(path, Kind::Qwen));
        }
    }
}

/// opencode: `opencode*.db` (SQLite, 1.2 on), and `storage/` JSON before that.
fn opencode_units(units: &mut Vec<Unit>) {
    let opencode = xdg_data().join("opencode");
    let mut databases: Vec<PathBuf> = children(&opencode)
        .into_iter()
        .filter(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
            name.is_some_and(|n| n.starts_with("opencode"))
                && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("db"))
        })
        .collect();
    if let Some(custom) = std::env::var_os("OPENCODE_DB").map(PathBuf::from) {
        let custom = if custom.is_absolute() {
            custom
        } else {
            opencode.join(custom)
        };
        if !databases.contains(&custom) {
            databases.push(custom);
        }
    }
    for db in databases {
        let mut wal = db.clone().into_os_string();
        wal.push("-wal");
        units.push(Unit {
            stamp_paths: vec![db.clone(), PathBuf::from(wal)],
            path: db,
            kind: Kind::OpencodeDb,
        });
    }
    let storage = opencode.join("storage");
    if storage.join("session").is_dir() {
        units.push(Unit {
            stamp_paths: vec![storage.join("session"), storage.join("part")],
            path: storage,
            kind: Kind::OpencodeStorage,
        });
    }
}

/// Cline before 4.x: `<root>/tasks/<id>/ui_messages.json`, with each task's
/// cwd and title in `<root>/state/taskHistory.json`.
fn cline_units(units: &mut Vec<Unit>) {
    let mut cline_roots = vec![cline_data()];
    cline_roots.extend(
        editor_global_storage()
            .into_iter()
            .map(|s| s.join("saoudrizwan.claude-dev")),
    );
    for root in cline_roots {
        let history: FxHashMap<String, (Option<PathBuf>, Option<String>)> =
            read_json(&root.join("state/taskHistory.json"))
                .and_then(|h| h.as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|item| {
                    let id = str_of(item, "/id")?.to_owned();
                    let cwd = str_of(item, "/cwdOnTaskInitialization").map(PathBuf::from);
                    let title = str_of(item, "/task").map(short_title);
                    Some((id, (cwd, title)))
                })
                .collect();
        for task in children(&root.join("tasks")) {
            let messages = task.join("ui_messages.json");
            if !messages.is_file() {
                continue;
            }
            let (cwd, title) = history.get(&dir_name(&task)).cloned().unwrap_or_default();
            units.push(Unit::file(messages, Kind::ClineTask { cwd, title }));
        }
    }
}

/// Cline 4.x: `<data>/sessions/<id>/` with `<id>.json` and `*.messages.json`.
fn cline_session_units(units: &mut Vec<Unit>) {
    let cline_sessions = std::env::var_os("CLINE_SESSION_DATA_DIR")
        .filter(|v| !v.is_empty())
        .map_or_else(|| cline_data().join("sessions"), PathBuf::from);
    for dir in children(&cline_sessions).into_iter().filter(|p| p.is_dir()) {
        let stamp_paths: Vec<PathBuf> = children(&dir)
            .into_iter()
            .filter(|p| p.to_string_lossy().ends_with(".messages.json"))
            .collect();
        if !stamp_paths.is_empty() {
            units.push(Unit {
                stamp_paths,
                path: dir,
                kind: Kind::ClineSession,
            });
        }
    }
}

/// Roo Code: `<globalStorage>/rooveterinaryinc.roo-cline/tasks/<id>/`, and
/// the Roo CLI's `~/.vscode-mock/global-storage/tasks/<id>/`.
fn roo_units(units: &mut Vec<Unit>) {
    let mut roo_roots: Vec<PathBuf> = editor_global_storage()
        .into_iter()
        .map(|s| s.join("rooveterinaryinc.roo-cline"))
        .collect();
    if let Some(home) = dirs::home_dir() {
        roo_roots.push(home.join(".vscode-mock/global-storage"));
    }
    for root in roo_roots {
        for task in children(&root.join("tasks")) {
            let messages = task.join("ui_messages.json");
            if messages.is_file() {
                units.push(Unit::file(messages, Kind::RooTask));
            }
        }
    }
}

/// Codex keeps titles outside its rollouts, in an append-only index where the
/// last line for a session wins.
pub fn name_untitled(sessions: &mut [Session]) {
    let index = codex_home().join("session_index.jsonl");
    let Ok(text) = std::fs::read_to_string(index) else {
        return;
    };
    let mut names: FxHashMap<String, String> = FxHashMap::default();
    for line in text.lines() {
        if let Ok(entry) = serde_json::from_str::<Value>(line)
            && let (Some(id), Some(name)) = (str_of(&entry, "/id"), str_of(&entry, "/thread_name"))
        {
            names.insert(id.to_owned(), short_title(name));
        }
    }
    for session in sessions.iter_mut().filter(|s| s.agent == Agent::Codex) {
        if let Some(name) = names.get(&session.id) {
            session.title = Some(name.clone());
        }
    }
}

/// Claude Code: `{"type":"assistant","timestamp","cwd","sessionId",
/// "message":{"content":[{"type":"tool_use","name":"Bash","input":{...}}]}}`,
/// and `{"type":"ai-title","aiTitle"}`.
fn parse_claude(path: &Path, bases: &[PathBuf]) -> Parsed {
    let mut parsed = Parsed::default();
    let records = json_lines(path, &["\"ai-title\"", "\"tool_use\""]);
    let mut recorder = Recorder {
        bases,
        parsed: &mut parsed,
    };
    let mut id = None;
    let mut title = None;
    for record in &records {
        if id.is_none() {
            id = str_of(record, "/sessionId").map(str::to_owned);
        }
        if let Some(t) = str_of(record, "/aiTitle") {
            title = Some(t.to_owned());
            continue;
        }
        let at = iso_secs(record.get("timestamp"));
        let cwd = str_of(record, "/cwd").map(PathBuf::from);
        let Some(blocks) = record.pointer("/message/content").and_then(Value::as_array) else {
            continue;
        };
        for block in blocks
            .iter()
            .filter(|b| str_of(b, "/type") == Some("tool_use"))
        {
            match str_of(block, "/name") {
                Some("Bash") => {
                    if let Some(command) = str_of(block, "/input/command") {
                        recorder.shell(command, cwd.as_deref(), at);
                    }
                }
                Some("Write") => {
                    if let Some(file) = str_of(block, "/input/file_path") {
                        recorder.wrote(file, cwd.as_deref(), at);
                    }
                }
                _ => {}
            }
        }
    }
    parsed.id = id;
    parsed.title = title;
    parsed
}

/// Codex rollouts: `{"timestamp","type":<tag>,"payload":{...}}`. Before the
/// envelope existed (0.2 to about 0.20) each line was the bare payload.
fn parse_codex(path: &Path, bases: &[PathBuf]) -> Parsed {
    let records = json_lines(
        path,
        &[
            "session_meta",
            "turn_context",
            "function_call",
            "local_shell_call",
            "custom_tool_call",
            "item_completed",
            "patch_apply_end",
            "user_message",
        ],
    );
    let mut parsed = Parsed::default();
    let mut id = None;
    let mut title = None;
    let mut cwd: Option<PathBuf> = None;
    let mut recorder = Recorder {
        bases,
        parsed: &mut parsed,
    };
    for record in &records {
        let at = iso_secs(record.get("timestamp"));
        let payload = record.get("payload").unwrap_or(record);
        match (str_of(record, "/type"), str_of(payload, "/type")) {
            (Some("session_meta"), _) => {
                id = str_of(payload, "/id").map(str::to_owned);
                cwd = str_of(payload, "/cwd").map(PathBuf::from);
            }
            (Some("turn_context"), _) => {
                if let Some(turn_cwd) = str_of(payload, "/cwd") {
                    cwd = Some(PathBuf::from(turn_cwd));
                }
            }
            (_, Some("user_message")) => {
                if title.is_none() {
                    title = str_of(payload, "/message").map(short_title);
                }
            }
            (_, Some("item_completed")) => {
                let item = payload.get("item").unwrap_or(&Value::Null);
                match str_of(item, "/type") {
                    Some("FileChange") => codex_changes(item.get("changes"), &mut recorder, at),
                    Some("CommandExecution") => {
                        let item_cwd = str_of(item, "/cwd")
                            .map(|c| PathBuf::from(c.strip_prefix("file://").unwrap_or(c)));
                        let command = argv_command(&strings(item.get("command")));
                        recorder.shell(&command, item_cwd.as_deref().or(cwd.as_deref()), at);
                    }
                    _ => {}
                }
            }
            (_, Some("patch_apply_end")) => {
                codex_changes(payload.get("changes"), &mut recorder, at);
            }
            (_, Some("function_call")) => codex_call(payload, cwd.as_deref(), &mut recorder, at),
            (_, Some("local_shell_call")) => {
                let here = str_of(payload, "/action/working_directory")
                    .map(PathBuf::from)
                    .or_else(|| cwd.clone());
                let argv = strings(payload.pointer("/action/command"));
                codex_argv(&argv, here.as_deref(), &mut recorder, at);
            }
            (_, Some("custom_tool_call")) => {
                if str_of(payload, "/name") == Some("apply_patch")
                    && let Some(patch) = str_of(payload, "/input")
                {
                    recorder.patch(patch, cwd.as_deref(), at);
                }
            }
            _ => {}
        }
    }
    parsed.id = id;
    parsed.title = title;
    parsed
}

/// A Codex `function_call`: `arguments` is a JSON string (an object in some
/// older rollouts), and the command's field depends on the tool's name.
fn codex_call(payload: &Value, cwd: Option<&Path>, recorder: &mut Recorder<'_>, at: u64) {
    let arguments = payload.get("arguments");
    let args = arguments
        .and_then(Value::as_str)
        .and_then(|a| serde_json::from_str::<Value>(a).ok())
        .or_else(|| arguments.cloned())
        .unwrap_or(Value::Null);
    let workdir = str_of(&args, "/workdir").and_then(|w| expand(w, cwd));
    let here = workdir.as_deref().or(cwd);
    match str_of(payload, "/name") {
        Some("exec_command") => {
            if let Some(command) = str_of(&args, "/cmd") {
                recorder.shell(command, here, at);
            }
        }
        Some("shell_command") => {
            if let Some(command) = str_of(&args, "/command") {
                recorder.shell(command, here, at);
            }
        }
        Some("shell" | "container.exec") => {
            codex_argv(&strings(args.get("command")), here, recorder, at);
        }
        Some("unified_exec") => {
            codex_argv(&strings(args.get("input")), here, recorder, at);
        }
        Some("apply_patch") => {
            if let Some(patch) = str_of(&args, "/input") {
                recorder.patch(patch, here, at);
            }
        }
        _ => {}
    }
}

fn codex_argv(argv: &[String], cwd: Option<&Path>, recorder: &mut Recorder<'_>, at: u64) {
    match argv {
        [tool, patch, ..] if tool == "apply_patch" || tool == "applypatch" => {
            recorder.patch(patch, cwd, at);
        }
        _ => recorder.shell(&argv_command(argv), cwd, at),
    }
}

/// `changes` maps absolute paths to `{"type":"add"}` (paginated items) or to
/// `{"add":{...}}` (legacy `patch_apply_end`).
fn codex_changes(changes: Option<&Value>, recorder: &mut Recorder<'_>, at: u64) {
    let Some(changes) = changes.and_then(Value::as_object) else {
        return;
    };
    for (path, change) in changes {
        if str_of(change, "/type") == Some("add") || change.get("add").is_some() {
            recorder.wrote(path, None, at);
        }
    }
}

/// Gemini CLI: JSONL whose first line is the session header, messages with
/// `toolCalls[{name,args,timestamp}]`, and `{"$set":{"summary"}}` updates; a
/// session from before JSONL is one document with `messages[]`.
fn parse_gemini(path: &Path, project_root: Option<&Path>, bases: &[PathBuf]) -> Parsed {
    let records = if path.extension().is_some_and(|x| x == "json") {
        read_json(path).map_or_else(Vec::new, |doc| {
            let mut records = vec![doc.clone()];
            records.extend(
                doc.get("messages")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            );
            records
        })
    } else {
        json_lines(path, &["toolCalls", "sessionId", "summary"])
    };
    let mut parsed = Parsed::default();
    let mut id = None;
    let mut title = None;
    let mut recorder = Recorder {
        bases,
        parsed: &mut parsed,
    };
    for record in &records {
        if id.is_none() {
            id = str_of(record, "/sessionId").map(str::to_owned);
        }
        if let Some(summary) =
            str_of(record, "/summary").or_else(|| str_of(record, "/$set/summary"))
        {
            title = Some(short_title(summary));
        }
        let Some(calls) = record.get("toolCalls").and_then(Value::as_array) else {
            continue;
        };
        for call in calls {
            let at = iso_secs(call.get("timestamp").or_else(|| record.get("timestamp")));
            match str_of(call, "/name") {
                Some("run_shell_command") => {
                    let dir = str_of(call, "/args/dir_path").filter(|d| !d.is_empty());
                    let here = match (dir, project_root) {
                        (Some(dir), root) => expand(dir, root),
                        (None, root) => root.map(Path::to_path_buf),
                    };
                    if let Some(command) = str_of(call, "/args/command") {
                        recorder.shell(command, here.as_deref(), at);
                    }
                }
                Some("write_file" | "replace") => {
                    if let Some(file) = str_of(call, "/args/file_path") {
                        recorder.wrote(file, project_root, at);
                    }
                }
                _ => {}
            }
        }
    }
    parsed.id = id;
    parsed.title = title;
    parsed
}

/// Qwen Code: every line carries `sessionId`, `timestamp` and `cwd`; tool calls
/// are `message.parts[].functionCall{name,args}`; a title is a `system` record
/// with `subtype:"custom_title"`.
fn parse_qwen(path: &Path, bases: &[PathBuf]) -> Parsed {
    let records = json_lines(path, &["functionCall", "custom_title"]);
    let mut parsed = Parsed::default();
    let mut id = None;
    let mut title = None;
    let mut recorder = Recorder {
        bases,
        parsed: &mut parsed,
    };
    for record in &records {
        if id.is_none() {
            id = str_of(record, "/sessionId").map(str::to_owned);
        }
        if str_of(record, "/subtype") == Some("custom_title") {
            title = str_of(record, "/systemPayload/customTitle").map(short_title);
            continue;
        }
        let at = iso_secs(record.get("timestamp"));
        let cwd = str_of(record, "/cwd").map(PathBuf::from);
        let Some(parts) = record.pointer("/message/parts").and_then(Value::as_array) else {
            continue;
        };
        for call in parts.iter().filter_map(|p| p.get("functionCall")) {
            match str_of(call, "/name") {
                Some("run_shell_command") => {
                    let here = str_of(call, "/args/directory")
                        .filter(|d| !d.is_empty())
                        .map(PathBuf::from)
                        .or_else(|| cwd.clone());
                    if let Some(command) = str_of(call, "/args/command") {
                        recorder.shell(command, here.as_deref(), at);
                    }
                }
                Some("write_file" | "edit") => {
                    if let Some(file) = str_of(call, "/args/file_path") {
                        recorder.wrote(file, cwd.as_deref(), at);
                    }
                }
                Some("notebook_edit") => {
                    if let Some(file) = str_of(call, "/args/notebook_path") {
                        recorder.wrote(file, cwd.as_deref(), at);
                    }
                }
                _ => {}
            }
        }
    }
    if id.is_none()
        && path.parent().is_some_and(|p| {
            p.ends_with("subagents") || p.parent().is_some_and(|g| g.ends_with("subagents"))
        })
    {
        id = path.parent().map(dir_name);
    }
    parsed.id = id;
    parsed.title = title;
    parsed
}

/// A session row from opencode: `{id, title, directory, time_updated}` (ms).
struct OpencodeSession {
    title: Option<String>,
    directory: Option<PathBuf>,
    updated: u64,
}

/// Build sessions from opencode's session rows and tool entries.
///
/// A tool entry is a 1.x part `{type:"tool",tool,state:{input,time:{start}}}`
/// or a 2.x entry `{type:"tool",name,state:{input},time:{created}}`; bash and
/// shell run `input.command` in `input.workdir` (relative to the session's
/// directory), write and edit name `filePath` (1.x) or `path` (2.x), and patch
/// tools carry `patchText`.
fn opencode_sessions(
    rows: &FxHashMap<String, OpencodeSession>,
    tools: &[(String, Value, u64)],
    bases: &[PathBuf],
) -> Vec<Session> {
    let mut parsed: FxHashMap<&str, Parsed> = FxHashMap::default();
    for (sid, data, row_at) in tools {
        let session = rows.get(sid);
        let dir = session.and_then(|s| s.directory.as_deref());
        let at = [
            data.pointer("/state/time/start"),
            data.pointer("/time/created"),
        ]
        .into_iter()
        .map(ms_secs)
        .find(|&t| t > 0)
        .unwrap_or(*row_at);
        let input = data.pointer("/state/input").unwrap_or(&Value::Null);
        let mut recorder = Recorder {
            bases,
            parsed: parsed.entry(sid.as_str()).or_default(),
        };
        match str_of(data, "/tool").or_else(|| str_of(data, "/name")) {
            Some("bash" | "shell") => {
                let here = str_of(input, "/workdir")
                    .and_then(|w| expand(w, dir))
                    .or_else(|| dir.map(Path::to_path_buf));
                if let Some(command) = str_of(input, "/command") {
                    recorder.shell(command, here.as_deref(), at);
                }
            }
            Some("write" | "edit" | "multiedit") => {
                if let Some(file) = str_of(input, "/filePath").or_else(|| str_of(input, "/path")) {
                    recorder.wrote(file, dir, at);
                }
            }
            Some("apply_patch" | "patch") => {
                if let Some(patch) = str_of(input, "/patchText") {
                    recorder.patch(patch, dir, at);
                }
            }
            _ => {}
        }
    }
    parsed
        .into_iter()
        .filter(|(_, p)| !p.created.is_empty())
        .map(|(sid, mut p)| {
            let session = rows.get(sid);
            p.id = Some(sid.to_owned());
            p.title = session.and_then(|s| s.title.as_deref()).map(short_title);
            let updated = session.map_or(0, |s| s.updated);
            p.into_session(Agent::Opencode, Path::new(sid), updated)
        })
        .collect()
}

fn opencode_row(row: &Value, updated_pointer: &str) -> Option<(String, OpencodeSession)> {
    Some((
        str_of(row, "/id")?.to_owned(),
        OpencodeSession {
            title: str_of(row, "/title").map(str::to_owned),
            directory: str_of(row, "/directory")
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
            updated: ms_secs(row.pointer(updated_pointer)),
        },
    ))
}

/// Rows of a read-only query against an opencode database, through the
/// `sqlite3` shell; a table this version does not have yields nothing.
fn sqlite_rows(db: &Path, sql: &str) -> Vec<Value> {
    let db = db.display().to_string();
    crate::util::run_with_timeout(
        &["sqlite3", "-readonly", "-json", &db, sql],
        Duration::from_secs(20),
    )
    .success()
    .and_then(|output| serde_json::from_slice::<Vec<Value>>(&output.stdout).ok())
    .unwrap_or_default()
}

/// opencode 1.2 and later: `session`/`part` (1.x) and `session_v2`,
/// `session_part` and inline `session_message.data.content` (2.x), which can
/// sit side by side after an upgrade.
fn opencode_db(db: &Path, bases: &[PathBuf]) -> Vec<Session> {
    let mut rows: FxHashMap<String, OpencodeSession> = FxHashMap::default();
    for table in ["session", "session_v2"] {
        let sql = format!("SELECT id, title, directory, time_updated FROM {table}");
        rows.extend(
            sqlite_rows(db, &sql)
                .iter()
                .filter_map(|row| opencode_row(row, "/time_updated")),
        );
    }

    let data_of =
        |row: &Value| str_of(row, "/data").and_then(|d| serde_json::from_str::<Value>(d).ok());
    let mut tools: Vec<(String, Value, u64)> = Vec::new();
    let tool_queries = [
        "SELECT session_id, time_created, data FROM part WHERE json_extract(data, '$.type') = 'tool'",
        "SELECT session_id, 0 AS time_created, data FROM session_part WHERE type = 'tool'",
    ];
    for sql in tool_queries {
        for row in sqlite_rows(db, sql) {
            if let (Some(sid), Some(data)) = (str_of(&row, "/session_id"), data_of(&row)) {
                tools.push((sid.to_owned(), data, ms_secs(row.get("time_created"))));
            }
        }
    }
    let inline = "SELECT session_id, time_created, data FROM session_message \
                  WHERE type = 'assistant' AND data LIKE '%\"type\":\"tool\"%'";
    for row in sqlite_rows(db, inline) {
        let (Some(sid), Some(data)) = (str_of(&row, "/session_id"), data_of(&row)) else {
            continue;
        };
        for entry in data
            .get("content")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|e| str_of(e, "/type") == Some("tool"))
        {
            tools.push((
                sid.to_owned(),
                entry.clone(),
                ms_secs(row.get("time_created")),
            ));
        }
    }
    opencode_sessions(&rows, &tools, bases)
}

/// opencode up to 1.1: `storage/session/<project>/<id>.json` and
/// `storage/part/<message>/<part>.json`, each part naming its `sessionID`.
fn opencode_storage(storage: &Path, bases: &[PathBuf]) -> Vec<Session> {
    let rows: FxHashMap<String, OpencodeSession> = walk_files(&storage.join("session"), 2, |p| {
        p.extension().is_some_and(|x| x == "json")
    })
    .iter()
    .filter_map(|path| read_json(path))
    .filter_map(|row| opencode_row(&row, "/time/updated"))
    .collect();
    let tool_marker = memchr::memmem::Finder::new(b"\"type\":\"tool\"");
    let tools: Vec<(String, Value, u64)> = walk_files(&storage.join("part"), 2, |p| {
        p.extension().is_some_and(|x| x == "json")
    })
    .iter()
    .filter_map(|path| {
        let bytes = std::fs::read(path).ok()?;
        tool_marker.find(&bytes)?;
        let data: Value = serde_json::from_slice(&bytes).ok()?;
        Some((str_of(&data, "/sessionID")?.to_owned(), data, 0))
    })
    .collect();
    opencode_sessions(&rows, &tools, bases)
}

/// Cline before 4.x and Roo Code: `ui_messages.json`, an array of
/// `{ts,type,say|ask,text,partial}`. A command is `say` or `ask` `"command"`
/// with the command in `text` (Cline appends `REQ_APP` when it asked first);
/// a created file is a `"tool"` message whose `text` is JSON with
/// `tool:"newFileCreated"` and a `path` relative to the task's directory.
fn parse_ui_messages(path: &Path, cwd: Option<&Path>, bases: &[PathBuf]) -> Parsed {
    let mut parsed = Parsed::default();
    let messages = read_json(path)
        .and_then(|m| m.as_array().cloned())
        .unwrap_or_default();
    let mut recorder = Recorder {
        bases,
        parsed: &mut parsed,
    };
    for message in &messages {
        if message.get("partial").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let at = ms_secs(message.get("ts"));
        let kind = str_of(message, "/say").or_else(|| str_of(message, "/ask"));
        let Some(text) = str_of(message, "/text") else {
            continue;
        };
        match kind {
            Some("command") => {
                let command = text.trim().trim_end_matches("REQ_APP").trim();
                recorder.shell(command, cwd, at);
            }
            Some("tool") => {
                if let Ok(tool) = serde_json::from_str::<Value>(text)
                    && str_of(&tool, "/tool") == Some("newFileCreated")
                    && let Some(file) = str_of(&tool, "/path")
                {
                    recorder.wrote(file, cwd, at);
                }
            }
            _ => {}
        }
    }
    parsed
}

/// Cline 4.x: `<id>/<id>.json` holds `cwd`, `prompt` and `metadata.title`;
/// each `*.messages.json` holds `messages[{ts,content[{type:"tool_use",name,
/// input}]}]`. `run_commands` takes `commands`, in any of several shapes.
fn parse_cline_session(dir: &Path, bases: &[PathBuf]) -> Parsed {
    let id = dir_name(dir);
    let manifest = read_json(&dir.join(format!("{id}.json")));
    let cwd = manifest
        .as_ref()
        .and_then(|m| str_of(m, "/cwd"))
        .map(PathBuf::from);
    let title = manifest.as_ref().and_then(|m| {
        str_of(m, "/metadata/title")
            .or_else(|| str_of(m, "/prompt"))
            .map(short_title)
    });
    let mut parsed = Parsed::default();
    let mut recorder = Recorder {
        bases,
        parsed: &mut parsed,
    };
    for file in children(dir)
        .into_iter()
        .filter(|p| p.to_string_lossy().ends_with(".messages.json"))
    {
        let Some(doc) = read_json(&file) else {
            continue;
        };
        for message in doc
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let at = ms_secs(message.get("ts"));
            for block in message
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter(|b| str_of(b, "/type") == Some("tool_use"))
            {
                let input = block.get("input").unwrap_or(&Value::Null);
                match str_of(block, "/name") {
                    Some("run_commands") => {
                        for command in cline_commands(input) {
                            recorder.shell(&command, cwd.as_deref(), at);
                        }
                    }
                    Some("execute_command") => {
                        if let Some(command) = str_of(input, "/command") {
                            recorder.shell(command, cwd.as_deref(), at);
                        }
                    }
                    Some("editor" | "write_to_file") => {
                        if let Some(file) = str_of(input, "/path") {
                            recorder.wrote(file, cwd.as_deref(), at);
                        }
                    }
                    Some("apply_patch") => {
                        if let Some(patch) = str_of(input, "/input") {
                            recorder.patch(patch, cwd.as_deref(), at);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    parsed.id = Some(id);
    parsed.title = title;
    parsed
}

/// `run_commands` input: `{commands:[...]}` of strings or `{command,args}` /
/// `{cmd}` objects, or a bare string or array.
fn cline_commands(input: &Value) -> Vec<String> {
    fn one(item: &Value) -> Option<String> {
        if let Some(s) = item.as_str() {
            return Some(s.to_owned());
        }
        let command = str_of(item, "/command").or_else(|| str_of(item, "/cmd"))?;
        let args = strings(item.get("args"));
        Some(if args.is_empty() {
            command.to_owned()
        } else {
            format!("{command} {}", args.join(" "))
        })
    }
    let list = input.get("commands").unwrap_or(input);
    match list {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        other => one(other).into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bases() -> Vec<PathBuf> {
        vec![PathBuf::from("/tmp")]
    }

    fn created(parsed: &Parsed) -> Vec<String> {
        let mut paths: Vec<String> = parsed
            .created
            .keys()
            .map(|p| p.display().to_string())
            .collect();
        paths.sort();
        paths
    }

    fn write_lines(dir: &Path, name: &str, lines: &[&str]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, lines.join("\n")).expect("write");
        path
    }

    #[test]
    fn claude_transcripts_yield_their_title_and_what_their_tools_created() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = write_lines(
            tmp.path(),
            "s.jsonl",
            &[
                r#"{"type":"ai-title","aiTitle":"Profile the engine","sessionId":"s1"}"#,
                r#"{"type":"assistant","timestamp":"2026-11-01T10:00:00Z","cwd":"/work/proj","sessionId":"s1","message":{"content":[{"type":"tool_use","name":"Bash","input":{"command":"mkdir -p /tmp/prof && cargo build --target-dir /tmp/prof-t"}}]}}"#,
                r#"{"type":"assistant","timestamp":"2026-11-01T10:05:00Z","message":{"content":[{"type":"tool_use","name":"Write","input":{"file_path":"/tmp/notes/a.md"}}]}}"#,
            ],
        );
        let parsed = parse_claude(&path, &bases());
        assert_eq!(parsed.title.as_deref(), Some("Profile the engine"));
        assert_eq!(parsed.id.as_deref(), Some("s1"));
        assert_eq!(created(&parsed), ["/tmp/notes", "/tmp/prof", "/tmp/prof-t"]);
        assert_eq!(parsed.created[Path::new("/tmp/prof")], 1_793_527_200);
    }

    #[test]
    fn codex_rollouts_in_every_shape_name_what_they_created() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = write_lines(
            tmp.path(),
            "rollout-2026-11-11T09-15-02-x.jsonl",
            &[
                r#"{"timestamp":"2026-11-11T09:15:02.417Z","type":"session_meta","payload":{"id":"019a","cwd":"/work/acme","source":"cli"}}"#,
                r#"{"timestamp":"2026-11-11T09:15:03.000Z","type":"event_msg","payload":{"type":"user_message","message":"clean up the build cache\nplease"}}"#,
                r#"{"timestamp":"2026-11-11T09:15:04.000Z","type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo build --target-dir /tmp/acme-t\",\"workdir\":\"/work/acme\"}","call_id":"c1"}}"#,
                r#"{"timestamp":"2026-11-11T09:15:05.000Z","type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"git clone https://example.com/r.git /tmp/ref\"]}","call_id":"c2"}}"#,
                r#"{"timestamp":"2026-11-11T09:15:06.000Z","type":"response_item","payload":{"type":"local_shell_call","call_id":"c3","status":"completed","action":{"type":"exec","command":["mkdir","-p","/tmp/scratch-a"],"working_directory":"/work/acme"}}}"#,
                r#"{"timestamp":"2026-11-11T09:15:07.000Z","type":"response_item","payload":{"type":"custom_tool_call","status":"completed","call_id":"c4","name":"apply_patch","input":"*** Begin Patch\n*** Add File: /tmp/patched/x.rs\n+x\n*** End Patch"}}"#,
                r#"{"timestamp":"2026-11-11T09:15:08.000Z","ordinal":3,"type":"event_msg","payload":{"type":"item_completed","item":{"type":"FileChange","id":"c5","changes":{"/tmp/changed/y.rs":{"type":"add","content":""}}}}}"#,
                r#"{"timestamp":"2026-11-11T09:15:09.000Z","type":"event_msg","payload":{"type":"patch_apply_end","changes":{"/tmp/legacy/z.rs":{"add":{"content":""}}}}}"#,
            ],
        );
        let parsed = parse_codex(&path, &bases());
        assert_eq!(parsed.id.as_deref(), Some("019a"));
        assert_eq!(parsed.title.as_deref(), Some("clean up the build cache"));
        assert_eq!(
            created(&parsed),
            [
                "/tmp/acme-t",
                "/tmp/changed",
                "/tmp/legacy",
                "/tmp/patched",
                "/tmp/ref",
                "/tmp/scratch-a"
            ]
        );
    }

    #[test]
    fn gemini_chats_resolve_relative_paths_against_the_project_root() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = write_lines(
            tmp.path(),
            "session-2026-11-11T10-00-3f2a9c1e.jsonl",
            &[
                r#"{"sessionId":"3f2a","projectHash":"h","startTime":"2026-11-11T10:00:01.123Z","lastUpdated":"2026-11-11T10:00:01.123Z","kind":"main"}"#,
                r#"{"id":"b2","timestamp":"2026-11-11T10:00:07.000Z","type":"gemini","content":"","toolCalls":[{"id":"c1","name":"run_shell_command","args":{"command":"mkdir out && cp -r out /tmp/gem-copy","dir_path":"pkg"},"status":"success","timestamp":"2026-11-11T10:00:07.000Z"}]}"#,
                r#"{"id":"c3","timestamp":"2026-11-11T10:00:15.000Z","type":"gemini","content":"","toolCalls":[{"id":"c2","name":"write_file","args":{"file_path":"/tmp/gem-notes/report.md"},"status":"success","timestamp":"2026-11-11T10:00:16.000Z"}]}"#,
                r#"{"$set":{"summary":"Run the tests and save output"}}"#,
            ],
        );
        let parsed = parse_gemini(&path, Some(Path::new("/tmp/proj")), &bases());
        assert_eq!(parsed.id.as_deref(), Some("3f2a"));
        assert_eq!(
            parsed.title.as_deref(),
            Some("Run the tests and save output")
        );
        assert_eq!(
            created(&parsed),
            ["/tmp/gem-copy", "/tmp/gem-notes", "/tmp/proj"]
        );
    }

    #[test]
    fn qwen_chats_read_function_calls_and_custom_titles() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = write_lines(
            tmp.path(),
            "3f2a.jsonl",
            &[
                r#"{"uuid":"u1","sessionId":"3f2a","timestamp":"2026-11-11T10:00:00.000Z","type":"system","subtype":"custom_title","cwd":"/work","systemPayload":{"customTitle":"Bench the parser"}}"#,
                r#"{"uuid":"u2","sessionId":"3f2a","timestamp":"2026-11-11T10:00:07.000Z","type":"assistant","cwd":"/work","message":{"role":"model","parts":[{"functionCall":{"id":"c1","name":"run_shell_command","args":{"command":"mkdir -p /tmp/qwen-bench","directory":"/work/pkg"}}}]}}"#,
                r#"{"uuid":"u3","sessionId":"3f2a","timestamp":"2026-11-11T10:00:09.000Z","type":"assistant","cwd":"/work","message":{"role":"model","parts":[{"functionCall":{"id":"c2","name":"write_file","args":{"file_path":"/tmp/qwen-out/r.md"}}}]}}"#,
            ],
        );
        let parsed = parse_qwen(&path, &bases());
        assert_eq!(parsed.id.as_deref(), Some("3f2a"));
        assert_eq!(parsed.title.as_deref(), Some("Bench the parser"));
        assert_eq!(created(&parsed), ["/tmp/qwen-bench", "/tmp/qwen-out"]);
    }

    #[test]
    fn opencode_tool_entries_of_both_generations_are_attributed_to_their_session() {
        let rows: FxHashMap<String, OpencodeSession> = [(
            "ses_1".to_owned(),
            OpencodeSession {
                title: Some("Fix the build".to_owned()),
                directory: Some(PathBuf::from("/tmp/oc-project")),
                updated: 1_762_890_000,
            },
        )]
        .into_iter()
        .collect();
        let tools = vec![
            (
                "ses_1".to_owned(),
                serde_json::json!({"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"mkdir -p /tmp/oc-v1"},"time":{"start":1_762_890_000_000_u64}}}),
                0,
            ),
            (
                "ses_1".to_owned(),
                serde_json::json!({"type":"tool","name":"shell","state":{"status":"completed","input":{"command":"cp -r a /tmp/oc-v2","workdir":"sub"}},"time":{"created":1_762_890_001_000_u64}}),
                0,
            ),
            (
                "ses_1".to_owned(),
                serde_json::json!({"type":"tool","tool":"write","state":{"input":{"filePath":"/tmp/oc-write/a.txt"}}}),
                0,
            ),
            (
                "ses_1".to_owned(),
                serde_json::json!({"type":"tool","name":"patch","state":{"input":{"patchText":"*** Begin Patch\n*** Add File: /tmp/oc-patch/b.rs\n*** End Patch"}}}),
                0,
            ),
        ];
        let sessions = opencode_sessions(&rows, &tools, &bases());
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title.as_deref(), Some("Fix the build"));
        assert_eq!(sessions[0].agent, Agent::Opencode);
        let mut paths: Vec<_> = sessions[0]
            .created
            .iter()
            .map(|c| c.path.display().to_string())
            .collect();
        paths.sort();
        assert_eq!(
            paths,
            ["/tmp/oc-patch", "/tmp/oc-v1", "/tmp/oc-v2", "/tmp/oc-write"]
        );
    }

    #[test]
    fn cline_and_roo_ui_messages_record_commands_and_created_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("ui_messages.json");
        std::fs::write(
            &path,
            r#"[
              {"ts":1762890000123,"type":"say","say":"command","text":"npm run build -- --out-dir /tmp/cline-out"},
              {"ts":1762890000200,"type":"ask","ask":"command","text":"mkdir /tmp/cline-asked REQ_APP"},
              {"ts":1762890000300,"type":"ask","ask":"command","text":"mkdir /tmp/partial","partial":true},
              {"ts":1762890000456,"type":"say","say":"tool","text":"{\"tool\":\"newFileCreated\",\"path\":\"/tmp/cline-file/out.txt\"}"},
              {"ts":1762890000500,"type":"say","say":"tool","text":"{\"tool\":\"editedExistingFile\",\"path\":\"/tmp/edited/x\"}"}
            ]"#,
        )
        .expect("write");
        let parsed = parse_ui_messages(&path, Some(Path::new("/work")), &bases());
        assert_eq!(
            created(&parsed),
            ["/tmp/cline-asked", "/tmp/cline-file", "/tmp/cline-out"]
        );
    }

    #[test]
    fn cline_sessions_read_every_shape_of_run_commands() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path().join("sess1");
        std::fs::create_dir(&dir).expect("dir");
        std::fs::write(
            dir.join("sess1.json"),
            r#"{"cwd":"/work","prompt":"Build the docs site\nwith details","started_at":"2026-11-11T10:00:00Z"}"#,
        )
        .expect("manifest");
        std::fs::write(
            dir.join("sess1.messages.json"),
            r#"{"version":1,"messages":[{"role":"assistant","ts":1762890000789,"content":[
              {"type":"tool_use","id":"t1","name":"run_commands","input":{"commands":["mkdir -p /tmp/docs-a",{"command":"mkdir","args":["/tmp/docs-b"]}]}},
              {"type":"tool_use","id":"t2","name":"editor","input":{"path":"/tmp/docs-c/index.md","new_text":"x"}},
              {"type":"tool_use","id":"t3","name":"apply_patch","input":{"input":"*** Add File: /tmp/docs-d/p.md"}}]}]}"#,
        )
        .expect("messages");
        let parsed = parse_cline_session(&dir, &bases());
        assert_eq!(parsed.id.as_deref(), Some("sess1"));
        assert_eq!(parsed.title.as_deref(), Some("Build the docs site"));
        assert_eq!(
            created(&parsed),
            ["/tmp/docs-a", "/tmp/docs-b", "/tmp/docs-c", "/tmp/docs-d"]
        );
    }

    #[test]
    fn only_disposable_locations_are_kept_from_a_record() {
        let mut parsed = Parsed::default();
        let bases = bases();
        let mut recorder = Recorder {
            bases: &bases,
            parsed: &mut parsed,
        };
        recorder.wrote("/work/proj/src/main.rs", None, 1);
        recorder.wrote("/tmp/keep/a", None, 5);
        recorder.wrote("/tmp/keep/b", None, 3);
        assert_eq!(created(&parsed), ["/tmp/keep"]);
        assert_eq!(parsed.created[Path::new("/tmp/keep")], 3);
    }
}
