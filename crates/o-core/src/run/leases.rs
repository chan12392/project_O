//! Resource leases (ADR-0039): a card's hold on a provider slot and on one of the day's
//! starts, and how a lease left by a dead owner is shown and recovered. Callers reach
//! these through `store::…`, which re-exports this module.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use serde_json::Value;

use super::store::{active_markers, started_in};
use crate::policy::{empty_counts, LIMITS};
use crate::run::index::{self, sql};
use crate::run::plan::ProviderId;
use crate::sys::js::iso;
use crate::sys::proc;

/// A card's hold on a slot and on one of the day's starts, taken before its CLI starts
/// (see `reserve_slot`). Given back with `release_slot`.
#[derive(Debug)]
pub struct Reservation {
    pub run_id: String,
    pub card_id: String,
    pub provider: ProviderId,
    pub(crate) slot_row: i64,
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
    // The RAM cap (ADR-0037) holds back cards beyond the first; it is a setting, not a LIMITS cap.
    if let Some(reason) = super::ram::hold_back(home, held.len()) {
        tx.commit().map_err(sql)?;
        return Ok(Reserved::Busy(reason));
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

/// The card's CLI is running: from now on the slot lives as long as that process. `job_name` is
/// the named kill-on-close job the worker is confirmed to run in (Windows, ADR-0039), if any.
pub fn link_slot(home: &Path, reservation: &Reservation, pid: u32, job_name: Option<&str>) -> io::Result<()> {
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let started = proc::started_at(pid).and_then(|at| i64::try_from(at).ok());
    let lease = tx.execute("UPDATE resource_leases SET worker_pid = ?1, worker_started = ?2, spawn_intent = 1, job_name = ?3 WHERE slot_id = ?4", params![pid, started, job_name, reservation.slot_row]).map_err(sql)?;
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
    /// The card's named kill-on-close job (Windows, ADR-0039); none for older leases.
    pub job_name: Option<String>,
    pub state: &'static str,
    pub processes_exited: bool,
}

fn owner_scope_matches(scope: Option<&str>) -> bool {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    { scope.is_some() && scope == proc::process_scope().as_deref() }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    { let _ = scope; true }
}

fn process_exited(pid: Option<u32>, started: Option<i64>) -> bool {
    pid.zip(started.and_then(|v| u64::try_from(v).ok()).filter(|v| *v > 0)).is_some_and(|(pid, started)| proc::definitely_exited(pid, started))
}

pub fn resource_leases(home: &Path) -> io::Result<Vec<ResourceLease>> {
    let conn = index::open(home)?;
    let mut stmt = conn.prepare("SELECT slot_id, run_id, card_id, resources, owner_pid, owner_started, worker_pid, worker_started, spawn_intent, exit_confirmed, start_id, owner_scope, job_name FROM resource_leases ORDER BY slot_id").map_err(sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get::<_, String>(3)?, row.get::<_, Option<u32>>(4)?, row.get::<_, Option<i64>>(5)?, row.get::<_, Option<u32>>(6)?, row.get::<_, Option<i64>>(7)?, row.get::<_, Option<bool>>(8)?, row.get::<_, Option<bool>>(9)?, row.get::<_, Option<i64>>(10)?, row.get::<_, Option<String>>(11)?, row.get::<_, Option<String>>(12)?))
    }).map_err(sql)?;
    let mut leases: Vec<ResourceLease> = rows.map(|row| {
        let (id, run_id, card_id, raw, owner_pid, owner_started, worker_pid, worker_started, spawn_intent, exit_confirmed, start_id, owner_scope, job_name) = row.map_err(sql)?;
        let resources = serde_json::from_str(&raw).map_err(io::Error::other)?;
        let evidence = (spawn_intent == Some(false) && worker_pid.is_none() && start_id.is_some()) || (spawn_intent == Some(true) && exit_confirmed == Some(true));
        let owner_gone = owner_scope_matches(owner_scope.as_deref()) && owner_pid.zip(owner_started.and_then(|v| u64::try_from(v).ok())).is_some_and(|(pid, started)| proc::definitely_exited(pid, started));
        let processes_exited = spawn_intent == Some(true) && owner_gone && owner_started.is_some_and(|v| v > 0) && process_exited(worker_pid, worker_started);
        let state = if owner_gone && evidence { "recoverable" } else { "unknown" };
        Ok(ResourceLease { id: id.to_string(), run_id, card_id, resources, owner_pid, owner_started: owner_started.map(|v| v.to_string()), worker_pid, worker_started: worker_started.map(|v| v.to_string()), spawn_intent, exit_confirmed, job_name, state, processes_exited })
    }).collect::<io::Result<_>>()?;
    let mut legacy = conn.prepare("SELECT id, run_id, card_id, owner, pid FROM slots WHERE id NOT IN (SELECT slot_id FROM resource_leases) ORDER BY id").map_err(sql)?;
    let rows = legacy.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get::<_, u32>(3)?, row.get::<_, u32>(4)?))).map_err(sql)?;
    for row in rows {
        let (id, run_id, card_id, owner, worker) = row.map_err(sql)?;
        leases.push(ResourceLease { id: format!("legacy:{id}"), run_id, card_id, resources: serde_json::json!([]), owner_pid: (owner != 0).then_some(owner), owner_started: None, worker_pid: (worker != 0).then_some(worker), worker_started: None, spawn_intent: None, exit_confirmed: None, job_name: None, state: "unknown", processes_exited: false });
    }
    Ok(leases)
}

/// A person has checked for remaining child processes; recheck both recorded identities under lock.
pub fn release_confirmed(home: &Path, id: i64) -> io::Result<()> {
    let mut conn = index::open(home)?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let row = tx.query_row(
        "SELECT owner_pid, owner_started, worker_pid, worker_started, spawn_intent, exit_confirmed, owner_scope FROM resource_leases WHERE slot_id = ?1",
        [id], |r| Ok((r.get::<_, Option<u32>>(0)?, r.get::<_, Option<i64>>(1)?, r.get::<_, Option<u32>>(2)?, r.get::<_, Option<i64>>(3)?, r.get::<_, Option<bool>>(4)?, r.get::<_, Option<bool>>(5)?, r.get::<_, Option<String>>(6)?)),
    ).optional().map_err(sql)?;
    let (owner, owner_started, worker, worker_started, phase, exited, scope) = row.ok_or_else(|| io::Error::other("예약을 찾지 못했습니다"))?;
    if phase != Some(true) || exited == Some(true) || !owner_scope_matches(scope.as_deref()) || !process_exited(owner, owner_started) || !process_exited(worker, worker_started) {
        return Err(io::Error::other("프로세스 종료 여부가 불명확해 예약을 복구할 수 없습니다"));
    }
    tx.execute("DELETE FROM slots WHERE id = ?1", [id]).map_err(sql)?;
    tx.execute("DELETE FROM resource_leases WHERE slot_id = ?1", [id]).map_err(sql)?;
    tx.commit().map_err(sql)
}

/// Recover only with durable evidence of no spawn or a confirmed process-tree exit, and a gone
/// owner. A Windows watcher can confirm the exit after the runner dies (ADR-0039).
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

