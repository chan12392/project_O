//! `<home>/o.db`: a SQLite index over the run records, and the state only
//! dispatch and the runner write (card starts, cooldowns, refused plans).
//!
//! Each run's `events.jsonl` stays the record. Cards append to it with `o msg`
//! and never open this database (ADR-0002: a Codex card may only write its run
//! folder). Readers call `ingest` first, which copies the whole lines appended
//! since the last read; a run whose file no longer continues what was ingested
//! (cut short or rewritten: its first ingested bytes no longer hash to `prefix_hash`)
//! is ingested again from the start.
//!
//! ```text
//! runs      run_id PK · title · project · started_at · ended_at · status · cards · done · failed · tokens · ingested_bytes · events · prefix_hash · modified
//! events    run_id · seq · json            every complete, parseable events.jsonl line, in file order
//! starts    run_id · card_id · provider · at   one row per started card (the daily cap)
//! slots     run_id · card_id · provider · owner · pid   cards holding a slot: reserved by the runner
//!                                  (`owner`) before the CLI starts, the CLI's `pid` once it runs
//! cooldowns provider PK · until            when a provider may be used again
//! blocked   at · title · violations        plans the policy refused (an audit trail)
//! ```
//!
//! Several processes use it at once (the runner, `o runs` from the dev server,
//! `o dispatch`): WAL lets readers go on while one writes, and every write is a
//! transaction that re-reads what it changes, so a run is never ingested twice.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Read};
use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde_json::Value;

use crate::sys::js::{iso, parse_time};
use crate::run::store::RunRecord;

/// 2: `runs.prefix_hash` and `runs.modified`, so a rewritten file is told from an appended one.
/// 3: `slots`, the card slots runners hold from before a CLI starts until it ends.
/// 7: Linux and macOS reservation owners include their boot scope.
/// 8: `resource_leases.job_name`, the Windows card's named kill-on-close job (ADR-0039).
const SCHEMA_VERSION: i64 = 8;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
  run_id TEXT PRIMARY KEY,
  title TEXT, project TEXT, started_at TEXT, ended_at TEXT, status TEXT,
  cards INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0, tokens INTEGER NOT NULL DEFAULT 0,
  ingested_bytes INTEGER NOT NULL DEFAULT 0, events INTEGER NOT NULL DEFAULT 0,
  prefix_hash INTEGER NOT NULL DEFAULT 0, modified INTEGER NOT NULL DEFAULT -1
);
CREATE INDEX IF NOT EXISTS runs_by_start ON runs (started_at);
CREATE TABLE IF NOT EXISTS events (run_id TEXT NOT NULL, seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY (run_id, seq)) WITHOUT ROWID;
CREATE TABLE IF NOT EXISTS starts (run_id TEXT NOT NULL, card_id TEXT NOT NULL, provider TEXT NOT NULL, at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS starts_by_at ON starts (at);
CREATE TABLE IF NOT EXISTS slots (id INTEGER PRIMARY KEY AUTOINCREMENT, run_id TEXT NOT NULL, card_id TEXT NOT NULL, provider TEXT NOT NULL, owner INTEGER NOT NULL, pid INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS resource_leases (slot_id INTEGER PRIMARY KEY, run_id TEXT NOT NULL, card_id TEXT NOT NULL, resources TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cooldowns (provider TEXT PRIMARY KEY, until TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS blocked (at TEXT NOT NULL, title TEXT, violations TEXT NOT NULL);
";

pub(crate) fn sql(error: rusqlite::Error) -> io::Error {
    io::Error::other(format!("o.db: {error}"))
}

/// Opens (and on first use creates) the database, then imports the files it replaced.
pub fn open(home: &Path) -> io::Result<Connection> {
    fs::create_dir_all(home)?;
    let mut conn = Connection::open(home.join("o.db")).map_err(sql)?;
    conn.busy_timeout(Duration::from_secs(5)).map_err(sql)?;
    // Sticks to the file; a no-op once set. Losing the race to another opener only means trying again next time.
    let _ = conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get::<_, String>(0));
    // In WAL mode a crash can lose the last commits but never corrupts the file.
    conn.pragma_update(None, "synchronous", "FULL").map_err(sql)?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).map_err(sql)?;
    if version < SCHEMA_VERSION {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        tx.execute_batch(SCHEMA).map_err(sql)?;
        // A version-1 file already has `runs` without the fingerprint; its rows (hash 0) are read again once.
        let has_hash: bool =
            tx.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('runs') WHERE name = 'prefix_hash')", [], |row| row.get(0)).map_err(sql)?;
        if !has_hash {
            tx.execute_batch(
                "ALTER TABLE runs ADD COLUMN prefix_hash INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE runs ADD COLUMN modified INTEGER NOT NULL DEFAULT -1;",
            )
            .map_err(sql)?;
        }
        let has_owner: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('resource_leases') WHERE name = 'owner_pid')", [], |row| row.get(0)).map_err(sql)?;
        if !has_owner {
            tx.execute_batch("ALTER TABLE resource_leases ADD COLUMN owner_pid INTEGER;\n\
                ALTER TABLE resource_leases ADD COLUMN owner_started INTEGER;\n\
                ALTER TABLE resource_leases ADD COLUMN worker_pid INTEGER;\n\
                ALTER TABLE resource_leases ADD COLUMN worker_started INTEGER;").map_err(sql)?;
        }
        let has_phase: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('resource_leases') WHERE name = 'spawn_intent')", [], |row| row.get(0)).map_err(sql)?;
        if !has_phase {
            tx.execute_batch("ALTER TABLE resource_leases ADD COLUMN spawn_intent INTEGER;\n\
                ALTER TABLE resource_leases ADD COLUMN start_id INTEGER;").map_err(sql)?;
        }
        let has_exit: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('resource_leases') WHERE name = 'exit_confirmed')", [], |row| row.get(0)).map_err(sql)?;
        if !has_exit {
            tx.execute_batch("ALTER TABLE resource_leases ADD COLUMN exit_confirmed INTEGER;").map_err(sql)?;
        }
        let has_scope: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('resource_leases') WHERE name = 'owner_scope')", [], |row| row.get(0)).map_err(sql)?;
        if !has_scope {
            tx.execute_batch("ALTER TABLE resource_leases ADD COLUMN owner_scope TEXT;").map_err(sql)?;
        }
        let has_job: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM pragma_table_info('resource_leases') WHERE name = 'job_name')", [], |row| row.get(0)).map_err(sql)?;
        if !has_job {
            tx.execute_batch("ALTER TABLE resource_leases ADD COLUMN job_name TEXT;").map_err(sql)?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION).map_err(sql)?;
        tx.commit().map_err(sql)?;
    }
    import_old_files(&mut conn, home)?;
    Ok(conn)
}

/// One-time move of ledger.jsonl, cooldowns.json and blocked.jsonl: a file is
/// imported only into an empty table, then renamed to `*.imported` (kept, not deleted).
fn import_old_files(conn: &mut Connection, home: &Path) -> io::Result<()> {
    for (name, table) in [("ledger.jsonl", "starts"), ("cooldowns.json", "cooldowns"), ("blocked.jsonl", "blocked")] {
        let path = home.join(name);
        if !path.is_file() {
            continue;
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        // Another process may have imported it between our look and our lock.
        let empty: bool = tx.query_row(&format!("SELECT NOT EXISTS (SELECT 1 FROM {table})"), [], |row| row.get(0)).map_err(sql)?;
        if !empty || !path.is_file() {
            continue;
        }
        let text = String::from_utf8_lossy(&fs::read(&path)?).into_owned();
        let lines = || text.split('\n').filter(|l| !l.trim().is_empty());
        let field = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let now = iso(Utc::now());
        match table {
            "starts" => {
                for line in lines() {
                    // A line that does not parse counted as started today; it still does, today.
                    let entry: Value = serde_json::from_str(line).unwrap_or(Value::Null);
                    let at = entry.get("at").and_then(Value::as_str).and_then(parse_time).map(iso).unwrap_or_else(|| now.clone());
                    tx.execute(
                        "INSERT INTO starts (run_id, card_id, provider, at) VALUES (?1, ?2, ?3, ?4)",
                        params![field(&entry, "runId"), field(&entry, "cardId"), field(&entry, "provider"), at],
                    )
                    .map_err(sql)?;
                }
            }
            "cooldowns" => {
                let map: serde_json::Map<String, Value> = serde_json::from_str(&text).map_err(|e| io::Error::other(format!("{name}: {e}")))?;
                for (provider, until) in map.iter().filter_map(|(k, v)| Some((k, v.as_str()?))) {
                    tx.execute("INSERT INTO cooldowns (provider, until) VALUES (?1, ?2)", params![provider, until]).map_err(sql)?;
                }
            }
            _ => {
                for entry in lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()) {
                    let at = entry.get("at").and_then(Value::as_str).map_or_else(|| now.clone(), str::to_string);
                    let violations = entry.get("violations").cloned().unwrap_or(Value::Array(Vec::new()));
                    tx.execute(
                        "INSERT INTO blocked (at, title, violations) VALUES (?1, ?2, ?3)",
                        params![at, title_text(entry.get("title")), violations.to_string()],
                    )
                    .map_err(sql)?;
                }
            }
        }
        tx.commit().map_err(sql)?;
        fs::rename(&path, home.join(format!("{name}.imported")))?;
    }
    Ok(())
}

/// A plan's title as stored: a string as is, anything else a plan carried as JSON.
pub(crate) fn title_text(title: Option<&Value>) -> Option<String> {
    title.map(|t| t.as_str().map_or_else(|| t.to_string(), str::to_string))
}

/// What the `runs` row says about a run, folded from its events as they arrive.
#[derive(Default)]
struct Summary {
    title: Option<String>,
    project: Option<String>,
    started_at: Option<String>,
    ended_at: Option<String>,
    status: Option<String>,
    cards: i64,
    done: i64,
    failed: i64,
    tokens: i64,
    ingested_bytes: i64,
    events: i64,
}

impl Summary {
    /// Like `snapshot`: a start begins the run afresh and events before it count for nothing.
    fn fold(&mut self, seq: i64, event: &Value) {
        let text = |key: &str| event.get(key).and_then(Value::as_str).map(str::to_string);
        let number = |key: &str| event.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        match event.get("t").and_then(Value::as_str) {
            Some("run.start") => {
                *self = Summary {
                    title: text("title"),
                    project: text("project"),
                    // Listed only when the start is the first line, as `newest_first` decides.
                    started_at: if seq == 0 { text("at") } else { self.started_at.take() },
                    status: Some("running".into()),
                    cards: event.get("cards").and_then(Value::as_array).map_or(0, |c| c.len() as i64),
                    ingested_bytes: self.ingested_bytes,
                    events: self.events,
                    ..Summary::default()
                }
            }
            _ if self.status.is_none() => {}
            Some("card.end") => match event.get("status").and_then(Value::as_str) {
                Some("done") => self.done += 1,
                Some("failed") => self.failed += 1,
                _ => {}
            },
            Some("card.usage") => self.tokens += (number("input") + number("cached") + number("output")) as i64,
            Some("run.end") => {
                self.ended_at = text("at");
                self.status = text("status");
            }
            _ => {}
        }
    }
}

/// Brings the index up to date with the runs folder. When nothing changed this
/// is one query and one stat per run, and takes no write lock.
pub fn ingest(conn: &mut Connection, home: &Path) -> io::Result<()> {
    let names: Vec<String> = match fs::read_dir(home.join("runs")) {
        Ok(entries) => entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect(),
        Err(error) if error.kind() == ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error),
    };
    let known: HashMap<String, (i64, i64)> = {
        let mut stmt = conn.prepare("SELECT run_id, ingested_bytes, modified FROM runs").map_err(sql)?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?)))).map_err(sql)?;
        rows.collect::<Result<_, _>>().map_err(sql)?
    };
    for run_id in &names {
        let file = home.join("runs").join(run_id).join("events.jsonl");
        let meta = fs::metadata(&file).ok();
        let size = meta.as_ref().map_or(0, |m| m.len());
        // Same size and same modification time: nothing was appended or rewritten since the last read.
        let (ingested, modified) = known.get(run_id).copied().unwrap_or((0, -1));
        if size != ingested as u64 || meta.as_ref().map_or(-1, modified_ns) != modified {
            ingest_run(conn, &file, run_id)?;
        }
    }
    // Run folders that were removed leave the index too.
    let present: HashSet<&String> = names.iter().collect();
    let gone: Vec<&String> = known.keys().filter(|id| !present.contains(id)).collect();
    if !gone.is_empty() {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
        for run_id in gone {
            tx.execute("DELETE FROM events WHERE run_id = ?1", [run_id]).map_err(sql)?;
            tx.execute("DELETE FROM runs WHERE run_id = ?1", [run_id]).map_err(sql)?;
        }
        tx.commit().map_err(sql)?;
    }
    Ok(())
}

/// Ingests one run's new lines in one transaction. The write lock is taken
/// first and the offset read under it (compare and advance): two readers that
/// both saw the file grow ingest it once.
fn ingest_run(conn: &mut Connection, path: &Path, run_id: &str) -> io::Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).map_err(sql)?;
    let mut summary = tx
        .query_row(
            "SELECT title, project, started_at, ended_at, status, cards, done, failed, tokens, ingested_bytes, events FROM runs WHERE run_id = ?1",
            [run_id],
            |r| {
                Ok(Summary {
                    title: r.get(0)?,
                    project: r.get(1)?,
                    started_at: r.get(2)?,
                    ended_at: r.get(3)?,
                    status: r.get(4)?,
                    cards: r.get(5)?,
                    done: r.get(6)?,
                    failed: r.get(7)?,
                    tokens: r.get(8)?,
                    ingested_bytes: r.get(9)?,
                    events: r.get(10)?,
                })
            },
        )
        .optional()
        .map_err(sql)?
        .unwrap_or_default();
    let stored_hash: i64 =
        tx.query_row("SELECT prefix_hash FROM runs WHERE run_id = ?1", [run_id], |r| r.get(0)).optional().map_err(sql)?.unwrap_or(0);
    let mut all = Vec::new();
    let modified;
    match File::open(path) {
        Ok(mut file) => {
            // Taken before reading: a write after it changes the time again and brings the next ingest back here.
            modified = modified_ns(&file.metadata()?);
            file.read_to_end(&mut all)?;
            // What was ingested must still be there byte for byte, ending on a line break; otherwise start over.
            let from = summary.ingested_bytes as usize;
            let continues = from == 0 || (all.len() >= from && all[from - 1] == b'\n' && fingerprint(&all[..from]) == stored_hash);
            if !continues {
                summary = Summary::default();
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            summary = Summary::default();
            modified = -1;
        }
        Err(error) => return Err(error),
    }
    let bytes = &all[summary.ingested_bytes as usize..];
    if summary.events == 0 {
        tx.execute("DELETE FROM events WHERE run_id = ?1", [run_id]).map_err(sql)?;
    }
    // Whole lines only: a line the writer is still appending waits for a later ingest.
    let whole = bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    {
        let mut insert = tx.prepare_cached("INSERT INTO events (run_id, seq, json) VALUES (?1, ?2, ?3)").map_err(sql)?;
        for line in bytes[..whole].split(|b| *b == b'\n') {
            let line = String::from_utf8_lossy(line);
            // Lines that do not parse are skipped, as `parse_event_values` skips them.
            let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
            insert.execute(params![run_id, summary.events, event.to_string()]).map_err(sql)?;
            summary.fold(summary.events, &event);
            summary.events += 1;
        }
    }
    summary.ingested_bytes += whole as i64;
    let prefix_hash = fingerprint(&all[..summary.ingested_bytes as usize]);
    let s = &summary;
    tx.execute(
        "INSERT OR REPLACE INTO runs (run_id, title, project, started_at, ended_at, status, cards, done, failed, tokens, ingested_bytes, events, prefix_hash, modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            run_id, s.title, s.project, s.started_at, s.ended_at, s.status, s.cards, s.done, s.failed, s.tokens, s.ingested_bytes, s.events, prefix_hash, modified
        ],
    )
    .map_err(sql)?;
    tx.commit().map_err(sql)
}

/// FNV-1a over the ingested bytes: tells a file that still starts with what was read
/// from one rewritten in place (same size, or longer with a line break at the old end).
/// The context index (`context`) checks its own read against the file the same way.
pub(crate) fn fingerprint(bytes: &[u8]) -> i64 {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |hash, b| (hash ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3));
    hash as i64
}

/// The file's modification time in nanoseconds since 1970, or -1 when the system does not say.
fn modified_ns(meta: &fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(-1, |d| d.as_nanos() as i64)
}

/// Which runs `o runs` lists.
pub struct RunQuery<'a> {
    pub limit: usize,
    /// Only runs whose plan named this project folder (slashes and, on Windows, case aside).
    pub project: Option<&'a str>,
}

/// Runs with a start event, newest first by that start, with every event.
pub fn runs(conn: &Connection, query: &RunQuery) -> io::Result<Vec<RunRecord>> {
    let project = query.project.map(|p| p.replace('\\', "/").trim_end_matches('/').to_string());
    let collate = if cfg!(windows) { "COLLATE NOCASE" } else { "" };
    let mut stmt = conn
        .prepare(&format!(
            r"SELECT run_id FROM runs WHERE started_at <> ''
              AND (?1 IS NULL OR replace(rtrim(project, '/\'), '\', '/') = ?1 {collate})
              ORDER BY started_at DESC, run_id DESC LIMIT ?2"
        ))
        .map_err(sql)?;
    let ids: Vec<String> = stmt
        .query_map(params![project, query.limit as i64], |row| row.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    let mut events = conn.prepare("SELECT json FROM events WHERE run_id = ?1 ORDER BY seq").map_err(sql)?;
    ids.into_iter()
        .map(|run_id| {
            let lines: Vec<String> = events.query_map([&run_id], |row| row.get(0)).map_err(sql)?.collect::<Result<_, _>>().map_err(sql)?;
            let events = lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
            Ok(RunRecord { run_id, events })
        })
        .collect()
}
