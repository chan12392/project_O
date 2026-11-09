//! A run whose runner died is closed on record, once, and only on strong evidence; on Windows a
//! dead worker's orphaned descendants keep its lease from being offered for recovery (ADR-0039).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use o_core::run::index::RunQuery;
use o_core::run::store::{self, parse_event_values, read_runs, run_dir};
use o_core::sys::proc;
use serde_json::{json, Value};

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn temp_home(tag: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("o-dead-runner-{tag}-{}-{n}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A process that has already exited, as (pid, start time) read while it still existed.
fn exited_process() -> (u32, u64) {
    let mut command = if cfg!(windows) { let mut c = Command::new("cmd"); c.args(["/C", "exit 0"]); c } else { let mut c = Command::new("sh"); c.args(["-c", "exit 0"]); c };
    let mut child = proc::hide_window(&mut command).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let pid = child.id();
    // Unreaped (Unix) or still held by our handle (Windows): its start time is still readable.
    let started = proc::started_at(pid).expect("start time of the short child");
    child.wait().unwrap();
    drop(child);
    assert!(proc::definitely_exited(pid, started));
    (pid, started)
}

fn write_run(home: &Path, run_id: &str, followup: bool) {
    let dir = run_dir(home, run_id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plan.json"), json!({"title":"t","project":"C:/p","cards":[{"id":"a"},{"id":"b"},{"id":"c"}]}).to_string()).unwrap();
    let mut lines = vec![json!({"t":"run.start","at":"2026-11-09T12:37:16.000Z","runId":run_id,"title":"t","project":"C:/p","cards":[{"id":"a","provider":"claude"},{"id":"b","provider":"claude"},{"id":"c","provider":"claude"}]})];
    if followup {
        lines.push(json!({"t":"run.followup","at":"2026-11-09T12:37:16.000Z","sourceRunId":"source","cardId":"b"}));
    }
    lines.push(json!({"t":"card.start","at":"2026-11-09T12:37:17.000Z","cardId":"a"}));
    lines.push(json!({"t":"card.end","at":"2026-11-09T12:38:00.000Z","cardId":"a","status":"done","exitCode":0}));
    lines.push(json!({"t":"card.start","at":"2026-11-09T12:38:01.000Z","cardId":"b"}));
    lines.push(json!({"t":"card.queued","at":"2026-11-09T12:38:01.000Z","cardId":"c","reason":"b를 기다림"}));
    fs::write(dir.join("events.jsonl"), lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
}

fn on_disk(home: &Path, run_id: &str) -> Vec<Value> {
    parse_event_values(&fs::read_to_string(run_dir(home, run_id).join("events.jsonl")).unwrap())
}

fn all(home: &Path) -> Vec<store::RunRecord> {
    read_runs(home, &RunQuery { limit: 50, project: None }).unwrap()
}

#[test]
fn a_dead_runners_run_is_closed_on_record_once() {
    let home = temp_home("close");
    let (pid, started) = exited_process();
    write_run(&home, "dead", false);
    fs::write(run_dir(&home, "dead").join("runner.json"), json!({"pid":pid,"started":started}).to_string()).unwrap();
    write_run(&home, "dead-followup", true);
    fs::write(run_dir(&home, "dead-followup").join("runner.json"), json!({"pid":pid,"started":started}).to_string()).unwrap();
    write_run(&home, "live", false);
    store::mark_runner(&home, "live").unwrap();
    write_run(&home, "unmarked", false);
    write_run(&home, "unknown-start", false);
    fs::write(run_dir(&home, "unknown-start").join("runner.json"), json!({"pid":pid}).to_string()).unwrap();

    let runs = all(&home);
    let events = on_disk(&home, "dead");
    let ends: Vec<&Value> = events.iter().filter(|e| e["t"] == "card.end" && e["status"] == "failed").collect();
    assert_eq!(ends.iter().map(|e| e["cardId"].as_str().unwrap()).collect::<Vec<_>>(), ["b", "c"]);
    assert!(ends.iter().all(|e| e["result"] == "실행기가 끝나 이 카드를 마치지 못했습니다(중단됨). 작업 폴더를 확인하고 다시 보내 주세요" && e["exitCode"].is_null()));
    assert_eq!(events.last().unwrap()["t"], "run.end");
    assert_eq!(events.last().unwrap()["status"], "failed");
    // The returned record already shows the closing events, and the index picks them up next time.
    let record = runs.iter().find(|r| r.run_id == "dead").unwrap();
    assert_eq!(record.events.len(), events.len());
    let followup = on_disk(&home, "dead-followup");
    assert!(followup.iter().any(|e| e["t"] == "card.end" && e["cardId"] == "b" && e["result"] == "후속 실행이 중단됐습니다. 다시 보내 주세요"));
    assert_eq!(followup.last().unwrap()["t"], "run.end");

    // A second read appends nothing more, and neither does `o status`'s path.
    let count = events.len();
    let again = all(&home);
    assert_eq!(on_disk(&home, "dead").len(), count);
    assert_eq!(again.iter().find(|r| r.run_id == "dead").unwrap().events.len(), count);
    assert!(!store::close_if_runner_died(&home, "dead", &store::read_events(&home, "dead"), &[]).unwrap());
    assert!(store::close_dead_run(&home, "dead", &[]).unwrap().is_empty());
    assert_eq!(on_disk(&home, "dead").len(), count);

    // A live runner, a run whose runner never wrote its mark, and an unknown start time stay open.
    for run in ["live", "unmarked", "unknown-start"] {
        assert!(!on_disk(&home, run).iter().any(|e| e["t"] == "run.end"), "{run}");
        assert!(!again.iter().find(|r| r.run_id == run).unwrap().events.iter().any(|e| e["t"] == "run.end"), "{run}");
    }
}

#[test]
fn a_live_card_marker_keeps_a_dead_runners_run_open() {
    let home = temp_home("marker");
    let (pid, started) = exited_process();
    write_run(&home, "dead", false);
    fs::write(run_dir(&home, "dead").join("runner.json"), json!({"pid":pid,"started":started}).to_string()).unwrap();
    let marker = store::ActiveMarker { run_id: "dead".into(), card_id: "b".into(), provider: o_core::run::plan::ProviderId::Claude, pid: std::process::id() };
    store::mark_active(&home, &marker).unwrap();
    all(&home);
    assert!(!on_disk(&home, "dead").iter().any(|e| e["t"] == "run.end"));
    store::clear_active(&home, "dead", "b");
    all(&home);
    assert_eq!(on_disk(&home, "dead").last().unwrap()["t"], "run.end");
}

#[cfg(windows)]
mod windows_tree {
    use super::*;
    use std::time::{Duration, Instant};

    const ROLE: &str = "O_TEST_DEAD_RUNNER_ROLE";
    const PID_FILE: &str = "O_TEST_DEAD_RUNNER_PID_FILE";
    const NAME: &str = "windows_tree::an_orphaned_descendant_keeps_the_worker_tree_alive";

    fn this_test() -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", NAME, "--nocapture", "--test-threads", "1"]);
        command
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if done() { return true; }
            std::thread::sleep(Duration::from_millis(50));
        }
        done()
    }

    #[test]
    fn an_orphaned_descendant_keeps_the_worker_tree_alive() {
        match std::env::var(ROLE).as_deref() {
            // The card CLI's child: outlives the worker that started it.
            Ok("sleeper") => std::thread::sleep(Duration::from_secs(5)),
            // The worker: starts the sleeper, says which pid it got, and exits.
            Ok("worker") => {
                // Left running on purpose: the orphan is what the test looks for.
                #[allow(clippy::zombie_processes)]
                let child = proc::spawn_detached(this_test().env(ROLE, "sleeper")).unwrap();
                fs::write(std::env::var(PID_FILE).unwrap(), child.id().to_string()).unwrap();
            }
            _ => {
                let home = temp_home("tree");
                let pid_file = home.join("sleeper.pid");
                let mut worker = proc::hide_window(this_test().env(ROLE, "worker").env(PID_FILE, &pid_file).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())).spawn().unwrap();
                let worker_pid = worker.id();
                let worker_started = proc::started_at(worker_pid).unwrap();
                assert!(worker.wait().unwrap().success());
                let sleeper: u32 = fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
                assert!(proc::definitely_exited(worker_pid, worker_started));
                assert!(proc::is_alive(sleeper), "the sleeper should still run");
                assert_eq!(proc::descendants_alive(worker_pid, worker_started), Some(true));
                // The sleeper was started after the worker: a later start time on the same pid is not its parent.
                assert_eq!(proc::descendants_alive(worker_pid, u64::MAX), Some(false));

                // The lease of a dead runner's card: owner gone, worker gone, no exit confirmation.
                let (owner, owner_started) = exited_process();
                let project = home.join("project");
                fs::create_dir_all(&project).unwrap();
                let card = o_core::run::plan::Card { id: "gui".into(), provider: o_core::run::plan::ProviderId::Claude, role: None, task: "t".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
                let store::Reserved::Granted(_) = store::reserve_slot(&home, "run", &card, project.to_str().unwrap(), chrono::Utc::now()).unwrap() else { panic!("not granted") };
                let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
                conn.execute("UPDATE resource_leases SET owner_pid=?1, owner_started=?2, worker_pid=?3, worker_started=?4, spawn_intent=1", rusqlite::params![owner, owner_started as i64, worker_pid, worker_started as i64]).unwrap();
                drop(conn);
                let id: i64 = store::resource_leases(&home).unwrap()[0].id.parse().unwrap();
                assert_eq!(store::resource_leases(&home).unwrap()[0].state, "unknown", "a live descendant keeps the lease");
                assert!(store::recover_resource(&home, id).is_err());

                assert!(wait_until(Duration::from_secs(15), || !proc::is_alive(sleeper)), "the sleeper did not end");
                assert_eq!(proc::descendants_alive(worker_pid, worker_started), Some(false));
                assert_eq!(store::resource_leases(&home).unwrap()[0].state, "recoverable");
                store::recover_resource(&home, id).unwrap();
                assert!(!store::resource_held(&home, "run", "gui").unwrap());
                // The card did start: its start still counts against the day.
                assert_eq!(store::started_on(&home, chrono::Utc::now()).unwrap(), 1);
                drop(worker);
            }
        }
    }
}
