//! The Tauri commands the screens call (app/src/backend.ts, app/src/useLiveRuns.ts).
//! Each runs its work on a blocking thread and calls o-core directly: no shell,
//! and any CLI it asks runs without a window (sign-in alone opens a console the
//! person drives).

use std::path::Path;
use std::sync::Mutex;

use o_core::agents::{self, ActionResult, AgentStatus};
use o_core::index::RunQuery;
use o_core::plan::ProviderId;
use o_core::usage::UsageReading;
use o_core::{clean, probe, sessions, store};
use serde::Serialize;
use serde_json::json;

/// `o runs --json` lists this many by default.
const DEFAULT_RUNS: usize = 50;

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| e.to_string())
}

fn provider(id: &str) -> Result<ProviderId, String> {
    ProviderId::parse(id).ok_or_else(|| format!("에이전트는 claude, codex, agy 중 하나입니다: {id}"))
}

/// The text `o runs --json --limit <limit> [--sessions]` prints.
pub fn runs_json(home: &Path, limit: usize, with_sessions: bool) -> Result<String, String> {
    let query = RunQuery { limit, project: None };
    let read = if with_sessions { sessions::read_runs_with_sessions(home, &query, sessions::hours(sessions::DEFAULT_SINCE_HOURS)) } else { store::read_runs(home, &query) };
    let runs = read.map_err(|e| e.to_string())?;
    Ok(json!({ "runs": runs }).to_string())
}

/// The text `o usage --cached --json` prints: the saved reading, or an empty one.
pub fn usage_json(home: &Path) -> Result<String, String> {
    let saved = probe::load_usage(home).map_err(|e| e.to_string())?;
    Ok(saved.unwrap_or_else(|| json!(UsageReading::empty())).to_string())
}

/// One usage probe at a time; a refresh asked for meanwhile gets that probe's reading.
static REFRESH: Mutex<()> = Mutex::new(());

fn refresh_usage(home: &Path) -> Result<String, String> {
    let Ok(_probing) = REFRESH.try_lock() else {
        drop(REFRESH.lock());
        return usage_json(home);
    };
    let reading = probe::read_usage();
    probe::save_usage(home, &reading).map_err(|e| e.to_string())?;
    Ok(json!(reading).to_string())
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub version: String,
    pub home: String,
    pub agents_home: String,
}

pub fn about_info(home: &Path, agents_home: &Path) -> About {
    About { version: env!("CARGO_PKG_VERSION").into(), home: home.to_string_lossy().into_owned(), agents_home: agents_home.to_string_lossy().into_owned() }
}

/// Runs with every event, newest first, as JSON text (the shape `o runs --json` prints);
/// with `sessions`, this PC's recent Claude Code sessions too (`o runs --json --sessions`).
#[tauri::command]
pub async fn runs(limit: Option<usize>, sessions: Option<bool>) -> Result<String, String> {
    let limit = limit.unwrap_or(DEFAULT_RUNS);
    blocking(move || runs_json(&store::default_home(), limit, sessions.unwrap_or(false))).await?
}

/// The last saved usage reading as JSON text, without asking any CLI.
#[tauri::command]
pub async fn usage() -> Result<String, String> {
    blocking(|| usage_json(&store::default_home())).await?
}

/// Asks Codex and Antigravity for their windows again and saves the reading (`o usage`).
#[tauri::command]
pub async fn usage_refresh() -> Result<String, String> {
    blocking(|| refresh_usage(&store::default_home())).await?
}

#[tauri::command]
pub async fn agents_status(clean: bool) -> Result<Vec<AgentStatus>, String> {
    blocking(move || agents::status(&clean::default_root(), clean)).await
}

#[tauri::command]
pub async fn agent_install(id: String) -> Result<ActionResult, String> {
    let provider = provider(&id)?;
    blocking(move || agents::install(provider)).await
}

#[tauri::command]
pub async fn agent_login(id: String, clean: bool) -> Result<ActionResult, String> {
    let provider = provider(&id)?;
    blocking(move || agents::open_login(&clean::default_root(), provider, clean)).await
}

#[tauri::command]
pub async fn open_external(url: String) -> Result<(), String> {
    blocking(move || agents::open_external(&url)).await?
}

#[tauri::command]
pub fn about() -> About {
    about_info(&store::default_home(), &clean::default_root())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::Value;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("project-o-shell-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

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

    #[test]
    fn usage_without_a_reading_is_empty() {
        let home = temp("usage");
        let body: Value = serde_json::from_str(&usage_json(&home).unwrap()).unwrap();
        assert_eq!(body, json!(UsageReading::empty()));
    }

    #[test]
    fn about_names_version_and_folders() {
        let about = about_info(Path::new("C:/state/project_O"), Path::new("C:/state/project_O-agents"));
        let value = json!(about);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["home"], "C:/state/project_O");
        assert_eq!(value["agentsHome"], "C:/state/project_O-agents");
    }

    #[test]
    fn unknown_agent_ids_are_refused() {
        assert!(provider("claude").is_ok());
        assert!(provider("../claude").is_err());
        assert!(provider("").is_err());
    }
}
