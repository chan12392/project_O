//! Claude Code: `claude -p` with a stream-json stdout, the prompt on stdin.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::stream::{error_text, limit_from_text, num, tool_summary};
use super::{opt, strings, LaunchSpec, LeadTurn, ModelChoice, Official, Provider, ReadLine, TeamChannel, AGENT_ONLY_COMMANDS, TEAM_COMMAND};
use crate::agents::clean::{has_content, is_blank, read_json, LoginState, INSTRUCTION_FILES};
use crate::agents::catalog::connection;
use crate::context::CONTEXT_COMMAND;
use crate::lead::approvals;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan};
use crate::run::store::to_pretty;
use crate::sys::js::{coalesce, iso_from_secs, js_string};

pub struct Claude;

/// The lead's whole tool set: reading files and searching them.
const LEAD_TOOLS: &str = "Read,Grep,Glob";

/// The lead run's own settings: no hook runs (project settings could turn them back on,
/// which `--setting-sources user` keeps them from doing).
fn lead_settings() -> String {
    let deny: Vec<String> = o_guard::rules::CREDENTIAL_DENY_PATHS.iter().map(|path| format!("Read({path})")).collect();
    json!({ "disableAllHooks": true, "permissions": { "deny": deny } }).to_string()
}

/// The tools O's guard hook (`o hook`) looks at: the same list as this repository's
/// `.claude/settings.json`.
const GUARD_MATCHER: &str = "Bash|PowerShell|Write|Edit|MultiEdit|NotebookEdit|Read|Grep|Glob";

/// Use the O sidecar beside this host directly, so a repository/PATH shadow cannot replace
/// the guard. Claude's exec-form hook takes argv without passing a path through a shell.
fn guard_settings() -> String {
    let program = std::env::current_exe().map(|exe| exe.with_file_name(if cfg!(windows) { "o.exe" } else { "o" }).to_string_lossy().into_owned()).unwrap_or_default();
    json!({ "hooks": { "PreToolUse": [{ "matcher": GUARD_MATCHER, "hooks": [{ "type": "command", "command": program, "args": ["hook"], "timeout": 10, "onFailure": "block" }] }] } })
        .to_string()
}

pub const PROJECT_SETTINGS_NOTICE: &str = "Claude 카드는 프로젝트 설정·훅을 읽지 않고 O 가드를 적용합니다";

/// Deny rules for the commands no agent may run; they hold in every permission mode.
fn denied_commands() -> Vec<String> {
    AGENT_ONLY_COMMANDS.iter().map(|c| format!("Bash({c}*)")).collect()
}

/// The same commands denied in either shell tool an approving lead has.
fn denied_shell_commands() -> Vec<String> {
    let mut rules = denied_commands();
    if cfg!(windows) { rules.extend(AGENT_ONLY_COMMANDS.iter().map(|c| format!("PowerShell({c}*)"))); }
    rules
}

/// An approving lead's tools: the read ones, and those that wait for the person. Claude Code
/// has its PowerShell tool on Windows only.
fn approved_tools() -> Vec<&'static str> {
    let mut tools = vec!["Read", "Grep", "Glob", "Edit", "Write", "Bash"];
    if cfg!(windows) { tools.push("PowerShell"); }
    tools.extend(["WebSearch", "WebFetch"]);
    tools
}

/// The tools an approving lead's own settings always ask for (`permissions.ask` wins over an
/// allow rule of the person's settings), with hooks off as in the read-only lead.
const ASKED_TOOLS: [&str; 7] = ["Bash", "PowerShell", "Edit", "Write", "NotebookEdit", "WebFetch", "WebSearch"];

fn approval_settings() -> String {
    let mut settings: Value = serde_json::from_str(&lead_settings()).expect("lead settings");
    settings["permissions"]["ask"] = json!(ASKED_TOOLS);
    settings.to_string()
}

/// `--mcp-config` with O's approval server as the only server.
fn approval_server(command: &[String]) -> String {
    let (program, args) = command.split_first().map_or(("o", &[][..]), |(p, a)| (p.as_str(), a));
    json!({ "mcpServers": { approvals::SERVER: { "type": "stdio", "command": program, "args": args } } }).to_string()
}

/// The start of a YOLO launch: print mode, no permission prompts, the denied commands.
fn yolo_head() -> Vec<String> {
    let mut args = strings(&["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "bypassPermissions", "--disallowedTools"]);
    args.extend(denied_commands());
    args
}

/// The model aliases `claude --model` documents, with the version each stands for now:
/// (alias, family, version, model id). Claude Code prints no alias→model mapping
/// (`claude --help` and `claude --version` name the aliases only, there is no models
/// command), so this table is kept by hand. **Update it when Claude Code's aliases move to
/// a new model** (code.claude.com/docs/en/model-config). `--model` still gets the alias, so
/// a stale version here only mislabels the menu; it never picks another model.
const LEAD_MODELS: [(&str, &str, &str, &str); 4] = [
    ("fable", "Fable", "5.1", "claude-fable-5-1"),
    ("opus", "Opus", "5.5", "claude-opus-5-5"),
    ("sonnet", "Sonnet", "5.5", "claude-sonnet-5-5"),
    ("haiku", "Haiku", "5.5", "claude-haiku-5-5"),
];

impl Provider for Claude {
    fn launch(&self, card: &Card, _plan: &Plan, _team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        let spec = connection("claude").headless;
        let mut args = strings(spec.fixed);
        args.push(format!("Bash({TEAM_COMMAND} *)"));
        args.push(format!("Bash({CONTEXT_COMMAND} *)"));
        // Print mode runs project hooks without a trust prompt, with either sign-in method.
        args.extend(strings(&["--setting-sources", "user", "--settings", &guard_settings(), "--strict-mcp-config", "--disallowedTools"]));
        args.extend(denied_shell_commands());
        args.extend(opt(spec.model_flag, card.model.as_deref()));
        args.extend(opt("--effort", card.effort.as_deref()));
        LaunchSpec { bin: "claude".into(), args, stdin: Some(prompt), env }
    }

    /// Headless runs cannot answer permission prompts, so a denied command ends the turn.
    fn tool_note(&self) -> Option<String> {
        Some(format!("셸 명령은 승인할 사람이 없어 거부될 수 있습니다({TEAM_COMMAND}·{CONTEXT_COMMAND}만 허용). 파일 읽기·쓰기 도구를 우선 쓰세요."))
    }

    /// The Agent tool asks no permission; its subagents keep the card's permission mode.
    fn subagent_tool(&self) -> Option<&'static str> {
        Some("Agent 도구")
    }

    fn searches_context(&self) -> bool {
        true
    }

    fn stream_reader(&self) -> ReadLine {
        let mut reset_given = false;
        let mut calls = HashMap::new();
        Box::new(move |data| read_stream(&mut reset_given, &mut calls, data))
    }

    fn lead_efforts(&self) -> &'static [&'static str] {
        &["low", "medium", "high", "xhigh", "max"]
    }

    /// Only the read tools exist (`--tools`), they are the only ones allowed without a prompt,
    /// the permission mode is the default one (a headless run denies anything else), and no
    /// MCP server is loaded. The attachment folder is added for reading.
    /// `claude -p` never asks to trust a folder, yet runs the hooks in its settings files
    /// (code.claude.com/docs/en/permissions#what-runs-before-you-trust-a-folder), so the
    /// project's settings are not read (`--setting-sources user`; the clean profile's
    /// settings are the user ones) and hooks are off for the run, first turn or resumed.
    ///
    /// With an approver (ADR-0008 개정 3) the write, shell and web tools exist too, but only the
    /// read tools are allowed without a prompt; every other call goes to O's approval server
    /// (`--permission-prompt-tool`, the only MCP server loaded), and the run's settings ask for
    /// those tools so no allow rule of the person's own settings lets one through unasked.
    fn lead(&self, turn: &LeadTurn) -> Option<LaunchSpec> {
        let mut args = strings(&["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "default"]);
        match &turn.approver {
            None => {
                args.extend(strings(&["--tools", LEAD_TOOLS, "--allowedTools", LEAD_TOOLS, "--strict-mcp-config"]));
                args.extend(strings(&["--setting-sources", "user", "--settings", &lead_settings()]));
            }
            Some(command) => {
                args.extend(strings(&["--tools", &approved_tools().join(","), "--allowedTools", LEAD_TOOLS, "--disallowedTools"]));
                args.extend(denied_shell_commands());
                args.extend(strings(&["--strict-mcp-config", "--mcp-config", &approval_server(command), "--permission-prompt-tool", &approvals::prompt_tool()]));
                args.extend(strings(&["--setting-sources", "user", "--settings", &approval_settings()]));
            }
        }
        args.extend(opt("--resume", turn.session));
        args.extend(opt("--model", turn.model));
        args.extend(opt("--effort", turn.effort));
        args.extend(opt("--add-dir", turn.files_dir));
        Some(LaunchSpec { bin: "claude".into(), args, stdin: Some(turn.prompt.clone()), env: BTreeMap::new() })
    }

    /// Every tool, no permission prompt (`bypassPermissions`, the same as Claude's
    /// skip-permissions flag); allow rules have no effect there, so `--allowedTools` goes.
    /// Deny rules still hold, and O's guard hook comes with the run's settings.
    fn yolo_launch(&self, card: &Card, _plan: &Plan, _team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> Option<LaunchSpec> {
        let mut args = yolo_head();
        args.extend(strings(&["--setting-sources", "user", "--settings", &guard_settings(), "--strict-mcp-config"]));
        args.extend(opt("--model", card.model.as_deref()));
        args.extend(opt("--effort", card.effort.as_deref()));
        Some(LaunchSpec { bin: "claude".into(), args, stdin: Some(prompt), env })
    }

    /// The lead with every tool and no prompts. The project's settings stay unread (as in
    /// `lead`); instead of turning hooks off, the run's settings bring O's guard hook.
    fn yolo_lead(&self, turn: &LeadTurn) -> Option<LaunchSpec> {
        let mut args = yolo_head();
        args.push("--strict-mcp-config".into());
        args.extend(strings(&["--setting-sources", "user", "--settings", &guard_settings()]));
        args.extend(opt("--resume", turn.session));
        args.extend(opt("--model", turn.model));
        args.extend(opt("--effort", turn.effort));
        args.extend(opt("--add-dir", turn.files_dir));
        Some(LaunchSpec { bin: "claude".into(), args, stdin: Some(turn.prompt.clone()), env: BTreeMap::new() })
    }

    /// Claude Code has no model list command; `--model` takes these aliases for the latest
    /// model of each family (`claude --help`, code.claude.com/docs/en/model-config).
    fn lead_models(&self) -> Result<Vec<ModelChoice>, String> {
        Ok(LEAD_MODELS
            .iter()
            .map(|(alias, family, version, model)| ModelChoice { id: alias.to_string(), label: format!("{family} {version}"), hint: Some(model.to_string()) })
            .collect())
    }

    // https://code.claude.com/docs/en/setup (native installer, WinGet, npm)
    fn official(&self) -> Official {
        Official {
            name: "Claude Code",
            npm_package: Some("@anthropic-ai/claude-code"),
            script: "irm https://claude.ai/install.ps1 | iex",
            url: "https://code.claude.com/docs/en/setup",
        }
    }

    fn login_args(&self) -> Option<&'static [&'static str]> {
        Some(&["auth", "login"])
    }

    fn status_args(&self) -> &'static [&'static str] {
        &["auth", "status"]
    }

    fn read_login(&self, _success: bool, stdout: &str, _stderr: &str) -> Option<LoginState> {
        let state = |logged_in: bool, detail: &str| Some(LoginState { logged_in: Some(logged_in), detail: detail.to_string() });
        match serde_json::from_str::<Value>(stdout.trim()).ok().and_then(|v| v.get("loggedIn").and_then(Value::as_bool)) {
            Some(true) => state(true, "로그인됨"),
            Some(false) => state(false, "로그인 안 됨 — o agents login claude"),
            None => None,
        }
    }

    fn clean_env(&self, home: &Path) -> (&'static str, String) {
        ("CLAUDE_CONFIG_DIR", home.join(".claude").to_string_lossy().into_owned())
    }

    /// Claude's only file in a clean profile: its settings.
    fn prepare_profile(&self, home: &Path, real_home: &Path) -> io::Result<()> {
        let settings = home.join(".claude").join("settings.json");
        if !settings.exists() {
            fs::create_dir_all(home.join(".claude"))?;
            fs::write(settings, to_pretty(&settings_for(real_home))?)?;
        }
        Ok(())
    }

    fn inspect_profile(&self, home: &Path, flag: &mut dyn FnMut(bool, String)) {
        let dir = home.join(".claude");
        match read_json(&dir.join("settings.json")) {
            None => flag(true, "settings.json이 없음 (홈 폴더 지시 파일 제외 설정이 빠짐)".into()),
            Some(Err(error)) => flag(true, error),
            Some(Ok(settings)) => {
                let keys: Vec<&String> = settings.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                flag(keys.iter().any(|k| *k == "hooks"), "settings.json에 hooks가 있음".into());
                let extra: Vec<&str> = keys.iter().map(|k| k.as_str()).filter(|k| *k != "claudeMdExcludes" && *k != "hooks").collect();
                flag(!extra.is_empty(), format!("settings.json에 예상 밖 설정: {}", extra.join(", ")));
            }
        }
        for name in ["skills", "plugins", "agents", "commands", "rules"] {
            flag(has_content(&dir.join(name)), format!("{} 폴더에 내용이 있음", dir.join(name).to_string_lossy()));
        }
        for name in ["CLAUDE.md", "AGENTS.md"] {
            flag(dir.join(name).exists(), format!("전역 지시 파일이 있음: {}", dir.join(name).to_string_lossy()));
        }
        for state in [dir.join(".claude.json"), home.join(".claude.json")] {
            match read_json(&state) {
                Some(Ok(value)) => flag(mcp_in_state(&value), format!("{}에 mcpServers가 있음", state.to_string_lossy())),
                Some(Err(error)) => flag(true, error),
                None => {}
            }
        }
    }
}

/// Claude's only settings in a clean profile: skip the real home's instruction
/// files, which it would otherwise read for any project under the home folder.
/// Claude matches these globs against `/`-separated absolute paths; a linked
/// file is listed by its target as well.
fn settings_for(real_home: &Path) -> Value {
    let slashes = |p: &Path| p.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    let mut excludes = Vec::new();
    for name in INSTRUCTION_FILES {
        let file = real_home.join(name);
        excludes.push(slashes(&file));
        if let Ok(target) = fs::canonicalize(&file) {
            let target = slashes(&target);
            if !excludes.contains(&target) {
                excludes.push(target);
            }
        }
    }
    excludes.push(format!("{}/.claude/rules/**", slashes(real_home).trim_end_matches('/')));
    json!({ "claudeMdExcludes": excludes })
}

fn mcp_in_state(state: &Value) -> bool {
    let servers = |v: &Value| v.get("mcpServers").is_some_and(|s| !is_blank(s));
    servers(state) || state.get("projects").and_then(Value::as_object).is_some_and(|p| p.values().any(servers))
}

/// Tool calls whose result is still awaited, at most.
const MAX_OPEN_CALLS: usize = 256;

/// `reset_given`: a rejected `rate_limit_event` already gave this card's reset time, so the
/// failure text that follows must not set one of its own.
///
/// `calls`: the tool calls still waiting for their result (id → name, summary), so a result
/// that reports an error can say which call it was.
fn read_stream(reset_given: &mut bool, calls: &mut HashMap<String, (String, String)>, data: &Map<String, Value>) -> Vec<StreamSignal> {
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
                        let summary = tool_summary(part.get("input"));
                        if let Some(id) = part.get("id").and_then(Value::as_str) {
                            // Results that never came must not pile up over a long card.
                            if calls.len() >= MAX_OPEN_CALLS { calls.clear(); }
                            calls.insert(id.to_string(), (name.to_string(), summary.clone()));
                        }
                        out.push(StreamSignal::Tool { name: name.to_string(), summary });
                    }
                }
            }
            return out;
        }
    }
    // Tool results come back as a user message; only a failed one is a signal.
    if kind == Some("user") {
        let content = data.get("message").and_then(Value::as_object).and_then(|m| m.get("content")).and_then(Value::as_array);
        let mut out = Vec::new();
        for part in content.into_iter().flatten().filter_map(Value::as_object).filter(|p| p.get("type").and_then(Value::as_str) == Some("tool_result")) {
            let Some((name, summary)) = part.get("tool_use_id").and_then(Value::as_str).and_then(|id| calls.remove(id)) else { continue };
            if part.get("is_error") != Some(&Value::Bool(true)) { continue; }
            let error = match part.get("content") {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
                _ => String::new(),
            };
            out.push(StreamSignal::ToolError { name, summary, error: error_text(&error) });
        }
        return out;
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
                    *reset_given = true;
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
                complete: super::stream::reported(&[u.get("input_tokens"), u.get("cache_creation_input_tokens"), u.get("cache_read_input_tokens"), u.get("output_tokens")]),
            });
        }
        let text = coalesce([data.get("result")]).map(js_string).unwrap_or_default().trim().to_string();
        // Usually a rejected rate_limit_event comes first; the text still catches a limit without one.
        // After that event its reset time stands: a wait read from the wording ("try again in
        // 1 hour", rounded or repeated late) must not stretch it.
        if !*reset_given {
            out.extend(limit_from_text(&text, ok));
        }
        out.push(StreamSignal::Final { ok, text });
        return out;
    }
    Vec::new()
}
