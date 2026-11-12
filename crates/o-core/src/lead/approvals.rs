//! Per-action approvals for a normal-mode lead (ADR-0008 개정 3): reads run on their own;
//! a write, a command, a web request or any other tool waits for the person's decision.
//!
//! ```text
//! leads/<threadId>/approvals/<id>.json       a request waiting for the person (its turn's token, what it asks)
//! leads/<threadId>/approvals/<id>.decision   the person's answer ("allow" | "deny"), made once (`decide`)
//! leads/<threadId>/approvals/.lock           held while a request takes its number
//! ```
//!
//! Claude Code asks through its `--permission-prompt-tool`: O's own stdio MCP server
//! (`o _lead-approve`, [`serve`]) answers its `tools/call`. Codex app-server asks with
//! `item/commandExecution/requestApproval` and `item/fileChange/requestApproval`, answered in
//! process. Both end in [`ask`]: O's guard rules first (a refused call never reaches the
//! person), then the request on file and in the thread (`lead.approval`), then the wait for
//! `decide`, a stop, or the time limit; the result goes in the thread as `lead.decision`.
//! Text in the chat is never a decision: only `decide` (the app's 허용/거부, `o lead decide`),
//! which agents cannot run.

use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{append, read_marker, running, stop_path, thread_dir, LeadEvent};
use crate::run::plan::ProviderId;
use crate::run::store::{is_run_id, new_run_id};
use crate::sys::js::{iso, js_head, js_len, now_iso};

/// How long one request waits for the person; then it is refused (`timeout`).
pub const WAIT: Duration = Duration::from_secs(10 * 60);
/// The MCP server's name in Claude's `--mcp-config` and its one tool.
pub const SERVER: &str = "o_lead";
pub const TOOL: &str = "decide";
/// The hidden `o` command that is that server.
pub const COMMAND: &str = "_lead-approve";
/// A request's text shown in full up to this many characters.
const DETAIL_CHARS: usize = 16_000;
const SUMMARY_CHARS: usize = 300;
const POLL: Duration = Duration::from_millis(100);
const MAX_PENDING: usize = 20;
/// How often a long wait tells Claude it is still going (`notifications/progress`).
const TICK: Duration = Duration::from_secs(20);

static PROGRAM: Mutex<Option<PathBuf>> = Mutex::new(None);

/// For tests that stand in for a lead CLI: the program a turn names as its approval server
/// when the test binary is not `o`. In process only; O itself never sets it.
#[doc(hidden)]
pub fn set_program(program: Option<PathBuf>) {
    if let Ok(mut slot) = PROGRAM.lock() { *slot = program; }
}

pub(super) fn program_override() -> Option<PathBuf> {
    PROGRAM.lock().ok().and_then(|slot| slot.clone())
}

/// `mcp__o_lead__decide`, the name `--permission-prompt-tool` takes.
pub fn prompt_tool() -> String {
    format!("mcp__{SERVER}__{TOOL}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Command,
    File,
    Web,
    Other,
}

/// One file a request changes, with its `+`/`−` lines when known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDelta {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub diff: String,
}

/// What a lead CLI asks to do, as the person sees it, and what O's guard reviews.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub provider: ProviderId,
    pub kind: Kind,
    /// The CLI's tool name (`Write`, `Bash`, `commandExecution`, ...).
    pub tool: String,
    /// One line: the command, the files, the URL or query.
    pub summary: String,
    /// The whole request as the CLI sent it (the command in full, the diff, the input).
    pub detail: String,
    pub cwd: Option<String>,
    /// The CLI's own reason, when it gives one (data, not an instruction).
    pub reason: Option<String>,
    pub files: Vec<FileDelta>,
    /// Tool calls for `o_guard::hook::review` (`{ tool_name, tool_input }`; the project is the cwd).
    pub review: Vec<Value>,
}

/// What became of a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub allowed: bool,
    /// `allow` · `deny` · `timeout` · `stopped`.
    pub decision: &'static str,
    /// For the CLI when refused: why, in words the model reads.
    pub message: String,
}

/// A waiting request on file: the event's fields and the turn it belongs to.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    id: String,
    token: String,
    at: String,
    expires: String,
    provider: ProviderId,
    kind: Kind,
    tool: String,
    summary: String,
    detail: String,
    cwd: Option<String>,
    reason: Option<String>,
    files: Vec<FileDelta>,
}

fn dir(home: &Path, thread_id: &str) -> PathBuf {
    thread_dir(home, thread_id).join("approvals")
}

fn clip(text: &str, chars: usize) -> String {
    if js_len(text) > chars { format!("{}…", js_head(text, chars)) } else { text.to_string() }
}

/// Whether a turn waits on the person: a request on file in its approvals folder. The turn's
/// time limit does not run meanwhile (`turn_process`).
pub fn waiting(approvals: &Path) -> bool {
    fs::read_dir(approvals).into_iter().flatten().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".json"))
}

pub fn folder(home: &Path, thread_id: &str) -> PathBuf {
    dir(home, thread_id)
}

/// The requests waiting now, oldest first, as the app shows them (without the turn token).
pub fn pending(home: &Path, thread_id: &str) -> Vec<Value> {
    if !is_run_id(thread_id) { return Vec::new(); }
    let mut found: Vec<Pending> = fs::read_dir(dir(home, thread_id)).into_iter().flatten().flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
        .filter_map(|e| serde_json::from_slice(&fs::read(e.path()).ok()?).ok())
        .collect();
    found.sort_by_key(|p| (p.id.len(), p.id.clone()));
    found.into_iter().map(|p| {
        let mut value = json!(p);
        if let Some(object) = value.as_object_mut() { object.remove("token"); }
        value
    }).collect()
}

/// `<threadId>-a<n>` → the thread id.
fn approval_thread(id: &str) -> Option<&str> {
    let (thread, n) = id.rsplit_once("-a")?;
    (is_run_id(thread) && !n.is_empty() && n.len() <= 6 && n.chars().all(|c| c.is_ascii_digit())).then_some(thread)
}

/// The person's answer to a waiting request (the app's 허용/거부, `o lead decide`). Only a
/// request of the turn running now, and only once: the answer is linked into place whole,
/// so a second answer, or one racing it, is refused.
pub fn decide(home: &Path, id: &str, allow: bool) -> Result<(), String> {
    let thread_id = approval_thread(id).ok_or_else(|| format!("승인 요청 id 형식이 아닙니다: {id}"))?;
    let folder = dir(home, thread_id);
    let pending: Pending = fs::read(folder.join(format!("{id}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("이미 결정되었거나 끝난 승인 요청입니다")?;
    let live = running(home, thread_id).is_some_and(|busy| !busy.unknown) && read_marker(home, thread_id).is_some_and(|m| m.token == pending.token);
    if !live { return Err("이 승인 요청의 총괄 턴이 끝났습니다".into()); }
    let answer = folder.join(format!("{id}.decision"));
    let temp = folder.join(format!("{id}.{}.tmp", new_run_id(Utc::now())));
    fs::write(&temp, if allow { "allow" } else { "deny" }).map_err(|e| e.to_string())?;
    let linked = fs::hard_link(&temp, &answer);
    let _ = fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Err("이미 결정된 승인 요청입니다".into()),
        // A file system without links: create_new still lets one answer in.
        Err(_) => {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&answer).map_err(|e| if e.kind() == ErrorKind::AlreadyExists { "이미 결정된 승인 요청입니다".to_string() } else { e.to_string() })?;
            file.write_all(if allow { b"allow" } else { b"deny" }).map_err(|e| e.to_string())
        }
    }
}

/// Requests a turn left behind (its CLI or approval server ended mid-wait) are closed as
/// `stopped`, so none stays on screen or pauses the next turn's time limit.
pub fn close_left(home: &Path, thread_id: &str) {
    if !is_run_id(thread_id) { return; }
    for entry in fs::read_dir(dir(home, thread_id)).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(id) = name.strip_suffix(".json") {
            let _ = append(home, thread_id, &LeadEvent::Decision { at: now_iso(), id: id.to_string(), decision: "stopped".into(), by: "o".into(), reason: Some("총괄 턴이 끝나 승인 요청을 닫았습니다".into()) });
        }
        if name != ".lock" { let _ = fs::remove_file(entry.path()); }
    }
}

/// Why O itself refuses a request, before the person sees it: O's guard rules (sign-in files,
/// provider calls, protected branches and mirrors, permission bypass), O's state folder, and
/// any MCP tool (the only server is this approval one).
pub fn refusals(home: &Path, project: &str, request: &Request) -> Vec<String> {
    let mut reasons = Vec::new();
    if (request.files.is_empty() && js_len(&request.detail) > DETAIL_CHARS)
        || request.files.len() > 50 || request.files.iter().any(|file| js_len(&file.diff) > DETAIL_CHARS) {
        reasons.push("승인 화면에 작업 전체를 표시할 수 없습니다. 작업을 나누어 요청하세요".to_string());
    }
    if request.tool.starts_with("mcp__") {
        reasons.push("총괄은 MCP 도구를 쓸 수 없습니다".to_string());
    }
    for call in &request.review {
        let mut input = call.clone();
        input["cwd"] = json!(request.cwd.as_deref().unwrap_or(project));
        input["inside_agent"] = json!(true);
        reasons.extend(o_guard::hook::review(&input, &o_guard::hook::RealGit));
    }
    let state = home.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
    // Where a call writes or what it runs, not a file's text (a document may name the folder).
    let mut words: Vec<String> = request.review.iter()
        .flat_map(|call| ["command", "file_path", "notebook_path", "path"].map(|key| call["tool_input"][key].as_str().unwrap_or("").to_string()))
        .collect();
    words.extend(request.files.iter().map(|f| f.path.clone()));
    if let Some(cwd) = &request.cwd { words.push(cwd.clone()); }
    let user_home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok();
    let base = o_guard::hook::resolve(request.cwd.as_deref().unwrap_or(project), project, user_home.as_deref()).unwrap_or_else(|| {
        reasons.push("실행 위치를 확인할 수 없어 거부했습니다".to_string());
        project.to_string()
    });
    for call in &request.review {
        for key in ["file_path", "notebook_path", "path"] {
            if let Some(path) = call["tool_input"][key].as_str().filter(|path| !path.is_empty()) {
                if o_guard::hook::resolve(path, &base, user_home.as_deref()).is_none() {
                    reasons.push("요청 경로를 확인할 수 없어 거부했습니다".to_string());
                }
            }
        }
        if let Some(command) = call["tool_input"]["command"].as_str() {
            match o_guard::hook::shell_targets(command, &base) {
                Ok(targets) => words.extend(targets),
                Err(_) => reasons.push("명령의 경로를 확인할 수 없어 거부했습니다".to_string()),
            }
        }
    }
    let names_state = words.iter().flat_map(|word| std::iter::once(word.as_str()).chain(word.split_whitespace()))
        .map(|w| w.trim_matches(['"', '\'', ';', ',']))
        .flat_map(|w| [w.replace('\\', "/").to_lowercase(), o_guard::hook::resolve(w, &base, user_home.as_deref()).unwrap_or_default().to_lowercase()]).any(|w| {
        (!state.is_empty() && w.contains(&state)) || (w.contains("localappdata") && w.contains("/project_o") && !w.contains("/project_o-"))
    });
    let names_real_state = words.iter().flat_map(|word| std::iter::once(word.as_str()).chain(word.split_whitespace()))
        .filter_map(|word| o_guard::hook::resolve(word.trim_matches(['"', '\'', ';', ',']), &base, user_home.as_deref()))
        .any(|path| o_guard::hook::within_state(&path, &home.to_string_lossy()));
    if names_state || names_real_state { reasons.push("O 상태 폴더는 총괄이 바꾸거나 실행할 수 없습니다".to_string()); }
    let mut unique = Vec::new();
    for reason in reasons { if !unique.contains(&reason) { unique.push(reason); } }
    unique
}

/// One request through to its outcome: O's own refusals, then the person's decision, a stop of
/// the turn (`token`'s), or `wait` running out. `tick` runs every few seconds of the wait.
pub fn ask(home: &Path, thread_id: &str, token: &str, project: &str, request: Request, wait: Duration, tick: &mut dyn FnMut()) -> Outcome {
    if !is_run_id(thread_id) || token.is_empty() || crate::agents::agy::inside_card()
        || !live_turn(home, thread_id, token)
        || super::thread_project(&super::read_thread(home, thread_id)).is_none_or(|found| !crate::projects::same(&found, project)) {
        return Outcome { allowed: false, decision: "deny", message: "현재 총괄 턴의 승인 요청이 아니어서 거부했습니다".into() };
    }
    let refused = refusals(home, project, &request);
    let folder = dir(home, thread_id);
    if let Err(error) = fs::create_dir_all(&folder) {
        return Outcome { allowed: false, decision: "deny", message: format!("승인 요청을 기록하지 못해 거부했습니다: {error}") };
    }
    let now = Utc::now();
    let pending = {
        // The number is taken and the request recorded under one lock: ids never repeat in a thread.
        let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(folder.join(".lock"));
        let Ok(_held) = lock.and_then(|file| { file.lock()?; Ok(file) }) else {
            return Outcome { allowed: false, decision: "deny", message: "승인 요청 잠금을 잡지 못해 거부했습니다.".into() };
        };
        if fs::read_dir(&folder).into_iter().flatten().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".json")).count() >= MAX_PENDING {
            return Outcome { allowed: false, decision: "deny", message: "승인 대기 요청이 너무 많아 거부했습니다.".into() };
        }
        let n = super::read_thread(home, thread_id).iter().filter(|e| matches!(e, LeadEvent::Approval { .. })).count() + 1;
        let pending = Pending {
            id: format!("{thread_id}-a{n}"),
            token: token.to_string(),
            at: iso(now),
            expires: iso(now + chrono::Duration::from_std(wait).unwrap_or_default()),
            provider: request.provider,
            kind: request.kind,
            tool: request.tool.clone(),
            summary: clip(request.summary.trim(), SUMMARY_CHARS),
            detail: clip(&request.detail, DETAIL_CHARS),
            cwd: request.cwd.clone(),
            reason: request.reason.as_ref().map(|r| clip(r.trim(), SUMMARY_CHARS)),
            files: request.files.iter().map(|f| FileDelta { diff: clip(&f.diff, DETAIL_CHARS), ..f.clone() }).take(50).collect(),
        };
        let file = folder.join(format!("{}.json", pending.id));
        if fs::symlink_metadata(folder.join(format!("{}.decision", pending.id))).is_ok() {
            return Outcome { allowed: false, decision: "deny", message: "승인 요청 전에 결정 파일이 있어 거부했습니다".into() };
        }
        let temp = folder.join(format!("{}.{}.tmp", pending.id, new_run_id(now)));
        // On file first: a person who sees the event can already answer it.
        let filed = refused.is_empty() && serde_json::to_vec(&pending).map_err(io::Error::other).and_then(|bytes| fs::write(&temp, bytes)).and_then(|()| fs::rename(&temp, &file)).is_ok();
        let recorded = append(home, thread_id, &LeadEvent::Approval {
            at: pending.at.clone(), id: pending.id.clone(), provider: pending.provider, kind: pending.kind, tool: pending.tool.clone(),
            summary: pending.summary.clone(), detail: pending.detail.clone(), cwd: pending.cwd.clone(), reason: pending.reason.clone(),
            files: pending.files.clone(), expires: pending.expires.clone(),
        }).is_ok();
        if !refused.is_empty() || !filed || !recorded {
            let _ = fs::remove_file(&temp);
            let _ = fs::remove_file(&file);
            let (by, reason, message) = if !refused.is_empty() {
                ("guard", refused.join(" / "), format!("O 정책이 이 작업을 막았습니다: {}. 우회하지 말고 사용자에게 알리세요.", refused.join(" / ")))
            } else {
                ("o", "승인 요청을 기록하지 못했습니다".to_string(), "승인 요청을 기록하지 못해 거부했습니다.".to_string())
            };
            let _ = append(home, thread_id, &LeadEvent::Decision { at: now_iso(), id: pending.id.clone(), decision: "deny".into(), by: by.into(), reason: Some(reason) });
            return Outcome { allowed: false, decision: "deny", message };
        }
        pending
    };
    let file = folder.join(format!("{}.json", pending.id));
    let answer = folder.join(format!("{}.decision", pending.id));
    let started = Instant::now();
    let mut ticked = Instant::now();
    let (decision, by) = loop {
        // A stop wins over an answer that came with it.
        if fs::read_to_string(stop_path(home, thread_id)).is_ok_and(|t| t.is_empty() || t == token) { break ("stopped", "stop"); }
        if !live_turn(home, thread_id, token) { break ("stopped", "o"); }
        if started.elapsed() >= wait { break ("timeout", "timeout"); }
        let decision = crate::run::workspaces::open_plain(&answer).and_then(|file| {
            use std::io::Read;
            let mut text = String::new();
            file.take(16).read_to_string(&mut text)?;
            Ok(text)
        });
        match decision.as_deref() {
            Ok("allow") => break ("allow", "user"),
            Ok("deny") => break ("deny", "user"),
            _ => {}
        }
        if !file.exists() { break ("stopped", "o"); }
        if ticked.elapsed() >= TICK { tick(); ticked = Instant::now(); }
        thread::sleep(POLL);
    };
    let _ = append(home, thread_id, &LeadEvent::Decision { at: now_iso(), id: pending.id.clone(), decision: decision.into(), by: by.into(), reason: None });
    let _ = fs::remove_file(&file);
    let _ = fs::remove_file(&answer);
    let minutes = wait.as_secs().div_ceil(60);
    let message = match decision {
        "allow" => String::new(),
        "deny" => "사용자가 이 작업을 거부했습니다. 같은 작업을 다시 시도하지 말고, 필요하면 사용자에게 이유를 묻거나 다른 방법을 제안하세요.".to_string(),
        "timeout" => format!("사용자가 {minutes}분 안에 승인하지 않아 거부되었습니다. 다시 시도하지 말고 사용자에게 알리세요."),
        _ => "사용자가 총괄을 중지했습니다.".to_string(),
    };
    Outcome { allowed: decision == "allow", decision, message }
}

fn live_turn(home: &Path, thread_id: &str, token: &str) -> bool {
    running(home, thread_id).is_some_and(|busy| !busy.unknown) && read_marker(home, thread_id).is_some_and(|m| m.token == token)
}

/// One line of a file's text count, for a write's `+` side.
fn lines(text: &str) -> u64 {
    text.lines().count() as u64
}

/// A Claude Code tool call (`tool_name`, `input`) as a request.
pub fn from_claude(tool: &str, input: &Value, project: &str) -> Request {
    let text = |key: &str| input[key].as_str().unwrap_or("").to_string();
    let review = vec![json!({ "tool_name": tool, "tool_input": input })];
    let pretty = serde_json::to_string_pretty(input).unwrap_or_default();
    let base = Request { provider: ProviderId::Claude, kind: Kind::Other, tool: tool.to_string(), summary: tool.to_string(), detail: pretty, cwd: None, reason: None, files: Vec::new(), review };
    match tool {
        "Bash" | "PowerShell" => {
            let command = text("command");
            Request { kind: Kind::Command, summary: command.lines().next().unwrap_or("").to_string(), detail: command, cwd: Some(project.to_string()), reason: Some(text("description")).filter(|d| !d.is_empty()), ..base }
        }
        "Write" => {
            let path = text("file_path");
            let content = text("content");
            let diff = content.lines().map(|l| format!("+{l}")).collect::<Vec<_>>().join("\n");
            let file = FileDelta { path: path.clone(), added: Some(lines(&content)), removed: None, diff: diff.clone() };
            Request { kind: Kind::File, summary: path, detail: diff, files: vec![file], ..base }
        }
        "Edit" | "MultiEdit" => {
            let path = text("file_path");
            let edits: Vec<(String, String)> = match input["edits"].as_array() {
                Some(edits) => edits.iter().map(|e| (e["old_string"].as_str().unwrap_or("").to_string(), e["new_string"].as_str().unwrap_or("").to_string())).collect(),
                None => vec![(text("old_string"), text("new_string"))],
            };
            let diff = edits.iter().map(|(old, new)| {
                let minus = old.lines().map(|l| format!("-{l}"));
                let plus = new.lines().map(|l| format!("+{l}"));
                minus.chain(plus).collect::<Vec<_>>().join("\n")
            }).collect::<Vec<_>>().join("\n@@\n");
            let (added, removed) = edits.iter().fold((0, 0), |(a, r), (old, new)| (a + lines(new), r + lines(old)));
            Request { kind: Kind::File, summary: path.clone(), detail: diff.clone(), files: vec![FileDelta { path, added: Some(added), removed: Some(removed), diff }], ..base }
        }
        "NotebookEdit" => {
            let path = text("notebook_path");
            Request { kind: Kind::File, summary: path.clone(), files: vec![FileDelta { path, ..FileDelta::default() }], ..base }
        }
        "WebFetch" => Request { kind: Kind::Web, summary: text("url"), detail: format!("{}\n{}", text("url"), text("prompt")), ..base },
        "WebSearch" => Request { kind: Kind::Web, summary: text("query"), detail: text("query"), ..base },
        _ => base,
    }
}

/// A Codex command approval (`item/commandExecution/requestApproval` params). An approved
/// command that asks for escalation runs outside the sandbox, so the whole command is shown.
pub fn from_codex_command(params: &Value) -> Request {
    let command = params["command"].as_str().unwrap_or("").to_string();
    let network = params.pointer("/networkApprovalContext/host").and_then(Value::as_str);
    let summary = network.map_or_else(|| command.lines().next().unwrap_or("").to_string(), |host| format!("네트워크: {host}"));
    Request {
        provider: ProviderId::Codex,
        kind: if network.is_some() { Kind::Web } else { Kind::Command },
        tool: "commandExecution".into(),
        summary,
        detail: if command.is_empty() { serde_json::to_string_pretty(params).unwrap_or_default() } else { command.clone() },
        cwd: params["cwd"].as_str().map(str::to_string),
        reason: params["reason"].as_str().map(str::to_string),
        files: Vec::new(),
        review: vec![json!({ "tool_name": "shell", "tool_input": { "command": command } })],
    }
}

/// A Codex file change approval (`item/fileChange/requestApproval` params) with the changes of
/// its item (`item/started` and `item/fileChange/patchUpdated` carry them; the request does not).
pub fn from_codex_files(params: &Value, changes: &[Value]) -> Request {
    let files: Vec<FileDelta> = changes.iter().map(|change| {
        let (added, removed) = super::steps::change_counts(change);
        FileDelta { path: change["path"].as_str().unwrap_or("").to_string(), added: Some(added), removed: Some(removed), diff: change["diff"].as_str().unwrap_or("").to_string() }
    }).collect();
    let summary = match files.len() {
        0 => params["grantRoot"].as_str().map_or_else(|| "파일 변경".to_string(), |root| format!("쓰기 권한: {root}")),
        _ => files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().join(", "),
    };
    let detail = files.iter().map(|f| format!("--- {}\n{}", f.path, f.diff)).collect::<Vec<_>>().join("\n");
    let mut review: Vec<Value> = files.iter().map(|f| json!({ "tool_name": "Write", "tool_input": { "file_path": f.path, "content": f.diff } })).collect();
    // A moved file is written at its destination too: that path meets the same checks (#62).
    for change in changes {
        let destination = change.pointer("/kind/move_path").or_else(|| change.pointer("/kind/movePath")).and_then(Value::as_str).filter(|path| !path.is_empty());
        if let Some(destination) = destination {
            review.push(json!({ "tool_name": "Write", "tool_input": { "file_path": destination, "content": change["diff"].as_str().unwrap_or("") } }));
        }
    }
    if let Some(root) = params["grantRoot"].as_str() { review.push(json!({ "tool_name": "Write", "tool_input": { "file_path": root, "content": "" } })); }
    Request { provider: ProviderId::Codex, kind: Kind::File, tool: "fileChange".into(), summary, detail, cwd: None, reason: params["reason"].as_str().map(str::to_string), files, review }
}

/// One turn's approval server as Claude Code launches it: `o _lead-approve <home> <thread> <token> <project>`.
pub struct Server {
    pub home: PathBuf,
    pub thread_id: String,
    pub token: String,
    pub project: String,
    pub wait: Duration,
}

/// The command line of that server (`program` is `o` itself).
pub fn server_command(program: &Path, home: &Path, thread_id: &str, token: &str, project: &str) -> Vec<String> {
    [program.to_string_lossy().as_ref(), COMMAND, home.to_string_lossy().as_ref(), thread_id, token, project].iter().map(|s| s.to_string()).collect()
}

/// The MCP stdio server Claude asks before any tool it may not use freely. Each `tools/call`
/// waits on its own thread, so parallel calls each get their own card. Ends when Claude closes
/// stdin: nobody reads an answer then, and the turn's owner closes what is left (`close_left`).
pub fn serve(server: Server, input: impl BufRead, output: impl Write + Send + 'static) {
    let output = Arc::new(Mutex::new(output));
    let server = Arc::new(server);
    let send = |output: &Arc<Mutex<dyn Write + Send>>, message: Value| {
        if let Ok(mut out) = output.lock() {
            let _ = writeln!(out, "{message}").and_then(|()| out.flush());
        }
    };
    let output: Arc<Mutex<dyn Write + Send>> = output;
    for line in input.lines() {
        let Ok(line) = line else { break };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        let Some(id) = message.get("id").cloned() else { continue };
        match message["method"].as_str() {
            Some("initialize") => {
                let version = message.pointer("/params/protocolVersion").and_then(Value::as_str).unwrap_or("2025-06-18");
                send(&output, json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":version,"capabilities":{"tools":{}},"serverInfo":{"name":SERVER,"version":env!("CARGO_PKG_VERSION")}}}));
            }
            Some("tools/list") => send(&output, json!({"jsonrpc":"2.0","id":id,"result":{"tools":[{
                "name": TOOL,
                "description": "Project O asks the person to approve or deny one tool call of the lead.",
                "inputSchema": {"type":"object","properties":{"tool_name":{"type":"string"},"input":{"type":"object"},"tool_use_id":{"type":"string"}},"required":["tool_name","input"]}
            }]}})),
            Some("tools/call") => {
                let (server, output) = (Arc::clone(&server), Arc::clone(&output));
                thread::spawn(move || {
                    let params = &message["params"];
                    let reply = if params["name"] != TOOL {
                        json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":"unknown tool"}],"isError":true}})
                    } else {
                        let tool = params.pointer("/arguments/tool_name").and_then(Value::as_str).unwrap_or("");
                        let input = params.pointer("/arguments/input").cloned().unwrap_or_else(|| json!({}));
                        let request = from_claude(tool, &input, &server.project);
                        let progress = params.pointer("/_meta/progressToken").cloned();
                        let mut n = 0;
                        let mut tick = || if let Some(token) = &progress {
                            n += 1;
                            send(&output, json!({"jsonrpc":"2.0","method":"notifications/progress","params":{"progressToken":token,"progress":n,"message":"사용자 승인을 기다리는 중"}}));
                        };
                        let outcome = ask(&server.home, &server.thread_id, &server.token, &server.project, request, server.wait, &mut tick);
                        let decision = if outcome.allowed { json!({"behavior":"allow","updatedInput":input}) }
                            else { json!({"behavior":"deny","message":outcome.message,"interrupt":outcome.decision == "stopped"}) };
                        json!({"jsonrpc":"2.0","id":id,"result":{"content":[{"type":"text","text":decision.to_string()}]}})
                    };
                    send(&output, reply);
                });
            }
            // `server/discover`, `ping` and anything else with an id get an empty result.
            _ => send(&output, json!({"jsonrpc":"2.0","id":id,"result":{}})),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lead::{read_thread, thread_for, TurnSlot};
    use std::io::BufReader;

    fn setup(name: &str) -> (PathBuf, String, String, TurnSlot) {
        let stamp = Utc::now().timestamp_nanos_opt().unwrap();
        let base = std::env::temp_dir().join(format!("o-approve-{name}-{}-{stamp}", std::process::id()));
        let home = base.join("state");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let project = project.to_string_lossy().into_owned();
        let thread = thread_for(&home, &project, false).unwrap();
        let slot = TurnSlot::take(&home, &thread, ProviderId::Claude).unwrap();
        (home, thread, project, slot)
    }

    fn write_request(path: &str) -> Request {
        from_claude("Write", &json!({"file_path": path, "content": "a\nb\n"}), "C:/p")
    }

    #[test]
    fn only_the_live_turn_and_its_project_can_request_approval() {
        let (home, thread, project, slot) = setup("binding");
        for (id, token, folder) in [("../escaped", slot.marker.token.as_str(), project.as_str()), (thread.as_str(), "old", project.as_str()), (thread.as_str(), slot.marker.token.as_str(), "C:/other-project")] {
            let outcome = ask(&home, id, token, folder, write_request("safe.txt"), Duration::ZERO, &mut || panic!("invalid request waited"));
            assert!(!outcome.allowed);
            assert!(outcome.message.contains("현재 총괄 턴"));
        }
        assert!(!folder(&home, &thread).exists());
        drop(slot);
        assert!(!ask(&home, &thread, "old", &project, write_request("safe.txt"), Duration::ZERO, &mut || {}).allowed);
    }

    #[test]
    fn a_linked_decision_cannot_allow_a_waiting_request() {
        let (home, thread, project, slot) = setup("linked-answer");
        let source = home.join("outside-decision");
        fs::write(&source, "allow").unwrap();
        let helper = {
            let (home, thread) = (home.clone(), thread.clone());
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if let Some(request) = pending(&home, &thread).first() {
                        fs::hard_link(&source, folder(&home, &thread).join(format!("{}.decision", request["id"].as_str().unwrap()))).unwrap();
                        return;
                    }
                    thread::sleep(Duration::from_millis(5));
                }
                panic!("approval request did not appear");
            })
        };
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&in_project(&project)), Duration::from_millis(500), &mut || {});
        helper.join().unwrap();
        assert_eq!(outcome.decision, "timeout");
    }

    /// Answers the first request that shows up (or only finds it), from another thread.
    fn answer(home: &Path, thread: &str, allow: Option<bool>) -> thread::JoinHandle<Option<String>> {
        let (home, thread) = (home.to_path_buf(), thread.to_string());
        thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if let Some(id) = pending(&home, &thread).first().and_then(|p| p["id"].as_str().map(str::to_string)) {
                    if let Some(allow) = allow { decide(&home, &id, allow).unwrap(); }
                    return Some(id);
                }
                thread::sleep(Duration::from_millis(20));
            }
            None
        })
    }

    /// A file inside the test's own project: `C:/p/...` is relative on Unix and would be judged
    /// against whatever checkout the test runs from (a protected branch in some CI runs).
    fn in_project(project: &str) -> String {
        Path::new(project).join("a.txt").to_string_lossy().into_owned()
    }

    fn decisions(home: &Path, thread: &str) -> Vec<(String, String)> {
        read_thread(home, thread).into_iter().filter_map(|e| match e { LeadEvent::Decision { decision, by, .. } => Some((decision, by)), _ => None }).collect()
    }

    #[test]
    fn allow_and_deny_come_from_the_person_once() {
        let (home, thread, project, slot) = setup("allow");
        let file = Path::new(&project).join("hello.txt").to_string_lossy().into_owned();
        let helper = answer(&home, &thread, Some(true));
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&file), Duration::from_secs(5), &mut || {});
        let id = helper.join().unwrap().unwrap();
        assert!(outcome.allowed && outcome.decision == "allow");
        assert!(decide(&home, &id, false).is_err(), "a decided request takes no second answer");
        let helper = answer(&home, &thread, Some(false));
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&file), Duration::from_secs(5), &mut || {});
        helper.join().unwrap().unwrap();
        assert!(!outcome.allowed && outcome.decision == "deny" && outcome.message.contains("거부"));
        assert_eq!(decisions(&home, &thread), [("allow".into(), "user".into()), ("deny".into(), "user".into())]);
        let ids: Vec<String> = read_thread(&home, &thread).into_iter().filter_map(|e| match e {
            LeadEvent::Approval { id, files, .. } => { assert_eq!(files[0].added, Some(2)); Some(id) }
            _ => None,
        }).collect();
        assert_eq!(ids, [format!("{thread}-a1"), format!("{thread}-a2")]);
        assert!(pending(&home, &thread).is_empty());
        drop(slot);
    }

    #[test]
    fn a_request_times_out_or_stops_with_the_turn() {
        let (home, thread, project, slot) = setup("timeout");
        let file = in_project(&project);
        let started = Instant::now();
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&file), Duration::from_millis(300), &mut || {});
        assert_eq!(outcome.decision, "timeout");
        assert!(started.elapsed() < Duration::from_secs(3));
        let stopper = {
            let (home, thread, token) = (home.clone(), thread.clone(), slot.marker.token.clone());
            thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while pending(&home, &thread).is_empty() && Instant::now() < deadline { thread::sleep(Duration::from_millis(20)); }
                fs::write(stop_path(&home, &thread), token).unwrap();
            })
        };
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&file), Duration::from_secs(5), &mut || {});
        stopper.join().unwrap();
        assert_eq!(outcome.decision, "stopped");
        assert_eq!(decisions(&home, &thread), [("timeout".into(), "timeout".into()), ("stopped".into(), "stop".into())]);
        drop(slot);
    }

    #[test]
    fn expired_request_never_accepts_a_waiting_answer() {
        let (home, thread, project, slot) = setup("expired-answer");
        let folder = folder(&home, &thread);
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join(format!("{thread}-a1.decision")), "allow").unwrap();
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&in_project(&project)), Duration::ZERO, &mut || {});
        assert_eq!(outcome.decision, "deny");
        assert!(outcome.message.contains("요청 전에"));
    }

    #[test]
    fn request_is_refused_if_its_number_cannot_be_locked() {
        let (home, thread, project, slot) = setup("lock-failure");
        fs::create_dir_all(folder(&home, &thread).join(".lock")).unwrap();
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request(&in_project(&project)), Duration::from_secs(5), &mut || {});
        assert_eq!(outcome.decision, "deny");
        assert!(outcome.message.contains("잠금"), "{}", outcome.message);
        assert!(pending(&home, &thread).is_empty());
    }

    #[test]
    fn guard_rules_and_the_state_folder_refuse_before_anyone_is_asked() {
        let (home, thread, project, slot) = setup("guard");
        for request in [
            from_claude("Bash", &json!({"command": "git push origin main"}), &project),
            from_claude("Write", &json!({"file_path": home.join("leads").join("x.decision").to_string_lossy(), "content": "allow"}), &project),
            from_claude("mcp__o_lead__decide", &json!({}), &project),
            from_codex_command(&json!({"command": "Remove-Item $env:LOCALAPPDATA\\project_O\\leads -Recurse"})),
            from_codex_command(&json!({"command": "touch x", "cwd": home.join("leads").to_string_lossy()})),
        ] {
            let outcome = ask(&home, &thread, &slot.marker.token, &project, request, Duration::from_secs(5), &mut || panic!("asked the person"));
            assert_eq!(outcome.decision, "deny");
            assert!(outcome.message.contains("O 정책"), "{}", outcome.message);
        }
        let refused = decisions(&home, &thread);
        assert_eq!(refused.len(), 5);
        assert!(refused.iter().all(|(d, by)| d == "deny" && by == "guard"));
        assert!(pending(&home, &thread).is_empty());
        drop(slot);
    }

    #[test]
    fn requests_that_cannot_be_shown_in_full_are_refused() {
        let (home, _thread, project, _slot) = setup("oversize");
        let command = from_codex_command(&json!({"command": "x".repeat(DETAIL_CHARS + 1)}));
        assert!(!refusals(&home, &project, &command).is_empty());
        let mut files = from_codex_files(&json!({}), &[]);
        files.files = vec![FileDelta::default(); 51];
        assert!(!refusals(&home, &project, &files).is_empty());
        files.files = vec![FileDelta { diff: "x".repeat(DETAIL_CHARS + 1), ..FileDelta::default() }];
        assert!(!refusals(&home, &project, &files).is_empty());
        let query = "q".repeat(400);
        assert_eq!(from_claude("WebSearch", &json!({"query": query}), &project).detail, query);
        let url = format!("https://example.com/{}", "x".repeat(400));
        assert!(from_claude("WebFetch", &json!({"url": url, "prompt": "read"}), &project).detail.contains(&url));
    }

    /// #62: a Codex move is checked at its destination too, with no grantRoot to fall back on.
    #[test]
    fn codex_move_destination_is_checked_before_approval() {
        let (home, thread, project, slot) = setup("codex-move");
        let source = in_project(&project);
        let into_state = home.join("leads").join("x.decision").to_string_lossy().into_owned();
        let elsewhere = Path::new(&project).join("b.txt").to_string_lossy().into_owned();
        let moved = |to: &str, key: &str| json!({"path": source, "kind": {"type": "update", key: to}, "diff": ""});
        for key in ["move_path", "movePath"] {
            let request = from_codex_files(&json!({}), &[moved(&into_state, key)]);
            assert!(refusals(&home, &project, &request).iter().any(|r| r.contains("O 상태 폴더")), "{key}");
            assert!(refusals(&home, &project, &from_codex_files(&json!({}), &[moved(&elsewhere, key)])).is_empty(), "{key}");
        }
        let outcome = ask(&home, &thread, &slot.marker.token, &project, from_codex_files(&json!({}), &[moved(&into_state, "move_path")]), Duration::from_secs(5), &mut || panic!("asked the person"));
        assert_eq!(outcome.decision, "deny");
        assert!(pending(&home, &thread).is_empty());
    }

    #[test]
    fn write_request_does_not_read_the_target_before_guard_review() {
        let (home, _thread, _project, _slot) = setup("write-before-guard");
        let path = home.join("secret.txt");
        fs::write(&path, "private\ncontent\n").unwrap();
        let request = write_request(&path.to_string_lossy());
        assert_eq!(request.files[0].removed, None);
        assert!(!refusals(&home, "C:/p", &request).is_empty());
    }

    #[test]
    fn state_paths_are_resolved_before_approval() {
        let (home, _thread, project, _slot) = setup("relative-state");
        for path in ["../state/leads/t/approvals/a.decision", "../state/./leads/t/approvals/a.decision"] {
            assert!(!refusals(&home, &project, &write_request(path)).is_empty(), "{path}");
        }
        assert!(!refusals(&home, &project, &write_request("$O_UNKNOWN_PATH/leads/t/approvals/a.decision")).is_empty());
        let command = from_claude("Bash", &json!({"command":"cd ../state && touch leads/t/approvals/a.decision"}), &project);
        assert!(!refusals(&home, &project, &command).is_empty());
        if let Ok(user_home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            let state = Path::new(&user_home).join("o-test-state");
            assert!(!refusals(&state, &project, &write_request("~/o-test-state/leads/t/approvals/a.decision")).is_empty());
            let variable = if cfg!(windows) { "%USERPROFILE%/o-test-state/leads/t/approvals/a.decision" } else { "$HOME/o-test-state/leads/t/approvals/a.decision" };
            assert!(!refusals(&state, &project, &write_request(variable)).is_empty());
        }
    }

    #[test]
    fn too_many_waiting_requests_are_refused_immediately() {
        let (home, thread, project, slot) = setup("cap");
        let folder = folder(&home, &thread);
        fs::create_dir_all(&folder).unwrap();
        for n in 0..MAX_PENDING { fs::write(folder.join(format!("{n}.json")), "{}").unwrap(); }
        let outcome = ask(&home, &thread, &slot.marker.token, &project, write_request("safe.txt"), Duration::from_secs(5), &mut || panic!("waited"));
        assert_eq!(outcome.decision, "deny");
        assert!(outcome.message.contains("너무 많아"));
    }

    #[test]
    fn decisions_need_the_live_turn_and_close_left_clears_what_it_left() {
        let (home, thread, project, slot) = setup("left");
        assert!(decide(&home, "bad-id", true).is_err());
        let helper = answer(&home, &thread, None);
        let asker = {
            let (home, thread, project, token) = (home.clone(), thread.clone(), project.clone(), slot.marker.token.clone());
            thread::spawn(move || ask(&home, &thread, &token, &project, write_request(&in_project(&project)), Duration::from_secs(5), &mut || {}))
        };
        let id = helper.join().unwrap().unwrap();
        assert!(waiting(&folder(&home, &thread)));
        assert!(decide(&home, &id.replace("-a", "-a9"), true).is_err(), "only a request on file takes an answer");
        close_left(&home, &thread);
        assert_eq!(asker.join().unwrap().decision, "stopped");
        assert!(!waiting(&folder(&home, &thread)));
        assert!(decide(&home, &id, true).is_err());
        drop(slot);
        // A request of a turn that is over takes no answer either.
        fs::write(folder(&home, &thread).join(format!("{id}.json")), serde_json::to_vec(&json!({"id":id,"token":"old","at":"","expires":"","provider":"claude","kind":"file","tool":"Write","summary":"","detail":"","cwd":null,"reason":null,"files":[]})).unwrap()).unwrap();
        assert!(decide(&home, &id, true).is_err());
    }

    /// Claude Code's side of the exchange, as the PoC recorded it (Claude Code 2.1.302).
    #[test]
    fn the_mcp_server_answers_claudes_permission_prompt() {
        let (home, thread, project, slot) = setup("mcp");
        let (server_in, mut to_server) = std::io::pipe().unwrap();
        let (from_server, server_out) = std::io::pipe().unwrap();
        let server = Server { home: home.clone(), thread_id: thread.clone(), token: slot.marker.token.clone(), project: project.clone(), wait: Duration::from_secs(5) };
        let serving = thread::spawn(move || serve(server, BufReader::new(server_in), server_out));
        let mut lines = BufReader::new(from_server).lines();
        let mut call = move |message: Value| writeln!(to_server, "{message}").unwrap();
        call(json!({"jsonrpc":"2.0","id":"server-discover-probe-1","method":"server/discover","params":{}}));
        call(json!({"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}},"jsonrpc":"2.0","id":0}));
        call(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        call(json!({"method":"tools/list","jsonrpc":"2.0","id":1}));
        let mut next = || serde_json::from_str::<Value>(&lines.next().unwrap().unwrap()).unwrap();
        assert_eq!(next()["id"], "server-discover-probe-1");
        assert_eq!(next()["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(next()["result"]["tools"][0]["name"], TOOL);
        let file = Path::new(&project).join("hello.txt").to_string_lossy().into_owned();
        let input = json!({"file_path": file, "content": "hi"});
        let helper = answer(&home, &thread, Some(true));
        call(json!({"method":"tools/call","params":{"name":"decide","arguments":{"tool_name":"Write","input":input,"tool_use_id":"toolu_1"},"_meta":{"progressToken":2}},"jsonrpc":"2.0","id":2}));
        helper.join().unwrap().unwrap();
        let reply = next();
        assert_eq!(reply["id"], 2);
        let decision: Value = serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(decision, json!({"behavior":"allow","updatedInput":input}));
        let helper = answer(&home, &thread, Some(false));
        call(json!({"method":"tools/call","params":{"name":"decide","arguments":{"tool_name":"Bash","input":{"command":"npm test"}}},"jsonrpc":"2.0","id":3}));
        helper.join().unwrap().unwrap();
        let decision: Value = serde_json::from_str(next()["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!((decision["behavior"].as_str(), decision["interrupt"].as_bool()), (Some("deny"), Some(false)));
        drop(call);
        serving.join().unwrap();
        drop(slot);
    }
}
