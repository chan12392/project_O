//! Codex: app-server for the read-only lead (read-only exec on a CLI without it), exec for
//! cards and the YOLO lead.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::{json, Map, Value};

use super::stream::{limit_from_text, num, short, short_value};
use super::{list_command, opt, strings, LaunchSpec, LeadTurn, ModelChoice, Official, Provider, ReadLine, TeamChannel};
use crate::agents::clean::LoginState;
use crate::agents::catalog::connection;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan, ProviderId};
use crate::sys::js::{coalesce, iso_from_secs, js_string, number_text};
use crate::sys::proc::{capture, Contained};
use crate::sys::resolve::resolve_cli;
use crate::usage::UsageWindow;

pub struct Codex;

impl Provider for Codex {
    /// The sandbox runs shell commands itself; the run folder is added so `o msg` can append.
    /// O never marks a project trusted (#35): Codex then skips the project's `.codex/` layers
    /// (config, MCP servers, hooks), and `--skip-git-repo-check` lets it run outside a Git repository.
    fn launch(&self, card: &Card, plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        let spec = connection("codex").headless;
        let mut args = strings(spec.fixed);
        args.extend(strings(&["-C", &plan.project, "--add-dir", team.run_dir, "--skip-git-repo-check"]));
        args.extend(opt(spec.model_flag, card.model.as_deref()));
        let effort = card.effort.as_deref().filter(|e| !e.is_empty()).map(|e| format!("model_reasoning_effort={e}"));
        args.extend(opt("-c", effort.as_deref()));
        args.push("-".into());
        LaunchSpec { bin: "codex".into(), args, stdin: Some(prompt), env }
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

    fn lead_efforts(&self) -> &'static [&'static str] {
        &["low", "medium", "high", "xhigh"]
    }

    /// The lead drives app-server over stdio; turn settings are sent through its protocol.
    /// A CLI without app-server gets the earlier read-only `exec` lead: a read-only sandbox in
    /// the project (it reads anywhere, writes nowhere), resumed with `exec resume <id>`.
    fn lead(&self, turn: &LeadTurn) -> Option<LaunchSpec> {
        // A trusted project's `.codex/` may add hooks; `--disable hooks` (`-c features.hooks=false`,
        // `codex app-server --help`, `codex exec --help`) outranks every config layer, so none runs
        // before a plan is approved.
        if turn.app_server {
            return Some(LaunchSpec { bin: "codex".into(), args: strings(&["app-server", "--disable", "hooks"]),
                stdin: None, env: BTreeMap::new() });
        }
        let head = strings(&["exec", "-s", "read-only", "-C", turn.project, "--skip-git-repo-check", "--disable", "hooks"]);
        Some(self.lead_with(head, turn))
    }

    /// No approvals and no sandbox. Codex has no flag that adds a hook for one run, so hooks
    /// are left on as they are for cards: a trusted project's own `.codex/hooks.json` (O's
    /// repositories run `o hook` there) still blocks protected-branch pushes and merges.
    fn yolo_lead(&self, turn: &LeadTurn) -> Option<LaunchSpec> {
        let head = strings(&["exec", "--dangerously-bypass-approvals-and-sandbox", "-C", turn.project, "--skip-git-repo-check"]);
        Some(self.lead_with(head, turn))
    }

    /// Without a sandbox there is nothing to open for `o msg`, so the run folder is not added.
    fn yolo_launch(&self, card: &Card, plan: &Plan, _team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> Option<LaunchSpec> {
        let mut args = strings(&["exec", "--json", "--dangerously-bypass-approvals-and-sandbox", "-C", &plan.project]);
        args.push("--skip-git-repo-check".into());
        args.extend(opt("-m", card.model.as_deref()));
        let effort = card.effort.as_deref().filter(|e| !e.is_empty()).map(|e| format!("model_reasoning_effort={e}"));
        args.extend(opt("-c", effort.as_deref()));
        args.push("-".into());
        Some(LaunchSpec { bin: "codex".into(), args, stdin: Some(prompt), env })
    }

    /// `codex debug models` (in `codex --help`) prints the CLI's model catalog as JSON.
    fn lead_models(&self) -> Result<Vec<ModelChoice>, String> {
        list_command("codex", connection("codex").models, &[]).map(|out| parse_codex_models(&out))
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

impl Codex {
    /// A lead turn after `head` (`exec` and its sandbox flags): a later turn resumes the session
    /// with `exec resume <id>`. Images go first (`-i` takes several values, so a flag must follow
    /// them), the prompt is read from stdin (`-`).
    fn lead_with(&self, mut args: Vec<String>, turn: &LeadTurn) -> LaunchSpec {
        let images: Vec<String> = turn.images.iter().flat_map(|path| ["-i".to_string(), path.clone()]).collect();
        let effort = turn.effort.filter(|e| !e.is_empty()).map(|e| format!("model_reasoning_effort={e}"));
        let mut tail = vec!["--json".to_string()];
        tail.extend(opt("-m", turn.model));
        tail.extend(opt("-c", effort.as_deref()));
        match turn.session.filter(|s| !s.is_empty()) {
            Some(session) => {
                args.push("resume".into());
                args.extend(images);
                args.extend(tail);
                args.push(session.to_string());
            }
            None => {
                args.extend(images);
                args.extend(tail);
            }
        }
        args.push("-".into());
        LaunchSpec { bin: "codex".into(), args, stdin: Some(turn.prompt.clone()), env: BTreeMap::new() }
    }
}

/// The arguments that ask the CLI whether it has `app-server` (`supports_app_server`).
pub const APP_SERVER_PROBE: [&str; 2] = ["app-server", "--help"];

/// The probe's launch, built by the lead's spawner like the turn itself.
pub fn app_server_probe() -> LaunchSpec {
    LaunchSpec { bin: "codex".into(), args: strings(&APP_SERVER_PROBE), stdin: None, env: BTreeMap::new() }
}

/// Versions before 0.166.0 have not been verified for the ephemeral model probe.
pub fn default_model_probe() -> LaunchSpec {
    LaunchSpec { bin: "codex".into(), args: strings(&["--version"]), stdin: None, env: BTreeMap::new() }
}

fn supports_ephemeral_version(version: &str) -> bool {
    let Some(version) = version.trim().strip_prefix("codex-cli ") else { return false };
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit())) { return false; }
    let Ok(parts) = parts.into_iter().map(str::parse::<u64>).collect::<Result<Vec<_>, _>>() else { return false };
    (parts[0], parts[1], parts[2]) >= (0, 166, 0)
}

pub fn supports_default_model_probe(mut command: Command) -> bool {
    let key = format!("default-model {:?} {:?}", command.get_program(), command.get_args().collect::<Vec<_>>());
    let mut probed = PROBED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = probed.get(&key) { return *known; }
    let supported = matches!(capture(&mut command, PROBE_TIMEOUT), Ok((status, stdout, _)) if status.success() && supports_ephemeral_version(&stdout));
    probed.insert(key, supported);
    supported
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Probe results per CLI command line (the resolved program and its arguments).
static PROBED: LazyLock<Mutex<HashMap<String, bool>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// Whether `command` (`codex app-server --help`, from `app_server_probe`) shows the
/// app-server subcommand's help: once per CLI command line per process, no shell, no window,
/// no stdin, within `PROBE_TIMEOUT`. A CLI without the subcommand reads `app-server` as a
/// prompt and prints the top-level help, so success alone is not enough. A failed or slow
/// probe counts as no app-server: the lead then uses the read-only `exec` launch.
pub fn supports_app_server(mut command: Command) -> bool {
    let key = format!("{:?} {:?}", command.get_program(), command.get_args().collect::<Vec<_>>());
    let mut probed = PROBED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = probed.get(&key) {
        return *known;
    }
    let supported = matches!(capture(&mut command, PROBE_TIMEOUT),
        Ok((status, stdout, _)) if status.success() && stdout.lines().any(|line| line.contains("Usage:") && line.contains("app-server")));
    probed.insert(key, supported);
    supported
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

/// The `codex debug models` catalog: the models it lists in its own picker
/// (`visibility: "list"`), in its order (`priority`, lowest first).
pub fn parse_codex_models(stdout: &str) -> Vec<ModelChoice> {
    let Ok(data) = serde_json::from_str::<Value>(stdout) else { return Vec::new() };
    let Some(models) = data.get("models").and_then(Value::as_array) else { return Vec::new() };
    let mut listed: Vec<(f64, ModelChoice)> = models
        .iter()
        .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
        .filter_map(|m| {
            let id = m.get("slug").and_then(Value::as_str)?.to_string();
            let label = m.get("display_name").and_then(Value::as_str).filter(|l| !l.trim().is_empty()).unwrap_or(&id).trim().to_string();
            Some((m.get("priority").and_then(Value::as_f64).unwrap_or(f64::MAX), ModelChoice { id, label, hint: None }))
        })
        .collect();
    listed.sort_by(|a, b| a.0.total_cmp(&b.0));
    listed.into_iter().map(|(_, m)| m).collect()
}

static PATH_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\\/]+").unwrap());

fn read_stream(data: &Map<String, Value>) -> Vec<StreamSignal> {
    let kind = data.get("type").and_then(Value::as_str);
    if kind == Some("o.approval_denied") {
        return vec![StreamSignal::Tool { name: "권한".into(), summary: "총괄 모드에서 승인 요청을 거절했습니다".into() }];
    }
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
                if item.get("status").and_then(Value::as_str) == Some("declined") {
                    return tool("권한", "명령 실행 요청이 거절됐습니다".into());
                }
                let command = coalesce([item.get("command")]).map(js_string).unwrap_or_default();
                return tool("shell", short(&unwrap_shell(&command)));
            }
            Some("file_change") => {
                if item.get("status").and_then(Value::as_str) == Some("declined") {
                    return tool("권한", "파일 변경 요청이 거절됐습니다".into());
                }
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
                                // app-server (v2) sends `{"type":"add"}`; exec sends the bare string.
                                if let Some(kind) = c.get("kind") {
                                    let kind = kind.get("type").filter(|t| t.is_string()).unwrap_or(kind);
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
    if kind == Some("turn.usage") {
        if let Some(u) = data.get("usage").and_then(Value::as_object) {
            let cached = num(u.get("cached_input_tokens"));
            return vec![StreamSignal::Usage { input: (num(u.get("input_tokens")) - cached).max(0.0), cached, output: num(u.get("output_tokens")), complete: super::stream::reported(&[u.get("input_tokens"), u.get("cached_input_tokens"), u.get("output_tokens")]) && cached <= num(u.get("input_tokens")) }];
        }
    }
    if kind == Some("turn.completed") {
        let mut out = Vec::new();
        if let Some(u) = data.get("usage").and_then(Value::as_object) {
            let cached = num(u.get("cached_input_tokens"));
            out.push(StreamSignal::Usage { input: (num(u.get("input_tokens")) - cached).max(0.0), cached, output: num(u.get("output_tokens")), complete: super::stream::reported(&[u.get("input_tokens"), u.get("cached_input_tokens"), u.get("output_tokens")]) && cached <= num(u.get("input_tokens")) });
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

#[cfg(test)]
mod default_model_version_tests {
    use super::supports_ephemeral_version;

    #[test]
    fn only_known_or_newer_release_versions_enable_the_probe() {
        assert!(supports_ephemeral_version("codex-cli 0.166.0\n"));
        assert!(supports_ephemeral_version("codex-cli 0.167.1"));
        assert!(!supports_ephemeral_version("codex-cli 0.165.9"));
        assert!(!supports_ephemeral_version("codex-cli 0.166.0-alpha"));
        assert!(!supports_ephemeral_version("not codex"));
    }
}
