//! Subscription windows read through each official CLI, the way Orca reads them:
//! Codex answers `account/rateLimits/read` on a short-lived read-only `codex app-server`,
//! Antigravity answers `agy -p /usage --output-format json` without spending a turn.
//! Claude reports its windows inside every run's stream (see `providers`).
//! Pure parsers; nothing here touches credentials or provider backends.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::js::{iso_from_secs, js_string, number_text, ser_num};
use crate::plan::ProviderId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub provider: ProviderId,
    /// A stable id: five_hour, seven_day, or the CLI's own bucket id (gemini-weekly).
    pub window: String,
    /// The model group a window covers, when the CLI splits by group (agy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Share used, 0 to 1.
    #[serde(serialize_with = "ser_num")]
    pub used: f64,
    pub resets_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unavailable {
    pub provider: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct UsageReading {
    pub at: String,
    pub windows: Vec<UsageWindow>,
    /// Providers whose CLI could not answer, with why.
    pub unavailable: Vec<Unavailable>,
}

impl UsageReading {
    /// What a screen shows before any reading was saved.
    pub fn empty() -> Self {
        UsageReading { at: String::new(), windows: Vec::new(), unavailable: Vec::new() }
    }
}

fn window_name(minutes: Option<&Value>) -> String {
    match minutes.and_then(Value::as_f64) {
        Some(300.0) => "five_hour".into(),
        Some(10080.0) => "seven_day".into(),
        Some(m) => format!("{}m", number_text(m)),
        None => "window".into(),
    }
}

/// The `account/rateLimits/read` result: primary and secondary windows of the codex limit.
pub fn parse_codex_rate_limits(result: &Value) -> Vec<UsageWindow> {
    let Some(limits) = result.get("rateLimits").and_then(Value::as_object) else { return Vec::new() };
    [limits.get("primary"), limits.get("secondary")]
        .into_iter()
        .flatten()
        .filter_map(|w| {
            let w = w.as_object()?;
            let used = w.get("usedPercent").and_then(Value::as_f64)?;
            let resets = w.get("resetsAt").and_then(Value::as_f64)?;
            Some(UsageWindow {
                provider: ProviderId::Codex,
                window: window_name(w.get("windowDurationMins")),
                group: None,
                used: used / 100.0,
                resets_at: iso_from_secs(resets),
            })
        })
        .collect()
}

/// The `agy -p /usage` JSON envelope: groups of buckets with a remaining fraction each.
pub fn parse_agy_usage(stdout: &str) -> Vec<UsageWindow> {
    let Ok(data) = serde_json::from_str::<Value>(stdout) else { return Vec::new() };
    let groups = data.get("command").and_then(|c| c.get("data")).and_then(|d| d.get("groups")).and_then(Value::as_array);
    let Some(groups) = groups else { return Vec::new() };
    let mut out = Vec::new();
    for group in groups.iter().filter_map(Value::as_object) {
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else { continue };
        for b in buckets.iter().filter_map(Value::as_object) {
            let (Some(remaining), Some(reset)) = (b.get("remaining_fraction").and_then(Value::as_f64), b.get("reset_time").and_then(Value::as_str)) else {
                continue;
            };
            if b.get("disabled") == Some(&Value::Bool(true)) {
                continue;
            }
            let window = match b.get("window").and_then(Value::as_str) {
                Some("5h") => "five_hour".to_string(),
                Some("weekly") => "seven_day".to_string(),
                _ => b.get("id").filter(|v| !v.is_null()).map(js_string).unwrap_or_else(|| "window".into()),
            };
            out.push(UsageWindow {
                provider: ProviderId::Agy,
                window,
                group: group.get("name").and_then(Value::as_str).map(str::to_string),
                used: (1.0 - remaining).clamp(0.0, 1.0),
                resets_at: reset.to_string(),
            });
        }
    }
    out
}
