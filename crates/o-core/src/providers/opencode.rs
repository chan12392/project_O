//! OpenCode v2 (anomalyco, open source; ADR-0015): `opencode run --format json --standalone`,
//! the prompt on stdin. It never joins the background service (`--standalone`), so the
//! profile and environment O chose are the ones that run the card.
//!
//! Permissions live in OpenCode's config, not in `run` flags (`opencode run --help` has no
//! settings option), so the clean profile's `opencode.json` is what keeps a card at
//! "edits in the project, no other approval": every action is denied first, then reading,
//! searching and editing are allowed, and the shell only for `o msg …` and `o context …`
//! (OpenCode applies the last matching rule). `--auto` (approve everything not denied) is
//! the YOLO flag and is never part of a card's launch here.
//!
//! OpenCode uses its own configured providers (OpenCode Console/Go, OpenRouter, a person's
//! own API keys). A Claude subscription must not be connected to it (Anthropic does not
//! allow Claude.ai sign-in in third-party apps), and O never passes Claude credentials.
//! It has no usage probe, so the daily budget (ADR-0004) does not apply to it.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::stream::{is_limit_text, num, short, tool_summary};
use super::{list_command, strings, LaunchSpec, ModelChoice, Official, Provider, ReadLine, TeamChannel, TEAM_COMMAND};
use crate::agents::clean::{has_content, is_blank, read_json, LoginState};
use crate::agents::catalog::connection;
use crate::context::CONTEXT_COMMAND;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan, ProviderId};
use crate::run::store::to_pretty;
use crate::sys::{proc, resolve::resolve_cli};

pub struct OpenCode;

/// 공식 상태 명령의 연결 종류만 확인하며 출력은 저장하지 않는다.
pub fn check_auth(root: Option<&Path>) -> Result<(), String> {
    crate::sys::resolve::ensure_paths();
    let cli = resolve_cli("opencode").ok_or("OpenCode CLI를 찾지 못했습니다")?;
    let mut command = Command::new(cli.command);
    command.args(cli.prefix).args(["auth", "list", "--format", "json", STANDALONE]);
    if let Some(root) = root {
        command.env_clear().envs(crate::agents::clean::environment(root, ProviderId::OpenCode).map_err(|e| e.to_string())?);
        command.current_dir(crate::agents::clean::profile_home(root, ProviderId::OpenCode));
    } else {
        command.env_clear().envs(crate::agents::clean::own_profile_environment());
    }
    let (status, output, _) = proc::capture(&mut command, Duration::from_secs(30)).map_err(|_| "OpenCode 연결 상태를 확인하지 못했습니다")?;
    if !status.success() { return Err("OpenCode 연결 상태를 확인하지 못했습니다".into()); }
    check_auth_list(&output)
}

pub fn check_auth_list(output: &str) -> Result<(), String> {
    let entries: Value = serde_json::from_str(output).map_err(|_| "OpenCode 연결 상태를 해석하지 못했습니다")?;
    let entries = entries.as_array().ok_or("OpenCode 연결 상태를 해석하지 못했습니다")?;
    for entry in entries {
        let id = entry.get("id").and_then(Value::as_str).ok_or("OpenCode 연결 상태를 해석하지 못했습니다")?.to_ascii_lowercase();
        let connections = entry.get("connections").and_then(Value::as_array).ok_or("OpenCode 연결 상태를 해석하지 못했습니다")?;
        if (id.contains("anthropic") || id.contains("claude")) && connections.iter().any(|connection| connection.get("method").and_then(Value::as_str) != Some("key")) {
            return Err("OpenCode의 Claude 구독 연결은 사용할 수 없습니다. 공식 연결 설정에서 해제해 주세요".into());
        }
    }
    Ok(())
}

/// A private server for this one command instead of the person's background service.
const STANDALONE: &str = "--standalone";

/// The YOLO flag (`opencode run --help`: "Auto-approve permissions that are not explicitly denied").
pub const YOLO: &[&str] = &["--auto"];

/// Shell text that would chain, pipe or redirect after an allowed `o msg`/`o context` prefix.
/// The allow rules match the whole command with `*`, so these deny rules come after them.
const SHELL_JOINS: [&str; 9] = ["*&*", "*|*", "*;*", "*>*", "*<*", "*`*", "*$(*", "*\n*", "*\r*"];

impl Provider for OpenCode {
    /// `run` with JSON events and a private server; the model as `provider/model`, the effort as
    /// its variant (`provider/model#high`, `opencode run --help`). OpenCode checks the variant;
    /// without a model there is no variant to name, so the effort is left to the CLI's default.
    fn launch(&self, card: &Card, _plan: &Plan, _team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        let spec = connection("opencode").headless;
        let mut args = strings(spec.fixed);
        if let Some(model) = card.model.as_deref().filter(|m| !m.is_empty()) {
            let model = match card.effort.as_deref().filter(|e| !e.is_empty()) {
                Some(effort) => format!("{model}#{effort}"),
                None => model.to_string(),
            };
            args.extend([spec.model_flag.to_string(), model]);
        }
        LaunchSpec { bin: "opencode".into(), args, stdin: Some(prompt), env, env_remove: strings(self.api_key_vars()) }
    }

    /// OpenCode reads provider keys from the environment (`{env:…}` in its config). Claude's are
    /// taken out so no Claude credential ever reaches it (ADR-0015); OpenAI's and OpenRouter's
    /// so a leftover key does not quietly bill an API.
    fn api_key_vars(&self) -> &'static [&'static str] {
        &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"]
    }

    fn tool_note(&self) -> Option<String> {
        Some(format!(
            "셸 명령은 {TEAM_COMMAND}·{CONTEXT_COMMAND}만 허용됩니다(project_O 프로필 기준, 이어 붙이기·파이프·리디렉션 없이). 파일 읽기·쓰기 도구를 우선 쓰세요."
        ))
    }

    fn searches_context(&self) -> bool {
        true
    }

    fn stream_reader(&self) -> ReadLine {
        let mut session = false;
        let mut answer = String::new();
        Box::new(move |data| read_stream(&mut session, &mut answer, data))
    }

    fn yolo_launch(&self, card: &Card, plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> Option<LaunchSpec> {
        let mut spec = self.launch(card, plan, team, prompt, env);
        spec.args.extend(strings(YOLO));
        Some(spec)
    }

    fn daily_budget(&self) -> bool {
        false
    }

    /// `opencode models --standalone`: one `provider/model` per line.
    fn card_models(&self) -> Option<Result<Vec<ModelChoice>, String>> {
        Some(list_command("opencode", connection("opencode").models, ProviderId::OpenCode, &[]).map(|out| parse_models(&out)))
    }

    // https://opencode.ai/v2/docs/ (v2 is the npm package @opencode/cli; v1's opencode-ai is another package)
    fn official(&self) -> Official {
        // No Windows installer script ("Windows package managers are not supported"): npm, or the page's zip.
        Official {
            name: "OpenCode",
            npm_package: Some("@opencode/cli"),
            script: "공식 안내(https://opencode.ai/v2/docs/)의 Windows zip을 받거나, Node.js를 설치한 뒤 npm install -g @opencode/cli",
            url: "https://opencode.ai/v2/docs/",
        }
    }

    fn login_args(&self) -> Option<&'static [&'static str]> {
        Some(&["auth", "login", STANDALONE])
    }

    /// The saved integrations, as JSON. Never `auth export`: it prints the secrets.
    fn status_args(&self) -> &'static [&'static str] {
        &["auth", "list", "--format", "json", STANDALONE]
    }

    fn read_login(&self, success: bool, stdout: &str, _stderr: &str) -> Option<LoginState> {
        read_auth_list(success, stdout)
    }

    /// OpenCode follows HOME for every folder (config, data, state, cache); its config folder is
    /// named as well so the one O writes is the one it reads.
    fn clean_env(&self, home: &Path) -> (&'static str, String) {
        ("XDG_CONFIG_HOME", home.join(".config").to_string_lossy().into_owned())
    }

    /// The profile's only file: O's permission rules, no plugin and no MCP server.
    fn prepare_profile(&self, home: &Path, _real_home: &Path) -> io::Result<()> {
        let dir = config_dir(home);
        let file = dir.join("opencode.json");
        if !file.exists() {
            fs::create_dir_all(&dir)?;
            fs::write(file, to_pretty(&profile_config())?)?;
        }
        Ok(())
    }

    fn inspect_profile(&self, home: &Path, flag: &mut dyn FnMut(bool, String)) {
        let dir = config_dir(home);
        for name in ["opencode.json", "opencode.jsonc"] {
            let path = dir.join(name);
            match read_json(&path) {
                Some(Ok(config)) => {
                    let shown = path.to_string_lossy();
                    for key in ["plugins", "plugin", "mcp", "agents", "agent"] {
                        flag(config.get(key).is_some_and(|v| !is_blank(v)), format!("{shown}에 {key}가 있음"));
                    }
                    flag(allows_everything(&config), format!("{shown}에 모든 동작을 허용하는 규칙이 있음"));
                }
                Some(Err(error)) => flag(true, error),
                None => {}
            }
        }
        flag(dir.join("AGENTS.md").exists(), format!("전역 지시 파일이 있음: {}", dir.join("AGENTS.md").to_string_lossy()));
        for name in ["plugins", "plugin", "skills", "agents", "agent", "commands", "command"] {
            flag(has_content(&dir.join(name)), format!("{} 폴더에 내용이 있음", dir.join(name).to_string_lossy()));
        }
    }
}

fn config_dir(home: &Path) -> std::path::PathBuf {
    home.join(".config").join("opencode")
}

/// The clean profile's `opencode.json`: deny everything, then allow reading, searching and
/// editing, and the shell only for the team and context commands with nothing joined to them.
pub fn profile_config() -> Value {
    let rule = |action: &str, resource: &str, effect: &str| json!({ "action": action, "resource": resource, "effect": effect });
    let mut rules = vec![rule("*", "*", "deny")];
    rules.extend(["read", "glob", "grep", "edit"].map(|action| rule(action, "*", "allow")));
    rules.push(rule("shell", &format!("{TEAM_COMMAND} *"), "allow"));
    rules.push(rule("shell", &format!("{CONTEXT_COMMAND} *"), "allow"));
    rules.extend(SHELL_JOINS.map(|join| rule("shell", join, "deny")));
    json!({ "$schema": "https://opencode.ai/config.json", "permissions": rules, "plugins": [] })
}

/// A rule allowing every action on every resource (this PC's own OpenCode config has one).
fn allows_everything(config: &Value) -> bool {
    let rules = config.get("permissions").and_then(Value::as_array);
    rules.is_some_and(|rules| {
        rules.iter().any(|r| {
            let text = |k: &str| r.get(k).and_then(Value::as_str);
            text("action") == Some("*") && text("resource").is_none_or(|v| v == "*") && text("effect") == Some("allow")
        })
    })
}

/// `opencode auth list --format json`: integrations, each with its saved connections. Only the
/// count is kept (names and labels can name the account). A Claude sign-in connected to OpenCode
/// is named in the detail: Anthropic does not allow it in third-party apps.
pub fn read_auth_list(success: bool, stdout: &str) -> Option<LoginState> {
    let list = serde_json::from_str::<Value>(stdout.trim()).ok()?;
    let list = list.as_array()?;
    let connections = |entry: &Value| entry.get("connections").and_then(Value::as_array).map_or(0, Vec::len);
    let count: usize = list.iter().map(connections).sum();
    if !success && count == 0 {
        return None;
    }
    let claude = list.iter().any(|entry| {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or_default().to_ascii_lowercase();
        (id.contains("anthropic") || id.contains("claude")) && connections(entry) > 0
    });
    let detail = match (count, claude) {
        (0, _) => "로그인 안 됨 — o agents login opencode".to_string(),
        (n, false) => format!("로그인됨 (OpenCode 연결 {n}개)"),
        (n, true) => format!("로그인됨 (OpenCode 연결 {n}개) — 주의: Anthropic(Claude) 연결이 있습니다. Claude 구독 로그인은 약관상 OpenCode에 쓰면 안 됩니다"),
    };
    Some(LoginState { logged_in: Some(count > 0), detail })
}

/// `opencode models` lines: `provider/model`, nothing else.
pub fn parse_models(stdout: &str) -> Vec<ModelChoice> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| line.contains('/') && !line.contains(char::is_whitespace))
        .map(|id| ModelChoice { id: id.to_string(), label: id.to_string(), hint: None })
        .collect()
}

/// `opencode run --format json` (v2.1.4, recorded in `crates/o-core/tests/providers.rs`): one
/// event per line, each with `type` and `sessionID`: `step_start`, `tool_use` (a finished tool
/// call), `step_finish` (the step's tokens), `text` (an answer part) and `error`. There is no
/// closing event and the last step's `step_finish` may not come, so every answer text is the
/// card's result so far (the runner keeps the last) and the exit code says whether it ended well.
fn read_stream(session: &mut bool, answer: &mut String, data: &Map<String, Value>) -> Vec<StreamSignal> {
    let mut out = Vec::new();
    if !*session {
        if let Some(id) = data.get("sessionID").and_then(Value::as_str).filter(|id| !id.is_empty()) {
            *session = true;
            out.push(StreamSignal::Session { session_id: id.to_string() });
        }
    }
    let part = data.get("part").and_then(Value::as_object);
    match data.get("type").and_then(Value::as_str) {
        Some("text") => {
            if let Some(text) = part.and_then(|p| p.get("text")).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) {
                out.push(StreamSignal::Message { text: text.to_string() });
                if !answer.is_empty() { answer.push('\n'); }
                answer.push_str(text);
                out.push(StreamSignal::Final { ok: true, text: answer.clone() });
            }
        }
        Some("tool_use") => {
            if let Some(part) = part {
                let name = part.get("tool").and_then(Value::as_str).unwrap_or("tool");
                let state = part.get("state").and_then(Value::as_object);
                let input = state.and_then(|s| s.get("input"));
                let mut summary = tool_summary(input);
                if summary.is_empty() {
                    let path = input.and_then(|i| i.get("path").or_else(|| i.get("filePath"))).and_then(Value::as_str);
                    summary = path.map(short).unwrap_or_default();
                }
                if state.and_then(|s| s.get("status")).and_then(Value::as_str) == Some("error") {
                    summary = short(&format!("거부·실패 {summary}"));
                }
                out.push(StreamSignal::Tool { name: name.to_string(), summary });
            }
        }
        Some("step_finish") => {
            if let Some(tokens) = part.and_then(|p| p.get("tokens")).and_then(Value::as_object) {
                let cache = tokens.get("cache").and_then(Value::as_object);
                out.push(StreamSignal::Usage {
                    input: num(tokens.get("input")) + num(cache.and_then(|c| c.get("write"))),
                    cached: num(cache.and_then(|c| c.get("read"))),
                    output: num(tokens.get("output")) + num(tokens.get("reasoning")),
                });
            }
        }
        Some("error") => {
            let error = data.get("error");
            let message = error.and_then(|e| e.get("message")).and_then(Value::as_str).unwrap_or_default();
            let kind = error.and_then(|e| e.get("type")).and_then(Value::as_str).unwrap_or_default();
            let text = if message.is_empty() { kind.to_string() } else { message.to_string() };
            if is_limit_text(&format!("{kind} {message}")) {
                out.push(StreamSignal::Limit { until: String::new() });
            }
            out.push(StreamSignal::Final { ok: false, text: if text.is_empty() { "OpenCode 오류".into() } else { text } });
        }
        _ => {}
    }
    out
}
