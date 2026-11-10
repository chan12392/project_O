//! Runs with their events (`o runs --json`, dev `/api/runs`).

use std::path::Path;

use o_core::run::index::RunQuery;
use o_core::run::store;
use o_core::sessions;
use serde_json::json;

use super::blocking;

#[tauri::command]
pub async fn run_changes(run: String, card: String) -> Result<serde_json::Value, String> {
    blocking(move || o_core::run::review_changes(&store::default_home(), &run, &card).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn run_followup(run: String, card: String, text: String) -> Result<serde_json::Value, String> {
    blocking(move || {
        let id = o_core::run::followup::start(&store::default_home(), &run, &card, &text, &start_followup)?;
        Ok(json!({ "runId": id }))
    }).await?
}

/// The person's answer to a card's question, sent as the lead on the run's team channel
/// (`o msg send --run <run> --as lead --to <card>`); the message policy checks it as any.
#[tauri::command]
pub async fn run_reply(run: String, card: String, text: String, message_id: Option<String>) -> Result<(), String> {
    blocking(move || reply(&store::default_home(), &run, &card, &text, message_id.as_deref())).await?
}

pub(super) fn reply(home: &Path, run: &str, card: &str, text: &str, message_id: Option<&str>) -> Result<(), String> {
    use o_core::run::team::{self, Caller, Refused};
    let caller = Caller { env_card: None, env_run: None, flag_run: Some(run.to_string()), flag_as: Some("lead".into()) };
    let speaker = team::speaker(home, caller)?;
    if !speaker.cards.iter().any(|c| c == card) { return Err(format!("이 실행에 없는 카드입니다: {card}")); }
    if let Some(id) = message_id {
        let messages = team::read(home, &speaker, true).map_err(|e| e.to_string())?;
        if !messages.iter().any(|m| m.id == id && m.kind == "question" && m.from == card && m.to == "lead") { return Err("카드의 질문을 찾지 못했습니다".into()); }
    }
    team::send_reply(home, &speaker, card, text, false, None, message_id).map(|_| ()).map_err(|refused| match refused {
        Refused::Policy(violations) => violations.into_iter().map(|v| v.message).collect::<Vec<_>>().join("\n"),
        Refused::Io(error) => error.to_string(),
    })
}

pub(super) fn start_followup(id: &str) -> std::io::Result<()> {
    let exe = super::o_binary().ok_or_else(|| std::io::Error::other("O CLI를 찾지 못했습니다"))?;
    let output = o_core::sys::proc::hide_window(std::process::Command::new(&exe).args(["about", "--json"])).output()?;
    let capable = output.status.success() && serde_json::from_slice::<serde_json::Value>(&output.stdout).ok().and_then(|v| v.get("capabilities").and_then(|v| v.as_array()).cloned()).is_some_and(|caps| caps.iter().any(|v| v.as_str() == Some("session-followup-v1")));
    if !capable { return Err(std::io::Error::other("이 O CLI는 세션 이어쓰기를 지원하지 않습니다. 앱과 같은 버전으로 설치하세요")); }
    o_core::sys::proc::spawn_detached(std::process::Command::new(&exe).args(["_followup", id])).map(|_| ())
}

/// `o _lead-forward <threadId> <n>`: a request the overall lead passed on (ADR-0032), and the review of its plan (ADR-0036).
pub(super) fn start_forward(thread_id: &str, index: usize) -> std::io::Result<()> {
    let exe = super::o_binary().ok_or_else(|| std::io::Error::other("O CLI를 찾지 못했습니다"))?;
    let output = o_core::sys::proc::hide_window(std::process::Command::new(&exe).args(["about", "--json"])).output()?;
    let capable = output.status.success() && serde_json::from_slice::<serde_json::Value>(&output.stdout).ok().and_then(|v| v.get("capabilities").and_then(|v| v.as_array()).cloned()).is_some_and(|caps| caps.iter().any(|v| v.as_str() == Some("overall-review-v1")));
    if !capable { return Err(std::io::Error::other("이 O CLI는 전체 총괄의 요청 전달을 지원하지 않습니다. 앱과 같은 버전으로 설치하세요")); }
    o_core::sys::proc::spawn_detached(std::process::Command::new(&exe).args(["_lead-forward", thread_id, &index.to_string()])).map(|_| ())
}

/// `o runs --json` lists this many by default.
const DEFAULT_RUNS: usize = 50;

/// The text `o runs --json --limit <limit> [--sessions]` prints.
pub fn runs_json(home: &Path, limit: usize, with_sessions: bool) -> Result<String, String> {
    let query = RunQuery { limit, project: None };
    let read = if with_sessions { sessions::read_runs_with_sessions(home, &query, sessions::hours(sessions::DEFAULT_SINCE_HOURS)) } else { store::read_runs(home, &query) };
    let runs = read.map_err(|e| e.to_string())?;
    Ok(json!({ "runs": runs }).to_string())
}

/// Runs with every event, newest first, as JSON text (the shape `o runs --json` prints);
/// with `sessions`, this PC's recent Claude Code sessions too (`o runs --json --sessions`).
#[tauri::command]
pub async fn runs(limit: Option<usize>, sessions: Option<bool>) -> Result<String, String> {
    let limit = limit.unwrap_or(DEFAULT_RUNS);
    blocking(move || runs_json(&store::default_home(), limit, sessions.unwrap_or(false))).await?
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::Value;

    use super::*;
    use crate::commands::temp;

    #[test]
    fn runs_lists_runs_with_events_newest_first() {
        let home = temp("runs");
        assert_eq!(serde_json::from_str::<Value>(&runs_json(&home, 50, false).unwrap()).unwrap(), json!({ "runs": [] }));
        for (id, at) in [("20261101-100000-aaaa", "2026-11-01T10:00:00.000Z"), ("20261102-100000-bbbb", "2026-11-02T10:00:00.000Z")] {
            let dir = home.join("runs").join(id);
            fs::create_dir_all(&dir).unwrap();
            let start = json!({ "t": "run.start", "at": at, "runId": id, "title": "t", "project": "C:/p", "cards": [] });
            fs::write(dir.join("events.jsonl"), format!("{start}\n")).unwrap();
        }
        let body: Value = serde_json::from_str(&runs_json(&home, 50, false).unwrap()).unwrap();
        let runs = body["runs"].as_array().unwrap();
        let ids: Vec<&str> = runs.iter().map(|r| r["runId"].as_str().unwrap()).collect();
        assert_eq!(ids, ["20261102-100000-bbbb", "20261101-100000-aaaa"]);
        assert_eq!(runs[0]["events"][0]["t"], "run.start");
        let one: Value = serde_json::from_str(&runs_json(&home, 1, false).unwrap()).unwrap();
        assert_eq!(one["runs"].as_array().unwrap().len(), 1);
    }
}
