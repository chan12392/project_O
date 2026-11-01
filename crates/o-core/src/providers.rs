//! How each official CLI is launched in headless mode and how its stream is
//! read. This is the only module that knows provider flags.
//!
//! Every launch keeps the CLI's own sign-in and permission checks: no token is
//! injected and no flag that disables approvals is ever added. The one opening
//! is narrow: each CLI may run `o msg`, the run's team channel, and nothing more.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::events::StreamSignal;
use crate::js::{as_number, coalesce, iso_from_secs, js_head, js_len, js_string, squash_spaces};
use crate::plan::{Card, Plan, ProviderId};

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchSpec {
    /// Executable name looked up on PATH.
    pub bin: String,
    pub args: Vec<String>,
    /// Prompt written to stdin, when the CLI reads it from there.
    pub stdin: Option<String>,
    /// Run and card ids for `o msg`; never credentials.
    pub env: BTreeMap<String, String>,
}

pub struct TeamChannel<'a> {
    pub run_id: &'a str,
    /// The run's folder, where `o msg` appends; opened for writing to sandboxed CLIs.
    pub run_dir: &'a str,
}

/// The team channel command each CLI may run without a prompt.
const TEAM_COMMAND: &str = "o msg";

pub fn launch_spec(card: &Card, plan: &Plan, team: &TeamChannel) -> LaunchSpec {
    let prompt = card_prompt(card, plan, team);
    let env = BTreeMap::from([("O_RUN_ID".to_string(), team.run_id.to_string()), ("O_CARD_ID".to_string(), card.id.clone())]);
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    match card.provider {
        ProviderId::Claude => {
            let mut args = s(&["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--allowedTools"]);
            args.push(format!("Bash({TEAM_COMMAND} *)"));
            args.extend(opt("--model", card.model.as_deref()));
            args.extend(opt("--effort", card.effort.as_deref()));
            LaunchSpec { bin: "claude".into(), args, stdin: Some(prompt), env }
        }
        ProviderId::Codex => {
            let mut args = s(&["exec", "--json", "-s", "workspace-write", "-C", &plan.project, "--add-dir", team.run_dir]);
            args.extend(opt("-m", card.model.as_deref()));
            let effort = card.effort.as_deref().filter(|e| !e.is_empty()).map(|e| format!("model_reasoning_effort={e}"));
            args.extend(opt("-c", effort.as_deref()));
            args.push("-".into());
            LaunchSpec { bin: "codex".into(), args, stdin: Some(prompt), env }
        }
        ProviderId::Agy => {
            // agy allow-rules match whole commands only, so it gets two fixed ones and
            // writes what it sends to its outbox file first (see `o msg post`).
            let settings = json!({ "permissions": { "allow": [format!("command({TEAM_COMMAND} read)"), format!("command({TEAM_COMMAND} post)")] } });
            let mut args = s(&["--output-format", "stream-json", "--mode", "accept-edits", "--add-dir", team.run_dir, "--settings"]);
            args.push(settings.to_string());
            args.extend(opt("--model", card.model.as_deref()));
            args.extend(opt("--effort", card.effort.as_deref()));
            args.push("-p".into());
            args.push(prompt);
            LaunchSpec { bin: "agy".into(), args, stdin: None, env }
        }
    }
}

/// Headless runs cannot answer permission prompts, so a denied command ends
/// the turn. Tell the agents that cannot run shell commands up front.
fn tool_note(provider: ProviderId) -> Option<String> {
    match provider {
        ProviderId::Claude => Some(format!("셸 명령은 승인할 사람이 없어 거부될 수 있습니다({TEAM_COMMAND}만 허용). 파일 읽기·쓰기 도구를 우선 쓰세요.")),
        ProviderId::Codex => None,
        ProviderId::Agy => Some(format!(
            "셸 명령은 \"{TEAM_COMMAND} read\"와 \"{TEAM_COMMAND} post\" 두 개만 그대로 실행할 수 있습니다(헤드리스 모드라 다른 명령은 승인 불가). 그 밖에는 파일 읽기·쓰기 도구만 쓰세요."
        )),
    }
}

/// Where an agent whose CLI cannot pass free text to a command writes what it sends.
pub fn outbox_path(run_dir: &str, card_id: &str) -> String {
    format!("{}/outbox/{card_id}.txt", run_dir.trim_end_matches(['\\', '/']))
}

fn team_how_to(card: &Card, team: &TeamChannel) -> Vec<String> {
    if card.provider == ProviderId::Agy {
        return vec![
            format!(
                "보내기: {} 파일에 첫 줄은 \"to: <카드 id|lead|all>\", 다음 줄부터 내용을 쓰고, 셸에서 {TEAM_COMMAND} post 를 그대로 실행합니다.",
                outbox_path(team.run_dir, &card.id)
            ),
            format!("받기: 셸에서 {TEAM_COMMAND} read 를 그대로 실행합니다."),
        ];
    }
    vec![format!("보내기: {TEAM_COMMAND} send --to <카드 id|lead|all> \"내용\"   받기: {TEAM_COMMAND} read")]
}

/// The work card text a provider receives. Kept short: it is paid for on every card.
pub fn card_prompt(card: &Card, plan: &Plan, team: &TeamChannel) -> String {
    let others: Vec<&Card> = plan.cards.iter().filter(|c| c.id != card.id).collect();
    let role = card.role.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" · {r}")).unwrap_or_default();
    let mut lines = vec![format!("[O 작업카드] {}{role}", plan.title), format!("프로젝트: {}", plan.project)];
    if !others.is_empty() {
        lines.push("같은 폴더에서 다른 카드가 동시에 일합니다. 맡은 파일 밖의 파일은 지우거나 고치지 마세요.".into());
    }
    lines.extend(tool_note(card.provider));
    lines.push(String::new());
    lines.push(card.task.trim().to_string());
    lines.push(String::new());
    lines.push(format!("팀 대화 (실행 {}, 내 카드 {}):", team.run_id, card.id));
    for c in &others {
        let model = c.model.as_deref().filter(|m| !m.is_empty()).map(|m| format!(" {m}")).unwrap_or_default();
        lines.push(format!("- {}: {} ({}{model})", c.id, c.label(), c.provider));
    }
    lines.push("- lead: 총괄".into());
    lines.extend(team_how_to(card, team));
    lines.push("시작할 때 한 번 받아 보고, 다른 카드가 알아야 할 결과나 막힌 점이 생기면 짧게 보내세요.".into());
    lines.push(String::new());
    lines.push("끝나면 결과를 다섯 줄 이내로 요약하세요: 한 일, 바꾼 파일, 확인한 것, 남은 일.".into());
    lines.join("\n")
}

fn opt(flag: &str, value: Option<&str>) -> Vec<String> {
    match value {
        Some(v) if !v.is_empty() => vec![flag.to_string(), v.to_string()],
        _ => Vec::new(),
    }
}

/// Turns one stdout line into zero or more signals. Unknown lines are ignored
/// so a CLI update degrades to "less detail", never to a crash.
pub struct StreamReader {
    provider: ProviderId,
    agy_text: String,
}

impl StreamReader {
    pub fn new(provider: ProviderId) -> Self {
        StreamReader { provider, agy_text: String::new() }
    }

    pub fn read(&mut self, line: &str) -> Vec<StreamSignal> {
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        match self.provider {
            ProviderId::Claude => read_claude(&data),
            ProviderId::Codex => read_codex(&data),
            ProviderId::Agy => self.read_agy(&data),
        }
    }

    fn read_agy(&mut self, data: &Map<String, Value>) -> Vec<StreamSignal> {
        let event = data.get("event").and_then(Value::as_str);
        if event == Some("init") {
            if let Some(id) = data.get("conversation_id").and_then(Value::as_str) {
                return vec![StreamSignal::Session { session_id: id.to_string() }];
            }
        }
        if event == Some("step_update") {
            let Some(step) = data.get("step_update").and_then(Value::as_object) else { return Vec::new() };
            let step_type = step.get("step_type").and_then(Value::as_str);
            let state = step.get("state").and_then(Value::as_str);
            if step_type == Some("agent_response") {
                if let Some(delta) = step.get("text_delta").and_then(Value::as_str) {
                    self.agy_text.push_str(delta);
                }
                if state == Some("DONE") && !self.agy_text.trim().is_empty() {
                    let text = self.agy_text.trim().to_string();
                    self.agy_text.clear();
                    return vec![StreamSignal::Message { text }];
                }
                return Vec::new();
            }
            if let (Some("ACTIVE"), Some("tool"), Some(name)) = (state, step_type, step.get("tool_name").and_then(Value::as_str)) {
                let params = step.get("tool_info").and_then(Value::as_object).and_then(|info| info.get("parameters"));
                let mut summary = tool_summary(params);
                if summary.is_empty() {
                    let raw = coalesce([params]).cloned().unwrap_or(Value::String(String::new()));
                    summary = short(&raw.to_string());
                }
                return vec![StreamSignal::Tool { name: name.to_string(), summary }];
            }
            return Vec::new();
        }
        if event == Some("result") {
            let Some(result) = data.get("result").and_then(Value::as_object) else { return Vec::new() };
            let text = coalesce([result.get("response"), result.get("error")]).map(js_string).unwrap_or_default().trim().to_string();
            // agy reports SUCCESS even when a denied permission ended the turn with nothing done.
            let denied: Vec<String> = result
                .get("denied_actions")
                .and_then(Value::as_array)
                .map(|actions| {
                    actions
                        .iter()
                        .map(|a| match a.as_object() {
                            Some(a) => coalesce([a.get("display_name"), a.get("action")]).map(js_string).unwrap_or_default(),
                            None => String::new(),
                        })
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            if !denied.is_empty() && text.is_empty() {
                return vec![StreamSignal::Final {
                    ok: false,
                    text: format!("권한 거부로 아무 작업 없이 끝남: {} (헤드리스 모드는 승인할 수 없음)", denied.join(", ")),
                }];
            }
            let ok = result.get("status").and_then(Value::as_str) == Some("SUCCESS");
            let mut out = Vec::new();
            if let Some(u) = result.get("usage").and_then(Value::as_object) {
                out.push(StreamSignal::Usage {
                    input: num(u.get("input_tokens")),
                    cached: num(u.get("cache_read_tokens")),
                    output: num(u.get("output_tokens")) + num(u.get("thinking_tokens")),
                });
            }
            out.extend(limit_from_text(&text, ok));
            out.push(StreamSignal::Final { ok, text });
            return out;
        }
        Vec::new()
    }
}

fn num(value: Option<&Value>) -> f64 {
    as_number(value).filter(|n| n.is_finite()).unwrap_or(0.0)
}

fn read_claude(data: &Map<String, Value>) -> Vec<StreamSignal> {
    let kind = data.get("type").and_then(Value::as_str);
    if kind == Some("system") && data.get("subtype").and_then(Value::as_str) == Some("init") {
        if let Some(id) = data.get("session_id").and_then(Value::as_str) {
            return vec![StreamSignal::Session { session_id: id.to_string() }];
        }
    }
    if kind == Some("assistant") {
        if let Some(content) = data.get("message").and_then(Value::as_object).and_then(|m| m.get("content")).and_then(Value::as_array) {
            let mut out = Vec::new();
            for part in content.iter().filter_map(Value::as_object) {
                let part_type = part.get("type").and_then(Value::as_str);
                if part_type == Some("text") {
                    if let Some(text) = part.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                        out.push(StreamSignal::Message { text: text.trim().to_string() });
                        continue;
                    }
                }
                if part_type == Some("tool_use") {
                    if let Some(name) = part.get("name").and_then(Value::as_str) {
                        out.push(StreamSignal::Tool { name: name.to_string(), summary: tool_summary(part.get("input")) });
                    }
                }
            }
            return out;
        }
    }
    if kind == Some("rate_limit_event") {
        if let Some(info) = data.get("rate_limit_info").and_then(Value::as_object) {
            let mut out: Vec<StreamSignal> = info
                .get("unifiedWindows")
                .and_then(Value::as_object)
                .map(|windows| {
                    windows
                        .iter()
                        .filter_map(|(window, w)| {
                            let w = w.as_object()?;
                            let used = w.get("utilization").and_then(Value::as_f64)?;
                            let resets = w.get("resetsAt").and_then(Value::as_f64)?;
                            Some(StreamSignal::Quota { window: window.clone(), used, resets_at: iso_from_secs(resets) })
                        })
                        .collect()
                })
                .unwrap_or_default();
            // "allowed_warning" is a heads-up while work still runs; only "rejected" blocks the provider.
            if info.get("status").and_then(Value::as_str) == Some("rejected") {
                if let Some(resets) = info.get("resetsAt").and_then(Value::as_f64) {
                    out.push(StreamSignal::Limit { until: iso_from_secs(resets) });
                }
            }
            return out;
        }
    }
    if kind == Some("result") {
        let ok = data.get("is_error") != Some(&Value::Bool(true)) && data.get("subtype").and_then(Value::as_str) == Some("success");
        let mut out = Vec::new();
        if let Some(u) = data.get("usage").and_then(Value::as_object) {
            out.push(StreamSignal::Usage {
                input: num(u.get("input_tokens")) + num(u.get("cache_creation_input_tokens")),
                cached: num(u.get("cache_read_input_tokens")),
                output: num(u.get("output_tokens")),
            });
        }
        let text = coalesce([data.get("result")]).map(js_string).unwrap_or_default().trim().to_string();
        out.push(StreamSignal::Final { ok, text });
        return out;
    }
    Vec::new()
}

static PATH_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\\/]+").unwrap());

fn read_codex(data: &Map<String, Value>) -> Vec<StreamSignal> {
    let kind = data.get("type").and_then(Value::as_str);
    if kind == Some("thread.started") {
        if let Some(id) = data.get("thread_id").and_then(Value::as_str) {
            return vec![StreamSignal::Session { session_id: id.to_string() }];
        }
    }
    if kind == Some("item.completed") {
        let Some(item) = data.get("item").and_then(Value::as_object) else { return Vec::new() };
        let tool = |name: &str, summary: String| vec![StreamSignal::Tool { name: name.to_string(), summary }];
        match item.get("type").and_then(Value::as_str) {
            Some("agent_message") => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    return vec![StreamSignal::Message { text: text.trim().to_string() }];
                }
            }
            Some("command_execution") => {
                let command = coalesce([item.get("command")]).map(js_string).unwrap_or_default();
                return tool("shell", short(&unwrap_shell(&command)));
            }
            Some("file_change") => {
                // File names only: full Windows paths of two files already overflow the summary's length.
                let changes: Vec<Value> = item
                    .get("changes")
                    .and_then(Value::as_array)
                    .map(|changes| {
                        changes
                            .iter()
                            .filter_map(Value::as_object)
                            .map(|c| {
                                let path = coalesce([c.get("path")]).map(js_string).unwrap_or_default();
                                let name = PATH_SPLIT.split(&path).last().unwrap_or_default().to_string();
                                let mut change = Map::new();
                                change.insert("path".into(), Value::String(name));
                                if let Some(kind) = c.get("kind") {
                                    change.insert("kind".into(), kind.clone());
                                }
                                Value::Object(change)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                return tool("edit", short(&Value::Array(changes).to_string()));
            }
            Some("web_search") => return tool("web_search", short_value(item.get("query"))),
            // item.type === "error" items are config warnings; the turn still runs.
            _ => {}
        }
        return Vec::new();
    }
    if kind == Some("turn.completed") {
        let mut out = Vec::new();
        if let Some(u) = data.get("usage").and_then(Value::as_object) {
            let cached = num(u.get("cached_input_tokens"));
            out.push(StreamSignal::Usage { input: (num(u.get("input_tokens")) - cached).max(0.0), cached, output: num(u.get("output_tokens")) });
        }
        out.push(StreamSignal::Final { ok: true, text: String::new() });
        return out;
    }
    if kind == Some("turn.failed") || kind == Some("error") {
        let source = match data.get("error").and_then(Value::as_object) {
            Some(error) => error.get("message"),
            None => data.get("message"),
        };
        let text = coalesce([source]).map(js_string).unwrap_or_default().trim().to_string();
        let mut out = limit_from_text(&text, false);
        out.push(StreamSignal::Final { ok: false, text });
        return out;
    }
    Vec::new()
}

static SHELL_WRAPPER: LazyLock<Regex> = LazyLock::new(|| {
    let shell = r"(?:pwsh|powershell|bash|sh|cmd|zsh)(?:\.exe)?";
    Regex::new(&format!(r#"(?is)^\s*(?:"[^"]*?{shell}"|\S*?{shell})\s+(?:-NoProfile\s+)?(?:-Command|-lc|-c|/c)\s+(.+)$"#)).unwrap()
});

/// Codex reports commands wrapped in their shell (`"C:\…\pwsh.exe" -Command "…"`);
/// people want the command itself.
pub fn unwrap_shell(command: &str) -> String {
    let Some(found) = SHELL_WRAPPER.captures(command) else { return command.to_string() };
    let inner = found[1].trim();
    let quoted = inner.len() >= 2 && ((inner.starts_with('"') && inner.ends_with('"')) || (inner.starts_with('\'') && inner.ends_with('\'')));
    if quoted { inner[1..inner.len() - 1].to_string() } else { inner.to_string() }
}

static LIMIT_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)usage limit|rate limit|quota|resource[_ ]exhausted|too many requests|429").unwrap());

fn limit_from_text(text: &str, ok: bool) -> Vec<StreamSignal> {
    if ok || !LIMIT_TEXT.is_match(text) {
        return Vec::new();
    }
    vec![StreamSignal::Limit { until: String::new() }]
}

fn tool_summary(input: Option<&Value>) -> String {
    let Some(input) = input.and_then(Value::as_object) else { return String::new() };
    let path = coalesce([input.get("file_path"), input.get("Path"), input.get("TargetFile")]);
    // Paths keep their last two segments; the project folder is already known.
    if let Some(Value::String(path)) = path {
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        return short(&parts[parts.len().saturating_sub(2)..].join("/"));
    }
    let detail = coalesce([input.get("command"), input.get("CommandLine"), input.get("pattern"), input.get("url"), input.get("description")]);
    short_value(detail)
}

fn short_value(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(v) => short(&js_string(v)),
    }
}

/// One line of at most 120 characters.
pub fn short(text: &str) -> String {
    let line = squash_spaces(text);
    if js_len(&line) > 120 { format!("{}...", js_head(&line, 117)) } else { line }
}
