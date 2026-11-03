//! YOLO 모드 (ADR-0014): the person's switch that lets the lead and the cards edit files and
//! run commands without asking (each CLI's official flag, `providers::*::yolo_*`), and the
//! limits on running a lead's plan without 실행.
//!
//! Kept by the machine like the daily budget switch (ADR-0013): `<O home>/settings.json`
//! `yoloEnabled` (only `true` turns it on) and the auto-run limits, so the app, the dev API
//! and `o` read one answer. `o dispatch`, `o lead send` and `o lead dispatch` take
//! `--mode yolo|normal` for one run; a plan can never turn it on (its fields are data only).
//! (Not a bare YOLO flag: Codex takes that name as an alias of its bypass flag, so the guard
//! reads it as that in any command and code; see `o_guard::rules`.)
//!
//! Auto-run: with YOLO on *and* the daily budget on, a plan that passed the policy runs at once
//! only when every limit holds (`check_auto_run`); otherwise it waits for 실행 with the reason.
//! The checks live here so every shell agrees. Pure except the settings file.

use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use serde_json::{json, Value};

use crate::run::plan::Plan;
use crate::usage::budget::{self, read_settings, write_setting};

/// The switch in `<O home>/settings.json`; only `true` turns YOLO on.
pub const ENABLED_KEY: &str = "yoloEnabled";
pub const MAX_CARDS_KEY: &str = "autoRunMaxCards";
pub const PER_HOUR_KEY: &str = "autoRunPerHour";
pub const DEDUPE_KEY: &str = "autoRunDedupeMinutes";

/// One adjustable limit: the range Settings offers and the default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Bound {
    pub min: u32,
    pub max: u32,
    pub default: u32,
}

/// Cards in a plan that may still run without 실행.
pub const MAX_CARDS: Bound = Bound { min: 1, max: 6, default: 3 };
/// Auto-runs per lead thread in the last hour.
pub const PER_HOUR: Bound = Bound { min: 1, max: 6, default: 2 };
/// Minutes in which the same plan is not auto-run again.
pub const DEDUPE_MINUTES: Bound = Bound { min: 10, max: 120, default: 30 };

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    pub max_cards: u32,
    pub per_hour: u32,
    pub dedupe_minutes: u32,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_cards: MAX_CARDS.default, per_hour: PER_HOUR.default, dedupe_minutes: DEDUPE_MINUTES.default }
    }
}

/// A whole number inside the bound.
fn within(value: &Value, bound: Bound) -> Option<u32> {
    let n = value.as_f64()?;
    (n.fract() == 0.0 && (f64::from(bound.min)..=f64::from(bound.max)).contains(&n)).then_some(n as u32)
}

/// Whether YOLO is on; missing or anything but `true` is off (the default).
pub fn read_enabled(home: &Path) -> bool {
    read_settings(home).get(ENABLED_KEY).and_then(Value::as_bool).unwrap_or(false)
}

/// Turns YOLO on or off (`o yolo --on|--off`, Settings → 에이전트).
pub fn write_enabled(home: &Path, enabled: bool) -> Result<bool, String> {
    write_setting(home, ENABLED_KEY, Value::Bool(enabled))?;
    Ok(enabled)
}

/// The auto-run limits; a missing or out-of-range value is its default.
pub fn read_limits(home: &Path) -> Limits {
    let settings = read_settings(home);
    let get = |key: &str, bound: Bound| settings.get(key).and_then(|v| within(v, bound)).unwrap_or(bound.default);
    Limits { max_cards: get(MAX_CARDS_KEY, MAX_CARDS), per_hour: get(PER_HOUR_KEY, PER_HOUR), dedupe_minutes: get(DEDUPE_KEY, DEDUPE_MINUTES) }
}

/// Saves the limits given (`None` keeps one); a value outside its range refuses the whole change.
pub fn write_limits(home: &Path, max_cards: Option<&Value>, per_hour: Option<&Value>, dedupe_minutes: Option<&Value>) -> Result<Limits, String> {
    let checks = [
        (MAX_CARDS_KEY, max_cards, MAX_CARDS, "자동 실행할 계획의 카드 수"),
        (PER_HOUR_KEY, per_hour, PER_HOUR, "한 시간에 자동 실행할 횟수"),
        (DEDUPE_KEY, dedupe_minutes, DEDUPE_MINUTES, "같은 계획을 다시 자동 실행하지 않는 시간(분)"),
    ];
    let mut valid = Vec::new();
    for (key, value, bound, what) in checks {
        let Some(value) = value else { continue };
        let n = within(value, bound).ok_or_else(|| format!("{what}는 {}~{} 사이의 정수입니다: {value}", bound.min, bound.max))?;
        valid.push((key, n));
    }
    for (key, n) in valid {
        write_setting(home, key, Value::from(n))?;
    }
    Ok(read_limits(home))
}

/// What `o yolo --json` prints and Settings reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub enabled: bool,
    /// Auto-run needs the daily budget on too (ADR-0013's switch).
    pub budget_enabled: bool,
    pub auto_run: Limits,
    pub bounds: Value,
}

pub fn report(home: &Path) -> Report {
    Report {
        enabled: read_enabled(home),
        budget_enabled: budget::read_enabled(home),
        auto_run: read_limits(home),
        bounds: json!({ "maxCards": MAX_CARDS, "perHour": PER_HOUR, "dedupeMinutes": DEDUPE_MINUTES }),
    }
}

/// `--mode yolo|normal` (or `--mode=…`) taken out of `args`: `Some(true)` for yolo, `Some(false)`
/// for normal, `None` when not given. Two different modes are refused.
pub fn take_flag(args: Vec<String>) -> Result<(Option<bool>, Vec<String>), String> {
    let mut yolo = None;
    let mut rest = Vec::new();
    let mut items = args.into_iter();
    while let Some(arg) = items.next() {
        let value = match arg.as_str() {
            "--mode" => items.next().unwrap_or_default(),
            other => match other.strip_prefix("--mode=") {
                Some(value) => value.to_string(),
                None => {
                    rest.push(arg);
                    continue;
                }
            },
        };
        let on = match value.as_str() {
            "yolo" => true,
            "normal" => false,
            _ => return Err(format!("--mode는 yolo 또는 normal입니다: {value}")),
        };
        if yolo.is_some_and(|y| y != on) {
            return Err("--mode를 서로 다르게 두 번 주었습니다".into());
        }
        yolo = Some(on);
    }
    Ok((yolo, rest))
}

/// One run's YOLO: the flag when given, else the saved switch.
pub fn resolve(home: &Path, flag: Option<bool>) -> bool {
    flag.unwrap_or_else(|| read_enabled(home))
}

/// An earlier auto-run of the same lead thread: when, and which plan (`plan_hash`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastAutoRun {
    pub at: DateTime<Utc>,
    pub hash: String,
}

/// What the auto-run decision looks at, read from the thread and its runs by `lead::autorun`.
#[derive(Clone, Copy)]
pub struct AutoRunFacts<'a> {
    pub budget_on: bool,
    pub cards: usize,
    /// Runs this thread started that are still going.
    pub running: &'a [String],
    pub past: &'a [PastAutoRun],
    pub hash: &'a str,
    pub now: DateTime<Utc>,
}

/// Whether a plan that passed the policy may run without 실행 (YOLO is on); `Err` is the
/// reason the plan card shows while it waits for 실행.
pub fn check_auto_run(facts: &AutoRunFacts, limits: Limits) -> Result<(), String> {
    if !facts.budget_on {
        return Err("하루 사용 예산이 꺼져 있어 자동 실행하지 않습니다. 실행을 눌러 주세요".into());
    }
    if facts.cards > limits.max_cards as usize {
        return Err(format!("자동 실행 한도: 카드 {}장 — {}장까지만 자동 실행합니다", facts.cards, limits.max_cards));
    }
    if let Some(run) = facts.running.first() {
        return Err(format!("자동 실행 한도: 이 대화의 실행 {run}가 아직 진행 중입니다"));
    }
    let hour_ago = facts.now - Duration::hours(1);
    let this_hour = facts.past.iter().filter(|p| p.at > hour_ago).count();
    if this_hour >= limits.per_hour as usize {
        return Err(format!("자동 실행 한도: 이번 시간 {this_hour}회 사용 (한 시간에 {}회까지)", limits.per_hour));
    }
    let window = facts.now - Duration::minutes(i64::from(limits.dedupe_minutes));
    if let Some(same) = facts.past.iter().filter(|p| p.hash == facts.hash && p.at > window).map(|p| p.at).max() {
        let minutes = (facts.now - same).num_minutes().max(0);
        return Err(format!("자동 실행 한도: 같은 계획을 {minutes}분 전에 자동 실행했습니다 ({}분 안에는 다시 하지 않음)", limits.dedupe_minutes));
    }
    Ok(())
}

/// Hash the complete plan with whitespace normalized in user-written text.
pub fn plan_hash(plan: &Plan) -> String {
    let mut normalized = plan.clone();
    normalized.title = normalized.title.split_whitespace().collect::<Vec<_>>().join(" ");
    for card in &mut normalized.cards {
        card.task = card.task.split_whitespace().collect::<Vec<_>>().join(" ");
        if let Some(role) = &mut card.role { *role = role.split_whitespace().collect::<Vec<_>>().join(" "); }
    }
    let text = serde_json::to_string(&normalized).expect("plans contain only serializable strings");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}
