//! Subscription windows read through each official CLI, the way Orca reads them.
//! Each provider file holds its own probe and parser (`Provider::usage_windows`):
//! Codex answers `account/rateLimits/read` on a short-lived read-only `codex app-server`,
//! Antigravity answers `agy -p /usage --output-format json` without spending a turn,
//! Claude reports its windows inside every run's stream. Nothing here touches
//! credentials or provider backends.

pub mod budget;
pub mod probe;

use serde::{Deserialize, Serialize};

use crate::run::plan::ProviderId;
use crate::sys::js::ser_num;

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
