//! The daily usage budget (ADR-0004): settings, snapshots and the pace math.

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use o_core::budget::{
    assess, local_day_of, lower_effort, over_budget, read_percent, record, weekly_snapshots, write_percent, Day, Pace, Snapshot, DEFAULT_PERCENT,
};
use o_core::plan::ProviderId;
use o_core::probe::save_usage;
use o_core::store::policy_context;
use o_core::usage::{UsageReading, UsageWindow};
use serde_json::{json, Value};

fn temp_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-budget-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn t(text: &str) -> DateTime<Utc> {
    text.parse().unwrap()
}

/// 2 November, as a UTC day so the numbers do not depend on this PC's time zone.
fn day() -> Day {
    Day { start: t("2026-11-02T00:00:00Z"), end: t("2026-11-03T00:00:00Z") }
}

fn snap(group: &str, used: f64, resets_at: &str, at: &str) -> Snapshot {
    Snapshot { group: group.into(), used, resets_at: resets_at.into(), at: t(at) }
}

const NEXT_WEEK: &str = "2026-11-06T09:00:00.000Z";

/// Weekly 40% at the start of the day, `today` more by 11:00; looked at noon (half the day left).
fn noon_with(today: f64) -> o_core::budget::Budget {
    let snaps = [snap("", 0.40, NEXT_WEEK, "2026-11-01T23:00:00Z"), snap("", 0.40 + today, NEXT_WEEK, "2026-11-02T11:00:00Z")];
    assess(ProviderId::Codex, 13, &snaps, t("2026-11-02T12:00:00Z"), day())
}

#[test]
fn a_normal_day_counts_from_the_start_of_the_day_and_keeps_effort() {
    let b = noon_with(0.04);
    assert_eq!(b.state, Pace::Ok);
    assert_eq!(b.counted_from, Some("dayStart"));
    assert_eq!((b.weekly_used, b.spent, b.allowance, b.remaining), (Some(44.0), Some(4.0), 13.0, Some(9.0)));
    assert_eq!((b.remaining_hours, b.time_share), (Some(12.0), Some(0.5)));
    assert_eq!(b.steps_down, 0);
    assert!(b.efforts.is_empty());
}

#[test]
fn behind_pace_lowers_effort_one_step_far_behind_two() {
    let behind = noon_with(0.08);
    assert_eq!((behind.state, behind.steps_down), (Pace::Behind, 1));
    let pairs: Vec<(Option<&str>, &str)> = behind.efforts.iter().map(|c| (c.from.as_deref(), c.to.as_str())).collect();
    assert_eq!(pairs, [(None, "low"), (Some("max"), "xhigh"), (Some("xhigh"), "high"), (Some("high"), "medium"), (Some("medium"), "low")]);
    let far = noon_with(0.105);
    assert_eq!((far.state, far.steps_down), (Pace::FarBehind, 2));
    assert!(far.message.contains("두 단계"));
}

#[test]
fn spending_the_whole_allowance_is_over_with_when_it_reopens() {
    let over = noon_with(0.13);
    assert_eq!((over.state, over.steps_down), (Pace::Over, 0));
    assert!(over.message.contains("사용 예산을 다 썼습니다") && over.message.contains("13.0%p"), "{}", over.message);
}

#[test]
fn a_weekly_reset_during_the_day_counts_from_the_reset() {
    let snaps = [
        snap("", 0.95, "2026-11-02T06:00:00.000Z", "2026-11-01T23:00:00Z"),
        snap("", 0.03, "2026-11-09T06:00:00.000Z", "2026-11-02T11:00:00Z"),
    ];
    let b = assess(ProviderId::Codex, 13, &snaps, t("2026-11-02T12:00:00Z"), day());
    assert_eq!(b.counted_from, Some("weeklyReset"));
    assert_eq!((b.spent, b.weekly_used), (Some(3.0), Some(3.0)));
    // The budget day runs from the 06:00 reset to midnight: 12 of 18 hours left.
    assert_eq!(b.time_share, Some(0.67));
    assert_eq!(b.state, Pace::Ok);
}

#[test]
fn a_weekly_reset_before_midnight_ends_the_budget_day_there() {
    let resets = "2026-11-02T18:00:00.000Z";
    let snaps = [snap("", 0.70, resets, "2026-11-01T23:00:00Z"), snap("", 0.72, resets, "2026-11-02T11:00:00Z")];
    let b = assess(ProviderId::Codex, 13, &snaps, t("2026-11-02T12:00:00Z"), day());
    assert_eq!(b.remaining_hours, Some(6.0));
    assert_eq!(b.period_end.as_deref(), Some("2026-11-02T18:00:00.000Z"));
    assert_eq!(b.time_share, Some(0.33));
}

#[test]
fn without_a_reading_before_today_the_first_reading_today_is_the_baseline() {
    let snaps = [snap("", 0.40, NEXT_WEEK, "2026-11-02T01:00:00Z"), snap("", 0.45, NEXT_WEEK, "2026-11-02T11:00:00Z")];
    let b = assess(ProviderId::Codex, 13, &snaps, t("2026-11-02T12:00:00Z"), day());
    assert_eq!((b.counted_from, b.spent), (Some("firstReading"), Some(5.0)));
}

#[test]
fn no_weekly_reading_means_unknown_and_no_adjustment() {
    let b = assess(ProviderId::Claude, 13, &[], t("2026-11-02T12:00:00Z"), day());
    assert_eq!((b.state, b.steps_down, b.spent), (Pace::Unknown, 0, None));
    assert!(b.efforts.is_empty());
    assert!(b.message.contains("기록이 없어"));
}

#[test]
fn split_windows_follow_the_group_that_spent_most_today() {
    let snaps = [
        snap("Gemini Pro", 0.10, NEXT_WEEK, "2026-11-01T23:00:00Z"),
        snap("Gemini Flash", 0.50, NEXT_WEEK, "2026-11-01T23:00:00Z"),
        snap("Gemini Pro", 0.19, NEXT_WEEK, "2026-11-02T11:00:00Z"),
        snap("Gemini Flash", 0.52, NEXT_WEEK, "2026-11-02T11:00:00Z"),
    ];
    let b = assess(ProviderId::Agy, 13, &snaps, t("2026-11-02T12:00:00Z"), day());
    assert_eq!((b.group.as_deref(), b.spent), (Some("Gemini Pro"), Some(9.0)));
}

#[test]
fn effort_only_goes_down_never_below_low_and_never_up() {
    assert_eq!(lower_effort(Some("high"), 0), None);
    assert_eq!(lower_effort(Some("high"), 1).as_deref(), Some("medium"));
    assert_eq!(lower_effort(Some("max"), 2).as_deref(), Some("high"));
    assert_eq!(lower_effort(Some("medium"), 2).as_deref(), Some("low"));
    assert_eq!(lower_effort(Some("low"), 2), None);
    assert_eq!(lower_effort(Some("minimal"), 1), None);
    assert_eq!(lower_effort(None, 1).as_deref(), Some("low"));
    assert_eq!(lower_effort(None, 0), None);
}

#[test]
fn the_budget_percent_is_12_to_14_and_anything_else_is_13() {
    let home = temp_home("settings");
    assert_eq!(read_percent(&home), DEFAULT_PERCENT);
    for bad in [json!(11), json!(15), json!(13.5), json!("13"), Value::Null] {
        assert!(write_percent(&home, &bad).is_err(), "{bad}");
    }
    assert!(!home.join("settings.json").exists());
    fs::write(home.join("settings.json"), r#"{ "other": true, "dailyBudgetPercent": 99 }"#).unwrap();
    assert_eq!(read_percent(&home), DEFAULT_PERCENT);
    assert_eq!(write_percent(&home, &json!(12)), Ok(12));
    assert_eq!(read_percent(&home), 12);
    let saved: Value = serde_json::from_str(&fs::read_to_string(home.join("settings.json")).unwrap()).unwrap();
    assert_eq!(saved, json!({ "other": true, "dailyBudgetPercent": 12 }));
    fs::write(home.join("settings.json"), "not json").unwrap();
    assert_eq!(read_percent(&home), DEFAULT_PERCENT);
}

fn window(provider: ProviderId, window: &str, used: f64) -> UsageWindow {
    // Against the real clock: a reset still ahead.
    UsageWindow { provider, window: window.into(), group: None, used, resets_at: o_core::js::iso(Utc::now() + Duration::days(3)) }
}

#[test]
fn readings_are_kept_as_weekly_snapshots_and_old_ones_dropped() {
    let home = temp_home("snapshots");
    let now = Utc::now();
    record(&home, &[window(ProviderId::Codex, "seven_day", 0.30)], now - Duration::days(10)).unwrap();
    record(&home, &[window(ProviderId::Codex, "five_hour", 0.90), window(ProviderId::Codex, "seven_day", 0.41)], now - Duration::hours(1)).unwrap();
    // `o usage` saving a reading records it too.
    let reading = UsageReading { at: o_core::js::iso(now), windows: vec![window(ProviderId::Codex, "seven_day", 0.42)], unavailable: Vec::new() };
    save_usage(&home, &reading).unwrap();
    let kept: Vec<f64> = weekly_snapshots(&home, ProviderId::Codex).unwrap().iter().map(|s| s.used).collect();
    assert_eq!(kept, [0.41, 0.42]);
    assert!(weekly_snapshots(&home, ProviderId::Agy).unwrap().is_empty());
}

#[test]
fn a_provider_over_budget_is_named_in_the_policy_context() {
    let home = temp_home("policy");
    let now = Utc::now();
    let start = local_day_of(now).start;
    record(&home, &[window(ProviderId::Codex, "seven_day", 0.20)], start - Duration::minutes(1)).unwrap();
    record(&home, &[window(ProviderId::Codex, "seven_day", 0.34)], now).unwrap();
    let over = over_budget(&home, now).unwrap();
    assert!(over.contains_key(&ProviderId::Codex));
    assert!(!over.contains_key(&ProviderId::Claude));
    let ctx = policy_context(&home, now).unwrap();
    assert!(ctx.over_budget[&ProviderId::Codex].contains("사용 예산"));
}
