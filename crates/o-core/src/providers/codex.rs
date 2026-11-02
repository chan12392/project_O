//! Codex: `codex exec --json` in a workspace-write sandbox, the prompt on stdin;
//! usage from a short-lived read-only `codex app-server`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::LazyLock;
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Map, Value};

use super::stream::{limit_from_text, num, short, short_value};
use super::{opt, strings, LaunchSpec, Official, Provider, ReadLine, TeamChannel};
use crate::agents::clean::LoginState;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan, ProviderId};
use crate::sys::js::{coalesce, iso_from_secs, js_string, number_text};
use crate::sys::proc::Contained;
use crate::sys::resolve::resolve_cli;
use crate::usage::UsageWindow;

pub struct Codex;

impl Provider for Codex {
    /// The sandbox runs shell commands itself; the run folder is added so `o msg` can append.
    fn launch(&self, card: &Card, plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        let mut args = strings(&["exec", "--json", "-s", "workspace-write", "-C", &plan.project, "--add-dir", team.run_dir]);
        args.extend(opt("-m", card.model.as_deref()));
        let effort = card.effort.as_deref().filter(|e| !e.is_empty()).map(|e| format!("model_reasoning_effort={e}"));
        args.extend(opt("-c", effort.as_deref()));
        args.push("-".into());
        LaunchSpec { bin: "codex".into(), args, stdin: Some(prompt), env, env_remove: strings(self.api_key_vars()) }
    }

    /// `codex exec` puts this ahead of the saved ChatGPT login (codex-rs/login/src/auth/manager.rs).
    fn api_key_vars(&self) -> &'static [&'static str] {
        &["CODEX_API_KEY"]
    }

    fn tool_note(&self) -> Option<String> {
        None
    }

    fn searches_context(&self) -> bool {
        true
    }

    fn stream_reader(&self) -> ReadLine {
        Box::new(read_stream)
    }

    // https://github.com/openai/codex (npm, Homebrew, Windows installer)
    fn official(&self) -> Official {
        Official {
            name: "Codex",
            npm_package: Some("@openai/codex"),
            script: "irm https://chatgpt.com/codex/install.ps1 | iex",
            url: "https://github.com/openai/codex",
        }
    }

    fn login_args(&self) -> Option<&'static [&'static str]> {
        Some(&["login"])
    }

    fn status_args(&self) -> &'static [&'static str] {
        &["login", "status"]
    }

    fn read_login(&self, success: bool, stdout: &str, stderr: &str) -> Option<LoginState> {
        let said = format!("{stdout}\n{stderr}");
        let state = |logged_in: bool, detail: &str| Some(LoginState { logged_in: Some(logged_in), detail: detail.to_string() });
        if said.contains("Not logged in") {
            return state(false, "로그인 안 됨 — o agents login codex");
        }
        if success && said.contains("Logged in") {
            return state(true, "로그인됨");
        }
        None
    }

    fn clean_env(&self, home: &Path) -> (&'static str, String) {
        ("CODEX_HOME", home.join(".codex").to_string_lossy().into_owned())
    }

    /// Codex needs its CODEX_HOME folder to exist.
    fn prepare_profile(&self, home: &Path, _real_home: &Path) -> io::Result<()> {
        fs::create_dir_all(home.join(".codex"))
    }

    fn inspect_profile(&self, home: &Path, flag: &mut dyn FnMut(bool, String)) {
        let dir = home.join(".codex");
        let config = fs::read_to_string(dir.join("config.toml")).unwrap_or_default();
        flag(HOOK_OR_MCP.is_match(&config), "config.toml에 hooks 또는 mcp_servers가 있음".into());
        for name in ["hooks.json", "AGENTS.md", "AGENTS.override.md"] {
            flag(dir.join(name).exists(), format!("{}가 있음", dir.join(name).to_string_lossy()));
        }
        let skills: Vec<String> = fs::read_dir(dir.join("skills"))
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != ".system").collect())
            .unwrap_or_default();
        flag(!skills.is_empty(), format!("skills에 내장(.system) 밖 스킬이 있음: {}", skills.join(", ")));
    }

    fn usage_windows(&self) -> Option<Result<Vec<UsageWindow>, String>> {
        Some(app_server_windows())
    }
}

static HOOK_OR_MCP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(?:\[\[?\s*)?(?:mcp_servers|hooks)\b").unwrap());

const TIMEOUT: Duration = Duration::from_secs(20);

/// A short-lived, read-only app-server answers account/rateLimits/read (no model turn).
fn app_server_windows() -> Result<Vec<UsageWindow>, String> {
    let cli = resolve_cli("codex").ok_or("codex CLI를 찾지 못했습니다")?;
    let mut command = Command::new(&cli.command);
    command
        .args(&cli.prefix)
        .args(["-c", "features.plugins=false", "-s", "read-only", "-a", "never", "app-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The windows of the ChatGPT sign-in cards use, not of a leftover API key.
    super::without_api_keys(&mut command, ProviderId::Codex);
    let mut probe = Contained::spawn(&mut command).map_err(|e| e.to_string())?;
    let mut stdin = probe.child.stdin.take().ok_or("codex app-server stdin")?;
    let stdout = probe.child.stdout.take().ok_or("codex app-server stdout")?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut send = |message: Value| -> Result<(), String> {
        let mut body = serde_json::Map::new();
        body.insert("jsonrpc".into(), json!("2.0"));
        if let Value::Object(fields) = message {
            body.extend(fields);
        }
        writeln!(stdin, "{}", Value::Object(body)).and_then(|_| stdin.flush()).map_err(|e| e.to_string())
    };
    send(json!({ "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "project-o", "version": "0.0.1" } } }))?;

    let timeout = || "codex app-server가 시간 안에 답하지 않았습니다".to_string();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.checked_duration_since(Instant::now()).ok_or_else(timeout)?;
        let line = rx.recv_timeout(left).map_err(|_| timeout())?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        match message.get("id").and_then(Value::as_f64) {
            Some(1.0) => {
                send(json!({ "method": "initialized", "params": {} }))?;
                send(json!({ "id": 2, "method": "account/rateLimits/read", "params": {} }))?;
            }
            Some(2.0) => {
                if let Some(error) = message.get("error").filter(|e| !e.is_null()) {
                    let text = error.get("message").and_then(Value::as_str).unwrap_or("rate limit read failed");
                    return Err(text.to_string());
                }
                return Ok(parse_codex_rate_limits(message.get("result").unwrap_or(&Value::Null)));
            }
            _ => {}
        }
    }
}

fn window_name(minutes: Option<&Value>) -> String {
    match minutes.and_then(Value::as_f64) {
        Some(300.0) => "five_hour".into(),
        Some(10080.0) => "seven_day".into(),
        Some(m) => format!("{}m", number_text(m)),
        None => "window".into(),
    }
}

/// The `account/rateLimits/read` result: primary and secondary windows of the codex limit.
pub fn parse_codex_rate_limits(result: &Value) -> Vec<UsageWindow> {
    let Some(limits) = result.get("rateLimits").and_then(Value::as_object) else { return Vec::new() };
    [limits.get("primary"), limits.get("secondary")]
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let w = w.as_object()?;
            let used = w.get("usedPercent").and_then(Value::as_f64)?;
            let resets = w.get("resetsAt").and_then(Value::as_f64)?;
            Some(UsageWindow {
                provider: ProviderId::Codex,
                window: window_name(w.get("windowDurationMins")),
                group: None,
                used: used / 100.0,
                resets_at: iso_from_secs(resets),
            })
        })
        .collect()
}

static PATH_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\\/]+").unwrap());

fn read_stream(data: &Map<String, Value>) -> Vec<StreamSignal> {
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
