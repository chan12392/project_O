//! O's state directory. Fixed per machine on purpose: if a caller could point
//! O at another directory, it could also escape the cooldowns and caps kept
//! here. Tests pass their own `home` to these functions instead.
//!
//! ```text
//! runs/<runId>/plan.json · events.jsonl · cards/<id>.raw.jsonl · stop · clean (o dispatch --clean)
//!              · yolo (YOLO 모드, ADR-0014) · runner.json (the runner's pid and start time)
//! active/<runId>.<cardId>.json   running card markers ({ pid, provider })
//! o.db                           the run index, card starts (daily cap), held slots, cooldowns and
//!                                refused plans; dispatch and the runner write it, cards never (see `index`)
//! ```
//!
//! Older builds kept starts, cooldowns and refused plans in ledger.jsonl,
//! cooldowns.json and blocked.jsonl; the first open of o.db imports them and
//! renames them to `*.imported`.

use std::collections::hash_map::RandomState;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::Value;

use crate::run::events::{CardStatus, OEvent};
use crate::run::index::{self, sql, title_text, RunQuery};
use crate::run::mode::RunMode;
use crate::sys::js::{iso, local_day, parse_time};
use crate::run::plan::{Plan, ProviderId};
use crate::policy::{empty_counts, PolicyContext, Violation, LIMITS};
use crate::sys::proc;

/// `%LOCALAPPDATA%/project_O`, or `~/.local/state/project_O` elsewhere.
pub fn default_home() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| user_home().join(".local").join("state"));
    base.join("project_O")
}

/// The user's home folder, as `os.homedir()` finds it.
pub fn user_home() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
}

/// Run ids name folders: letters, digits, "-" and "_" only, so none can climb out of the runs folder.
static RUN_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$").unwrap());

pub fn is_run_id(value: &str) -> bool {
    RUN_ID.is_match(value)
}

pub fn run_dir(home: &Path, run_id: &str) -> PathBuf {
    home.join("runs").join(run_id)
}

/// `20261101-100916-402x`: the UTC start second and four random base-36 characters.
pub fn new_run_id(now: DateTime<Utc>) -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u128(now.timestamp_nanos_opt().unwrap_or_default() as u128 ^ u128::from(std::process::id()));
    let mut n = hasher.finish();
    let mut tail = String::new();
    for _ in 0..4 {
        tail.push(char::from_digit((n % 36) as u32, 36).unwrap_or('0'));
        n /= 36;
    }
    format!("{}-{tail}", now.format("%Y%m%d-%H%M%S"))
}

pub fn create_run(home: &Path, run_id: &str, plan: &Plan) -> io::Result<()> {
    let dir = run_dir(home, run_id);
    fs::create_dir_all(dir.join("cards"))?;
    fs::create_dir_all(dir.join("outbox"))?;
    fs::write(dir.join("plan.json"), to_pretty(plan)?)
}

/// plan.json as written, unchecked: `o _run` checks it again before running anything.
pub fn read_plan(home: &Path, run_id: &str) -> io::Result<Value> {
    let text = fs::read_to_string(run_dir(home, run_id).join("plan.json"))?;
    serde_json::from_str(&text).map_err(io::Error::other)
}

/// The card ids of a run's plan.
pub fn plan_card_ids(plan: &Value) -> io::Result<Vec<String>> {
    let cards = plan.get("cards").and_then(Value::as_array).ok_or_else(|| io::Error::other("plan.json에 cards가 없습니다"))?;
    Ok(cards.iter().map(|c| c.get("id").and_then(Value::as_str).unwrap_or_default().to_string()).collect())
}

/// Serializes threads as well as cooperating processes using this run's lock file.
static APPEND: Mutex<()> = Mutex::new(());

pub(crate) fn with_events<T>(home: &Path, run_id: &str, f: impl FnOnce(&mut std::fs::File) -> io::Result<T>) -> io::Result<T> {
    let _guard = APPEND.lock().unwrap_or_else(|e| e.into_inner());
    let dir = run_dir(home, run_id);
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("events.lock"))?;
    lock.lock()?;
    let mut file = OpenOptions::new().create(true).read(true).append(true).open(dir.join("events.jsonl"))?;
    // Preserve an interrupted record as an invalid line; never join it to the next event.
    if file.metadata()?.len() > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0];
        file.read_exact(&mut last)?;
        if last[0] != b'\n' { file.write_all(b"\n")?; file.sync_data()?; }
    }
    f(&mut file)
}

pub(crate) fn event_text(file: &mut std::fs::File) -> io::Result<String> {
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

/// Read a stable event log without changing a partially written record.
pub(crate) fn locked_event_text(home: &Path, run_id: &str) -> io::Result<String> {
    with_locked_event_text(home, run_id, |text| text)
}

/// Keep the event snapshot stable until its dependent operation finishes.
pub(crate) fn with_locked_event_text<T>(home: &Path, run_id: &str, f: impl FnOnce(String) -> T) -> io::Result<T> {
    let _guard = APPEND.lock().unwrap_or_else(|e| e.into_inner());
    let dir = run_dir(home, run_id);
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("events.lock"))?;
    lock.lock()?;
    Ok(f(fs::read_to_string(dir.join("events.jsonl"))?))
}

pub(crate) fn append_value(file: &mut std::fs::File, event: &Value) -> io::Result<()> {
    file.write_all(format!("{}\n", serde_json::to_string(event).map_err(io::Error::other)?).as_bytes())?;
    file.sync_data()
}

pub fn append_event(home: &Path, run_id: &str, event: &OEvent) -> io::Result<()> {
    let value = serde_json::to_value(event).map_err(io::Error::other)?;
    with_events(home, run_id, |file| append_value(file, &value))
}

fn events_text(home: &Path, run_id: &str) -> String {
    fs::read(run_dir(home, run_id).join("events.jsonl")).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default()
}

pub fn read_events(home: &Path, run_id: &str) -> Vec<OEvent> {
    parse_events(&events_text(home, run_id))
}

/// The events as JSON, every field kept: what `o events` and `o runs` pass on to screens.
pub fn read_event_values(home: &Path, run_id: &str) -> Vec<Value> {
    parse_event_values(&events_text(home, run_id))
}

/// events.jsonl lines, skipping any that do not parse. A run that is still going
/// can be read between O's append and its newline, so the last line may be half written.
pub fn parse_event_values(text: &str) -> Vec<Value> {
    text.split('\n').filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect()
}

/// Typed events; lines of a shape this build does not know are skipped like half-written ones.
pub fn parse_events(text: &str) -> Vec<OEvent> {
    parse_event_values(text).into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect()
}

pub fn list_runs(home: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(home.join("runs")) else { return Vec::new() };
    let mut names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub run_id: String,
    pub events: Vec<Value>,
}

/// Runs with a start event, newest first by that start (run ids sort the same way, but the event is the record).
pub fn newest_first(runs: Vec<RunRecord>) -> Vec<RunRecord> {
    let start_of = |run: &RunRecord| -> String {
        match run.events.first() {
            Some(e) if e.get("t").and_then(Value::as_str) == Some("run.start") => e.get("at").and_then(Value::as_str).unwrap_or_default().to_string(),
            _ => String::new(),
        }
    };
    let mut kept: Vec<(String, RunRecord)> = runs.into_iter().map(|r| (start_of(&r), r)).filter(|(s, _)| !s.is_empty()).collect();
    kept.sort_by(|(a, ra), (b, rb)| b.cmp(a).then_with(|| rb.run_id.cmp(&ra.run_id)));
    kept.into_iter().map(|(_, r)| r).collect()
}

/// The runs `o runs --json` and the app read, through the index brought up to date first.
pub fn read_runs(home: &Path, query: &RunQuery) -> io::Result<Vec<RunRecord>> {
    let mut conn = index::open(home)?;
    index::ingest(&mut conn, home)?;
    let mut runs = index::runs(&conn, query)?;
    let active = active_markers(home);
    for run in &mut runs {
        let card = run.events.iter().find(|event| event["t"] == "run.followup").and_then(|event| event["cardId"].as_str());
        if let Some(card) = card.filter(|_| !run.events.iter().any(|event| event["t"] == "run.end")) {
            if !run_is_live(home, &run.run_id) && !active.iter().any(|marker| marker.run_id == run.run_id) {
                let at = crate::sys::js::now_iso();
                if !run.events.iter().any(|event| event["t"] == "card.end" && event["cardId"] == card) {
                    run.events.push(serde_json::to_value(OEvent::CardEnd { at: at.clone(), card_id: card.into(), status: CardStatus::Failed, result: Some("후속 실행이 중단됐습니다. 다시 보내 주세요".into()), exit_code: None }).map_err(io::Error::other)?);
                }
                run.events.push(serde_json::to_value(OEvent::RunEnd { at, run_id: run.run_id.clone(), status: CardStatus::Failed }).map_err(io::Error::other)?);
            }
        }
    }
    Ok(runs)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMarker {
    pub run_id: String,
    pub card_id: String,
    pub provider: ProviderId,
    pub pid: u32,
}

pub fn mark_active(home: &Path, marker: &ActiveMarker) -> io::Result<()> {
    let dir = home.join("active");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{}.{}.json", marker.run_id, marker.card_id)), serde_json::to_string(marker).map_err(io::Error::other)?)
}

pub fn clear_active(home: &Path, run_id: &str, card_id: &str) {
    let _ = fs::remove_file(home.join("active").join(format!("{run_id}.{card_id}.json")));
}

/// Live markers only: a marker whose process is gone is stale and ignored.
pub fn active_markers(home: &Path) -> Vec<ActiveMarker> {
    let Ok(entries) = fs::read_dir(home.join("active")) else { return Vec::new() };
    let mut names: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    names.sort();
    names
        .iter()
        .filter_map(|path| {
            // A half-written marker from a crashed runner no longer counts.
            let marker: ActiveMarker = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
            proc::is_alive(marker.pid).then_some(marker)
        })
        .collect()
}

pub fn record_start(home: &Path, run_id: &str, card_id: &str, provider: ProviderId, at: DateTime<Utc>) -> io::Result<()> {
    index::open(home)?
        .execute("INSERT INTO starts (run_id, card_id, provider, at) VALUES (?1, ?2, ?3, ?4)", params![run_id, card_id, provider.as_str(), iso(at)])
        .map_err(sql)?;
    Ok(())
}

/// Cards started on the local day of `day`. Start times are stored as UTC ISO
/// text, which sorts like time: only the last two days can share a local day.
pub fn started_on(home: &Path, day: DateTime<Utc>) -> io::Result<usize> {
    started_in(&index::open(home)?, day)
}

fn started_in(conn: &rusqlite::Connection, day: DateTime<Utc>) -> io::Result<usize> {
    let mut stmt = conn.prepare("SELECT at FROM starts WHERE at >= ?1").map_err(sql)?;
    let since = iso(day - Duration::days(2));
    let times: Vec<String> = stmt.query_map([since], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    let today = local_day(day);
    Ok(times.iter().filter(|at| parse_time(at).is_some_and(|at| local_day(at) == today)).count())
}

/// A card's hold on a slot and on one of the day's starts, taken before its CLI starts
/// (see `reserve_slot`). Given back with `release_slot`.
#[derive(Debug)]
pub struct Reservation {
    pub run_id: String,
    pub card_id: String,
    pub provider: ProviderId,
    slot_row: i64,
    start_row: i64,
}

pub enum Reserved {
    Granted(Reservation),
    /// The slots are full; the reason says which cap.
    Busy(String),
    /// Today's starts are all used.
    DayFull,
}

/// Count durable leases until release or recovery, plus legacy slots and live markers.
fn held_slots(home: &Path, tx: &rusqlite::Transaction) -> io::Result<Vec<ProviderId>> {
    let rows: Vec<(String, String, String)> = {
        let mut stmt = tx.prepare("SELECT run_id, card_id, provider FROM slots").map_err(sql)?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    let mut held = Vec::new();
    let mut cards = HashSet::new();
    for (run_id, card_id, provider) in rows {
        // Legacy slots lack process identity, so their owners cannot be proven gone.
        held.push(ProviderId::parse(&provider).ok_or_else(|| io::Error::other("예약의 공급자를 확인할 수 없어 동시 실행 상한을 계산하지 못했습니다"))?);
        cards.insert((run_id, card_id));
    }
    // Older versions could delete the slot while retaining its lease.
    let mut orphaned = tx.prepare("SELECT l.run_id, l.card_id, s.provider FROM resource_leases l LEFT JOIN slots ON slots.id = l.slot_id LEFT JOIN starts s ON s.rowid = l.start_id AND s.run_id = l.run_id AND s.card_id = l.card_id WHERE slots.id IS NULL").map_err(sql)?;
    let rows = orphaned.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(sql)?;
    for row in rows {
        let (run_id, card_id, provider) = row.map_err(sql)?;
        let provider = provider.as_deref().and_then(ProviderId::parse).ok_or_else(|| io::Error::other("예약의 공급자를 확인할 수 없어 동시 실행 상한을 계산하지 못했습니다"))?;
        held.push(provider);
        cards.insert((run_id, card_id));
    }
    for marker in active_markers(home) {
        if !cards.contains(&(marker.run_id, marker.card_id)) {
            held.push(marker.provider);
        }
    }
    Ok(held)
}

/// Every card holding a slot right now, by provider (what `o dispatch` checks against the caps).
pub fn held_by_provider(home: &Path) -> io::Result<HashMap<ProviderId, usize>> {
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let mut counts = empty_counts();
    for provider in held_slots(home, &tx)? {
        *counts.entry(provider).or_default() += 1;
    }
    tx.commit().map_err(sql)?;
    Ok(counts)
}

/// Takes a slot and one of today's starts for a card in one write lock, so runners of
/// separate dispatches never both see the last free slot or the day's last start.
/// The start counts from here; `release_slot` gives it back if the CLI never starts.
pub fn reserve_slot(home: &Path, run_id: &str, card: &crate::run::plan::Card, project: &str, now: DateTime<Utc>) -> io::Result<Reserved> {
    let card_id = &card.id;
    let provider = card.provider;
    let resources = super::resources::normalize(project, &card.resources)?;
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    if started_in(&tx, now)? >= LIMITS.max_cards_per_day {
        return Ok(Reserved::DayFull);
    }
    let held = held_slots(home, &tx)?;
    {
        let mut stmt = tx.prepare("SELECT resources FROM resource_leases").map_err(sql)?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0)).map_err(sql)?;
        for row in rows {
            let raw = row.map_err(sql)?;
            let existing: Vec<super::resources::Key> = serde_json::from_str(&raw).map_err(io::Error::other)?;
            if super::resources::conflicts(&resources, &existing) {
                return Ok(Reserved::Busy("공용 자원 사용 중 또는 종료 확인 불가로 대기".into()));
            }
        }
    }
    let legacy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM slots WHERE id NOT IN (SELECT slot_id FROM resource_leases))", [], |r| r.get(0)).map_err(sql)?;
    if legacy || active_markers(home).iter().any(|m| !tx.query_row("SELECT EXISTS(SELECT 1 FROM resource_leases WHERE run_id=?1 AND card_id=?2)", params![m.run_id, m.card_id], |r| r.get::<_, bool>(0)).unwrap_or(false)) {
        return Ok(Reserved::Busy("기존 실행의 자원 범위를 확인할 수 없어 대기".into()));
    }
    if held.len() >= LIMITS.max_active_total {
        tx.commit().map_err(sql)?;
        return Ok(Reserved::Busy(format!("전체 동시 실행 {}개가 차서 대기", LIMITS.max_active_total)));
    }
    let max = LIMITS.max_active_per_provider(provider);
    if held.iter().filter(|p| **p == provider).count() >= max {
        tx.commit().map_err(sql)?;
        return Ok(Reserved::Busy(format!("{provider} 동시 실행 {max}개가 차서 대기")));
    }
    tx.execute(
        "INSERT INTO slots (run_id, card_id, provider, owner) VALUES (?1, ?2, ?3, ?4)",
        params![run_id, card_id, provider.as_str(), std::process::id()],
    )
    .map_err(sql)?;
    let slot_row = tx.last_insert_rowid();
    let owner_started = proc::started_at(std::process::id()).and_then(|at| i64::try_from(at).ok());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let owner_scope = proc::process_scope();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let owner_scope: Option<String> = None;
    tx.execute("INSERT INTO resource_leases (slot_id, run_id, card_id, resources, owner_pid, owner_started, spawn_intent, owner_scope) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)", params![slot_row, run_id, card_id, serde_json::to_string(&resources).map_err(io::Error::other)?, std::process::id(), owner_started, owner_scope]).map_err(sql)?;
    tx.execute("INSERT INTO starts (run_id, card_id, provider, at) VALUES (?1, ?2, ?3, ?4)", params![run_id, card_id, provider.as_str(), iso(now)])
        .map_err(sql)?;
    let start_row = tx.last_insert_rowid();
    tx.execute("UPDATE resource_leases SET start_id = ?1 WHERE slot_id = ?2", params![start_row, slot_row]).map_err(sql)?;
    tx.commit().map_err(sql)?;
    Ok(Reserved::Granted(Reservation { run_id: run_id.to_string(), card_id: card_id.to_string(), provider, slot_row, start_row }))
}

/// The card's CLI is running: from now on the slot lives as long as that process.
pub fn link_slot(home: &Path, reservation: &Reservation, pid: u32) -> io::Result<()> {
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let started = proc::started_at(pid).and_then(|at| i64::try_from(at).ok());
    let lease = tx.execute("UPDATE resource_leases SET worker_pid = ?1, worker_started = ?2, spawn_intent = 1 WHERE slot_id = ?3", params![pid, started, reservation.slot_row]).map_err(sql)?;
    let slot = tx.execute("UPDATE slots SET pid = ?1 WHERE rowid = ?2", params![pid, reservation.slot_row]).map_err(sql)?;
    if lease != 1 || slot != 1 { return Err(io::Error::other("자원 예약을 찾지 못해 작업을 시작하지 않았습니다")); }
    tx.commit().map_err(sql)
}

/// Durably mark the ambiguous spawn window before creating the process.
pub fn mark_spawning(home: &Path, reservation: &Reservation) -> io::Result<()> {
    let changed = index::open(home)?.execute("UPDATE resource_leases SET spawn_intent = 1 WHERE slot_id = ?1 AND spawn_intent = 0", [reservation.slot_row]).map_err(sql)?;
    if changed != 1 { return Err(io::Error::other("자원 예약의 시작 상태를 확인하지 못했습니다")); }
    Ok(())
}

/// The owning runner records the already-confirmed process-tree exit before giving up the reservation.
pub fn mark_resource_exited(home: &Path, reservation: &Reservation) -> io::Result<()> {
    let changed = index::open(home)?.execute("UPDATE resource_leases SET exit_confirmed = 1 WHERE slot_id = ?1 AND owner_pid = ?2 AND spawn_intent = 1", params![reservation.slot_row, std::process::id()]).map_err(sql)?;
    if changed != 1 { return Err(io::Error::other("자원 예약의 종료 확인을 기록하지 못했습니다")); }
    Ok(())
}

/// Frees the slot. `started: false` (the CLI never ran) also gives back the day's start.
pub fn release_slot(home: &Path, reservation: &Reservation, started: bool) -> io::Result<()> {
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    tx.execute("DELETE FROM resource_leases WHERE slot_id = ?1", [reservation.slot_row]).map_err(sql)?;
    tx.execute("DELETE FROM slots WHERE rowid = ?1", [reservation.slot_row]).map_err(sql)?;
    if !started {
        tx.execute("DELETE FROM starts WHERE rowid = ?1", [reservation.start_row]).map_err(sql)?;
    }
    tx.commit().map_err(sql)
}

/// A surviving lease prevents resuming the same session, including after a runner crash.
pub fn resource_held(home: &Path, run_id: &str, card_id: &str) -> io::Result<bool> {
    index::open(home)?.query_row("SELECT EXISTS(SELECT 1 FROM resource_leases WHERE run_id=?1 AND card_id=?2)", params![run_id, card_id], |row| row.get(0)).map_err(sql)
}

/// Durable observations only: missing identity or a missing PID never proves termination.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceLease {
    pub id: String,
    pub run_id: String,
    pub card_id: String,
    pub resources: Value,
    pub owner_pid: Option<u32>,
    pub owner_started: Option<String>,
    pub worker_pid: Option<u32>,
    pub worker_started: Option<String>,
    pub spawn_intent: Option<bool>,
    pub exit_confirmed: Option<bool>,
    pub state: &'static str,
}

fn owner_scope_matches(scope: Option<&str>) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    { scope.is_some() && scope == proc::process_scope().as_deref() }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    { let _ = scope; true }
}

pub fn resource_leases(home: &Path) -> io::Result<Vec<ResourceLease>> {
    let conn = index::open(home)?;
    let mut stmt = conn.prepare("SELECT slot_id, run_id, card_id, resources, owner_pid, owner_started, worker_pid, worker_started, spawn_intent, exit_confirmed, start_id, owner_scope FROM resource_leases ORDER BY slot_id").map_err(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get::<_, String>(3)?, row.get::<_, Option<u32>>(4)?, row.get::<_, Option<i64>>(5)?, row.get::<_, Option<u32>>(6)?, row.get::<_, Option<i64>>(7)?, row.get::<_, Option<bool>>(8)?, row.get::<_, Option<bool>>(9)?, row.get::<_, Option<i64>>(10)?, row.get::<_, Option<String>>(11)?))
    }).map_err(sql)?;
    let mut leases: Vec<ResourceLease> = rows.map(|row| {
        let (id, run_id, card_id, raw, owner_pid, owner_started, worker_pid, worker_started, spawn_intent, exit_confirmed, start_id, owner_scope) = row.map_err(sql)?;
        let resources = serde_json::from_str(&raw).map_err(io::Error::other)?;
        let evidence = (spawn_intent == Some(false) && worker_pid.is_none() && start_id.is_some()) || (spawn_intent == Some(true) && exit_confirmed == Some(true));
        let owner_gone = owner_scope_matches(owner_scope.as_deref()) && owner_pid.zip(owner_started.and_then(|v| u64::try_from(v).ok())).is_some_and(|(pid, started)| proc::definitely_exited(pid, started));
        let state = if evidence && owner_gone { "recoverable" } else { "unknown" };
        Ok(ResourceLease { id: id.to_string(), run_id, card_id, resources, owner_pid, owner_started: owner_started.map(|v| v.to_string()), worker_pid, worker_started: worker_started.map(|v| v.to_string()), spawn_intent, exit_confirmed, state })
    }).collect::<io::Result<_>>()?;
    let mut legacy = conn.prepare("SELECT id, run_id, card_id, owner, pid FROM slots WHERE id NOT IN (SELECT slot_id FROM resource_leases) ORDER BY id").map_err(sql)?;
    let rows = legacy.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get::<_, u32>(3)?, row.get::<_, u32>(4)?))).map_err(sql)?;
    for row in rows {
        let (id, run_id, card_id, owner, worker) = row.map_err(sql)?;
        leases.push(ResourceLease { id: format!("legacy:{id}"), run_id, card_id, resources: serde_json::json!([]), owner_pid: (owner != 0).then_some(owner), owner_started: None, worker_pid: (worker != 0).then_some(worker), worker_started: None, spawn_intent: None, exit_confirmed: None, state: "unknown" });
    }
    Ok(leases)
}

/// Recover only with durable evidence of no spawn or a confirmed process-tree exit, and a gone owner.
pub fn recover_resource(home: &Path, id: i64) -> io::Result<()> {
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let row = tx.query_row(
        "SELECT owner_pid, owner_started, spawn_intent, start_id, run_id, card_id, worker_pid, exit_confirmed, owner_scope FROM resource_leases WHERE slot_id = ?1",
        [id], |row| Ok((row.get::<_, Option<u32>>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<bool>>(2)?, row.get::<_, Option<i64>>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?, row.get::<_, Option<u32>>(6)?, row.get::<_, Option<bool>>(7)?, row.get::<_, Option<String>>(8)?)),
    ).optional().map_err(sql)?;
    let (owner, started, phase, start_id, run_id, card_id, worker_pid, exited, owner_scope) = row.ok_or_else(|| io::Error::other("예약을 찾지 못했습니다"))?;
    let unstarted = phase == Some(false) && worker_pid.is_none() && start_id.is_some();
    let finished = phase == Some(true) && exited == Some(true);
    if !unstarted && !finished { return Err(io::Error::other("프로세스 종료 여부가 불명확해 예약을 복구할 수 없습니다")); }
    let (Some(owner), Some(started)) = (owner, started) else {
        return Err(io::Error::other("예약 소유자를 확인할 수 없습니다"));
    };
    let started = u64::try_from(started).map_err(|_| io::Error::other("소유자 생성 시각이 올바르지 않습니다"))?;
    if !owner_scope_matches(owner_scope.as_deref()) || !proc::definitely_exited(owner, started) {
        return Err(io::Error::other("예약 소유자가 살아 있거나 종료 여부를 확인할 수 없습니다"));
    }
    tx.execute("DELETE FROM slots WHERE id = ?1", [id]).map_err(sql)?;
    if unstarted { tx.execute("DELETE FROM starts WHERE rowid = ?1 AND run_id = ?2 AND card_id = ?3", params![start_id, run_id, card_id]).map_err(sql)?; }
    tx.execute("DELETE FROM resource_leases WHERE slot_id = ?1", [id]).map_err(sql)?;
    tx.commit().map_err(sql)
}

/// Provider → ISO time it is blocked until.
pub fn cooldown_times(home: &Path) -> io::Result<HashMap<ProviderId, String>> {
    let conn = index::open(home)?;
    let mut stmt = conn.prepare("SELECT provider, until FROM cooldowns").map_err(sql)?;
    let rows: Vec<(String, String)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?))).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
    Ok(rows.into_iter().filter_map(|(k, v)| Some((ProviderId::parse(&k)?, v))).collect())
}

/// Records a provider limit, keeping the later of the old and new times, and
/// returns the time now on record. An empty `until` means the CLI gave no reset time.
pub fn set_cooldown(home: &Path, provider: ProviderId, until: &str, now: DateTime<Utc>) -> io::Result<String> {
    let resolved = if until.is_empty() { iso(now + Duration::milliseconds(LIMITS.default_cooldown_ms)) } else { until.to_string() };
    let mut conn = index::open(home)?;
    // Read and write under one lock: two cards hitting the limit at once keep the later time.
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let current: String = tx
        .query_row("SELECT until FROM cooldowns WHERE provider = ?1", [provider.as_str()], |row| row.get(0))
        .optional()
        .map_err(sql)?
        .unwrap_or_default();
    let later = match (parse_time(&current), parse_time(&resolved)) {
        (Some(c), Some(r)) => c < r,
        _ => false,
    };
    if !current.is_empty() && !later {
        return Ok(current);
    }
    tx.execute("INSERT OR REPLACE INTO cooldowns (provider, until) VALUES (?1, ?2)", params![provider.as_str(), resolved]).map_err(sql)?;
    tx.commit().map_err(sql)?;
    Ok(resolved)
}

pub fn record_blocked(home: &Path, at: DateTime<Utc>, title: Option<&Value>, violations: &[Violation]) -> io::Result<()> {
    let violations = serde_json::to_string(violations).map_err(io::Error::other)?;
    index::open(home)?
        .execute("INSERT INTO blocked (at, title, violations) VALUES (?1, ?2, ?3)", params![iso(at), title_text(title), violations])
        .map_err(sql)?;
    Ok(())
}

pub fn policy_context(home: &Path, now: DateTime<Utc>) -> io::Result<PolicyContext> {
    Ok(PolicyContext {
        now,
        active_by_provider: held_by_provider(home)?,
        started_today: started_on(home, now)?,
        cooldowns: cooldown_times(home)?,
        over_budget: crate::usage::budget::over_budget(home, now)?,
        project_exists: Box::new(|path| Path::new(path).exists()),
        is_mirror: Box::new(o_guard::is_mirror),
        o_home: home.to_string_lossy().into_owned(),
        user_home: user_home().to_string_lossy().into_owned(),
        resolve: Box::new(resolved_path),
    })
}

/// The folder a path names on disk, with links, junctions and short names
/// resolved, or the path itself when it cannot be resolved (it does not exist).
pub fn resolved_path(path: &str) -> String {
    let Ok(real) = fs::canonicalize(path) else { return path.to_string() };
    let text = real.to_string_lossy().into_owned();
    // Windows answers in the extended form (\\?\C:\…, \\?\UNC\server\…); compare the plain one.
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    text.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(text)
}

pub fn request_stop(home: &Path, run_id: &str) -> io::Result<()> {
    #[cfg(any(windows, target_os = "macos"))]
    { with_start_gate(home, run_id, || fs::write(run_dir(home, run_id).join("stop"), "")) }
    #[cfg(not(any(windows, target_os = "macos")))]
    { fs::write(run_dir(home, run_id).join("stop"), "") }
}

/// Serialize the stop record with the final decision to resume a suspended card.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn with_start_gate<T>(home: &Path, run_id: &str, action: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(run_dir(home, run_id).join("start-stop.lock"))?;
    lock.lock()?;
    action()
}

pub fn stop_requested(home: &Path, run_id: &str) -> bool {
    run_dir(home, run_id).join("stop").exists()
}

/// `o dispatch --clean`: the runner starts this run's cards with clean agent profiles (see `clean`).
pub fn mark_clean(home: &Path, run_id: &str) -> io::Result<()> {
    fs::write(run_dir(home, run_id).join("clean"), "")
}

pub fn is_clean(home: &Path, run_id: &str) -> bool {
    run_dir(home, run_id).join("clean").exists()
}

/// project_O 프로필 사용 (ADR-0013): how this run's cards are set up, as the `clean` and
/// `no-context` markers the runner and `o tokens` read.
pub fn mark_mode(home: &Path, run_id: &str, mode: &RunMode) -> io::Result<()> {
    if mode.clean() {
        mark_clean(home, run_id)?;
    }
    if !mode.context {
        crate::context::mark_off(home, run_id)?;
    }
    if mode.yolo {
        fs::write(run_dir(home, run_id).join("yolo"), "")?;
    }
    Ok(())
}

/// YOLO 모드 (ADR-0014): the run's cards launch with their CLI's YOLO flag (`providers::launch_spec_for`).
pub fn is_yolo(home: &Path, run_id: &str) -> bool {
    run_dir(home, run_id).join("yolo").exists()
}

/// The runner (`o _run`) of a run: its pid and start time, written when it starts.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct RunnerMark {
    pid: u32,
    #[serde(default)]
    started: Option<u64>,
}

/// Notes this process as the run's runner, so others can tell a run that is still going
/// from one whose runner died without `run.end` (`run_is_live`).
pub fn mark_runner(home: &Path, run_id: &str) -> io::Result<()> {
    let pid = std::process::id();
    let mark = RunnerMark { pid, started: proc::started_at(pid) };
    fs::write(run_dir(home, run_id).join("runner.json"), serde_json::to_string(&mark).map_err(io::Error::other)?)
}

/// How long a dispatched run may take to start its runner before it no longer counts as going.
const RUNNER_START_GRACE: std::time::Duration = std::time::Duration::from_secs(120);

/// Cheap display precheck: a dead runner needs no scan of its historical event log.
pub(crate) fn runner_may_be_live(home: &Path, run_id: &str) -> bool {
    let dir = run_dir(home, run_id);
    match fs::read_to_string(dir.join("runner.json")).ok().and_then(|t| serde_json::from_str::<RunnerMark>(&t).ok()) {
        Some(mark) => proc::is_alive(mark.pid) && (mark.started.is_none() || proc::started_at(mark.pid) == mark.started),
        None => fs::metadata(dir.join("plan.json")).and_then(|m| m.modified()).is_ok_and(|at| at.elapsed().is_ok_and(|age| age < RUNNER_START_GRACE)),
    }
}

/// Whether a run is still going: no `run.end` yet, and its runner alive (the same pid and start
/// time), or not written yet within a short while of the dispatch.
pub fn run_is_live(home: &Path, run_id: &str) -> bool {
    if read_events(home, run_id).iter().any(|e| matches!(e, OEvent::RunEnd { .. })) {
        return false;
    }
    let dir = run_dir(home, run_id);
    match fs::read_to_string(dir.join("runner.json")).ok().and_then(|t| serde_json::from_str::<RunnerMark>(&t).ok()) {
        Some(mark) => proc::is_alive(mark.pid) && (mark.started.is_none() || proc::started_at(mark.pid) == mark.started),
        None => fs::metadata(dir.join("plan.json")).and_then(|m| m.modified()).is_ok_and(|at| at.elapsed().is_ok_and(|age| age < RUNNER_START_GRACE)),
    }
}

/// `JSON.stringify(value, null, 2)`.
pub fn to_pretty<T: Serialize>(value: &T) -> io::Result<String> {
    serde_json::to_string_pretty(value).map_err(io::Error::other)
}
