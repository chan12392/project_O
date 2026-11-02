//! `o usage`: asks each official CLI that has a usage probe (`Provider::usage_windows`)
//! for its subscription windows, and keeps the last reading in `usage.json`.

use std::fs;
use std::io;
use std::path::Path;
use std::thread;

use serde_json::Value;

use crate::providers;
use crate::run::plan::ProviderId;
use crate::run::store::to_pretty;
use crate::sys::js::now_iso;
use crate::usage::{Unavailable, UsageReading};

/// Every probe at once; windows and failures keep the provider order (codex, agy).
pub fn read_usage() -> UsageReading {
    let mut reading = UsageReading { at: now_iso(), windows: Vec::new(), unavailable: Vec::new() };
    let outcomes: Vec<_> = thread::scope(|s| {
        let probes: Vec<_> = ProviderId::ALL.into_iter().map(|p| (p, s.spawn(move || providers::get(p).usage_windows()))).collect();
        probes.into_iter().map(|(provider, probe)| (provider, probe.join())).collect()
    });
    for (provider, outcome) in outcomes {
        let outcome = match outcome {
            Ok(None) => continue,
            Ok(Some(outcome)) => outcome,
            Err(_) => Err("내부 오류".into()),
        };
        match outcome {
            Ok(windows) => reading.windows.extend(windows),
            Err(reason) => reading.unavailable.push(Unavailable { provider: provider.as_str().into(), reason }),
        }
    }
    reading
}

pub fn save_usage(home: &Path, reading: &UsageReading) -> io::Result<()> {
    fs::create_dir_all(home)?;
    fs::write(home.join("usage.json"), to_pretty(reading)?)?;
    // The daily budget counts today's spend from these snapshots.
    let at = crate::sys::js::parse_time(&reading.at).unwrap_or_else(chrono::Utc::now);
    crate::usage::budget::record(home, &reading.windows, at)
}

/// The last saved reading as written, without asking any CLI.
pub fn load_usage(home: &Path) -> io::Result<Option<Value>> {
    let path = home.join("usage.json");
    if !path.exists() {
        return Ok(None);
    }
    serde_json::from_str(&fs::read_to_string(path)?).map(Some).map_err(io::Error::other)
}
