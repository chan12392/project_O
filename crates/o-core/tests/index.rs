//! The run index (o.db): incremental ingest of events.jsonl, the one-time
//! import of the old ledger/cooldowns/blocked files, and the daily cap.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::{Duration, Utc};
use o_core::run::events::OEvent;
use o_core::run::index::{self, RunQuery};
use o_core::sys::js::iso;
use o_core::run::plan::ProviderId;
use o_core::run::store::{append_event, cooldown_times, parse_event_values, read_runs, record_start, run_dir, started_on, RunRecord};
use serde_json::Value;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn temp_home() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("o-index-{}-{n}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn append(home: &Path, run_id: &str, text: &str) {
    let dir = run_dir(home, run_id);
    fs::create_dir_all(&dir).unwrap();
    OpenOptions::new().create(true).append(true).open(dir.join("events.jsonl")).unwrap().write_all(text.as_bytes()).unwrap();
}

fn start(run_id: &str, at: &str, project: &str) -> String {
    format!(r#"{{"t":"run.start","at":"{at}","runId":"{run_id}","title":"t","project":"{project}","cards":[{{"id":"a","provider":"codex"}},{{"id":"b","provider":"claude"}}]}}"#)
        + "\n"
}

fn msg(text: &str) -> String {
    format!(r#"{{"t":"msg","at":"2026-11-01T10:00:01.000Z","from":"lead","to":"all","text":"{text}"}}"#) + "\n"
}

fn all(home: &Path) -> Vec<RunRecord> {
    read_runs(home, &RunQuery { limit: 50, project: None }).unwrap()
}

fn event_count(home: &Path, run_id: &str) -> i64 {
    index::open(home).unwrap().query_row("SELECT count(*) FROM events WHERE run_id = ?1", [run_id], |r| r.get(0)).unwrap()
}

#[test]
fn appended_lines_are_ingested_once_each() {
    let home = temp_home();
    append(&home, "r1", &start("r1", "2026-11-01T10:00:00.000Z", "C:/p"));
    assert_eq!(all(&home)[0].events.len(), 1);
    append(&home, "r1", &(msg("one") + &msg("two")));
    assert_eq!(all(&home)[0].events.len(), 3);
    // Nothing new: nothing added.
    assert_eq!(all(&home)[0].events.len(), 3);
    append(&home, "r1", &msg("three"));
    let runs = all(&home);
    let texts: Vec<&str> = runs[0].events.iter().filter_map(|e| e.get("text").and_then(Value::as_str)).collect();
    assert_eq!(texts, ["one", "two", "three"]);
    assert_eq!(event_count(&home, "r1"), 4);
}

#[test]
fn a_half_written_line_is_ingested_only_once_it_is_complete() {
    let home = temp_home();
    let line = msg("half");
    let (head, tail) = line.split_at(20);
    append(&home, "r1", &(start("r1", "2026-11-01T10:00:00.000Z", "C:/p") + head));
    assert_eq!(all(&home)[0].events.len(), 1);
    assert_eq!(all(&home)[0].events.len(), 1);
    append(&home, "r1", tail);
    let runs = all(&home);
    assert_eq!(runs[0].events.len(), 2);
    assert_eq!(runs[0].events[1]["text"], "half");
    assert_eq!(all(&home)[0].events.len(), 2);
    assert_eq!(event_count(&home, "r1"), 2);
}

#[test]
fn unparsable_lines_are_skipped_and_a_rewritten_file_is_read_again() {
    let home = temp_home();
    append(&home, "r1", &(start("r1", "2026-11-01T10:00:00.000Z", "C:/p") + "not json\n" + &msg("a")));
    assert_eq!(all(&home)[0].events.len(), 2);
    // Cut short (as if edited by hand): the index starts over from the file.
    fs::write(run_dir(&home, "r1").join("events.jsonl"), start("r1", "2026-11-01T11:00:00.000Z", "C:/q")).unwrap();
    let runs = all(&home);
    assert_eq!(runs[0].events.len(), 1);
    assert_eq!(runs[0].events[0]["at"], "2026-11-01T11:00:00.000Z");
    assert_eq!(event_count(&home, "r1"), 1);
    // A removed run folder leaves the index.
    fs::remove_dir_all(run_dir(&home, "r1")).unwrap();
    assert!(all(&home).is_empty());
    assert_eq!(event_count(&home, "r1"), 0);
}

/// A file replaced by one of the same size (restored, edited by hand) is read again, not kept from the index.
#[test]
fn a_rewritten_file_of_the_same_size_is_read_again() {
    let home = temp_home();
    append(&home, "r1", &(start("r1", "2026-11-01T10:00:00.000Z", "C:/p") + &msg("aaaa")));
    assert_eq!(all(&home)[0].events[1]["text"], "aaaa");
    let rewritten = start("r1", "2026-11-01T10:00:00.000Z", "C:/q") + &msg("bbbb");
    assert_eq!(rewritten.len() as u64, fs::metadata(run_dir(&home, "r1").join("events.jsonl")).unwrap().len());
    fs::write(run_dir(&home, "r1").join("events.jsonl"), &rewritten).unwrap();
    let runs = all(&home);
    assert_eq!(runs[0].events[0]["project"], "C:/q");
    assert_eq!(runs[0].events[1]["text"], "bbbb");
    assert_eq!(event_count(&home, "r1"), 2);
}

/// A longer replacement whose byte before the old offset happens to be a line break is
/// not a continuation: the index must not keep the old lines and add the new tail.
#[test]
fn a_longer_rewrite_that_breaks_a_line_at_the_old_offset_is_read_again() {
    let home = temp_home();
    let first = start("r1", "2026-11-01T10:00:00.000Z", "C:/p") + &msg("aaaa");
    append(&home, "r1", &first);
    assert_eq!(all(&home)[0].events.len(), 2);
    let rewritten = start("r1", "2026-11-01T10:00:00.000Z", "C:/q") + &msg("bbbb") + &msg("cccc");
    assert_eq!(rewritten.as_bytes()[first.len() - 1], b'\n');
    fs::write(run_dir(&home, "r1").join("events.jsonl"), &rewritten).unwrap();
    let runs = all(&home);
    let texts: Vec<&str> = runs[0].events.iter().filter_map(|e| e.get("text").and_then(Value::as_str)).collect();
    assert_eq!(texts, ["bbbb", "cccc"]);
    assert_eq!(runs[0].events[0]["project"], "C:/q");
    assert_eq!(event_count(&home, "r1"), 3);
}

/// An o.db written by the previous build (no fingerprint yet) opens, and its runs are read again once.
#[test]
fn an_index_from_the_previous_build_is_upgraded() {
    let home = temp_home();
    {
        let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
        conn.execute_batch(
            "CREATE TABLE runs (run_id TEXT PRIMARY KEY, title TEXT, project TEXT, started_at TEXT, ended_at TEXT, status TEXT,
               cards INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0, failed INTEGER NOT NULL DEFAULT 0, tokens INTEGER NOT NULL DEFAULT 0,
               ingested_bytes INTEGER NOT NULL DEFAULT 0, events INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE events (run_id TEXT NOT NULL, seq INTEGER NOT NULL, json TEXT NOT NULL, PRIMARY KEY (run_id, seq)) WITHOUT ROWID;
             CREATE TABLE starts (run_id TEXT NOT NULL, card_id TEXT NOT NULL, provider TEXT NOT NULL, at TEXT NOT NULL);
             CREATE TABLE cooldowns (provider TEXT PRIMARY KEY, until TEXT NOT NULL);
             CREATE TABLE blocked (at TEXT NOT NULL, title TEXT, violations TEXT NOT NULL);
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    append(&home, "r1", &(start("r1", "2026-11-01T10:00:00.000Z", "C:/p") + &msg("a")));
    assert_eq!(all(&home)[0].events.len(), 2);
    assert_eq!(event_count(&home, "r1"), 2);
}

/// A slot reserved by a runner that died (or whose CLI ended) is not held forever.
#[test]
fn slots_of_dead_holders_are_dropped() {
    let home = temp_home();
    let mut gone = std::process::Command::new(std::env::current_exe().unwrap()).arg("--list").stdout(std::process::Stdio::null()).spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    let conn = index::open(&home).unwrap();
    conn.execute("INSERT INTO slots (run_id, card_id, provider, owner) VALUES ('r1', 'a', 'codex', ?1)", [dead]).unwrap();
    conn.execute("INSERT INTO slots (run_id, card_id, provider, owner, pid) VALUES ('r1', 'b', 'codex', ?1, ?2)", [std::process::id(), dead]).unwrap();
    conn.execute("INSERT INTO slots (run_id, card_id, provider, owner) VALUES ('r2', 'c', 'claude', ?1)", [std::process::id()]).unwrap();
    let held = o_core::run::store::held_by_provider(&home).unwrap();
    assert_eq!((held[&ProviderId::Codex], held[&ProviderId::Claude]), (0, 1));
    let left: i64 = conn.query_row("SELECT count(*) FROM slots", [], |r| r.get(0)).unwrap();
    assert_eq!(left, 1);
}

#[test]
fn the_runs_row_sums_up_the_run() {
    let home = temp_home();
    let usage = r#"{"t":"card.usage","at":"x","cardId":"a","input":100,"cached":20,"output":5}"#;
    let end = |card: &str, status: &str| format!(r#"{{"t":"card.end","at":"x","cardId":"{card}","status":"{status}","exitCode":0}}"#);
    let run_end = r#"{"t":"run.end","at":"2026-11-01T10:05:00.000Z","runId":"r1","status":"failed"}"#;
    append(&home, "r1", &start("r1", "2026-11-01T10:00:00.000Z", "C:/p"));
    all(&home);
    append(&home, "r1", &format!("{usage}\n{usage}\n{}\n{}\n{run_end}\n", end("a", "done"), end("b", "failed")));
    all(&home);
    let row: (String, String, i64, i64, i64, i64, String) = index::open(&home)
        .unwrap()
        .query_row("SELECT title, status, cards, done, failed, tokens, ended_at FROM runs WHERE run_id = 'r1'", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })
        .unwrap();
    assert_eq!(row, ("t".into(), "failed".into(), 2, 1, 1, 250, "2026-11-01T10:05:00.000Z".into()));
}

#[test]
fn readers_ingesting_at_once_never_insert_a_line_twice() {
    let home = temp_home();
    append(&home, "r1", &start("r1", "2026-11-01T10:00:00.000Z", "C:/p"));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let home = home.clone();
            std::thread::spawn(move || {
                for _ in 0..40 {
                    all(&home);
                }
            })
        })
        .collect();
    for i in 0..200 {
        let event = OEvent::Msg { at: iso(Utc::now()), from: "lead".into(), to: "all".into(), text: format!("m{i}"), id: None, kind: None };
        append_event(&home, "r1", &event).unwrap();
    }
    for reader in readers {
        reader.join().unwrap();
    }
    let file = parse_event_values(&fs::read_to_string(run_dir(&home, "r1").join("events.jsonl")).unwrap());
    assert_eq!(file.len(), 201);
    assert_eq!(all(&home)[0].events, file);
    assert_eq!(event_count(&home, "r1"), 201);
}

#[test]
fn old_ledger_cooldowns_and_blocked_files_are_imported_once_and_kept_renamed() {
    let home = temp_home();
    let now = Utc::now();
    let ledger = format!(
        "{}\n{}\n{{broken\n",
        serde_json::json!({ "at": iso(now), "runId": "r0", "cardId": "a", "provider": "codex" }),
        serde_json::json!({ "at": iso(now - Duration::days(3)), "runId": "r0", "cardId": "b", "provider": "claude" }),
    );
    fs::write(home.join("ledger.jsonl"), ledger).unwrap();
    let until = iso(now + Duration::hours(1));
    fs::write(home.join("cooldowns.json"), serde_json::json!({ "codex": until, "nobody": 5 }).to_string()).unwrap();
    fs::write(home.join("blocked.jsonl"), r#"{"at":"2026-11-01T10:00:00.000Z","title":"x","violations":[{"rule":"r","message":"m"}]}"#.to_string() + "\n").unwrap();

    // Today's line and the broken one (counted as today, as the ledger did) — not the one three days ago.
    assert_eq!(started_on(&home, now).unwrap(), 2);
    assert_eq!(cooldown_times(&home).unwrap().get(&ProviderId::Codex), Some(&until));
    for name in ["ledger.jsonl", "cooldowns.json", "blocked.jsonl"] {
        assert!(!home.join(name).exists(), "{name} still there");
        assert!(home.join(format!("{name}.imported")).exists(), "{name}.imported missing");
    }
    let blocked: (String, String) = index::open(&home).unwrap().query_row("SELECT title, violations FROM blocked", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(blocked, ("x".into(), r#"[{"rule":"r","message":"m"}]"#.into()));

    // A file that shows up again later (an older build still writing) is not imported over the table.
    fs::write(home.join("ledger.jsonl"), serde_json::json!({ "at": iso(now), "runId": "r9", "cardId": "z", "provider": "agy" }).to_string() + "\n").unwrap();
    assert_eq!(started_on(&home, now).unwrap(), 2);
    assert!(home.join("ledger.jsonl").exists());
}

#[test]
fn the_daily_cap_counts_starts_of_the_local_day() {
    let home = temp_home();
    let now = Utc::now();
    assert_eq!(started_on(&home, now).unwrap(), 0);
    record_start(&home, "r1", "a", ProviderId::Codex, now).unwrap();
    record_start(&home, "r1", "b", ProviderId::Claude, now).unwrap();
    record_start(&home, "r0", "a", ProviderId::Codex, now - Duration::days(1)).unwrap();
    record_start(&home, "old", "a", ProviderId::Agy, now - Duration::days(10)).unwrap();
    assert_eq!(started_on(&home, now).unwrap(), 2);
    assert_eq!(started_on(&home, now - Duration::days(1)).unwrap(), 1);
}
