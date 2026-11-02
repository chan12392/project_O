//! Runs with their events (`o runs --json`, dev `/api/runs`).

use std::path::Path;

use o_core::run::index::RunQuery;
use o_core::run::store;
use o_core::sessions;
use serde_json::json;

use super::blocking;

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
