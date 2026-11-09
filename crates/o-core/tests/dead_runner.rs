//! A run whose runner died is closed on record, once, and only on strong evidence; a card it cut
//! off is followed up in a fresh session; on Windows a dead runner's lease becomes recoverable only
//! once the card's named kill-on-close job is gone (ADR-0039).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use o_core::run::events::{CardStatus, OEvent};
use o_core::run::index::RunQuery;
use o_core::run::plan::{Card, Plan, ProviderId};
use o_core::run::store::{self, parse_event_values, read_runs, run_dir};
use o_core::sys::proc;
use serde_json::{json, Value};

static NEXT: AtomicUsize = AtomicUsize::new(0);

const RUNNER_STOPPED: &str = "실행기가 끝나 이 카드를 마치지 못했습니다(중단됨). 작업 폴더를 확인하고 다시 보내 주세요";
const FOLLOWUP_STOPPED: &str = "후속 실행이 중단됐습니다. 다시 보내 주세요";

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

fn mark_dead_runner(home: &Path, run_id: &str) {
    let (pid, started) = exited_process();
    fs::write(run_dir(home, run_id).join("runner.json"), json!({"pid":pid,"started":started}).to_string()).unwrap();
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

fn has_run_end(events: &[Value]) -> bool {
    events.iter().any(|e| e["t"] == "run.end")
}

fn card(id: &str) -> Card {
    Card { id: id.into(), provider: ProviderId::Claude, role: None, task: "t".into(), model: None, effort: None, depends_on: vec![], resources: vec![] }
}

#[test]
fn a_dead_runners_run_is_closed_on_record_once() {
    let home = temp_home("close");
    write_run(&home, "dead", false);
    mark_dead_runner(&home, "dead");
    write_run(&home, "dead-followup", true);
    mark_dead_runner(&home, "dead-followup");
    write_run(&home, "live", false);
    store::mark_runner(&home, "live").unwrap();
    write_run(&home, "unmarked", false);
    write_run(&home, "unknown-start", false);
    let (pid, _) = exited_process();
    fs::write(run_dir(&home, "unknown-start").join("runner.json"), json!({"pid":pid}).to_string()).unwrap();

    let runs = all(&home);
    let events = on_disk(&home, "dead");
    let ends: Vec<&Value> = events.iter().filter(|e| e["t"] == "card.end" && e["status"] == "failed").collect();
    assert_eq!(ends.iter().map(|e| e["cardId"].as_str().unwrap()).collect::<Vec<_>>(), ["b", "c"]);
    assert!(ends.iter().all(|e| e["result"] == RUNNER_STOPPED && e["exitCode"].is_null()));
    assert_eq!(events.last().unwrap()["t"], "run.end");
    assert_eq!(events.last().unwrap()["status"], "failed");
    // The returned record is the whole current event list, the closing events included.
    assert_eq!(runs.iter().find(|r| r.run_id == "dead").unwrap().events, events);
    let followup = on_disk(&home, "dead-followup");
    assert!(followup.iter().any(|e| e["t"] == "card.end" && e["cardId"] == "b" && e["result"] == FOLLOWUP_STOPPED));
    assert_eq!(followup.last().unwrap()["t"], "run.end");

    // A second read appends nothing more, and neither does `o status`'s path.
    let count = events.len();
    let again = all(&home);
    assert_eq!(on_disk(&home, "dead").len(), count);
    assert_eq!(again.iter().find(|r| r.run_id == "dead").unwrap().events.len(), count);
    assert!(store::close_if_runner_died(&home, "dead", &store::read_events(&home, "dead")).unwrap().is_none());
    assert_eq!(store::close_dead_run(&home, "dead").unwrap().unwrap(), events);
    assert_eq!(on_disk(&home, "dead").len(), count);

    // A live runner, a run whose runner never wrote its mark, and an unknown start time stay open.
    for run in ["live", "unmarked", "unknown-start"] {
        assert!(!has_run_end(&on_disk(&home, run)), "{run}");
        assert!(!has_run_end(&again.iter().find(|r| r.run_id == run).unwrap().events), "{run}");
        assert!(store::close_dead_run(&home, run).unwrap().is_none(), "{run}");
    }
}

#[test]
fn a_live_card_marker_keeps_a_dead_runners_run_open() {
    let home = temp_home("marker");
    write_run(&home, "dead", false);
    mark_dead_runner(&home, "dead");
    let marker = store::ActiveMarker { run_id: "dead".into(), card_id: "b".into(), provider: ProviderId::Claude, pid: std::process::id() };
    store::mark_active(&home, &marker).unwrap();
    // The marker is read under the append lock: the run's current events come back, still open.
    let current = store::close_dead_run(&home, "dead").unwrap().expect("looked under the lock");
    assert!(!has_run_end(&current));
    assert_eq!(current, on_disk(&home, "dead"));
    all(&home);
    assert!(!has_run_end(&on_disk(&home, "dead")));
    store::clear_active(&home, "dead", "b");
    all(&home);
    assert_eq!(on_disk(&home, "dead").last().unwrap()["t"], "run.end");
}

#[test]
fn a_run_whose_plan_cannot_be_read_is_left_open() {
    let home = temp_home("plan");
    for (run, plan) in [("missing", None), ("broken", Some("{")), ("no-cards", Some(r#"{"title":"t"}"#))] {
        write_run(&home, run, false);
        mark_dead_runner(&home, run);
        let path = run_dir(&home, run).join("plan.json");
        match plan { None => fs::remove_file(&path).unwrap(), Some(text) => fs::write(&path, text).unwrap() }
        let before = on_disk(&home, run);
        // Never a run.end without the cards' ends: nothing is written.
        assert!(store::close_dead_run(&home, run).unwrap().is_none(), "{run}");
        assert_eq!(on_disk(&home, run), before, "{run}");
    }
    let runs = all(&home);
    for run in ["missing", "broken", "no-cards"] {
        assert!(!has_run_end(&on_disk(&home, run)), "{run}");
        assert!(!has_run_end(&runs.iter().find(|r| r.run_id == run).unwrap().events), "{run}");
    }
}

#[test]
fn a_reader_with_stale_events_reports_the_close_another_reader_made() {
    let home = temp_home("stale");
    write_run(&home, "dead", false);
    mark_dead_runner(&home, "dead");
    // `o status` read the events, then another reader (the app) closed the run.
    let stale = store::read_events(&home, "dead");
    let closed = store::close_dead_run(&home, "dead").unwrap().unwrap();
    assert!(has_run_end(&closed));
    let current = store::close_if_runner_died(&home, "dead", &stale).unwrap().expect("the current events");
    assert!(matches!(current.last(), Some(OEvent::RunEnd { status: CardStatus::Failed, .. })));
    assert_eq!(current.len(), closed.len());
    // Events appended after a reader's view (here: a team message) are in what it gets back.
    store::append_event(&home, "dead", &serde_json::from_value(json!({"t":"card.message","at":"2026-11-09T12:40:00.000Z","cardId":"lead","text":"확인"})).unwrap()).unwrap();
    assert_eq!(store::close_if_runner_died(&home, "dead", &stale).unwrap().unwrap().len(), closed.len() + 1);
    // `o runs` agrees with the file.
    assert_eq!(all(&home).iter().find(|r| r.run_id == "dead").unwrap().events, on_disk(&home, "dead"));
}

#[test]
fn a_card_closed_by_a_dead_runner_is_followed_up_in_a_fresh_session() {
    let fixture = temp_home("followup");
    let home = fixture.join("state");
    let project = fixture.join("project");
    fs::create_dir_all(&project).unwrap();
    let plan = Plan { title: "t".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("a")] };
    let start = |run: &str| vec![
        json!({"t":"run.start","at":"2026-11-09T12:37:16.000Z","runId":run,"title":"t","project":plan.project,"cards":[{"id":"a","provider":"claude"}]}),
        json!({"t":"card.start","at":"2026-11-09T12:37:17.000Z","cardId":"a"}),
        json!({"t":"card.session","at":"2026-11-09T12:37:18.000Z","cardId":"a","sessionId":"s1"}),
    ];
    for run in ["cut", "failed"] {
        store::create_run(&home, run, &plan).unwrap();
        let mut lines = start(run);
        if run == "failed" {
            lines.push(json!({"t":"card.end","at":"2026-11-09T12:38:00.000Z","cardId":"a","status":"failed","result":"오류","exitCode":1}));
            lines.push(json!({"t":"run.end","at":"2026-11-09T12:38:00.000Z","runId":run,"status":"failed"}));
        }
        fs::write(run_dir(&home, run).join("events.jsonl"), lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    }
    mark_dead_runner(&home, "cut");
    all(&home);
    assert!(on_disk(&home, "cut").iter().any(|e| e["t"] == "card.end" && e["cardId"] == "a" && e["result"] == RUNNER_STOPPED));

    // The cut-off session is not resumed; the follow-up starts fresh in the same project.
    let fresh = o_core::run::followup::target(&home, "cut", "a").unwrap();
    assert_eq!(fresh.session, None);
    assert_eq!(fresh.workspace, PathBuf::from(store::resolved_path(&plan.project)));
    // A card that ended on its own, even failed, is resumed as before.
    assert_eq!(o_core::run::followup::target(&home, "failed", "a").unwrap().session.as_deref(), Some("s1"));

    // A follow-up of that fresh start which reported no session of its own starts fresh too.
    let next = o_core::run::followup::start(&home, "cut", "a", "다시", &|_| Ok(())).unwrap();
    store::append_event(&home, &next, &OEvent::CardEnd { at: "2026-11-09T12:39:00.000Z".into(), card_id: "a".into(), status: CardStatus::Done, result: Some("ok".into()), exit_code: Some(0) }).unwrap();
    store::append_event(&home, &next, &OEvent::RunEnd { at: "2026-11-09T12:39:00.000Z".into(), run_id: next.clone(), status: CardStatus::Done }).unwrap();
    assert_eq!(o_core::run::followup::target(&home, &next, "a").unwrap().session, None);

    // The rules that keep a follow-up off a running card are unchanged.
    let marker = store::ActiveMarker { run_id: "cut".into(), card_id: "a".into(), provider: ProviderId::Claude, pid: std::process::id() };
    store::mark_active(&home, &marker).unwrap();
    assert!(o_core::run::followup::target(&home, "cut", "a").is_err());
    store::clear_active(&home, "cut", "a");
}

#[test]
fn a_dead_runners_lease_without_a_job_name_stays_unknown() {
    let home = temp_home("legacy");
    let project = home.join("project");
    fs::create_dir_all(&project).unwrap();
    let store::Reserved::Granted(lease) = store::reserve_slot(&home, "run", &card("old"), project.to_str().unwrap(), chrono::Utc::now()).unwrap() else { panic!("not granted") };
    store::mark_spawning(&home, &lease).unwrap();
    // Owner and worker both certainly exited, as for a lease made before job names were recorded.
    let (owner, owner_started) = exited_process();
    let (worker, worker_started) = exited_process();
    let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
    conn.execute("UPDATE resource_leases SET owner_pid=?1, owner_started=?2, worker_pid=?3, worker_started=?4", rusqlite::params![owner, owner_started as i64, worker, worker_started as i64]).unwrap();
    drop(conn);
    let leases = store::resource_leases(&home).unwrap();
    assert_eq!(leases[0].job_name, None);
    assert_eq!(leases[0].state, "unknown");
    assert!(store::recover_resource(&home, leases[0].id.parse().unwrap()).is_err());
}

#[cfg(windows)]
mod windows_job {
    use super::*;
    use std::ffi::OsString;
    use std::os::windows::io::AsRawHandle;
    use o_core::sys::win_launch::{Child, HandleOwner, Launch};
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows_sys::Win32::System::JobObjects::CreateJobObjectW;
    use windows_sys::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

    /// A worker that runs until its input closes or its job ends it.
    fn reader(cwd: &Path) -> Launch {
        let root = std::env::var_os("SystemRoot").unwrap();
        Launch { executable: PathBuf::from(&root).join("System32").join("findstr.exe"), args: vec![OsString::from("x")], cwd: cwd.to_path_buf(), environment: vec![(OsString::from("SystemRoot"), root)] }
    }

    fn owner() -> HandleOwner {
        HandleOwner::start(env!("CARGO_BIN_EXE_o-handle-owner").into(), "--o-handle-owner").unwrap()
    }

    /// Waits on the child's handle; the bound only turns a kill that never comes into a failure.
    fn ends(child: &Child) -> bool {
        let handle = child.watch_handle().unwrap();
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 60_000) == WAIT_OBJECT_0 }
    }

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain([0]).collect()
    }

    fn nanos() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    fn state(home: &Path) -> &'static str {
        store::resource_leases(home).unwrap()[0].state
    }

    #[test]
    fn a_dead_runners_lease_is_recoverable_once_the_cards_job_is_gone() {
        let home = temp_home("job");
        let project = home.join("project");
        fs::create_dir_all(&project).unwrap();
        let store::Reserved::Granted(lease) = store::reserve_slot(&home, "run", &card("gui"), project.to_str().unwrap(), chrono::Utc::now()).unwrap() else { panic!("not granted") };
        store::mark_spawning(&home, &lease).unwrap();
        // What the card runner does: a named kill-on-close job, its name linked with the worker.
        let owner = owner();
        let (mut child, job) = reader(&project).spawn_in_named_job(&owner, "run-gui").unwrap();
        let name = job.job_name().expect("a named job").to_string();
        assert!(name.starts_with(r"Global\project_O-job-run-gui-"), "{name}");
        store::link_slot(&home, &lease, child.id(), Some(&name)).unwrap();
        child.resume().unwrap();
        assert!(!proc::job_gone(&name), "the job exists while the runner holds its handle");

        // The runner died: the lease's owner has exited.
        let (dead, dead_started) = exited_process();
        let conn = rusqlite::Connection::open(home.join("o.db")).unwrap();
        conn.execute("UPDATE resource_leases SET owner_pid=?1, owner_started=?2", rusqlite::params![dead, dead_started as i64]).unwrap();
        drop(conn);
        let leases = store::resource_leases(&home).unwrap();
        let id: i64 = leases[0].id.parse().unwrap();
        assert_eq!(leases[0].job_name.as_deref(), Some(name.as_str()));
        assert_eq!(leases[0].state, "unknown", "the worker still runs");

        // The worker exits while its job still exists (a process left in it would keep it so).
        drop(child.stdin.take());
        assert!(ends(&child));
        child.wait().unwrap();
        let worker_started: u64 = leases[0].worker_started.as_deref().unwrap().parse().unwrap();
        assert!(proc::definitely_exited(leases[0].worker_pid.unwrap(), worker_started));
        assert_eq!(state(&home), "unknown", "an open job keeps the lease");
        assert!(store::recover_resource(&home, id).is_err());

        // The last handle closes (the runner's death): the job and its name are gone.
        drop(job);
        assert!(proc::job_gone(&name));
        assert_eq!(state(&home), "recoverable");
        store::recover_resource(&home, id).unwrap();
        assert!(!store::resource_held(&home, "run", "gui").unwrap());
        // The card did start: its start still counts against the day.
        assert_eq!(store::started_on(&home, chrono::Utc::now()).unwrap(), 1);
        drop(owner);
    }

    #[test]
    fn closing_the_last_job_handle_ends_the_card() {
        let home = temp_home("kill");
        let owner = owner();
        let (child, job) = reader(&home).spawn_in_named_job(&owner, "run-kill").unwrap();
        let name = job.job_name().expect("a named job").to_string();
        child.resume().unwrap();
        // Its input stays open: only the job can end it.
        assert!(child.try_wait().unwrap().is_none());
        assert!(!proc::job_gone(&name));
        drop(job);
        assert!(ends(&child), "kill-on-close ended the worker");
        assert!(proc::job_gone(&name));
        drop(child);
        drop(owner);
    }

    #[test]
    fn only_a_missing_job_name_reads_as_gone() {
        let n = nanos();
        assert!(proc::job_gone(&format!(r"Global\project_O-job-never-made-{n}")));
        // An existing job, even one O did not make, is not gone.
        let unrelated = format!(r"Global\o-test-unrelated-job-{n}");
        let job = unsafe { CreateJobObjectW(std::ptr::null(), wide(&unrelated).as_ptr()) };
        assert!(!job.is_null());
        assert!(!proc::job_gone(&unrelated));
        unsafe { CloseHandle(job) };
        assert!(proc::job_gone(&unrelated));
        // A name held by another kind of object, a missing directory and an empty name prove nothing.
        let other = format!(r"Global\o-test-event-{n}");
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide(&other).as_ptr()) };
        assert!(!event.is_null());
        assert!(!proc::job_gone(&other));
        unsafe { CloseHandle(event) };
        assert!(!proc::job_gone(&format!(r"Global\o-test-missing-{n}\job")));
        assert!(!proc::job_gone(""));
    }
}
