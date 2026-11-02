//! The daily usage budget (ADR-0004): each provider may spend about a seventh
//! of its weekly subscription window per local day — `dailyBudgetPercent`
//! (12 to 14, default 13) percent points of the weekly (`seven_day`) window.
//!
//! Today's spend is the weekly `used` now minus the weekly `used` at the local
//! start of the day, read from usage snapshots kept in `o.db`:
//!
//! ```text
//! usage_snapshots  provider · win · grp · used (0–1) · resets_at · at
//! ```
//!
//! Snapshots are written whenever a window is read: `o usage` and the app's
//! refresh (`probe::save_usage`), `o budget` and `o _run` probing a stale
//! reading, and Claude's `quota` stream events in the runner.
//!
//! The runner then keeps the pace: a provider that spent its allowance is
//! refused (policy rule `daily-budget`); one spending faster than the day goes
//! gets each card's effort lowered one step (two when far behind), never
//! raised. A provider with no weekly reading is left alone.
//!
//! The whole budget is a switch (`dailyBudgetEnabled`, on unless saved as false,
//! ADR-0013): off, every provider reads `Pace::Off` — the numbers stay, nothing is
//! refused or lowered. Subscription limits and cooldowns are not part of it.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Utc};
use rusqlite::{params, Connection, TransactionBehavior};
use serde::Serialize;
use serde_json::{Map, Value};

use crate::run::index::{self, sql};
use crate::sys::js::{iso, parse_time};
use crate::run::plan::ProviderId;
use crate::policy::EFFORTS;
use crate::usage::probe;
use crate::run::store::to_pretty;
use crate::usage::{UsageReading, UsageWindow};

pub const DEFAULT_PERCENT: u8 = 13;
pub const MIN_PERCENT: u8 = 12;
pub const MAX_PERCENT: u8 = 14;
/// The key in `<O home>/settings.json`.
pub const PERCENT_KEY: &str = "dailyBudgetPercent";
/// The on/off switch in `<O home>/settings.json`; only `false` turns the budget off.
pub const ENABLED_KEY: &str = "dailyBudgetEnabled";
pub const WEEKLY: &str = "seven_day";
/// `o budget` and `o _run` ask the CLIs again when the saved reading is older than this.
pub const STALE_MINUTES: i64 = 10;
/// Snapshots older than this are dropped; a day's baseline needs at most the last week.
const KEEP_DAYS: i64 = 9;
/// Not every CLI takes `minimal` (Claude's lowest is `low`), so lowering stops here.
pub const FLOOR_EFFORT: &str = "low";
/// What a card without an effort is taken to run at (the CLIs' usual default).
pub const ASSUMED_EFFORT: &str = "medium";

const TABLE: &str = "
CREATE TABLE IF NOT EXISTS usage_snapshots (
  provider TEXT NOT NULL, win TEXT NOT NULL, grp TEXT NOT NULL DEFAULT '',
  used REAL NOT NULL, resets_at TEXT NOT NULL, at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_snapshots_by_at ON usage_snapshots (provider, win, at);
";

fn settings_path(home: &Path) -> PathBuf {
    home.join("settings.json")
}

/// A budget percent as settings.json may hold it: a whole number from 12 to 14.
pub fn valid_percent(value: &Value) -> Option<u8> {
    let n = value.as_f64()?;
    (n.fract() == 0.0 && (f64::from(MIN_PERCENT)..=f64::from(MAX_PERCENT)).contains(&n)).then_some(n as u8)
}

fn read_settings(home: &Path) -> Map<String, Value> {
    fs::read(settings_path(home))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

/// The owner's daily budget percent; a missing or invalid setting is the default 13.
pub fn read_percent(home: &Path) -> u8 {
    read_settings(home).get(PERCENT_KEY).and_then(valid_percent).unwrap_or(DEFAULT_PERCENT)
}

/// Whether the daily budget is on; a missing or invalid setting is on (the default).
pub fn read_enabled(home: &Path) -> bool {
    read_settings(home).get(ENABLED_KEY).and_then(Value::as_bool).unwrap_or(true)
}

/// Saves one setting, keeping the file's others.
fn write_setting(home: &Path, key: &str, value: Value) -> Result<(), String> {
    let mut settings = read_settings(home);
    settings.insert(key.into(), value);
    let write = || -> io::Result<()> {
        fs::create_dir_all(home)?;
        fs::write(settings_path(home), to_pretty(&Value::Object(settings))?)
    };
    write().map_err(|e| format!("settings.json: {e}"))
}

/// Saves the daily budget percent, keeping the file's other settings. Anything but 12, 13 or 14 is refused.
pub fn write_percent(home: &Path, value: &Value) -> Result<u8, String> {
    let percent = valid_percent(value).ok_or_else(|| format!("하루 사용 예산은 {MIN_PERCENT}, 13, {MAX_PERCENT} 중 하나입니다: {value}"))?;
    write_setting(home, PERCENT_KEY, Value::from(percent))?;
    Ok(percent)
}

/// Turns the daily budget on or off (`o budget --on|--off`, Settings → 에이전트), keeping the percent.
pub fn write_enabled(home: &Path, enabled: bool) -> Result<bool, String> {
    write_setting(home, ENABLED_KEY, Value::Bool(enabled))?;
    Ok(enabled)
}

fn open(home: &Path) -> io::Result<Connection> {
    let conn = index::open(home)?;
    conn.execute_batch(TABLE).map_err(sql)?;
    Ok(conn)
}

/// Keeps every window of a reading as a snapshot (and drops the ones no day needs any more).
pub fn record(home: &Path, windows: &[UsageWindow], at: DateTime<Utc>) -> io::Result<()> {
    if windows.is_empty() {
        return Ok(());
    }
    let mut conn = open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    for w in windows {
        tx.execute(
            "INSERT INTO usage_snapshots (provider, win, grp, used, resets_at, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![w.provider.as_str(), w.window, w.group.as_deref().unwrap_or_default(), w.used, w.resets_at, iso(at)],
        )
        .map_err(sql)?;
    }
    tx.execute("DELETE FROM usage_snapshots WHERE at < ?1", [iso(at - Duration::days(KEEP_DAYS))]).map_err(sql)?;
    tx.commit().map_err(sql)
}

/// One weekly reading of one provider.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// The model group (agy splits by group); empty when the CLI does not.
    pub group: String,
    /// Share used, 0 to 1.
    pub used: f64,
    pub resets_at: String,
    pub at: DateTime<Utc>,
}

/// A provider's weekly snapshots, oldest first.
pub fn weekly_snapshots(home: &Path, provider: ProviderId) -> io::Result<Vec<Snapshot>> {
    let conn = open(home)?;
    let mut stmt = conn.prepare("SELECT grp, used, resets_at, at FROM usage_snapshots WHERE provider = ?1 AND win = ?2 ORDER BY at").map_err(sql)?;
    let rows: Vec<(String, f64, String, String)> = stmt
        .query_map(params![provider.as_str(), WEEKLY], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    Ok(rows.into_iter().filter_map(|(group, used, resets_at, at)| Some(Snapshot { group, used, resets_at, at: parse_time(&at)? })).collect())
}

/// The local calendar day around a moment, as UTC times.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Day {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

pub fn local_day_of(now: DateTime<Utc>) -> Day {
    let date = now.with_timezone(&Local).date_naive();
    let midnight = |d: NaiveDate| Local.from_local_datetime(&d.and_time(NaiveTime::MIN)).earliest().map(|t| t.with_timezone(&Utc));
    let start = midnight(date).unwrap_or(now);
    let end = date.succ_opt().and_then(midnight).unwrap_or(start + Duration::days(1));
    Day { start, end }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Pace {
    /// No weekly reading: nothing is adjusted or refused.
    Unknown,
    /// Spending no faster than the day goes.
    Ok,
    /// Remaining share below the remaining time share: one effort step down.
    Behind,
    /// Remaining share below half the remaining time share: two steps down.
    FarBehind,
    /// The day's allowance is spent: new cards for this provider are refused.
    Over,
    /// The daily budget is switched off: nothing is adjusted or refused, the numbers stay.
    Off,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EffortChange {
    /// The card's effort; None for a card that named none.
    pub from: Option<String>,
    pub to: String,
}

/// Where a provider stands against today's budget. Shares are 0 to 1; `weekly_used`,
/// `allowance` and `spent` are percent points of the weekly window.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    pub provider: ProviderId,
    pub state: Pace,
    pub allowance: f64,
    /// The model group the numbers come from (the one that spent most today).
    pub group: Option<String>,
    pub weekly_used: Option<f64>,
    pub spent: Option<f64>,
    pub remaining: Option<f64>,
    /// Hours until the budget day ends: local midnight, or the weekly reset if sooner.
    pub remaining_hours: Option<f64>,
    pub time_share: Option<f64>,
    pub remaining_share: Option<f64>,
    /// Remaining share over remaining time share: 1 or more keeps pace.
    pub pace: Option<f64>,
    pub steps_down: u8,
    /// What each effort becomes on a card started now.
    pub efforts: Vec<EffortChange>,
    /// "dayStart", "firstReading" (no reading before today) or "weeklyReset" (the window reset today).
    pub counted_from: Option<&'static str>,
    pub period_end: Option<String>,
    pub weekly_resets_at: Option<String>,
    pub read_at: Option<String>,
    pub message: String,
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn local_text(time: DateTime<Utc>) -> String {
    time.with_timezone(&Local).format("%m-%d %H:%M").to_string()
}

/// One effort lowered by `steps`, never below `low` and never raised. A card
/// without an effort counts as `medium` and gets an explicit one. None: no change.
pub fn lower_effort(effort: Option<&str>, steps: u8) -> Option<String> {
    if steps == 0 {
        return None;
    }
    let from = effort.unwrap_or(ASSUMED_EFFORT);
    let index = EFFORTS.iter().position(|e| *e == from)?;
    let floor = EFFORTS.iter().position(|e| *e == FLOOR_EFFORT).unwrap_or(0).min(index);
    let to = EFFORTS[index.saturating_sub(usize::from(steps)).max(floor)];
    (effort.is_none() || to != from).then(|| to.to_string())
}

fn effort_changes(steps: u8) -> Vec<EffortChange> {
    std::iter::once(None)
        .chain(EFFORTS.iter().rev().map(|e| Some(*e)))
        .filter_map(|from| Some(EffortChange { from: from.map(str::to_string), to: lower_effort(from, steps)? }))
        .collect()
}

/// Today's spend in one model group's series (oldest first).
struct GroupSpend<'a> {
    current: &'a Snapshot,
    spent: f64,
    counted_from: &'static str,
    period_start: DateTime<Utc>,
}

fn group_spend<'a>(series: &[&'a Snapshot], day: Day, now: DateTime<Utc>) -> Option<GroupSpend<'a>> {
    let current = *series.last()?;
    // The latest reading's window has reset by now: its spend belongs to the old week, and
    // nothing is known of the new one yet, so the old window's use must not keep blocking.
    if let Some(reset) = parse_time(&current.resets_at).filter(|r| *r <= now) {
        return Some(GroupSpend { current, spent: 0.0, counted_from: "weeklyReset", period_start: reset.max(day.start) });
    }
    let before = series.iter().rev().find(|s| s.at <= day.start);
    if let Some(base) = before {
        let reset = parse_time(&base.resets_at);
        let same_window = reset.is_some_and(|r| r > current.at) && current.used + 1e-9 >= base.used;
        if same_window {
            return Some(GroupSpend { current, spent: current.used - base.used, counted_from: "dayStart", period_start: day.start });
        }
        // The window the day started in reset today: count from the reset (nothing used then).
        if let Some(reset) = reset.filter(|r| *r > day.start && *r <= current.at) {
            return Some(GroupSpend { current, spent: current.used, counted_from: "weeklyReset", period_start: reset });
        }
    }
    // No reading before midnight, but an earlier window seen today reset today: count from that reset.
    let previous_reset = series.iter().rev().find(|s| s.resets_at != current.resets_at).and_then(|s| parse_time(&s.resets_at));
    if let Some(reset) = previous_reset.filter(|r| *r > day.start && *r <= current.at) {
        return Some(GroupSpend { current, spent: current.used, counted_from: "weeklyReset", period_start: reset });
    }
    // No usable reading at the start of the day: count from today's first reading of this window.
    let first = series.iter().find(|s| s.at > day.start && s.resets_at == current.resets_at).copied().unwrap_or(current);
    Some(GroupSpend { current, spent: (current.used - first.used).max(0.0), counted_from: "firstReading", period_start: day.start })
}

/// Pure: where `provider` stands at `now` given its weekly snapshots (oldest first).
pub fn assess(provider: ProviderId, percent: u8, snapshots: &[Snapshot], now: DateTime<Utc>, day: Day) -> Budget {
    let allowance = f64::from(percent);
    let mut groups: Vec<&str> = snapshots.iter().map(|s| s.group.as_str()).collect();
    groups.sort_unstable();
    groups.dedup();
    let worst = groups
        .into_iter()
        .filter_map(|g| {
            let series: Vec<&Snapshot> = snapshots.iter().filter(|s| s.group == g).collect();
            group_spend(&series, day, now)
        })
        .max_by(|a, b| a.spent.total_cmp(&b.spent).then(a.current.used.total_cmp(&b.current.used)));
    let Some(g) = worst else {
        return Budget {
            provider,
            state: Pace::Unknown,
            allowance,
            group: None,
            weekly_used: None,
            spent: None,
            remaining: None,
            remaining_hours: None,
            time_share: None,
            remaining_share: None,
            pace: None,
            steps_down: 0,
            efforts: Vec::new(),
            counted_from: None,
            period_end: None,
            weekly_resets_at: None,
            read_at: None,
            message: format!("{provider} 주간 사용량 기록이 없어 예산을 조절하지 않습니다"),
        };
    };

    let next_reset = parse_time(&g.current.resets_at).filter(|r| *r > now);
    // A reading whose window already reset says nothing of the new week: nothing used yet.
    let weekly_used = if next_reset.is_some() { g.current.used * 100.0 } else { 0.0 };
    let end = next_reset.map_or(day.end, |r| r.min(day.end));
    let total = (end - g.period_start).num_seconds().max(1) as f64;
    let left = (end - now).num_seconds().max(0) as f64;
    let time_share = (left / total).clamp(0.0, 1.0);
    let spent = g.spent * 100.0;
    let remaining = (allowance - spent).max(0.0);
    let remaining_share = remaining / allowance;
    let hours = left / 3600.0;
    let state = if spent + 1e-9 >= allowance {
        Pace::Over
    } else if remaining_share < time_share / 2.0 {
        Pace::FarBehind
    } else if remaining_share < time_share {
        Pace::Behind
    } else {
        Pace::Ok
    };
    let steps_down = match state {
        Pace::Behind => 1,
        Pace::FarBehind => 2,
        _ => 0,
    };
    let numbers = format!("오늘 주간의 {spent:.1}%p 사용 / 하루 예산 {allowance:.0}%p, {hours:.1}시간 남음");
    let message = match state {
        Pace::Over => format!(
            "{provider} 오늘 사용 예산을 다 썼습니다 — 주간의 {spent:.1}%p 사용 / 하루 예산 {allowance:.0}%p. {}에 다시 열립니다",
            local_text(end)
        ),
        Pace::FarBehind => format!("{provider} {numbers} — 쓰는 속도가 매우 빨라 effort를 두 단계 낮춥니다"),
        Pace::Behind => format!("{provider} {numbers} — 쓰는 속도가 빨라 effort를 한 단계 낮춥니다"),
        _ => format!("{provider} {numbers} — 여유 있음"),
    };
    Budget {
        provider,
        state,
        allowance,
        group: Some(g.current.group.clone()).filter(|s| !s.is_empty()),
        weekly_used: Some(round2(weekly_used)),
        spent: Some(round2(spent)),
        remaining: Some(round2(remaining)),
        remaining_hours: Some(round2(hours)),
        time_share: Some(round2(time_share)),
        remaining_share: Some(round2(remaining_share)),
        pace: (time_share > 0.0).then(|| round2(remaining_share / time_share)),
        steps_down,
        efforts: effort_changes(steps_down),
        counted_from: Some(g.counted_from),
        period_end: Some(iso(end)),
        weekly_resets_at: Some(g.current.resets_at.clone()),
        read_at: Some(iso(g.current.at)),
        message,
    }
}

impl Budget {
    /// The same reading with the daily budget switched off: no state that refuses or lowers anything.
    pub fn switched_off(self) -> Budget {
        let spent = self.spent.map(|s| format!(" — 오늘 주간의 {s:.1}%p 사용")).unwrap_or_default();
        Budget {
            state: Pace::Off,
            steps_down: 0,
            efforts: Vec::new(),
            message: format!("{} 하루 사용 예산 꺼짐{spent} (막거나 effort를 낮추지 않음)", self.provider),
            ..self
        }
    }
}

/// One provider's budget now, from this machine's snapshots and settings (percent and on/off).
pub fn assess_now(home: &Path, provider: ProviderId, now: DateTime<Utc>) -> io::Result<Budget> {
    let budget = assess(provider, read_percent(home), &weekly_snapshots(home, provider)?, now, local_day_of(now));
    Ok(if read_enabled(home) { budget } else { budget.switched_off() })
}

/// Provider → why it may take no more cards today (policy rule `daily-budget`).
pub fn over_budget(home: &Path, now: DateTime<Utc>) -> io::Result<HashMap<ProviderId, String>> {
    let mut over = HashMap::new();
    for provider in ProviderId::ALL {
        let budget = assess_now(home, provider, now)?;
        if budget.state == Pace::Over {
            over.insert(provider, budget.message);
        }
    }
    Ok(over)
}

/// What `o budget --json` prints.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub at: String,
    /// The daily budget switch; off, every provider is `Pace::Off`.
    pub enabled: bool,
    pub percent: u8,
    /// When `o usage` last asked Codex and Antigravity (empty when never).
    pub usage_at: String,
    pub providers: Vec<Budget>,
}

pub fn report(home: &Path, now: DateTime<Utc>) -> io::Result<Report> {
    let providers = ProviderId::ALL.into_iter().map(|p| assess_now(home, p, now)).collect::<io::Result<_>>()?;
    let usage_at = probe::load_usage(home).ok().flatten().and_then(|v| v.get("at").and_then(Value::as_str).map(str::to_string)).unwrap_or_default();
    Ok(Report { at: iso(now), enabled: read_enabled(home), percent: read_percent(home), usage_at, providers })
}

pub fn report_now(home: &Path) -> io::Result<Report> {
    report(home, Utc::now())
}

/// Whether the saved Codex/Antigravity reading is missing or older than ten minutes.
pub fn usage_is_stale(home: &Path, now: DateTime<Utc>) -> bool {
    let at = probe::load_usage(home).ok().flatten().and_then(|v| v.get("at").and_then(Value::as_str).and_then(parse_time));
    at.is_none_or(|at| now - at > Duration::minutes(STALE_MINUTES))
}

/// Asks the CLIs again (`o usage`) when the saved reading is stale; saving it records the snapshots.
pub fn refresh_if_stale(home: &Path, now: DateTime<Utc>) -> Option<UsageReading> {
    if !usage_is_stale(home, now) {
        return None;
    }
    let reading = probe::read_usage();
    if let Err(error) = probe::save_usage(home, &reading) {
        eprintln!("{error}");
    }
    Some(reading)
}
