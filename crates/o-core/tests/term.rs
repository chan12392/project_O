#[cfg(windows)]
#[test]
fn lead_terminal_rejects_a_thread_from_another_project() {
    use o_core::{lead, term::{self, OpenRequest}};
    let base = std::env::temp_dir().join(format!("o-term-thread-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    let home = base.join("state");
    let project = base.join("project");
    let other = base.join("other");
    for path in [&home, &project, &other] { std::fs::create_dir_all(path).unwrap(); }
    let thread = lead::new_thread(&home, &other.to_string_lossy()).unwrap();
    let request = OpenRequest { provider: "agy".into(), project: project.to_string_lossy().into_owned(), thread: Some(thread), cols: None, rows: None };
    let error = term::open_with_host(&home, request, &base.join("unused.exe")).unwrap_err();
    assert!(error.to_string().contains("열린 총괄 대화가 아닙니다"), "{error}");
    assert!(term::list(&home).unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn interactive_session_round_trip() {
    use std::{path::PathBuf, thread, time::{Duration, Instant}};
    use o_core::{sys::launch::Launch, term};
    let home = std::env::temp_dir().join(format!("project-o-term-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    std::fs::create_dir_all(&home).unwrap();
    let mut launch = Launch::new(PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32").join("cmd.exe"));
    launch.arg("/K");
    launch.current_dir(&home);
    let session = term::open_card(&home, "test".into(), home.to_string_lossy().into(), "run".into(), "card".into(), launch, "echo READYMARK".into(), true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while term::read(&home, &session.id, 0).unwrap().offset == 0 { assert!(Instant::now() < deadline, "no startup output"); thread::sleep(Duration::from_millis(30)); }
    assert_eq!(term::get(&home, &session.id).unwrap().state, "awaiting-paste");
    assert!(!String::from_utf8_lossy(&term::decode_data(&term::read(&home, &session.id, 0).unwrap().data).unwrap()).contains("READYMARK"));
    term::paste(&home, &session.id).unwrap();
    thread::sleep(Duration::from_secs(2));
    assert_eq!(term::get(&home, &session.id).unwrap().state, "running");
    term::write(&home, &session.id, b"echo ROUNDTRIPMARK\r").unwrap();
    term::resize(&home, &session.id, 90, 27).unwrap();
    loop {
        let chunk = term::read(&home, &session.id, 0).unwrap();
        let bytes = term::decode_data(&chunk.data).unwrap();
        if String::from_utf8_lossy(&bytes).contains("ROUNDTRIPMARK") { break; }
        assert!(Instant::now() < deadline, "no echoed input: {:?}", String::from_utf8_lossy(&bytes));
        thread::sleep(Duration::from_millis(5));
    }
    let received = home.join("received.txt");
    thread::scope(|scope| for writer in 0..4 {
        let home = &home; let id = &session.id; let received = &received;
        scope.spawn(move || for number in 0..25 {
            term::write(home, id, format!("echo {writer}:{number}>>\"{}\"\r", received.display()).as_bytes()).unwrap();
        });
    });
    let delivery_deadline = Instant::now() + Duration::from_secs(30);
    let lines = loop {
        let contents = std::fs::read_to_string(&received).unwrap_or_default();
        let lines: Vec<_> = contents.lines().map(str::to_string).collect();
        if lines.len() >= 100 { break lines; }
        assert!(Instant::now() < delivery_deadline, "only {} inputs delivered", lines.len());
        thread::sleep(Duration::from_millis(30));
    };
    let mut counts = [0; 4];
    assert_eq!(lines.len(), 100);
    for line in lines {
        let (writer, number) = line.trim().split_once(':').unwrap();
        let writer: usize = writer.parse().unwrap();
        assert_eq!(number.parse::<usize>().unwrap(), counts[writer]);
        counts[writer] += 1;
    }
    assert_eq!(counts, [25; 4]);
    term::close(&home, &session.id).unwrap();
    let ended = term::wait(&home, &session.id, Duration::from_secs(10)).unwrap();
    assert_eq!(ended.state, "exited");
    assert_eq!((ended.cols, ended.rows), (90, 27));
}

#[cfg(windows)]
#[test]
fn terminal_close_remains_responsive_when_child_does_not_read() {
    use std::time::{Duration, Instant};
    use o_core::{sys::launch::Launch, term};
    let home = std::env::temp_dir().join(format!("project-o-term-blocked-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    std::fs::create_dir_all(&home).unwrap();
    let mut launch = Launch::new(std::env::current_exe().unwrap());
    launch.args(["--exact", "tree_child_fixture"]).current_dir(&home).env("O_TERM_TEST_CHILD", "1");
    let session = term::open_card(&home, "test".into(), home.to_string_lossy().into(), "run".into(), "card".into(), launch, "".into(), true).unwrap();
    for _ in 0..16 { term::write(&home, &session.id, &vec![b'x'; 65536]).unwrap(); }
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    term::close(&home, &session.id).unwrap();
    assert_eq!(term::wait(&home, &session.id, Duration::from_secs(8)).unwrap().state, "exited");
    assert!(started.elapsed() < Duration::from_secs(8));
}

#[cfg(windows)]
#[test]
fn terminal_close_kills_the_child_tree() {
    use std::{thread, time::{Duration, Instant}};
    use o_core::{sys::{launch::Launch, proc}, term};
    let home = std::env::temp_dir().join(format!("project-o-term-tree-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    std::fs::create_dir_all(&home).unwrap();
    let pid_file = home.join("child-pid");
    let mut launch = Launch::new(std::env::current_exe().unwrap());
    launch.args(["--exact", "tree_parent_fixture", "--nocapture"]).current_dir(&home).env("O_TERM_TEST_PID_FILE", pid_file.as_os_str());
    let session = term::open_card(&home, "test".into(), home.to_string_lossy().into(), "run".into(), "card".into(), launch, "".into(), true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let pid = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file) { if let Ok(pid) = text.parse::<u32>() { break pid; } }
        assert!(Instant::now() < deadline, "child did not start");
        thread::sleep(Duration::from_millis(20));
    };
    let started = proc::started_at(pid).expect("child is alive");
    term::close(&home, &session.id).unwrap();
    term::wait(&home, &session.id, Duration::from_secs(10)).unwrap();
    while !proc::definitely_exited(pid, started) { assert!(Instant::now() < deadline, "child survived close"); thread::sleep(Duration::from_millis(20)); }
}

#[cfg(windows)]
#[test]
fn a_dead_host_is_reported_exited() {
    use std::{os::windows::process::CommandExt, process::{Command, Stdio}};
    use o_core::{sys::proc, term};
    let home = std::env::temp_dir().join(format!("project-o-term-host-{}", o_core::run::store::new_run_id(chrono::Utc::now())));
    let id = "t-20261111-093000-ab12";
    let folder = home.join("terminals").join(id);
    std::fs::create_dir_all(&folder).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap()).args(["--exact", "tree_child_fixture"]).env("O_TERM_TEST_CHILD", "1").creation_flags(0x0800_0000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    let started = proc::started_at(child.id()).unwrap();
    std::fs::write(folder.join("session.json"), serde_json::to_vec(&serde_json::json!({"id":id,"provider":"test","project":"test","purpose":"lead","threadId":null,"runId":null,"cardId":null,"state":"running","exitCode":null,"cols":80,"rows":24,"startedAt":"2026-11-11T00:00:00.000Z"})).unwrap()).unwrap();
    std::fs::write(folder.join("host.json"), serde_json::to_vec(&serde_json::json!({"pid":child.id(),"started":started})).unwrap()).unwrap();
    assert_eq!(term::get(&home, id).unwrap().state, "running");
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(term::get(&home, id).unwrap().state, "exited");
}

#[cfg(windows)]
#[test]
fn tree_parent_fixture() {
    use std::{os::windows::process::CommandExt, process::{Command, Stdio}};
    let Some(path) = std::env::var_os("O_TERM_TEST_PID_FILE") else { return };
    let mut child = Command::new(std::env::current_exe().unwrap()).args(["--exact", "tree_child_fixture"]).env("O_TERM_TEST_CHILD", "1").creation_flags(0x0800_0000).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
    std::fs::write(path, child.id().to_string()).unwrap();
    let _ = child.wait();
}

#[cfg(windows)]
#[test]
fn tree_child_fixture() {
    if std::env::var_os("O_TERM_TEST_CHILD").is_some() { std::thread::sleep(std::time::Duration::from_secs(60)); }
}
