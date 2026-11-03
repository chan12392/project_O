//! Local findings and read-only issue search (ADR-0027).
use o_core::{issues::{self, Finding, IssueRef, Resolution}, review::OfficialGh, run::store};
use serde_json::{json, Value};
use super::blocking;

#[tauri::command]
pub async fn issues_search(repo: String, query: String) -> Result<Value, String> {
    blocking(move || {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        serde_json::to_value(issues::search(&OfficialGh, &cwd, &repo, &query)?).map_err(|e| e.to_string())
    }).await?
}

#[tauri::command]
pub async fn issues_list(repo: String) -> Result<Value, String> {
    blocking(move || issues::list(&store::default_home(), &repo).map(|rows| json!(rows))).await?
}

#[tauri::command]
pub async fn issues_record(finding: Finding) -> Result<Value, String> {
    blocking(move || {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        issues::record(&store::default_home(), &OfficialGh, &cwd, &finding).map(|id| json!({"id": id}))
    }).await?
}

#[tauri::command]
pub async fn issues_classify(id: i64, classification: String, issue: Option<IssueRef>) -> Result<Value, String> {
    blocking(move || {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        issues::classify(&store::default_home(), &OfficialGh, &cwd, id, &classification, issue)?;
        Ok(json!({"id": id}))
    }).await?
}

#[tauri::command]
pub async fn issues_resolve(id: i64, resolution: Option<Resolution>) -> Result<Value, String> {
    blocking(move || {
        issues::resolve(&store::default_home(), id, resolution)?;
        Ok(json!({"id": id}))
    }).await?
}
