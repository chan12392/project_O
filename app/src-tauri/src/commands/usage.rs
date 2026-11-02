//! Subscription windows and the daily budget (`o usage`, `o budget`; dev `/api/usage`, `/api/budget`).

use std::path::Path;
use std::sync::Mutex;

use o_core::run::store;
use o_core::usage::{budget, probe, UsageReading};
use serde_json::{json, Value};

use super::blocking;

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

/// Turns the daily budget on or off (ADR-0013) and returns the new report.
pub fn set_budget_enabled(home: &Path, enabled: bool) -> Result<String, String> {
    budget::write_enabled(home, enabled)?;
    budget_json(home)
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

/// Settings → 에이전트 → 하루 사용 예산 켜기·끄기 (`o budget --on|--off`).
#[tauri::command]
pub async fn budget_set_enabled(enabled: bool) -> Result<String, String> {
    blocking(move || set_budget_enabled(&store::default_home(), enabled)).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::temp;

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
    fn budget_switch_is_saved_and_reported() {
        let home = temp("budget-switch");
        let on: Value = serde_json::from_str(&budget_json(&home).unwrap()).unwrap();
        assert_eq!(on["enabled"], true);
        let off: Value = serde_json::from_str(&set_budget_enabled(&home, false).unwrap()).unwrap();
        assert_eq!(off["enabled"], false);
        assert!(off["providers"].as_array().unwrap().iter().all(|p| p["state"] == "off"));
        let again: Value = serde_json::from_str(&budget_json(&home).unwrap()).unwrap();
        assert_eq!((again["enabled"].clone(), again["percent"].clone()), (json!(false), json!(13)));
    }
}
