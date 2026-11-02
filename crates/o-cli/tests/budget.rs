//! `o budget` with the real `o` against a throwaway O home (LOCALAPPDATA/project_O).
//! Always `--cached`: tests never ask a provider CLI.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use chrono::{Duration, Utc};
use o_core::usage::budget::{local_day_of, record};
use o_core::sys::js::iso;
use o_core::run::plan::ProviderId;
use o_core::usage::UsageWindow;
use serde_json::Value;

fn o(base: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o")).args(args).env("LOCALAPPDATA", base).env("O_RUN_ID", "").env("O_CARD_ID", "").output().unwrap()
}

fn json(base: &Path, args: &[&str]) -> Value {
    let out = o(base, args);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("o-budget-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    (base.clone(), base.join("project_O"))
}

#[test]
fn budget_json_lists_every_provider_with_the_setting_and_today_s_numbers() {
    let (base, home) = setup("shape");
    let empty = json(&base, &["budget", "--json", "--cached"]);
    assert_eq!(empty["percent"], 13);
    assert_eq!(empty["usageAt"], "");
    let providers = empty["providers"].as_array().unwrap();
    let ids: Vec<&str> = providers.iter().map(|p| p["provider"].as_str().unwrap()).collect();
    assert_eq!(ids, ["claude", "codex", "agy"]);
    assert!(providers.iter().all(|p| p["state"] == "unknown" && p["spent"].is_null() && p["stepsDown"] == 0));

    let now = Utc::now();
    let weekly = |used: f64| UsageWindow { provider: ProviderId::Codex, window: "seven_day".into(), group: None, used, resets_at: iso(now + Duration::days(3)) };
    record(&home, &[weekly(0.30)], local_day_of(now).start - Duration::minutes(1)).unwrap();
    record(&home, &[weekly(0.33)], now).unwrap();

    let report = json(&base, &["budget", "--json", "--cached", "--percent", "12"]);
    assert_eq!(report["percent"], 12);
    let codex = &report["providers"][1];
    assert_eq!((codex["weeklyUsed"].as_f64(), codex["spent"].as_f64(), codex["allowance"].as_f64()), (Some(33.0), Some(3.0), Some(12.0)));
    assert_eq!(codex["countedFrom"], "dayStart");
    for key in ["remainingHours", "timeShare", "remainingShare", "periodEnd", "weeklyResetsAt", "readAt", "message"] {
        assert!(!codex[key].is_null(), "{key}");
    }
    assert!(codex["efforts"].is_array());
    let settings: Value = serde_json::from_str(&fs::read_to_string(home.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["dailyBudgetPercent"], 12);

    // Out of range: refused, the saved setting stays.
    assert_eq!(o(&base, &["budget", "--cached", "--percent", "15"]).status.code(), Some(1));
    assert_eq!(json(&base, &["budget", "--json", "--cached"])["percent"], 12);
    let text = o(&base, &["budget", "--cached"]);
    assert!(String::from_utf8_lossy(&text.stdout).contains("주간 한도의 12%"));
}

#[test]
fn budget_on_and_off_are_saved_and_shown() {
    let (base, home) = setup("switch");
    assert_eq!(json(&base, &["budget", "--json", "--cached"])["enabled"], true);
    let off = json(&base, &["budget", "--json", "--cached", "--off"]);
    assert_eq!(off["enabled"], false);
    assert!(off["providers"].as_array().unwrap().iter().all(|p| p["state"] == "off"));
    let settings: Value = serde_json::from_str(&fs::read_to_string(home.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["dailyBudgetEnabled"], false);
    let text = String::from_utf8_lossy(&o(&base, &["budget", "--cached"]).stdout).into_owned();
    assert!(text.contains("하루 사용 예산: 꺼짐"), "{text}");
    assert_eq!(json(&base, &["budget", "--json", "--cached", "--on"])["enabled"], true);
}

#[test]
fn about_prints_the_same_version_as_the_app() {
    let (base, _) = setup("about");
    let app: Value = serde_json::from_str(&fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../app/package.json")).unwrap()).unwrap();
    assert_eq!(json(&base, &["about", "--json"])["version"], app["version"]);
}
