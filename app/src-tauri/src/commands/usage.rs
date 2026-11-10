//! Subscription windows and the daily budget (`o usage`, `o budget`; dev `/api/usage`, `/api/budget`).

use std::path::Path;
use std::sync::Mutex;

use o_core::run::mode::Profile;
use o_core::run::store;
use o_core::usage::{budget, probe, UsageReading};
use serde_json::{json, Value};

use super::blocking;

#[tauri::command]
pub async fn resource_leases() -> Result<Vec<store::ResourceLease>, String> {
    blocking(|| store::resource_leases(&store::default_home()).map_err(|error| error.to_string())).await?
}

#[tauri::command]
pub async fn resource_recover(id: String, confirm_exited: bool) -> Result<(), String> {
    if confirm_exited && o_core::agents::agy::inside_agent() {
        return Err("에이전트 안에서는 직접 확인 후 복구할 수 없습니다. 사용자가 실행해 주세요".into());
    }
    let id = id.parse::<i64>().ok().filter(|id| *id > 0).ok_or("예약 ID가 올바르지 않습니다")?;
    blocking(move || {
        let home = store::default_home();
        if confirm_exited { store::release_confirmed(&home, id) } else { store::recover_resource(&home, id) }.map_err(|error| error.to_string())
    }).await?
}

/// The text `o usage --cached --json` prints: the saved reading, or an empty one.
pub fn usage_json(home: &Path) -> Result<String, String> {
    let saved = probe::load_usage(home).map_err(|e| e.to_string())?;
    Ok(saved.unwrap_or_else(|| json!(UsageReading::empty())).to_string())
}

/// One usage probe at a time; a refresh asked for meanwhile gets that probe's reading.
static REFRESH: Mutex<()> = Mutex::new(());

fn refresh_usage(home: &Path, profile: Option<Profile>) -> Result<String, String> {
    let Ok(_probing) = REFRESH.try_lock() else {
        drop(REFRESH.lock());
        return usage_json(home);
    };
    let reading = probe::read_usage(home, profile);
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
/// `profile`: whose login to ask — the app's "project_O 프로필 사용" switch (`o usage --profile`).
#[tauri::command]
pub async fn usage_refresh(profile: Option<String>) -> Result<String, String> {
    let profile = match profile.as_deref() {
        None => None,
        Some(name) => Some(Profile::parse(name).ok_or("profile은 project-o 또는 own이어야 합니다")?),
    };
    blocking(move || refresh_usage(&store::default_home(), profile)).await?
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

/// Settings → 에이전트 → RAM 상한 (`o ram --json`, ADR-0037).
#[tauri::command]
pub async fn ram_get() -> Result<Value, String> {
    blocking(|| json!(o_core::run::ram::report(&store::default_home()))).await
}

/// Settings → 에이전트 → RAM 상한 켜기·끄기와 비율 (`o ram --on|--off --percent <n>`).
#[tauri::command]
pub async fn ram_set(enabled: Option<bool>, percent: Option<Value>) -> Result<Value, String> {
    blocking(move || {
        let home = store::default_home();
        if let Some(percent) = percent { o_core::run::ram::write_percent(&home, &percent)?; }
        if let Some(enabled) = enabled { o_core::run::ram::write_enabled(&home, enabled)?; }
        Ok(json!(o_core::run::ram::report(&home)))
    })
    .await?
}

/// Settings → 에이전트 → API 과금 (`o billing --json`, ADR-0035 개정 1).
#[tauri::command]
pub async fn billing_get() -> Result<Value, String> {
    blocking(|| json!(o_core::agents::billing::report(&store::default_home()))).await
}

/// Settings → 에이전트 → API 과금 경고만·구독만 (`o billing --warn|--subscription`).
#[tauri::command]
pub async fn billing_set(mode: String) -> Result<Value, String> {
    blocking(move || {
        let home = store::default_home();
        o_core::agents::billing::write_mode(&home, &mode)?;
        Ok(json!(o_core::agents::billing::report(&home)))
    })
    .await?
}

#[tauri::command]
pub async fn yolo_get() -> Result<Value, String> {
    blocking(|| json!(o_core::run::yolo::report(&store::default_home()))).await
}

#[tauri::command]
pub async fn yolo_set(enabled: Option<bool>, max_cards: Option<Value>, per_hour: Option<Value>, dedupe_minutes: Option<Value>) -> Result<Value, String> {
    blocking(move || {
        let home = store::default_home();
        o_core::run::yolo::write_limits(&home, max_cards.as_ref(), per_hour.as_ref(), dedupe_minutes.as_ref())?;
        if let Some(enabled) = enabled { o_core::run::yolo::write_enabled(&home, enabled)?; }
        Ok(json!(o_core::run::yolo::report(&home)))
    }).await?
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
        assert_eq!(body["providers"].as_array().unwrap().len(), 5);
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
        assert!(off["providers"].as_array().unwrap().iter().all(|p| if p["provider"] == "agy" || p["provider"] == "opencode" || p["provider"] == "devin" { p["state"] == "notApplied" } else { p["state"] == "off" }));
        let again: Value = serde_json::from_str(&budget_json(&home).unwrap()).unwrap();
        assert_eq!((again["enabled"].clone(), again["percent"].clone()), (json!(false), json!(13)));
    }
}
