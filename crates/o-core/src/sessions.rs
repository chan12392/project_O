//! Claude Code sessions as runs, so a person using one Claude Code (no O plan)
//! sees its main thread and subagents in the office and on the boards.
//!
//! Read-only. Only transcript files are opened, and only these, under each
//! Claude config folder (`CLAUDE_CONFIG_DIR`, else `~/.claude`, plus the clean
//! profile O keeps):
//!
//! ```text
//! <config>/projects/<folder>/<session>.jsonl                          the main thread
//! <config>/projects/<folder>/<session>/subagents/agent-<id>.jsonl     a subagent's thread
//! <config>/projects/<folder>/<session>/subagents/agent-<id>.meta.json its type and description
//! ```
//!
//! Links are not followed and nothing else in the config folder is read.
//! Transcript text is data: it is cut short and shown, never acted on.
//!
//! Each session becomes a synthetic run (`session-<id>`) in the event shape O
//! writes: `run.start` with one card per subagent, `card.start/tool/message/end`,
//! the main thread's words as the lead's team messages, and `run.end` once the
//! session has been quiet for [`IDLE_MINUTES`]. Files are read incrementally:
//! a cache in O's state folder keeps each file's size, time, read offset and
//! what was folded from it, so a poll reads only what was appended.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration as StdDuration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agents::clean;
use crate::run::events::{CardInfo, CardStatus, OEvent};
use crate::run::index::RunQuery;
use crate::sys::js::parse_time;
use crate::run::plan::ProviderId;
use crate::run::store::{self, newest_first, user_home, RunRecord};

/// Synthetic run ids start with this; the rest is the session id.
pub const SESSION_PREFIX: &str = "session-";
/// `o sessions` and `o runs --sessions` look back this far by default.
pub const DEFAULT_SINCE_HOURS: f64 = 24.0;
/// A session with nothing new for this long has ended (its open subagents stopped).
pub const IDLE_MINUTES: i64 = 10;
/// At most this many sessions, the most recently active first.
pub const MAX_SESSIONS: usize = 30;

/// Each card keeps its newest steps only, the lead its newest lines: enough to see what it does.
const STEP_CAP: usize = 40;
const LEAD_CAP: usize = 20;
const TEXT_CAP: usize = 400;
const SUMMARY_CAP: usize = 160;
const RESULT_CAP: usize = 300;
const LABEL_CAP: usize = 60;
/// The start of a subagent's prompt, to match an older in-file sidechain to its call.
const PROMPT_HEAD: usize = 200;
const META_MAX_BYTES: u64 = 64 * 1024;
const CACHE_VERSION: u32 = 1;

/// The Claude config folders whose transcripts are read: `CLAUDE_CONFIG_DIR`
/// (else `~/.claude`), and the clean profile `o agents` keeps.
pub fn default_roots() -> Vec<PathBuf> {
    let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => user_home().join(".claude"),
    };
    vec![config, clean::profile_home(&clean::default_root(), ProviderId::Claude).join(".claude")]
}

/// Where the read offsets and folds are kept between polls.
pub fn cache_path(home: &Path) -> PathBuf {
    home.join("sessions-cache.json")
}

// ---------------------------------------------------------------------------
// Finding the files (the privacy boundary: nothing else is ever opened)

/// One session's transcript files.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionFiles {
    pub session_id: String,
    pub main: PathBuf,
    /// (agent id, its thread, its meta file when present)
    pub subagents: Vec<(String, PathBuf, Option<PathBuf>)>,
}

impl SessionFiles {
    /// Every file a read of this session opens.
    pub fn paths(&self) -> Vec<&Path> {
        let mut paths = vec![self.main.as_path()];
        for (_, thread, meta) in &self.subagents {
            paths.push(thread);
            paths.extend(meta.as_deref());
        }
        paths
    }
}

/// Session ids and agent ids name files; only plain names count, and they must fit a run id.
fn plain_id(text: &str) -> bool {
    !text.is_empty() && text.len() <= 56 && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// No reparse point of any kind (symlink, junction, mount point, …) on Windows.
#[cfg(windows)]
fn not_reparse(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
}

#[cfg(not(windows))]
fn not_reparse(_: &fs::Metadata) -> bool {
    true
}

/// A file entry that is a regular file and no link.
fn regular_file(path: &Path) -> Option<fs::Metadata> {
    fs::symlink_metadata(path).ok().filter(|m| m.file_type().is_file() && not_reparse(m))
}

/// A folder entry that is a real folder: no link, junction or other reparse point.
fn real_dir(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir() && not_reparse(&m))
}

fn modified(meta: &fs::Metadata) -> SystemTime {
    meta.modified().unwrap_or(UNIX_EPOCH)
}

fn names(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut names: Vec<String> = entries.flatten().filter_map(|e| e.file_name().to_str().map(str::to_string)).collect();
    names.sort();
    names
}

/// Sessions under `<root>/projects` written to at or after `since`.
///
/// The config root may itself be a link (it is the trusted place the user named), but
/// nothing below it is followed: `projects` and every folder and file under it must be a
/// real folder or file — no symlink, junction or other reparse point — and every file
/// must resolve inside the real `<root>/projects`.
pub fn transcript_files(root: &Path, since: SystemTime) -> Vec<SessionFiles> {
    let projects = root.join("projects");
    let Ok(real_projects) = fs::canonicalize(root).map(|r| r.join("projects")) else { return Vec::new() };
    if !real_dir(&projects) {
        return Vec::new();
    }
    let inside = |path: &Path| fs::canonicalize(path).is_ok_and(|p| p.starts_with(&real_projects));
    let mut found = Vec::new();
    for folder in names(&projects) {
        let folder = projects.join(folder);
        if !real_dir(&folder) {
            continue;
        }
        for name in names(&folder) {
            let Some(session_id) = name.strip_suffix(".jsonl").filter(|id| plain_id(id)) else { continue };
            let main = folder.join(&name);
            let Some(meta) = regular_file(&main).filter(|_| inside(&main)) else { continue };
            let mut latest = modified(&meta);
            let mut subagents = Vec::new();
            let dir = folder.join(session_id).join("subagents");
            if real_dir(&folder.join(session_id)) && real_dir(&dir) {
                for file in names(&dir) {
                    let Some(agent_id) = file.strip_prefix("agent-").and_then(|f| f.strip_suffix(".jsonl")).filter(|id| plain_id(id)) else { continue };
                    let thread = dir.join(&file);
                    let Some(thread_meta) = regular_file(&thread).filter(|_| inside(&thread)) else { continue };
                    latest = latest.max(modified(&thread_meta));
                    let meta_file = dir.join(format!("agent-{agent_id}.meta.json"));
                    let meta_file = regular_file(&meta_file).filter(|m| m.len() <= META_MAX_BYTES && inside(&meta_file)).map(|_| meta_file);
                    subagents.push((agent_id.to_string(), thread, meta_file));
                }
            }
            if latest >= since {
                found.push(SessionFiles { session_id: session_id.to_string(), main, subagents });
            }
        }
    }
    found
}

// ---------------------------------------------------------------------------
// Folding a transcript, line by line

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Step {
    at: String,
    /// A tool call's name; a message when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct End {
    at: String,
    status: String,
    result: String,
}

/// A Task/Agent tool call in the main thread: one subagent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Call {
    id: String,
    at: String,
    kind: String,
    description: String,
    prompt: String,
    agent_id: Option<String>,
    end: Option<End>,
}

/// A background subagent's completion, as Claude Code tells the main thread.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Notice {
    at: String,
    task_id: String,
    tool_use_id: String,
    status: String,
    summary: String,
}

/// Sidechain lines kept in the main file (older Claude Code), one per subagent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Chain {
    first_at: String,
    last_at: String,
    agent_id: Option<String>,
    prompt: String,
    steps: Vec<Step>,
}

/// What a transcript file says, folded as its lines arrive.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Fold {
    cwd: Option<String>,
    title: Option<String>,
    first_at: String,
    last_at: String,
    /// Main thread: what it said, newest last.
    lead: Vec<Step>,
    /// Subagent thread: what it did, newest last.
    steps: Vec<Step>,
    calls: Vec<Call>,
    notices: Vec<Notice>,
    chains: BTreeMap<String, Chain>,
    /// Older sidechains without an agent id: line uuid → chain key.
    chain_of: HashMap<String, String>,
}

fn push_capped(list: &mut Vec<Step>, step: Step, cap: usize) {
    list.push(step);
    if list.len() > cap {
        let extra = list.len() - cap;
        list.drain(..extra);
    }
}

/// The first `max` characters on one line, with "…" when cut.
fn head(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match flat.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", flat[..cut].trim_end()),
        None => flat,
    }
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// What a tool call was about, in a few words.
fn tool_summary(input: &Value) -> String {
    for key in ["description", "command", "file_path", "path", "pattern", "url", "query", "skill", "prompt"] {
        if let Some(text) = input.get(key).and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
            return head(text, SUMMARY_CAP);
        }
    }
    String::new()
}

fn is_agent_tool(name: &str) -> bool {
    name == "Task" || name == "Agent"
}

static NOTICE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<task-notification>(.*?)</task-notification>").unwrap());
static NOTICE_FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<(task-id|tool-use-id|status|summary)>(.*?)</").unwrap());

fn notices_in(text: &str, at: &str, out: &mut Vec<Notice>) {
    for block in NOTICE.captures_iter(text) {
        let mut notice = Notice { at: at.to_string(), ..Notice::default() };
        for field in NOTICE_FIELD.captures_iter(&block[1]) {
            let value = field[2].trim();
            match &field[1] {
                "task-id" => notice.task_id = value.to_string(),
                "tool-use-id" => notice.tool_use_id = value.to_string(),
                "status" => notice.status = value.to_string(),
                _ => notice.summary = head(value, RESULT_CAP),
            }
        }
        if notice.status.is_empty() || (notice.task_id.is_empty() && notice.tool_use_id.is_empty()) {
            continue;
        }
        // The same notice arrives as a queued command and again as the prompt it became.
        out.retain(|n| !(n.task_id == notice.task_id && n.tool_use_id == notice.tool_use_id));
        out.push(notice);
    }
}

/// Strings anywhere in a value that carry a notification (attachments nest them).
fn notice_strings<'a>(value: &'a Value, depth: usize, out: &mut Vec<&'a str>) {
    match value {
        Value::String(s) if s.contains("<task-notification>") => out.push(s),
        Value::Array(items) if depth > 0 => items.iter().for_each(|v| notice_strings(v, depth - 1, out)),
        Value::Object(map) if depth > 0 => map.values().for_each(|v| notice_strings(v, depth - 1, out)),
        _ => {}
    }
}

/// Assistant content as steps: tool calls and what it wrote.
fn steps_of(content: &Value, at: &str) -> Vec<Step> {
    let Some(blocks) = content.as_array() else { return Vec::new() };
    blocks
        .iter()
        .filter_map(|block| match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                let name = block.get("name").and_then(Value::as_str).unwrap_or("tool");
                Some(Step { at: at.into(), tool: Some(name.into()), text: tool_summary(block.get("input").unwrap_or(&Value::Null)) })
            }
            Some("text") => {
                let text = head(block.get("text").and_then(Value::as_str).unwrap_or_default(), TEXT_CAP);
                (!text.is_empty()).then(|| Step { at: at.into(), tool: None, text })
            }
            _ => None,
        })
        .collect()
}

impl Fold {
    fn seen(&mut self, at: &str) {
        if at.is_empty() {
            return;
        }
        if self.first_at.is_empty() || at < self.first_at.as_str() {
            self.first_at = at.to_string();
        }
        if at > self.last_at.as_str() {
            self.last_at = at.to_string();
        }
    }

    fn line(&mut self, v: &Value, main: bool) {
        let kind = v.get("type").and_then(Value::as_str).unwrap_or_default();
        let at = v.get("timestamp").and_then(Value::as_str).unwrap_or_default();
        match kind {
            "ai-title" => {
                if let Some(title) = v.get("aiTitle").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                    self.title = Some(head(title, 120));
                }
                return;
            }
            "summary" => {
                if self.title.is_none() {
                    self.title = v.get("summary").and_then(Value::as_str).map(|t| head(t, 120));
                }
                return;
            }
            "attachment" => {
                if main {
                    let mut texts = Vec::new();
                    notice_strings(v.get("attachment").unwrap_or(&Value::Null), 6, &mut texts);
                    for text in texts {
                        notices_in(text, at, &mut self.notices);
                    }
                }
                return;
            }
            "user" | "assistant" => {}
            _ => return,
        }
        let content = v.get("message").and_then(|m| m.get("content")).unwrap_or(&Value::Null);
        if main && v.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            self.sidechain(v, kind, at, content);
            return;
        }
        self.seen(at);
        if main && self.cwd.is_none() {
            self.cwd = v.get("cwd").and_then(Value::as_str).map(str::to_string);
        }
        if kind == "assistant" {
            for step in steps_of(content, at) {
                match (&step.tool, main) {
                    (None, true) => push_capped(&mut self.lead, step, LEAD_CAP),
                    (_, false) => push_capped(&mut self.steps, step, STEP_CAP),
                    _ => {}
                }
            }
            if main {
                self.calls_in(content, at);
            }
            return;
        }
        // A user line: tool results and, in the main thread, background notices.
        if !main {
            return;
        }
        let blocks = content.as_array().map(Vec::as_slice).unwrap_or_default();
        for block in blocks.iter().filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result")) {
            let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or_default();
            let Some(call) = self.calls.iter_mut().find(|c| c.id == id) else { continue };
            let detail = v.get("toolUseResult").filter(|r| r.is_object());
            if let Some(agent) = detail.and_then(|r| r.get("agentId")).and_then(Value::as_str) {
                call.agent_id = Some(agent.to_string());
            }
            // A background launch reports only that it started; its end comes as a notice.
            if detail.and_then(|r| r.get("status")).and_then(Value::as_str) == Some("async_launched") {
                continue;
            }
            let failed = block.get("is_error").and_then(Value::as_bool) == Some(true);
            let result = head(&text_of(block.get("content").unwrap_or(&Value::Null)), RESULT_CAP);
            call.end = Some(End { at: at.into(), status: if failed { "failed" } else { "done" }.into(), result });
        }
        let text = text_of(content);
        if text.contains("<task-notification>") {
            notices_in(&text, at, &mut self.notices);
        }
    }

    fn calls_in(&mut self, content: &Value, at: &str) {
        let Some(blocks) = content.as_array() else { return };
        for block in blocks {
            let name = block.get("name").and_then(Value::as_str).unwrap_or_default();
            if block.get("type").and_then(Value::as_str) != Some("tool_use") || !is_agent_tool(name) {
                continue;
            }
            let input = block.get("input").unwrap_or(&Value::Null);
            let field = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or_default();
            self.calls.push(Call {
                id: block.get("id").and_then(Value::as_str).unwrap_or_default().into(),
                at: at.into(),
                kind: field("subagent_type").into(),
                description: head(field("description"), LABEL_CAP),
                prompt: head(field("prompt"), PROMPT_HEAD),
                // A call that resumes a subagent names it before its result does.
                agent_id: Some(field("resume")).filter(|id| plain_id(id)).map(str::to_string),
                end: None,
            });
        }
    }

    fn sidechain(&mut self, v: &Value, kind: &str, at: &str, content: &Value) {
        let text = |key: &str| v.get(key).and_then(Value::as_str);
        let agent = text("agentId");
        let key = agent
            .map(str::to_string)
            .or_else(|| text("parentUuid").and_then(|p| self.chain_of.get(p).cloned()))
            .or_else(|| text("uuid").map(str::to_string))
            .unwrap_or_else(|| "sidechain".into());
        if let (None, Some(uuid)) = (agent, text("uuid")) {
            self.chain_of.insert(uuid.into(), key.clone());
        }
        let chain = self.chains.entry(key).or_default();
        if agent.is_some() {
            chain.agent_id = agent.map(str::to_string);
        }
        if !at.is_empty() {
            if chain.first_at.is_empty() {
                chain.first_at = at.into();
            }
            chain.last_at = chain.last_at.clone().max(at.into());
        }
        if kind == "user" && chain.prompt.is_empty() {
            if let Some(prompt) = content.as_str() {
                chain.prompt = head(prompt, PROMPT_HEAD);
            }
        }
        if kind == "assistant" {
            for step in steps_of(content, at) {
                push_capped(&mut chain.steps, step, STEP_CAP);
            }
        }
    }
}

/// Lines worth parsing. Most of a transcript is request logs and attachments,
/// some many megabytes long; they are passed over without parsing.
fn wanted(line: &[u8]) -> bool {
    fn has(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }
    const NOTICE_TAG: &[u8] = b"<task-notification>";
    if let Some(rest) = line.strip_prefix(br#"{"type":""#) {
        let tag = &rest[..rest.iter().position(|b| *b == b'"').unwrap_or(0)];
        return match tag {
            b"attachment" => has(line, NOTICE_TAG),
            b"user" | b"assistant" | b"ai-title" | b"summary" => true,
            _ => false,
        };
    }
    [&br#""type":"user""#[..], br#""type":"assistant""#, br#""type":"ai-title""#, br#""type":"summary""#].iter().any(|tag| has(line, tag))
        || (has(line, br#""type":"attachment""#) && has(line, NOTICE_TAG))
}

// ---------------------------------------------------------------------------
// The incremental cache

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cached {
    size: u64,
    mtime: i64,
    offset: u64,
    fold: Fold,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Cache {
    version: u32,
    files: HashMap<String, Cached>,
    #[serde(skip)]
    changed: bool,
}

fn load_cache(path: &Path) -> Cache {
    let cache: Cache = fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if cache.version == CACHE_VERSION {
        cache
    } else {
        Cache { version: CACHE_VERSION, changed: true, ..Cache::default() }
    }
}

/// Written whole to a temporary file, then moved over the old one.
fn save_cache(path: &Path, cache: &Cache) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temp, serde_json::to_vec(cache).map_err(io::Error::other)?)?;
    fs::rename(&temp, path)
}

fn millis(time: SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// A file's fold, reading only the whole lines appended since the cached offset.
/// A file that no longer continues what was read (cut short, rewritten) is read again.
fn fold_file(cache: &mut Cache, path: &Path, main: bool) -> Option<Fold> {
    let meta = regular_file(path)?;
    let (size, mtime) = (meta.len(), millis(modified(&meta)));
    let entry = cache.files.entry(path.to_string_lossy().into_owned()).or_default();
    if entry.size == size && entry.mtime == mtime && entry.offset <= size {
        return Some(entry.fold.clone());
    }
    let mut file = File::open(path).ok()?;
    let mut last = [0u8; 1];
    let continues = entry.offset > 0
        && size >= entry.offset
        && file.seek(SeekFrom::Start(entry.offset - 1)).is_ok()
        && file.read_exact(&mut last).is_ok()
        && last[0] == b'\n';
    if !continues {
        *entry = Cached::default();
    }
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(entry.offset)).ok()?;
    file.read_to_end(&mut bytes).ok()?;
    // Whole lines only: a line Claude Code is still writing waits for the next read.
    let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    for line in bytes[..whole].split(|b| *b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if !wanted(line) {
            continue;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(line) {
            entry.fold.line(&value, main);
        }
    }
    entry.offset += whole as u64;
    entry.size = size;
    entry.mtime = mtime;
    cache.changed = true;
    Some(entry.fold.clone())
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Meta {
    agent_type: Option<String>,
    description: Option<String>,
    tool_use_id: Option<String>,
    finished: Option<bool>,
    model: Option<String>,
}

fn read_meta(path: Option<&Path>) -> Meta {
    path.and_then(|p| regular_file(p).filter(|m| m.len() <= META_MAX_BYTES).and_then(|_| fs::read(p).ok()))
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// From folds to events

struct Subagent {
    agent_id: String,
    fold: Fold,
    meta: Meta,
}

struct Session {
    id: String,
    main: Fold,
    subagents: Vec<Subagent>,
}

/// One subagent as a card, before it becomes events.
struct Card {
    id: String,
    label: String,
    role: Option<String>,
    model: Option<String>,
    start: String,
    last: String,
    steps: Vec<Step>,
    end: Option<End>,
    /// Claude Code marked its thread finished.
    finished: bool,
    /// The tool-use id and time of the latest call that started or resumed it.
    call: Option<(String, String)>,
}

/// `C:\work\app` → `C:/work/app`; a worktree session belongs to its repository.
fn project_of(cwd: &str) -> String {
    let path = cwd.replace('\\', "/");
    let path = path.trim_end_matches('/');
    match path.find("/.claude/worktrees/") {
        Some(cut) => path[..cut].to_string(),
        None => path.to_string(),
    }
}

fn non_empty(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn end_status(status: &str) -> CardStatus {
    match status {
        "failed" | "error" => CardStatus::Failed,
        "killed" | "stopped" | "cancelled" => CardStatus::Stopped,
        _ => CardStatus::Done,
    }
}

impl Session {
    fn cards(&self) -> Vec<Card> {
        let main = &self.main;
        let mut used: HashSet<&str> = HashSet::new();
        let mut cards: Vec<Card> = Vec::new();
        // Every call of one subagent, oldest first: the call that started it (by tool-use id, agentId or
        // prompt) and the calls that resumed it (their results report the same agentId).
        let calls_for = |agent: Option<&str>, tool_use: Option<&str>, prompt: &str| -> Vec<&Call> {
            let first = main.calls.iter().find(|c| {
                tool_use.is_some_and(|t| t == c.id)
                    || (agent.is_some() && c.agent_id.as_deref() == agent)
                    || (!prompt.is_empty() && c.prompt == prompt)
            });
            let agent = agent.or_else(|| first.and_then(|c| c.agent_id.as_deref()));
            main.calls
                .iter()
                .filter(|c| first.is_some_and(|f| std::ptr::eq(f, *c)) || (agent.is_some() && c.agent_id.as_deref() == agent))
                .collect()
        };
        // The first call names the card; the latest one says whether it is running or how it ended.
        let card = |id: String, calls: &[&Call], fold_first: &str, fold_last: &str, steps: Vec<Step>, kind: Option<String>, description: Option<String>, model: Option<String>, finished: bool| {
            let (first, latest) = (calls.first().copied(), calls.last().copied());
            let role = kind.or_else(|| first.and_then(|c| non_empty(&c.kind)));
            let label = description.or_else(|| first.and_then(|c| non_empty(&c.description))).or_else(|| role.clone()).unwrap_or_else(|| "서브에이전트".into());
            let start = non_empty(fold_first).or_else(|| first.map(|c| c.at.clone())).unwrap_or_default();
            let call_times = calls.iter().flat_map(|c| [c.at.as_str(), c.end.as_ref().map_or("", |e| e.at.as_str())]);
            let last = [fold_last, &start].into_iter().chain(call_times).max().unwrap_or_default().to_string();
            let end = latest.and_then(|c| c.end.clone());
            // A thread marked finished and then resumed is open again until the resuming call ends.
            let finished = finished && (calls.len() < 2 || end.is_some());
            Card { id, label: head(&label, LABEL_CAP), role, model, start, last, steps, end, finished, call: latest.map(|c| (c.id.clone(), c.at.clone())) }
        };
        for sub in &self.subagents {
            let calls = calls_for(Some(&sub.agent_id), sub.meta.tool_use_id.as_deref(), "");
            used.extend(calls.iter().map(|c| c.id.as_str()));
            let meta = &sub.meta;
            let description = meta.description.as_deref().and_then(non_empty);
            let kind = meta.agent_type.as_deref().and_then(non_empty);
            cards.push(card(sub.agent_id.clone(), &calls, &sub.fold.first_at, &sub.fold.last_at, sub.fold.steps.clone(), kind, description, meta.model.clone(), meta.finished == Some(true)));
        }
        for (i, (key, chain)) in main.chains.iter().enumerate() {
            let calls = calls_for(chain.agent_id.as_deref(), None, &chain.prompt);
            used.extend(calls.iter().map(|c| c.id.as_str()));
            let id = chain.agent_id.clone().filter(|a| plain_id(a)).unwrap_or_else(|| if plain_id(key) { key.clone() } else { format!("chain{}", i + 1) });
            cards.push(card(id, &calls, &chain.first_at, &chain.last_at, chain.steps.clone(), None, None, None, false));
        }
        // Calls whose thread is not on disk (yet) still put a person in the office: one per agent.
        for call in &main.calls {
            if used.contains(call.id.as_str()) {
                continue;
            }
            let calls: Vec<&Call> = match call.agent_id.as_deref() {
                Some(agent) => main.calls.iter().filter(|c| c.agent_id.as_deref() == Some(agent) && !used.contains(c.id.as_str())).collect(),
                None => vec![call],
            };
            used.extend(calls.iter().map(|c| c.id.as_str()));
            let id = call.agent_id.clone().or_else(|| non_empty(&call.id)).filter(|i| plain_id(i)).unwrap_or_else(|| format!("call{}", cards.len() + 1));
            if cards.iter().any(|c| c.id == id) {
                continue;
            }
            cards.push(card(id, &calls, "", "", Vec::new(), None, None, None, false));
        }
        cards.sort_by(|a, b| a.start.cmp(&b.start));
        cards
    }

    /// The session's events in the order O writes them, or None when it has said nothing yet.
    fn events(&self, now: DateTime<Utc>, idle: Duration) -> Option<Vec<OEvent>> {
        let mut cards = self.cards();
        let first = std::iter::once(self.main.first_at.as_str()).chain(cards.iter().map(|c| c.start.as_str())).filter(|a| !a.is_empty()).min()?.to_string();
        let last = std::iter::once(self.main.last_at.as_str()).chain(cards.iter().map(|c| c.last.as_str())).max().unwrap_or_default().to_string();
        let quiet = parse_time(&last).is_none_or(|t| now - t > idle);
        let run_id = format!("{SESSION_PREFIX}{}", self.id);

        // How each subagent ended: its result, else a background notice, else its own mark; open ones stop with the session.
        for card in &mut cards {
            if card.end.is_some() {
                continue;
            }
            // Only a notice about the latest call: one from before a resume is about the earlier run.
            let (call_id, call_at) = card.call.as_ref().map_or(("", ""), |(id, at)| (id.as_str(), at.as_str()));
            let notice = self.main.notices.iter().rev().find(|n| {
                (n.at.is_empty() || n.at.as_str() >= call_at) && (n.task_id == card.id || (!n.tool_use_id.is_empty() && n.tool_use_id == call_id))
            });
            card.end = match notice {
                Some(n) => Some(End { at: n.at.clone(), status: end_status(&n.status).as_str().into(), result: n.summary.clone() }),
                None if card.finished => Some(End { at: card.last.clone(), status: "done".into(), result: String::new() }),
                None if quiet => Some(End { at: card.last.clone(), status: "stopped".into(), result: "세션이 멈춰 끝나지 않았습니다".into() }),
                None => None,
            };
        }

        let infos = cards
            .iter()
            .map(|c| CardInfo { id: c.id.clone(), provider: ProviderId::Claude, label: Some(c.label.clone()), role: c.role.clone(), model: c.model.clone(), effort: None })
            .collect();
        let title = self.main.title.clone().unwrap_or_else(|| format!("Claude Code 세션 {}", self.id.chars().take(8).collect::<String>()));
        let project = self.main.cwd.as_deref().map(project_of).unwrap_or_else(|| "(폴더 모름)".into());
        let mut body: Vec<OEvent> = Vec::new();
        for line in &self.main.lead {
            body.push(OEvent::Msg { at: line.at.clone(), from: "lead".into(), to: "all".into(), text: line.text.clone(), id: None, kind: None, reply_to: None });
        }
        let mut failed = false;
        for card in &cards {
            body.push(OEvent::CardStart { at: card.start.clone(), card_id: card.id.clone(), pid: None });
            for step in &card.steps {
                body.push(match &step.tool {
                    Some(name) => OEvent::CardTool { at: step.at.clone(), card_id: card.id.clone(), name: name.clone(), summary: step.text.clone() },
                    None => OEvent::CardMessage { at: step.at.clone(), card_id: card.id.clone(), text: step.text.clone() },
                });
            }
            if let Some(end) = &card.end {
                let status = end_status(&end.status);
                failed |= status == CardStatus::Failed;
                let result = non_empty(&end.result);
                body.push(OEvent::CardEnd { at: end.at.clone().max(card.start.clone()), card_id: card.id.clone(), status, result, exit_code: None });
            }
        }
        body.sort_by(|a, b| at_of(a).cmp(at_of(b)));
        let mut events = vec![OEvent::RunStart { at: first, run_id: run_id.clone(), title, project, cards: infos }];
        events.extend(body);
        if quiet {
            events.push(OEvent::RunEnd { at: last, run_id, status: if failed { CardStatus::Failed } else { CardStatus::Done } });
        }
        Some(events)
    }
}

fn at_of(event: &OEvent) -> &str {
    match event {
        OEvent::RunStart { at, .. }
        | OEvent::RunBranch { at, .. }
        | OEvent::RunFollowup { at, .. }
        | OEvent::CardQueued { at, .. }
        | OEvent::CardStart { at, .. }
        | OEvent::CardSession { at, .. }
        | OEvent::CardInput { at, .. }
        | OEvent::CardMessage { at, .. }
        | OEvent::CardTool { at, .. }
        | OEvent::CardToolError { at, .. }
        | OEvent::CardLimit { at, .. }
        | OEvent::CardEnd { at, .. }
        | OEvent::Msg { at, .. }
        | OEvent::MsgState { at, .. }
        | OEvent::CardUsage { at, .. }
        | OEvent::Quota { at, .. }
        | OEvent::RunEnd { at, .. }
        | OEvent::ReviewPosted { at, .. }
        | OEvent::CardBudget { at, .. }
        | OEvent::CardGuides { at, .. }
        | OEvent::RunContext { at, .. }
        | OEvent::RunBlocked { at, .. }
        | OEvent::RunMode { at, .. } => at,
    }
}

// ---------------------------------------------------------------------------
// Reading

/// Sessions active since `now - since`, the most recently active first, as runs.
/// `cache` keeps read offsets between calls (None reads every file whole).
pub fn read_sessions(roots: &[PathBuf], cache: Option<&Path>, now: DateTime<Utc>, since: Duration) -> Vec<RunRecord> {
    let mut store = cache.map(load_cache).unwrap_or_else(|| Cache { version: CACHE_VERSION, ..Cache::default() });
    let cutoff = UNIX_EPOCH + StdDuration::from_millis((now - since).timestamp_millis().max(0) as u64);
    let mut seen: HashSet<String> = HashSet::new();
    let mut roots_seen: HashSet<PathBuf> = HashSet::new();
    let mut sessions: Vec<(String, Session)> = Vec::new();
    for root in roots {
        let key = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if !roots_seen.insert(key) {
            continue;
        }
        for files in transcript_files(root, cutoff) {
            seen.extend(files.paths().iter().map(|p| p.to_string_lossy().into_owned()));
            let Some(main) = fold_file(&mut store, &files.main, true) else { continue };
            let subagents = files
                .subagents
                .iter()
                .filter_map(|(agent_id, thread, meta)| Some(Subagent { agent_id: agent_id.clone(), fold: fold_file(&mut store, thread, false)?, meta: read_meta(meta.as_deref()) }))
                .collect();
            let session = Session { id: files.session_id, main, subagents };
            let active = std::iter::once(session.main.last_at.clone()).chain(session.subagents.iter().map(|s| s.fold.last_at.clone())).max().unwrap_or_default();
            sessions.push((active, session));
        }
    }
    let before = store.files.len();
    store.files.retain(|path, _| seen.contains(path));
    if let Some(path) = cache.filter(|_| store.changed || store.files.len() != before) {
        // A cache that cannot be written only means reading more next time.
        let _ = save_cache(path, &store);
    }
    sessions.sort_by(|a, b| b.0.cmp(&a.0));
    let idle = Duration::minutes(IDLE_MINUTES);
    sessions
        .iter()
        .filter_map(|(_, s)| {
            let events = s.events(now, idle)?;
            Some(RunRecord { run_id: format!("{SESSION_PREFIX}{}", s.id), events: events.iter().filter_map(|e| serde_json::to_value(e).ok()).collect() })
        })
        .take(MAX_SESSIONS)
        .collect()
}

/// `--since <hours>` as a duration.
pub fn hours(since: f64) -> Duration {
    Duration::milliseconds((since * 3_600_000.0) as i64)
}

/// O runs and sessions together, newest start first. A session that is one of
/// O's own cards (its `card.session`) is that card already and is left out.
pub fn with_sessions(runs: Vec<RunRecord>, sessions: Vec<RunRecord>) -> Vec<RunRecord> {
    let cards: HashSet<String> = runs
        .iter()
        .flat_map(|r| r.events.iter())
        .filter(|e| e.get("t").and_then(Value::as_str) == Some("card.session"))
        .filter_map(|e| e.get("sessionId").and_then(Value::as_str))
        .map(|id| format!("{SESSION_PREFIX}{id}"))
        .collect();
    let mut all = runs;
    all.extend(sessions.into_iter().filter(|s| !cards.contains(&s.run_id)));
    newest_first(all)
}

/// [`with_sessions`] under the same `--project` and `--limit` as the O runs: sessions of
/// other projects are left out, and the merged list is cut to the limit.
pub fn with_sessions_for(runs: Vec<RunRecord>, sessions: Vec<RunRecord>, query: &RunQuery) -> Vec<RunRecord> {
    let sessions = match query.project {
        Some(project) => sessions
            .into_iter()
            .filter(|s| s.events.first().and_then(|e| e.get("project")).and_then(Value::as_str).is_some_and(|p| crate::projects::same(p, project)))
            .collect(),
        None => sessions,
    };
    let mut all = with_sessions(runs, sessions);
    all.truncate(query.limit);
    all
}

/// What `o runs --json --sessions` and the app read: O's runs, then this PC's recent sessions.
pub fn read_runs_with_sessions(home: &Path, query: &RunQuery, since: Duration) -> io::Result<Vec<RunRecord>> {
    let runs = store::read_runs(home, query)?;
    let sessions = read_sessions(&default_roots(), Some(&cache_path(home)), Utc::now(), since);
    // The lead chat's own Claude sessions are O's lead, not someone's separate work (ADR-0008).
    let leads: HashSet<String> = crate::lead::session_ids(home).into_iter().map(|id| format!("{SESSION_PREFIX}{id}")).collect();
    let sessions = sessions.into_iter().filter(|s| !leads.contains(&s.run_id)).collect();
    Ok(with_sessions_for(runs, sessions, query))
}
