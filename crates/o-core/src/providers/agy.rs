//! Antigravity: `agy -p` with a stream-json stdout. Its allow rules match whole
//! commands only, so a card gets two fixed team commands and nothing else; it
//! signs in through the system, and `-p /usage` reports quota without a model turn.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Map, Value};

use super::stream::{limit_from_text, num, short, tool_summary};
use super::{opt, outbox_path, strings, LaunchSpec, Official, Provider, ReadLine, TeamChannel, TEAM_COMMAND};
use crate::agents::clean::{has_content, is_blank, read_json, LoginState};
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan, ProviderId};
use crate::sys::js::{coalesce, js_string};
use crate::sys::proc;
use crate::sys::resolve::resolve_cli;
use crate::usage::UsageWindow;

pub struct Agy;

/// No self-update while O runs it.
const NO_UPDATE: (&str, &str) = ("AGY_CLI_DISABLE_AUTO_UPDATE", "true");

/// agy's quota report in print mode: no model turn (agy 1.1.11 and later).
const USAGE: [&str; 6] = ["-p", "/usage", "--output-format", "json", "--print-timeout", "20s"];

impl Provider for Agy {
    fn launch(&self, card: &Card, _plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec {
        // agy allow-rules match whole commands only, so it gets two fixed ones and
        // writes what it sends to its outbox file first (see `o msg post`).
        let settings = json!({ "permissions": { "allow": [format!("command({TEAM_COMMAND} read)"), format!("command({TEAM_COMMAND} post)")] } });
        let mut args = strings(&["--output-format", "stream-json", "--mode", "accept-edits", "--add-dir", team.run_dir, "--settings"]);
        args.push(settings.to_string());
        args.extend(opt("--model", card.model.as_deref()));
        args.extend(opt("--effort", card.effort.as_deref()));
        args.push("-p".into());
        args.push(prompt);
        LaunchSpec { bin: "agy".into(), args, stdin: None, env, env_remove: strings(self.api_key_vars()) }
    }

    /// agy signs in through the system; no key variable is taken out.
    fn api_key_vars(&self) -> &'static [&'static str] {
        &[]
    }

    fn tool_note(&self) -> Option<String> {
        Some(format!(
            "셸 명령은 \"{TEAM_COMMAND} read\"와 \"{TEAM_COMMAND} post\" 두 개만 그대로 실행할 수 있습니다(헤드리스 모드라 다른 명령은 승인 불가). 그 밖에는 파일 읽기·쓰기 도구만 쓰세요."
        ))
    }

    fn team_how_to(&self, card: &Card, team: &TeamChannel) -> Vec<String> {
        vec![
            format!(
                "보내기: {} 파일에 첫 줄은 \"to: <카드 id|lead|all>\", 다음 줄부터 내용을 쓰고, 셸에서 {TEAM_COMMAND} post 를 그대로 실행합니다.",
                outbox_path(team.run_dir, &card.id)
            ),
            format!("받기: 셸에서 {TEAM_COMMAND} read 를 그대로 실행합니다."),
        ]
    }

    /// A free question cannot fit a fixed allow rule, so `o context` stays closed.
    fn searches_context(&self) -> bool {
        false
    }

    fn stream_reader(&self) -> ReadLine {
        let mut text = String::new();
        Box::new(move |data| read_stream(&mut text, data))
    }

    // https://antigravity.google/docs/getting-started (installer script only)
    fn official(&self) -> Official {
        Official {
            name: "Antigravity",
            npm_package: None,
            script: "irm https://antigravity.google/cli/install.ps1 | iex",
            url: "https://antigravity.google/docs/getting-started",
        }
    }

    fn login_args(&self) -> Option<&'static [&'static str]> {
        None
    }

    /// Its quota report, empty when signed out.
    fn status_args(&self) -> &'static [&'static str] {
        &USAGE
    }

    fn read_login(&self, success: bool, stdout: &str, _stderr: &str) -> Option<LoginState> {
        // Signed out, agy has no quota to report.
        (success && !parse_agy_usage(stdout).is_empty()).then(|| LoginState { logged_in: Some(true), detail: "로그인됨 (시스템 로그인)".into() })
    }

    fn check_env(&self) -> &'static [(&'static str, &'static str)] {
        &[NO_UPDATE]
    }

    /// agy makes its own `.gemini` folder in the profile home.
    fn clean_env(&self, _home: &Path) -> (&'static str, String) {
        (NO_UPDATE.0, NO_UPDATE.1.into())
    }

    fn prepare_profile(&self, _home: &Path, _real_home: &Path) -> io::Result<()> {
        Ok(())
    }

    fn inspect_profile(&self, home: &Path, flag: &mut dyn FnMut(bool, String)) {
        let dir = home.join(".gemini");
        for name in ["hooks.json", "mcp_config.json"] {
            let path = dir.join("config").join(name);
            match read_json(&path) {
                Some(Ok(value)) => flag(!is_blank(&value), format!("{}가 비어 있지 않음", path.to_string_lossy())),
                Some(Err(error)) => flag(true, error),
                None => {}
            }
        }
        flag(has_content(&dir.join("skills")), format!("{} 폴더에 내용이 있음", dir.join("skills").to_string_lossy()));
        for name in ["GEMINI.md", "AGENTS.md"] {
            flag(dir.join(name).exists(), format!("전역 지시 파일이 있음: {}", dir.join(name).to_string_lossy()));
        }
    }

    fn usage_windows(&self) -> Option<Result<Vec<UsageWindow>, String>> {
        Some(usage_report())
    }
}

/// Print-mode /usage, within agy's own print timeout and a margin.
fn usage_report() -> Result<Vec<UsageWindow>, String> {
    let cli = resolve_cli("agy").ok_or("agy CLI를 찾지 못했습니다")?;
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).args(USAGE).env(NO_UPDATE.0, NO_UPDATE.1);
    let (status, out, _) = proc::capture(&mut command, Duration::from_secs(20 + 10)).map_err(|e| format!("agy /usage {e}"))?;
    if !status.success() {
        let code = proc::exit_code(status).map(|c| c.to_string()).unwrap_or_else(|| "시간 초과".into());
        return Err(format!("agy /usage 실패 ({code})"));
    }
    Ok(parse_agy_usage(&out))
}

/// The `agy -p /usage` JSON envelope: groups of buckets with a remaining fraction each.
pub fn parse_agy_usage(stdout: &str) -> Vec<UsageWindow> {
    let Ok(data) = serde_json::from_str::<Value>(stdout) else { return Vec::new() };
    let groups = data.get("command").and_then(|c| c.get("data")).and_then(|d| d.get("groups")).and_then(Value::as_array);
    let Some(groups) = groups else { return Vec::new() };
    let mut out = Vec::new();
    for group in groups.iter().filter_map(Value::as_object) {
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else { continue };
        for b in buckets.iter().filter_map(Value::as_object) {
            let (Some(remaining), Some(reset)) = (b.get("remaining_fraction").and_then(Value::as_f64), b.get("reset_time").and_then(Value::as_str)) else {
                continue;
            };
            if b.get("disabled") == Some(&Value::Bool(true)) {
                continue;
            }
            let window = match b.get("window").and_then(Value::as_str) {
                Some("5h") => "five_hour".to_string(),
                Some("weekly") => "seven_day".to_string(),
                _ => b.get("id").filter(|v| !v.is_null()).map(js_string).unwrap_or_else(|| "window".into()),
            };
            out.push(UsageWindow {
                provider: ProviderId::Agy,
                window,
                group: group.get("name").and_then(Value::as_str).map(str::to_string),
                used: (1.0 - remaining).clamp(0.0, 1.0),
                resets_at: reset.to_string(),
            });
        }
    }
    out
}

/// agy streams its answer in deltas; `text` collects them until the step is done.
fn read_stream(text: &mut String, data: &Map<String, Value>) -> Vec<StreamSignal> {
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
                text.push_str(delta);
            }
            if state == Some("DONE") && !text.trim().is_empty() {
                let done = text.trim().to_string();
                text.clear();
                return vec![StreamSignal::Message { text: done }];
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
