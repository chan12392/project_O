//! Tests for stopping cards during process startup and exit.
use super::*;

#[test]
fn fake_child() {
    if let Some(home) = std::env::var_os("O_STOP_RACE_STOP_HOME") {
        assert!(stop_run(Path::new(&home), "run").unwrap());
        return;
    }
    if let Some(path) = std::env::var_os("O_STOP_RACE_MARKER") { fs::write(path, "started").unwrap(); }
    if std::env::var_os("O_STOP_RACE_FIXTURE").is_some() {
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
    }
}

#[test]
fn stop_completed_before_resume_never_starts_child() {
    for dependent in [false, true] {
        let base = std::env::temp_dir().join(format!("o-stop-race-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
        let home = base.join("home");
        let project = base.join("project");
        fs::create_dir_all(&project).unwrap();
        let marker = base.join("started");
        let mut cards = vec![];
        if dependent { cards.push(serde_json::json!({"id":"first","provider":"claude","task":"first","resources":[{"kind":"external","key":"first","mode":"write"}]})); }
        cards.push(serde_json::json!({"id":"stop-race-test","provider":"claude","task":"target","depends_on":if dependent { vec!["first"] } else { vec![] },"resources":[{"kind":"external","key":"target","mode":"write"}]}));
        let plan: Plan = serde_json::from_value(serde_json::json!({"title":"test","project":project.to_string_lossy(),"cards":cards})).unwrap();
        super::super::store::create_run(&home, "run", &plan).unwrap();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (continue_tx, continue_rx) = mpsc::sync_channel(1);
        *BEFORE_TEST_RESUME.lock().unwrap() = Some(Box::new(move || {
            ready_tx.send(()).unwrap();
            let _ = continue_rx.recv_timeout(Duration::from_secs(10));
        }));
        let output = marker.clone();
        let helper = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("o-handle-owner.exe");
        let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
            let mut launch = Launch::new(std::env::current_exe()?);
            launch.handle_owner_executable = Some(helper.clone());
            launch.current_dir(cwd).args(["--exact", "run::runner::stop_race_tests::fake_child", "--nocapture"]).env("O_STOP_RACE_FIXTURE", "1");
            if spec.env["O_CARD_ID"] == "stop-race-test" { launch.env("O_STOP_RACE_MARKER", &output); }
            Ok(launch)
        });
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let worker_home = home.clone();
        let worker = thread::spawn(move || { let result = run_plan(&worker_home, "run", &plan, None, spawner); let _ = done_tx.send(result); });
        ready_rx.recv_timeout(Duration::from_secs(10)).expect("child did not reach resume boundary");
        assert!(stop_run(&home, "run").unwrap());
        continue_tx.send(()).unwrap();
        let status = done_rx.recv_timeout(Duration::from_secs(10)).expect("runner did not finish").unwrap();
        worker.join().unwrap();
        assert_eq!(status, CardStatus::Stopped);
        assert!(!marker.exists(), "stopped child executed (dependent={dependent})");
        let events = super::super::store::read_events(&home, "run");
        assert!(!events.iter().any(|e| matches!(e, OEvent::CardStart { card_id, .. } if card_id == "stop-race-test")));
        assert!(events.iter().any(|e| matches!(e, OEvent::CardEnd { card_id, status: CardStatus::Stopped, .. } if card_id == "stop-race-test")));
        assert!(!super::super::store::resource_held(&home, "run", "stop-race-test").unwrap());
        assert_eq!(super::super::store::started_on(&home, Utc::now()).unwrap(), usize::from(dependent));
    }
}

#[test]
fn stop_from_another_process_waits_for_start_gate() {
    let base = std::env::temp_dir().join(format!("o-stop-gate-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    let home = base.join("home");
    fs::create_dir_all(super::super::store::run_dir(&home, "run")).unwrap();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let held_home = home.clone();
    let holder = thread::spawn(move || super::super::store::with_start_gate(&held_home, "run", || {
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).map_err(io::Error::other)?;
        Ok(())
    }));
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "run::runner::stop_race_tests::fake_child", "--nocapture"])
        .env("O_STOP_RACE_STOP_HOME", &home).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let mut child = proc::hide_window(&mut command).spawn().unwrap();
    thread::sleep(Duration::from_millis(200));
    let waited = child.try_wait().unwrap().is_none();
    let before = stop_requested(&home, "run");
    release_tx.send(()).unwrap();
    holder.join().unwrap().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() { break Some(status); }
        if Instant::now() >= deadline { let _ = child.kill(); let _ = child.wait(); break None; }
        thread::sleep(Duration::from_millis(20));
    };
    assert!(waited && !before, "stop crossed the active start gate");
    assert!(status.is_some_and(|status| status.success()));
    assert!(stop_requested(&home, "run"));
}

#[test]
fn stop_in_same_process_waits_for_start_gate() {
    let home = std::env::temp_dir().join(format!("o-stop-thread-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(super::super::store::run_dir(&home, "run")).unwrap();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let held_home = home.clone();
    let holder = thread::spawn(move || super::super::store::with_start_gate(&held_home, "run", || {
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).map_err(io::Error::other)?;
        Ok(())
    }));
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let stop_home = home.clone();
    let stopper = thread::spawn(move || { let _ = done_tx.send(stop_run(&stop_home, "run")); });
    let waited = matches!(done_rx.recv_timeout(Duration::from_millis(200)), Err(mpsc::RecvTimeoutError::Timeout));
    let before = stop_requested(&home, "run");
    release_tx.send(()).unwrap();
    holder.join().unwrap().unwrap();
    let stopped = done_rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    stopper.join().unwrap();
    assert!(waited && !before && stopped);
}

#[test]
fn stop_does_not_wait_for_event_lock() {
    let home = std::env::temp_dir().join(format!("o-stop-events-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(super::super::store::run_dir(&home, "run")).unwrap();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let held_home = home.clone();
    let holder = thread::spawn(move || super::super::store::with_events(&held_home, "run", |_| {
        ready_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).map_err(io::Error::other)?;
        Ok(())
    }));
    ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let stop_home = home.clone();
    let stopper = thread::spawn(move || { let _ = done_tx.send(stop_run(&stop_home, "run")); });
    let stopped = done_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    holder.join().unwrap().unwrap();
    stopper.join().unwrap();
    assert!(stopped.unwrap().unwrap());
}
