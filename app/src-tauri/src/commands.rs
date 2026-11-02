//! The Tauri commands the screens call (app/src/backend.ts, app/src/useLiveRuns.ts).
//! Each runs its work on a blocking thread and calls o-core directly: no shell,
//! and any CLI it asks runs without a window (sign-in alone opens a console the
//! person drives).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use o_core::agents::{self, ActionResult, AgentStatus};
use o_core::index::RunQuery;
use o_core::plan::ProviderId;
use o_core::usage::UsageReading;
use o_core::{budget, clean, probe, proc, resolve, review, sessions, store};
use serde::Serialize;
use serde_json::{json, Value};

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

/// The text `o budget --json --cached` prints: today's usage budget per provider (ADR-0004).
pub fn budget_json(home: &Path) -> Result<String, String> {
    budget::report_now(home).map(|report| json!(report).to_string()).map_err(|e| e.to_string())
}

/// Saves the daily budget percent (12, 13 or 14; anything else is refused) and returns the new report.
pub fn set_budget_percent(home: &Path, percent: &Value) -> Result<String, String> {
    budget::write_percent(home, percent)?;
    budget_json(home)
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub version: String,
    pub commit: String,
    pub built_at: String,
    pub home: String,
    pub agents_home: String,
}

pub fn about_info(home: &Path, agents_home: &Path) -> About {
    About { version: env!("CARGO_PKG_VERSION").into(), commit: crate::update::BUILD_SHA.into(), built_at: crate::update::BUILD_TIME.into(), home: home.to_string_lossy().into_owned(), agents_home: agents_home.to_string_lossy().into_owned() }
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

/// Today's usage budget from the saved snapshots, without asking any CLI (`o budget --json --cached`).
#[tauri::command]
pub async fn budget() -> Result<String, String> {
    blocking(|| budget_json(&store::default_home())).await?
}

/// Settings → 에이전트 → 하루 사용 예산 (`o budget --percent <n>`).
#[tauri::command]
pub async fn budget_set_percent(percent: Value) -> Result<String, String> {
    blocking(move || set_budget_percent(&store::default_home(), &percent)).await?
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

/// The `o` built beside this app (target/release), else the one on PATH.
fn o_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "o.exe" } else { "o" };
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))).filter(|p| p.is_file());
    beside.or_else(|| resolve::which("o"))
}

/// Checks the request before anything runs: a PR number or github.com pull address, and an existing absolute folder.
pub fn review_request(project: &str, pr: &str) -> Result<(), String> {
    if !review::is_selector(pr) {
        return Err(format!("PR 번호나 github.com PR 주소가 아닙니다: {pr}"));
    }
    if !Path::new(project).is_absolute() || !Path::new(project).is_dir() {
        return Err(format!("프로젝트 폴더가 없습니다: {project}"));
    }
    Ok(())
}

/// "에이전트 검토 요청": `o review <pr> --post --detach` in the project folder, as the dev
/// server's `/api/review` does. `o` reads the PR with gh, checks the review plan like any
/// dispatch and starts it in the background; the review is posted as a PR comment, never
/// an approval or a merge (ADR-0006).
#[tauri::command]
pub async fn review(project: String, pr: String, clean: bool) -> Result<serde_json::Value, String> {
    review_request(&project, &pr)?;
    blocking(move || {
        let o = o_binary().ok_or("o CLI를 찾지 못했습니다 (cargo install --path crates/o-cli)")?;
        let mut command = Command::new(o);
        command.args(review::request_args(&pr, clean)).current_dir(&project).stdin(Stdio::null());
        let out = proc::hide_window(&mut command).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("o review 출력을 읽지 못했습니다: {e}"))
    })
    .await?
}

/// The token report `o tokens --json [--project <폴더>]` prints, as JSON text (ADR-0005).
pub fn tokens_json(home: &Path, project: Option<&str>) -> Result<String, String> {
    let report = o_core::context::tokens::report(home, &o_core::context::tokens::TokenQuery { project, since: None }).map_err(|e| e.to_string())?;
    Ok(json!(report).to_string())
}

#[tauri::command]
pub async fn tokens(project: Option<String>) -> Result<String, String> {
    blocking(move || tokens_json(&store::default_home(), project.as_deref())).await?
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
    fn budget_lists_providers_and_saves_only_12_to_14() {
        let home = temp("budget");
        let body: Value = serde_json::from_str(&budget_json(&home).unwrap()).unwrap();
        assert_eq!(body["percent"], 13);
        assert_eq!(body["providers"].as_array().unwrap().len(), 3);
        assert!(set_budget_percent(&home, &json!(11)).is_err());
        assert!(set_budget_percent(&home, &json!("14")).is_err());
        let saved: Value = serde_json::from_str(&set_budget_percent(&home, &json!(14)).unwrap()).unwrap();
        assert_eq!(saved["percent"], 14);
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
    fn review_requests_take_only_a_pr_and_an_existing_folder() {
        let dir = temp("review");
        let folder = dir.to_string_lossy();
        assert!(review_request(&folder, "7").is_ok());
        assert!(review_request(&folder, "https://github.com/acme/app/pull/7").is_ok());
        for pr in ["", "0", "--repo", "7 --approve", "https://evil.example/a/b/pull/7"] {
            assert!(review_request(&folder, pr).is_err(), "{pr}");
        }
        assert!(review_request("relative/dir", "7").is_err());
        assert!(review_request(&dir.join("missing").to_string_lossy(), "7").is_err());
    }

    #[test]
    fn tokens_without_runs_is_an_empty_report() {
        let home = temp("tokens");
        let body: Value = serde_json::from_str(&tokens_json(&home, None).unwrap()).unwrap();
        assert_eq!(body, json!({ "groups": [], "compare": { "baseline": null, "shared": null }, "runs": [] }));
    }

    #[test]
    fn unknown_agent_ids_are_refused() {
        assert!(provider("claude").is_ok());
        assert!(provider("../claude").is_err());
        assert!(provider("").is_err());
    }
}
