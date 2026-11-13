//! Devin CLI (Cognition, `devin` 3000.11.205): cards only, through `devin -p` (ADR-0016).
//! The lead comes later through its ACP server (`devin acp`).
//!
//! Headless `-p` prints only the final answer, as plain text, so a card's tool steps,
//! tokens and session id come from the transcript Devin exports after the turn
//! (`--export`, ATIF-v1.7 JSON) and are read once the CLI has exited.
//!
//! The permission mode is `accept-edits` (workspace edits approved, nothing else): a
//! headless run cannot answer a prompt, so a tool that needs one is refused and the turn
//! ends there. No shell command is opened, not even `o msg`/`o context`: Devin's
//! `Exec(prefix)` allow rule also let `o msg read && <anything>` through (2026-11-03), so
//! an allow rule would open every command. Devin's config folder cannot be moved
//! (`USERPROFILE` elsewhere fails with "Failed to determine config directory"), so it
//! does not run on the project_O profile.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::Path;

use serde_json::{Map, Value};

use super::stream::{limit_from_text, num, short, tool_summary};
use super::{opt, strings, LaunchSpec, Official, Provider, ReadLine, TeamChannel};
use crate::agents::clean::LoginState;
use crate::agents::catalog::connection;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan};
use crate::sys::js::js_string;

pub struct Devin;

/// Devin has no separate project_O profile, including for clean-profile commands.
pub const PROFILE_UNSUPPORTED: &str = "Devin은 project_O 프로필을 지원하지 않습니다. Devin 카드는 평소 프로필로 실행합니다(개인 훅·설정이 적용됨)";
pub const OWN_PROFILE_NOTICE: &str = "Devin은 평소 프로필로 실행합니다(개인 훅·설정이 적용됨). 환경 변수는 시스템·홈 폴더·Devin 것만 넘깁니다";

/// The permission mode a card runs in: workspace edits approved, every other tool asked
/// (and so refused, headless). `smart` is not used: a model judges what runs there.
const CARD_MODE: &str = "accept-edits";
/// The mode that approves every tool (`devin --help`: "dangerous" auto-approves all tools).
const YOLO_MODE: &str = "dangerous";

/// An observation of a tool headless Devin refused (3000.11.205).
const REJECTED: &str = "Tool execution was rejected";
/// Its warning on stderr for the same.
const REJECTED_WARNING: &str = "rejected a tool call that requires confirmation";

/// Where a card's transcript goes: `<run>/cards/<card>.atif.json`, next to its logs.
pub fn transcript_path(run_dir: &str, card_id: &str) -> String {
    format!("{}/cards/{card_id}.atif.json", run_dir.trim_end_matches(['\\', '/']))
}

fn read_transcript(run_dir: &Path, path: &Path) -> Option<String> {
    let run_dir = run_dir.canonicalize().ok()?;
    let parent = path.parent()?.canonicalize().ok()?;
    let resolved = path.canonicalize().ok()?;
    if parent != run_dir.join("cards") || resolved.parent()? != parent || !fs::symlink_metadata(path).ok()?.file_type().is_file() { return None; }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x2_0000);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 { return None; }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
        if metadata.file_attributes() & 0x400 != 0 { return None; }
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, info.as_mut_ptr()) } == 0 { return None; }
        if unsafe { info.assume_init() }.nNumberOfLinks != 1 { return None; }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 { return None; }
    }
    let mut text = String::new();
    file.take(8 * 1024 * 1024 + 1).read_to_string(&mut text).ok()?;
    (text.len() <= 8 * 1024 * 1024).then_some(text)
}

/// `devin -p` with the prompt after `--` (the documented form), the transcript exported
/// to the run folder while retaining the CLI's workspace trust check.
/// No effort flag exists; a Devin model id names its level (`swe-2-medium`).
fn card_args(card: &Card, team: &TeamChannel, prompt: String, mode: &str) -> Vec<String> {
    let spec = connection("devin").headless;
    let mut args = strings(spec.fixed);
    args.extend(strings(&["--permission-mode", mode, "--export"]));
    args.push(transcript_path(team.run_dir, &card.id));
    args.extend(opt(spec.model_flag, card.model.as_deref()));
    args.push("--".into());
    args.push(prompt);
    args
}

impl Provider for Devin {
    fn launch(&self, card: &Card, _plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        LaunchSpec { bin: "devin".into(), args: card_args(card, team, prompt, CARD_MODE), stdin: None, env }
    }

    fn yolo_launch(&self, card: &Card, _plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> Option<LaunchSpec> {
        Some(LaunchSpec { bin: "devin".into(), args: card_args(card, team, prompt, YOLO_MODE), stdin: None, env })
    }

    fn tool_note(&self) -> Option<String> {
        Some("셸 명령은 승인할 사람이 없어 부르는 순간 작업이 끝납니다. 셸은 쓰지 말고 파일 읽기·찾기·고치기 도구만 쓰세요.".into())
    }

    /// No team command can run (see the module note): what others should know goes in the summary.
    fn team_how_to(&self, _card: &Card, _team: &TeamChannel) -> Vec<String> {
        vec!["이 에이전트는 팀 대화 명령을 실행할 수 없습니다. 다른 카드와 총괄이 알아야 할 것은 마지막 요약에 적으세요.".into()]
    }

    fn searches_context(&self) -> bool {
        false
    }

    fn stream_reader(&self) -> ReadLine {
        Box::new(|_| Vec::new())
    }

    fn plain_text_stdout(&self) -> bool {
        true
    }

    fn finish_text_run(&self, card: &Card, team: &TeamChannel, stdout: &str, stderr: &str) -> Vec<StreamSignal> {
        let transcript = read_transcript(Path::new(team.run_dir), Path::new(&transcript_path(team.run_dir, &card.id)));
        read_run(stdout, stderr, transcript.as_deref())
    }

    // https://docs.devin.ai/cli (Windows: PowerShell installer or the updater .exe)
    fn official(&self) -> Official {
        Official { name: "Devin", npm_package: None, script: "irm https://static.devin.ai/cli/setup.ps1 | iex", url: "https://docs.devin.ai/cli" }
    }

    fn login_args(&self) -> Option<&'static [&'static str]> {
        Some(&["auth", "login"])
    }

    fn status_args(&self) -> &'static [&'static str] {
        &["auth", "status"]
    }

    /// Only the first line: the rest names the account, its e-mail and the credentials file.
    fn read_login(&self, _success: bool, stdout: &str, _stderr: &str) -> Option<LoginState> {
        let first = stdout.lines().map(str::trim).find(|l| !l.is_empty())?.to_ascii_lowercase();
        if first.starts_with("logged in") {
            Some(LoginState { logged_in: Some(true), detail: "로그인됨".into() })
        } else if first.starts_with("not logged in") {
            Some(LoginState { logged_in: Some(false), detail: "로그인 안 됨 — devin auth login".into() })
        } else {
            None
        }
    }

    fn profile_unsupported(&self) -> Option<&'static str> {
        Some(PROFILE_UNSUPPORTED)
    }

    /// Never used: `agents::clean` refuses a profile for Devin before it asks for this.
    fn clean_env(&self, _home: &Path) -> (&'static str, String) {
        ("O_PROFILE_UNSUPPORTED", "devin".into())
    }

    fn prepare_profile(&self, _home: &Path, _real_home: &Path) -> io::Result<()> {
        Ok(())
    }

    fn inspect_profile(&self, _home: &Path, _flag: &mut dyn FnMut(bool, String)) {}

    /// Devin has no usage command (only `/usage` inside a session) and reports no window.
    fn daily_budget(&self) -> bool {
        false
    }
}

/// One finished `devin -p` card: the answer on stdout, the stderr tail, and the exported
/// ATIF transcript (`schema_version` "ATIF-v1.7") when Devin wrote one. Pure.
///
/// From the transcript: the session id, each agent step's tool calls and text, and the
/// token totals. A turn that ended on a refused tool prints nothing and still exits 0, so
/// an empty answer is a failure, named after the refused tool when there was one.
pub fn read_run(stdout: &str, stderr: &str, transcript: Option<&str>) -> Vec<StreamSignal> {
    let mut out = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    let atif = transcript.and_then(|t| serde_json::from_str::<Value>(t).ok());
    if let Some(Value::Object(atif)) = &atif {
        out.extend(transcript_signals(atif, &mut refused));
    }
    let transcript_answer = || atif.as_ref()?.get("steps")?.as_array()?.iter().rev().find(|s| s.get("source").and_then(Value::as_str) == Some("agent"))
        .filter(|s| s.get("tool_calls").and_then(Value::as_array).is_none_or(Vec::is_empty))?
        .get("message")?.as_str().map(str::trim).filter(|s| !s.is_empty());
    let answer = if stdout.trim().is_empty() && stderr.trim().is_empty() && refused.is_empty() { transcript_answer().unwrap_or("") } else { stdout.trim() };
    if !answer.is_empty() {
        out.push(StreamSignal::Final { ok: true, text: answer.to_string() });
        return out;
    }
    let text = if !refused.is_empty() || stderr.contains(REJECTED_WARNING) {
        let what = if refused.is_empty() { "셸 명령".to_string() } else { refused.join(", ") };
        format!("권한 거부로 답 없이 끝남: {what} (헤드리스 모드는 승인할 수 없음)")
    } else {
        let last = stderr.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("Devin이 답 없이 끝났습니다");
        short(last)
    };
    out.extend(limit_from_text(stderr, false));
    out.push(StreamSignal::Final { ok: false, text });
    out
}

fn transcript_signals(atif: &Map<String, Value>, refused: &mut Vec<String>) -> Vec<StreamSignal> {
    let mut out = Vec::new();
    if let Some(id) = atif.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        out.push(StreamSignal::Session { session_id: id.to_string() });
    }
    let steps = atif.get("steps").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for step in steps.iter().filter_map(Value::as_object).filter(|s| s.get("source").and_then(Value::as_str) == Some("agent")) {
        let calls: Vec<&Map<String, Value>> = step.get("tool_calls").and_then(Value::as_array).map(|c| c.iter().filter_map(Value::as_object).collect()).unwrap_or_default();
        let shown: Vec<(Option<&str>, String, String)> = calls
            .iter()
            .filter_map(|call| {
                let name = call.get("function_name").and_then(Value::as_str)?;
                Some((call.get("tool_call_id").and_then(Value::as_str), name.to_string(), call_summary(name, call.get("arguments"))))
            })
            .collect();
        if let Some(text) = step.get("message").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) {
            out.push(StreamSignal::Message { text: text.to_string() });
        }
        for (_, name, summary) in &shown {
            out.push(StreamSignal::Tool { name: name.clone(), summary: summary.clone() });
        }
        let results = step.get("observation").and_then(|o| o.get("results")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
        for result in results.iter().filter_map(Value::as_object) {
            if !result.get("content").and_then(Value::as_str).is_some_and(|c| c.starts_with(REJECTED)) {
                continue;
            }
            let id = result.get("source_call_id").and_then(Value::as_str);
            if let Some((_, name, summary)) = shown.iter().find(|(call, _, _)| call.is_some() && *call == id) {
                out.push(StreamSignal::ToolError { name: name.clone(), summary: summary.clone(), error: "확인이 필요한 도구 호출이 거절됐습니다".into() });
                refused.push(if summary.is_empty() { name.clone() } else { format!("{name} {summary}") });
            }
        }
    }
    if let Some(m) = atif.get("final_metrics").and_then(Value::as_object) {
        let cached = num(m.get("total_cached_tokens"));
        out.push(StreamSignal::Usage {
            input: (num(m.get("total_prompt_tokens")) - cached).max(0.0),
            cached,
            output: num(m.get("total_completion_tokens")),
            complete: super::stream::reported(&[m.get("total_prompt_tokens"), m.get("total_cached_tokens"), m.get("total_completion_tokens")]) && cached <= num(m.get("total_prompt_tokens")),
        });
    }
    out
}

/// A tool call in a few words: the file, command, pattern or address it names; for an MCP
/// tool, which server and tool it called.
fn call_summary(name: &str, arguments: Option<&Value>) -> String {
    if name == "mcp_call_tool" {
        let text = |key: &str| arguments.and_then(|a| a.get(key)).map(js_string).unwrap_or_default();
        return short(&format!("{}/{}", text("server_name"), text("tool_name")));
    }
    tool_summary(arguments)
}

#[cfg(test)]
mod transcript_tests {
    use super::*;

    #[test]
    fn reads_only_unlinked_regular_transcripts() {
        let name = format!("o-devin-transcript-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos());
        let dir = std::env::temp_dir().join(name);
        fs::create_dir(&dir).unwrap();
        fs::create_dir(dir.join("cards")).unwrap();
        let path = dir.join("cards/transcript.json");
        fs::write(&path, "{}").unwrap();
        assert_eq!(read_transcript(&dir, &path).as_deref(), Some("{}"));
        let outside = dir.join("outside.json");
        fs::write(&outside, "{}").unwrap();
        assert!(read_transcript(&dir, &outside).is_none());
        fs::hard_link(&path, dir.join("other.json")).unwrap();
        assert!(read_transcript(&dir, &path).is_none());
    }
}
