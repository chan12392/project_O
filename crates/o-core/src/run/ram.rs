//! The RAM cap on card starts (ADR-0037): while this machine's used memory is at or above the
//! cap, a card waits for its turn instead of starting. It only holds back cards beyond the
//! first, so O never stops for memory that other programs use, and it never ends a running card.
//! Kept in `<O home>/settings.json` beside the daily budget; `o ram`, Settings → 에이전트.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::usage::budget::{read_settings, write_setting};

pub const ENABLED_KEY: &str = "ramLimitEnabled";
pub const PERCENT_KEY: &str = "ramLimitPercent";
pub const DEFAULT_PERCENT: u8 = 70;
pub const MIN_PERCENT: u8 = 50;
pub const MAX_PERCENT: u8 = 95;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RamReport {
    pub enabled: bool,
    pub percent: u8,
    /// This machine's used memory now; `None` where O cannot read it (the cap then does nothing).
    pub used: Option<u8>,
}

/// A cap as settings.json may hold it: a whole number from 50 to 95.
pub fn valid_percent(value: &Value) -> Option<u8> {
    let n = value.as_f64()?;
    (n.fract() == 0.0 && (f64::from(MIN_PERCENT)..=f64::from(MAX_PERCENT)).contains(&n)).then_some(n as u8)
}

/// Whether the cap is on; a missing or invalid setting is on (the default).
pub fn read_enabled(home: &Path) -> bool {
    read_settings(home).get(ENABLED_KEY).and_then(Value::as_bool).unwrap_or(true)
}

/// The cap in percent; a missing or invalid setting is the default 70.
pub fn read_percent(home: &Path) -> u8 {
    read_settings(home).get(PERCENT_KEY).and_then(valid_percent).unwrap_or(DEFAULT_PERCENT)
}

pub fn write_enabled(home: &Path, enabled: bool) -> Result<bool, String> {
    write_setting(home, ENABLED_KEY, Value::Bool(enabled))?;
    Ok(enabled)
}

/// Saves the cap; anything but a whole number from 50 to 95 is refused.
pub fn write_percent(home: &Path, value: &Value) -> Result<u8, String> {
    let percent = valid_percent(value).ok_or_else(|| format!("RAM 상한은 {MIN_PERCENT}~{MAX_PERCENT} 사이의 정수입니다: {value}"))?;
    write_setting(home, PERCENT_KEY, Value::from(percent))?;
    Ok(percent)
}

pub fn report(home: &Path) -> RamReport {
    RamReport { enabled: read_enabled(home), percent: read_percent(home), used: crate::sys::memory::used_percent() }
}

/// Why another card may not start now: the cap is on and used memory is at or above it.
/// `running` is how many cards hold a slot; with none, the card starts whatever the memory.
pub fn hold_back(home: &Path, running: usize) -> Option<String> {
    if running == 0 || !read_enabled(home) {
        return None;
    }
    over(read_percent(home), crate::sys::memory::used_percent()?)
}

fn over(cap: u8, used: u8) -> Option<String> {
    (used >= cap).then(|| format!("RAM 사용률 {used}%가 상한 {cap}% 이상이라 대기"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("o-ram-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_cap_defaults_to_on_at_70_and_saves_only_valid_values() {
        let home = home("settings");
        assert!(read_enabled(&home));
        assert_eq!(read_percent(&home), 70);
        assert_eq!(write_percent(&home, &Value::from(80)), Ok(80));
        assert!(write_percent(&home, &Value::from(40)).is_err());
        assert!(write_percent(&home, &Value::from(70.5)).is_err());
        assert_eq!(read_percent(&home), 80);
        write_enabled(&home, false).unwrap();
        assert!(!read_enabled(&home));
        assert_eq!(hold_back(&home, 3), None);
    }

    #[test]
    fn only_cards_beyond_the_first_wait_and_only_at_or_above_the_cap() {
        assert_eq!(hold_back(&home("first"), 0), None);
        assert_eq!(over(70, 69), None);
        assert_eq!(over(70, 70).as_deref(), Some("RAM 사용률 70%가 상한 70% 이상이라 대기"));
    }
}
