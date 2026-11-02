//! Claude Code: `claude -p` with a stream-json stdout, the prompt on stdin.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::stream::{limit_from_text, num, tool_summary};
use super::{opt, strings, LaunchSpec, Official, Provider, ReadLine, TeamChannel, TEAM_COMMAND};
use crate::agents::clean::{has_content, is_blank, read_json, LoginState, INSTRUCTION_FILES};
use crate::context::CONTEXT_COMMAND;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan};
use crate::run::store::to_pretty;
use crate::sys::js::{coalesce, iso_from_secs, js_string};

pub struct Claude;

impl Provider for Claude {
    fn launch(&self, card: &Card, _plan: &Plan, _team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        let mut args = strings(&["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--allowedTools"]);
        args.push(format!("Bash({TEAM_COMMAND} *)"));
        args.push(format!("Bash({CONTEXT_COMMAND} *)"));
        args.extend(opt("--model", card.model.as_deref()));
        args.extend(opt("--effort", card.effort.as_deref()));
        LaunchSpec { bin: "claude".into(), args, stdin: Some(prompt), env, env_remove: strings(self.api_key_vars()) }
    }

    /// `claude -p` always uses these when present (code.claude.com/docs/en/authentication).
    fn api_key_vars(&self) -> &'static [&'static str] {
        &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]
    }

    /// Headless runs cannot answer permission prompts, so a denied command ends the turn.
    fn tool_note(&self) -> Option<String> {
        Some(format!("셸 명령은 승인할 사람이 없어 거부될 수 있습니다({TEAM_COMMAND}·{CONTEXT_COMMAND}만 허용). 파일 읽기·쓰기 도구를 우선 쓰세요."))
    }

    fn searches_context(&self) -> bool {
        true
    }

    fn stream_reader(&self) -> ReadLine {
        Box::new(read_stream)
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

fn read_stream(data: &Map<String, Value>) -> Vec<StreamSignal> {
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
        // Usually a rejected rate_limit_event comes first; the text still catches a limit without one.
        out.extend(limit_from_text(&text, ok));
        out.push(StreamSignal::Final { ok, text });
        return out;
    }
    Vec::new()
}
