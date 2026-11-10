//! `o usage`: asks each official CLI that has a usage probe (`Provider::usage_windows`)
//! for its subscription windows, and keeps the last reading in `usage.json`.

use std::fs;
use std::io;
use std::path::Path;
use std::thread;

use serde_json::Value;

use crate::agents::clean;
use crate::providers;
use crate::run::mode::Profile;
use crate::run::plan::ProviderId;
use crate::run::store::to_pretty;
use crate::sys::js::now_iso;
use crate::usage::{Unavailable, UsageReading};

/// The profile the saved reading was taken in, when it says.
pub fn saved_profile(home: &Path) -> Option<Profile> {
    load_usage(home).ok().flatten().and_then(|v| v.get("profile").and_then(Value::as_str).and_then(Profile::parse))
}

/// A reading in the profile `asked` for: the login whose subscription a run in that profile
/// spends. Not told, it is the profile the last reading was taken in; with none on record, the
/// project_O profile runs use unless told otherwise, and the person's own setup when that
/// profile answers nothing (nobody signed in there).
pub fn read_usage(home: &Path, asked: Option<Profile>) -> UsageReading {
    if let Some(profile) = asked.or_else(|| saved_profile(home)) {
        return read_in(profile);
    }
    let reading = read_in(Profile::ProjectO);
    if reading.windows.is_empty() {
        let own = read_in(Profile::Own);
        if !own.windows.is_empty() {
            return own;
        }
    }
    reading
}

/// Every probe at once; windows and failures keep the provider order (codex, agy).
fn read_in(profile: Profile) -> UsageReading {
    let mut reading = UsageReading { at: now_iso(), profile: Some(profile), windows: Vec::new(), unavailable: Vec::new() };
    let root = profile.is_clean().then(clean::default_root);
    let root = root.as_deref();
    let outcomes: Vec<_> = thread::scope(|s| {
        let probes: Vec<_> = ProviderId::ALL.into_iter().map(|p| (p, s.spawn(move || providers::get(p).usage_windows(root)))).collect();
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
