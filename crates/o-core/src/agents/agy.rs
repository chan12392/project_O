//! Antigravity worker consent (ADR-0042).

use std::path::Path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use crate::usage::budget::{read_settings, write_setting};

const KEY: &str = "agyWorkers";
const QUOTES: [&str; 3] = [
    "Using third-party software, tools, or services to access Antigravity is a violation of our Terms of Service and severely degrades the experience for legitimate product users.",
    "Such actions can result in suspension or termination of your account.",
    "Wrapping `agy` inside third-party coding agents or applications, or automating account rotation, is not supported under consumer plans.",
];
const WARNING: &str = "O가 agy 터미널에 글을 자동으로 넣는 것은 제3자 앱이 agy를 다루는 것으로 볼 수 있습니다. 계정 정지·해지의 위험은 이 PC에 로그인한 Google 계정이 집니다. 기업 계약은 조건이 다를 수 있습니다.";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub workers: Option<bool>,
    pub ack: Option<bool>,
    pub confirm_paste: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgyReport {
    pub workers: bool,
    pub acknowledged_at: Option<String>,
    pub confirm_paste: bool,
    pub notice: Value,
}

pub fn report(home: &Path) -> AgyReport {
    let settings = read_settings(home);
    let value = settings.get(KEY);
    let acknowledged_at = value.and_then(|v| v.get("acknowledgedAt")).and_then(Value::as_str)
        .filter(|at| chrono::DateTime::parse_from_rfc3339(at).is_ok()).map(str::to_string);
    let workers = value.and_then(|v| v.get("workers")).and_then(Value::as_bool).unwrap_or(false) && acknowledged_at.is_some();
    AgyReport {
        workers,
        acknowledged_at: if workers { acknowledged_at } else { None },
        confirm_paste: value.and_then(|v| v.get("confirmPaste")).and_then(Value::as_bool).unwrap_or(true),
        notice: json!({ "quotes": QUOTES, "links": [
            { "label": "Antigravity FAQ", "url": "https://antigravity.google/docs/faq" },
            { "label": "Antigravity 약관", "url": "https://antigravity.google/terms" }
        ], "warning": WARNING }),
    }
}

pub fn workers_allowed(home: &Path) -> bool { report(home).workers }

/// Whether a lead turn or a card runs this process: the runner's variables, or, when an agent
/// cleared them, the job the runner started it in (`agent_job`, ADR-0043).
pub fn inside_agent() -> bool {
    ["O_AGENT_ROLE", "O_CARD_ID"].iter().any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())) || crate::sys::agent_job::current().is_some()
}

/// Whether a card runs this process, by the same two signs.
pub fn inside_card() -> bool {
    std::env::var_os("O_CARD_ID").is_some_and(|value| !value.is_empty()) || matches!(crate::sys::agent_job::current(), Some(crate::sys::agent_job::AgentJob::Card(_)))
}

pub fn set(home: &Path, change: Change) -> Result<AgyReport, String> {
    if inside_agent() { return Err("에이전트 안에서는 Antigravity 설정을 바꿀 수 없습니다. 사용자가 실행해 주세요.".into()); }
    if change.workers == Some(true) && change.ack != Some(true) {
        return Err("안내와 약관을 읽고 동의 확인을 해 주세요 (--ack).".into());
    }
    let mut current = report(home);
    if let Some(workers) = change.workers {
        current.workers = workers;
        current.acknowledged_at = workers.then(crate::sys::js::now_iso);
    }
    if let Some(confirm) = change.confirm_paste { current.confirm_paste = confirm; }
    write_setting(home, KEY, json!({ "workers": current.workers, "acknowledgedAt": current.acknowledged_at, "confirmPaste": current.confirm_paste }))?;
    Ok(report(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_and_defaults() {
        let temp = std::env::temp_dir().join(format!("o-agy-settings-{}", std::process::id()));
        std::fs::create_dir_all(&temp).unwrap();
        let home = temp.as_path();
        assert!(!workers_allowed(home));
        assert!(report(home).confirm_paste);
        assert!(set(home, Change { workers: Some(true), ..Default::default() }).is_err());
        let enabled = set(home, Change { workers: Some(true), ack: Some(true), confirm_paste: Some(false) }).unwrap();
        assert!(enabled.workers && enabled.acknowledged_at.is_some() && !enabled.confirm_paste);
        let disabled = set(home, Change { workers: Some(false), ..Default::default() }).unwrap();
        assert!(!disabled.workers && disabled.acknowledged_at.is_none());
        std::fs::write(home.join("settings.json"), r#"{"agyWorkers":{"workers":"invalid","confirmPaste":3}}"#).unwrap();
        assert!(!workers_allowed(home) && report(home).confirm_paste);
    }
}
