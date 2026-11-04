use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use o_core::lead::{self, SendRequest};
use o_core::run::plan::ProviderId;

fn fixture() -> (PathBuf, String) {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-lead-recovery-{}-{unique}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    (base.join("state"), project.to_string_lossy().into_owned())
}

fn request(project: &str) -> SendRequest<'_> {
    SendRequest { project, provider: ProviderId::Claude, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false }
}

fn page_text(row: &serde_json::Value) -> String {
    use std::io::{BufRead, BufReader};
    let path = row["file"].as_str().unwrap();
    let first = row["pageStart"].as_u64().unwrap() as usize - 1;
    let count = row["pageCount"].as_u64().unwrap() as usize;
    BufReader::new(std::fs::File::open(path).unwrap()).lines().skip(first).take(count).map(|line| serde_json::from_str::<String>(&line.unwrap()).unwrap()).collect()
}

#[test]
fn fake_child() {
    if std::env::var_os("O_LEAD_SUCCESS_REPLY").is_some() { println!("\n{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"확인했습니다\"}}"); }
    if let Some(index) = std::env::var_os("O_STREAM_INDEX") {
        use std::io::{BufRead, BufReader};
        use sha2::{Digest, Sha256};
        let rows: Vec<serde_json::Value> = std::fs::read_to_string(index).unwrap().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
        let row = rows.iter().find(|row| row["type"] == "card.end").unwrap();
        let path = row["file"].as_str().unwrap();
        assert!(std::fs::write(path, b"changed").is_err());
        let first = row["pageStart"].as_u64().unwrap() as usize - 1;
        let count = row["pageCount"].as_u64().unwrap() as usize;
        let mut hash = Sha256::new();
        let mut seen = 0;
        for line in BufReader::new(std::fs::File::open(path).unwrap()).lines().skip(first).take(count) {
            let text: String = serde_json::from_str(&line.unwrap()).unwrap();
            hash.update(text.as_bytes());
            seen += 1;
        }
        assert_eq!(seen, count);
        assert_eq!(format!("{:x}", hash.finalize()), std::env::var("O_STREAM_SHA").unwrap());
    }
    if std::env::var_os("O_LEAD_RECOVERY_WAIT").is_some() { std::thread::sleep(Duration::from_secs(30)); }
    if std::env::var_os("O_LEAD_RECOVERY_SHORT").is_some() { std::thread::sleep(Duration::from_millis(500)); }
    if let Some(path) = std::env::var_os("O_LEAD_STATUS_PATH") {
        use std::io::Write;
        let index = std::fs::read_to_string(&path).unwrap();
        let item = index.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).find(|item| item["runId"] == "run-7" && item["cardId"] == "card" && item["type"] == "card.end").unwrap();
        let source = page_text(&item);
        assert_eq!(item["sha256"], o_core::run::result::digest(&source));
        assert!(std::fs::write(item["file"].as_str().unwrap(), b"tampered").is_err());
        assert!(source.contains("LAST-LINE-한글"));
        assert!(std::fs::write(path, b"tampered").is_err());
        let seen = std::env::var_os("O_LEAD_STATUS_SEEN").unwrap();
        writeln!(std::fs::OpenOptions::new().create(true).append(true).open(seen).unwrap(), "read").unwrap();
    }
}

#[test]
fn orphan_and_legacy_markers_never_authorize_a_kill_or_new_turn() {
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_WAIT", "1").stdout(Stdio::null()).stderr(Stdio::null());
    let mut other = o_core::sys::proc::hide_window(&mut command).spawn().unwrap();
    let pid = other.id();
    let started = o_core::sys::proc::started_at(pid).unwrap();
    for content in [
        serde_json::json!({ "provider": "claude", "owner": 123, "token": "old", "child": { "pid": pid, "started": started } }).to_string(),
        serde_json::json!({ "provider": "claude", "owner": 123, "token": "old", "child": { "pid": pid, "started": started + 1 } }).to_string(),
        serde_json::json!({ "provider": "claude", "owner": 123, "token": "old", "child": { "pid": pid } }).to_string(),
        "{broken".to_string(),
    ] {
        std::fs::write(&marker, &content).unwrap();
        assert_eq!(lead::show(&home, Some(&project))["thread"]["busy"], "unknown");
        assert!(!lead::stop(&home, Some(&project)));
        assert!(lead::new_thread(&home, &project).is_err());
        let spawner = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> { panic!("unexpected spawn") };
        assert!(lead::send(&home, &request(&project), &spawner).unwrap_err().contains("종료를 확인"));
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), content);
        assert!(other.try_wait().unwrap().is_none());
    }
    other.kill().unwrap();
    other.wait().unwrap();
}

#[cfg(windows)]
#[test]
fn simultaneous_fresh_turns_start_at_most_one_child() {
    use std::sync::{Arc, Barrier, atomic::{AtomicUsize, Ordering}};
    let (home, project) = fixture();
    let barrier = Arc::new(Barrier::new(3));
    let starts = Arc::new(AtomicUsize::new(0));
    let workers: Vec<_> = (0..2).map(|_| {
        let (home, project, barrier, starts) = (home.clone(), project.clone(), Arc::clone(&barrier), Arc::clone(&starts));
        std::thread::spawn(move || {
            let spawner = move |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
                starts.fetch_add(1, Ordering::SeqCst);
                let mut cmd = Command::new(std::env::current_exe()?);
                cmd.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_SHORT", "1");
                Ok(cmd)
            };
            barrier.wait();
            lead::send(&home, &SendRequest { fresh: true, ..request(&project) }, &spawner)
        })
    }).collect();
    barrier.wait();
    for worker in workers { let _ = worker.join().unwrap(); }
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

#[cfg(windows)]
#[test]
fn live_owner_stops_its_contained_child_and_releases_the_turn() {
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    let (worker_home, worker_project) = (home.clone(), project.clone());
    let worker = std::thread::spawn(move || {
        let spawner = |_: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
            let mut cmd = Command::new(std::env::current_exe()?);
            cmd.args(["--exact", "fake_child"]).env("O_LEAD_RECOVERY_WAIT", "1");
            Ok(cmd)
        };
        lead::send(&worker_home, &request(&worker_project), &spawner).unwrap()
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !std::fs::read_to_string(&marker).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).is_some_and(|v| v.get("child").is_some()) {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(lead::new_thread(&home, &project).is_err());
    assert!(lead::stop(&home, Some(&project)));
    worker.join().unwrap();
    assert!(!marker.exists());
    assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
}

#[cfg(windows)]
#[test]
fn lead_reads_complete_results_across_repeated_turns() {
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use o_core::run::events::{CardInfo, CardStatus, OEvent};
    use o_core::run::plan::{Card, Plan};
    use o_core::run::store::{append_event, create_run};
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let end = "LAST-LINE-한글";
    let result = format!("{}\n{end}", "long data\n".repeat(450));
    let plan = Plan { title: "test".into(), project: project.clone(), cards: vec![Card { id: "card".into(), provider: ProviderId::Claude, role: None, task: "test".into(), model: None, effort: None, depends_on: vec![], resources: vec![] }] };
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    for index in 0..12 {
        let id = format!("run-{index}");
        create_run(&home, &id, &plan).unwrap();
        append_event(&home, &id, &OEvent::RunStart { at: "now".into(), run_id: id.clone(), title: "test".into(), project: project.clone(), cards: vec![CardInfo { id: "card".into(), provider: ProviderId::Claude, label: None, role: None, model: None, effort: None }] }).unwrap();
        append_event(&home, &id, &OEvent::CardEnd { at: "now".into(), card_id: "card".into(), status: CardStatus::Failed, result: Some(result.clone()), exit_code: Some(1) }).unwrap();
        append_event(&home, &id, &OEvent::RunEnd { at: "now".into(), run_id: id.clone(), status: CardStatus::Failed }).unwrap();
        let dispatch = serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":id, "clean":false});
        writeln!(std::fs::OpenOptions::new().append(true).open(&log).unwrap(), "{dispatch}").unwrap();
    }
    let observations = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&observations);
    let evidence = home.parent().unwrap().join("child-read.txt");
    let child_evidence = evidence.clone();
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let prompt = spec.stdin.as_ref().unwrap();
        assert!(prompt.contains("생략"));
        assert!(prompt.contains("신뢰할 수 없는"));
        assert!(prompt.contains("failed") && prompt.contains("결과 미리보기"));
        assert!(!prompt.contains(end));
        let tools = spec.args.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(spec.args[tools + 1], "Read,Grep,Glob");
        let index = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let dir = PathBuf::from(&spec.args[index + 1]);
        let files: Vec<_> = std::fs::read_dir(&dir)?.map(|e| e.unwrap().path()).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("run-pages-")).collect();
        let index_file = files.iter().find(|p| std::fs::read_to_string(p).is_ok_and(|s| s.contains("run-summary"))).unwrap();
        let index_text = std::fs::read_to_string(index_file)?;
        let entries: Vec<serde_json::Value> = index_text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        for n in 0..12 {
            let id = format!("run-{n}");
            let row = entries.iter().find(|e| e["runId"] == id && e["type"] == "card.end").unwrap();
            let restored = page_text(row);
            assert_eq!(row["sha256"], o_core::run::result::digest(&restored));
            let source: serde_json::Value = serde_json::from_str(&restored)?;
            assert_eq!(source["result"], result);
        }
        assert!(std::fs::write(index_file, b"tampered").is_err());
        captured.lock().unwrap().push((entries.iter().find(|e| e["runId"] == "run-7" && e["type"] == "card.end").unwrap()["sha256"].clone(), files.len()));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child"]).env("O_LEAD_STATUS_PATH", index_file).env("O_LEAD_STATUS_SEEN", &child_evidence);
        Ok(command)
    };
    lead::send(&home, &request(&project), &spawner).unwrap();
    lead::send(&home, &request(&project), &spawner).unwrap();
    let observed = observations.lock().unwrap();
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0], observed[1]);
    assert_eq!(std::fs::read_to_string(evidence).unwrap().lines().count(), 2);
}

#[cfg(windows)]
#[test]
fn unknown_result_record_is_preserved_and_does_not_block_the_lead() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run_dir = o_core::run::store::run_dir(&home, "broken");
    std::fs::create_dir_all(&run_dir).unwrap();
    let unknown = "{\"t\":\"future.result\",\"cardId\":\"card\",\"result\":\"완전한 원문\"}\n{not-json}\n{\"t\":\"future.partial\",\"result\":\"마지막\"}";
    std::fs::write(run_dir.join("events.jsonl"), unknown).unwrap();
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":"broken", "clean":false})).unwrap();
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        assert!(spec.stdin.as_ref().unwrap().contains("추측"));
        let index = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let dir = PathBuf::from(&spec.args[index + 1]);
        let files: Vec<_> = std::fs::read_dir(dir)?.map(|e| e.unwrap().path()).collect();
        let index_file = files.iter().find(|p| std::fs::read_to_string(p).is_ok_and(|s| s.contains("run-summary"))).unwrap();
        let index = std::fs::read_to_string(index_file)?;
        let unknown_entries: Vec<serde_json::Value> = index.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).filter(|v| v["unknown"] == true).collect();
        assert_eq!(unknown_entries.len(), 2);
        let restored: String = unknown_entries.iter().map(page_text).collect();
        assert_eq!(restored, unknown.replace("{not-json}\n", ""));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_SUCCESS_REPLY", "1");
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Warning { text, .. } if text.contains("실행 상태 확인 불가"))));
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Reply { text, .. } if text.contains("확인했습니다"))));
    assert!(!events.iter().any(|e| matches!(e, lead::LeadEvent::Error { .. })));
}

#[cfg(windows)]
#[test]
fn unavailable_result_record_does_not_block_the_lead() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run = o_core::run::store::run_dir(&home, "busy");
    std::fs::create_dir_all(&run).unwrap();
    let _writer = std::fs::OpenOptions::new().write(true).create_new(true).open(run.join("events.jsonl")).unwrap();
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":"busy", "clean":false})).unwrap();
    let spawner = |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let prompt = spec.stdin.as_ref().unwrap();
        assert!(prompt.contains("원문과 상태를 확인할 수 없음"));
        assert!(prompt.contains("추측하지"));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child"]);
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Warning { text, .. } if text.contains("실행 상태 확인 불가"))));
}

#[cfg(windows)]
#[test]
fn large_result_is_paged_for_the_read_only_lead() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run = o_core::run::store::run_dir(&home, "large");
    std::fs::create_dir_all(&run).unwrap();
    let source = format!("{{\"t\":\"future.result\",\"result\":\"{}\"}}\n", "A".repeat(512 * 257));
    let source_path = run.join("events.jsonl");
    std::fs::write(&source_path, &source).unwrap();
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":"large", "clean":false})).unwrap();
    let expected = source.clone();
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let prompt = spec.stdin.as_ref().unwrap();
        assert!(prompt.contains("json-string-lines"));
        assert!(prompt.contains("추측하지"));
        let i = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let paths = std::fs::read_dir(&spec.args[i + 1])?.map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
        let index = paths.iter().find_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).find(|v| v["format"] == "json-string-lines")
        }).unwrap();
        let pages = std::fs::read_to_string(index["file"].as_str().unwrap())?;
        let start = index["pageStart"].as_u64().unwrap() as usize - 1;
        let count = index["pageCount"].as_u64().unwrap() as usize;
        let reconstructed: String = pages.lines().skip(start).take(count).map(|line| serde_json::from_str::<String>(line).unwrap()).collect();
        assert_eq!(reconstructed, expected);
        assert_eq!(index["sha256"], o_core::run::result::digest(&expected));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_SUCCESS_REPLY", "1");
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Warning { text, .. } if text.contains("상태 확인 불가"))));
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Reply { text, .. } if text.contains("확인했습니다"))));
    assert!(!events.iter().any(|e| matches!(e, lead::LeadEvent::Error { .. })));
    assert_eq!(std::fs::read_to_string(source_path).unwrap(), source);
}

#[cfg(windows)]
#[test]
fn very_large_card_result_streams_without_tool_or_team_events() {
    use std::io::{BufRead, BufReader, Write};
    use sha2::{Digest, Sha256};
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run = o_core::run::store::run_dir(&home, "huge");
    std::fs::create_dir_all(&run).unwrap();
    let source = run.join("events.jsonl");
    let mut log = std::fs::File::create(&source).unwrap();
    writeln!(log, "{}", serde_json::json!({"t":"msg","at":"now","from":"a","to":"b","text":"PRIVATE-TEAM"})).unwrap();
    writeln!(log, "{}", serde_json::json!({"t":"msg","text":"PRIVATE-BROKEN"})).unwrap();
    for _ in 0..4200 { writeln!(log, "{}", serde_json::json!({"t":"card.tool","at":"now","cardId":"card","name":"tool","summary":"PRIVATE-TOOL"})).unwrap(); }
    let prefix = b"{\"t\":\"card.end\",\"at\":\"now\",\"cardId\":\"card\",\"status\":\"done\",\"result\":\"";
    let block = "한글abc\\n".repeat(1024);
    let suffix = b"\",\"exitCode\":0}\n";
    let mut digest = Sha256::new();
    digest.update(prefix);
    log.write_all(prefix).unwrap();
    for _ in 0..700 { digest.update(block.as_bytes()); log.write_all(block.as_bytes()).unwrap(); }
    digest.update(suffix);
    log.write_all(suffix).unwrap();
    writeln!(log, "{}", serde_json::json!({"t":"run.end","at":"now","runId":"huge","status":"done"})).unwrap();
    drop(log);
    let expected = format!("{:x}", digest.finalize());
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"huge","clean":false})).unwrap();
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        assert!(spec.stdin.as_ref().unwrap().contains("json-string-lines"), "{}", spec.stdin.as_ref().unwrap());
        let at = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let dir = PathBuf::from(&spec.args[at + 1]);
        let paths: Vec<_> = std::fs::read_dir(&dir)?.map(|e| e.unwrap().path()).collect();
        let (index_path, index) = paths.iter().find_map(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            let rows: Vec<serde_json::Value> = text.lines().filter_map(|s| serde_json::from_str(s).ok()).collect();
            rows.iter().any(|v| v["type"] == "card.end").then(|| (p.clone(), rows))
        }).unwrap();
        let row = index.iter().find(|v| v["type"] == "card.end").unwrap();
        assert_eq!(row["sha256"], expected);
        let page_path = row["file"].as_str().unwrap();
        assert!(std::fs::write(page_path, b"altered").is_err());
        let mut hash = Sha256::new();
        let start = row["pageStart"].as_u64().unwrap() as usize - 1;
        let count = row["pageCount"].as_u64().unwrap() as usize;
        let mut actual_count = 0;
        for (i, line) in BufReader::new(std::fs::File::open(page_path)?).lines().enumerate() {
            let line = line?;
            let text: String = serde_json::from_str(&line)?;
            assert!(!text.contains("PRIVATE-"));
            if (start..start + count).contains(&i) { hash.update(text.as_bytes()); actual_count += 1; }
        }
        assert_eq!(actual_count, count);
        assert_eq!(format!("{:x}", hash.finalize()), expected);
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_SUCCESS_REPLY", "1").env("O_STREAM_INDEX", index_path).env("O_STREAM_SHA", &expected);
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Reply { .. })));
    assert!(!events.iter().any(|e| matches!(e, lead::LeadEvent::Error { .. })));
    assert_eq!(std::fs::read_dir(lead::files::thread_files(&home, &thread)).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn both_sides_of_the_previous_four_mib_boundary_stream() {
    use std::io::{BufRead, BufReader, Write};
    use sha2::{Digest, Sha256};
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let thread_log = lead::thread_dir(&home, &thread).join("events.jsonl");
    let mut expected = Vec::new();
    for (id, size) in [("under", 4 * 1024 * 1024 - 1), ("over", 4 * 1024 * 1024 + 1)] {
        let run = o_core::run::store::run_dir(&home, id);
        std::fs::create_dir_all(&run).unwrap();
        let path = run.join("events.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        let prefix = b"{\"t\":\"card.end\",\"at\":\"now\",\"cardId\":\"card\",\"status\":\"done\",\"result\":\"";
        let suffix = b"\",\"exitCode\":0}\n";
        let mut hash = Sha256::new();
        file.write_all(prefix).unwrap();
        hash.update(prefix);
        let mut left = size - prefix.len() - suffix.len();
        let chunk = [b'a'; 8192];
        while left > 0 { let n = left.min(chunk.len()); file.write_all(&chunk[..n]).unwrap(); hash.update(&chunk[..n]); left -= n; }
        file.write_all(suffix).unwrap();
        hash.update(suffix);
        drop(file);
        assert_eq!(std::fs::metadata(path).unwrap().len(), size as u64);
        expected.push((id.to_owned(), format!("{:x}", hash.finalize())));
        writeln!(std::fs::OpenOptions::new().append(true).open(&thread_log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch","at":"now","planId":"p","runId":id,"clean":false})).unwrap();
    }
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let at = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let paths = std::fs::read_dir(&spec.args[at + 1])?.map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
        let (index_path, rows) = paths.iter().find_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            let rows: Vec<serde_json::Value> = text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
            rows.iter().any(|row| row["type"] == "card.end").then(|| (path.clone(), rows))
        }).unwrap();
        for (id, digest) in &expected {
            let row = rows.iter().find(|row| row["runId"].as_str() == Some(id.as_str()) && row["type"] == "card.end").unwrap();
            assert_eq!(row["sha256"], *digest);
            let first = row["pageStart"].as_u64().unwrap() as usize - 1;
            let count = row["pageCount"].as_u64().unwrap() as usize;
            let mut hash = Sha256::new();
            for line in BufReader::new(std::fs::File::open(row["file"].as_str().unwrap())?).lines().skip(first).take(count) {
                hash.update(serde_json::from_str::<String>(&line?)?.as_bytes());
            }
            assert_eq!(format!("{:x}", hash.finalize()), *digest);
        }
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_SUCCESS_REPLY", "1").env("O_STREAM_INDEX", index_path).env("O_STREAM_SHA", &expected[0].1);
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|event| matches!(event, lead::LeadEvent::Reply { .. })));
    assert!(!events.iter().any(|event| matches!(event, lead::LeadEvent::Error { .. })));
}

#[cfg(windows)]
#[test]
fn truncated_large_record_is_unavailable_and_partial_pages_are_removed() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run = o_core::run::store::run_dir(&home, "truncated");
    std::fs::create_dir_all(&run).unwrap();
    let mut record = std::fs::File::create(run.join("events.jsonl")).unwrap();
    writeln!(record, "{}", serde_json::json!({"t":"run.end","at":"now","runId":"truncated","status":"failed"})).unwrap();
    record.write_all(b"{\"t\":\"card.end\",\"at\":\"now\",\"cardId\":\"card\",\"status\":\"done\",\"result\":\"").unwrap();
    for _ in 0..1100 { record.write_all(&[b'a'; 4096]).unwrap(); }
    drop(record);
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(&log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"truncated","clean":false})).unwrap();
    let good = o_core::run::store::run_dir(&home, "good");
    std::fs::create_dir_all(&good).unwrap();
    let event = format!("{}\n", serde_json::json!({"t":"card.end","at":"now","cardId":"card","status":"done","result":"보존","exitCode":0}));
    std::fs::write(good.join("events.jsonl"), &event).unwrap();
    writeln!(std::fs::OpenOptions::new().append(true).open(&log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"good","clean":false})).unwrap();
    let hash = o_core::run::result::digest(&event);
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        assert!(spec.stdin.as_ref().unwrap().contains("잘렸"));
        let at = spec.args.iter().position(|a| a == "--add-dir").unwrap();
        let index = std::fs::read_dir(&spec.args[at + 1])?.filter_map(|entry| entry.ok()).map(|entry| entry.path()).find(|path| std::fs::read_to_string(path).is_ok_and(|text| text.contains("unavailable") && text.contains("card.end"))).unwrap();
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_SUCCESS_REPLY", "1").env("O_STREAM_INDEX", index).env("O_STREAM_SHA", &hash);
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Warning { text, .. } if text.contains("잘렸"))));
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Reply { .. })));
    assert_eq!(std::fs::read_dir(lead::files::thread_files(&home, &thread)).unwrap().count(), 0);
}

#[cfg(windows)]
#[test]
fn many_short_events_stream_before_the_preview_is_loaded() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run = o_core::run::store::run_dir(&home, "many");
    std::fs::create_dir_all(&run).unwrap();
    let mut log = std::fs::File::create(run.join("events.jsonl")).unwrap();
    for _ in 0..4100 { writeln!(log, "{}", serde_json::json!({"t":"card.tool","at":"now","cardId":"card","name":"tool","summary":"PRIVATE"})).unwrap(); }
    let event = format!("{}\n", serde_json::json!({"t":"card.end","at":"now","cardId":"card","status":"done","result":"끝","exitCode":0}));
    log.write_all(event.as_bytes()).unwrap();
    drop(log);
    let thread_log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(thread_log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch","at":"now","planId":"p","runId":"many","clean":false})).unwrap();
    let hash = o_core::run::result::digest(&event);
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        let at = spec.args.iter().position(|arg| arg == "--add-dir").unwrap();
        let index = std::fs::read_dir(&spec.args[at + 1])?.filter_map(|entry| entry.ok()).map(|entry| entry.path()).find(|path| std::fs::read_to_string(path).is_ok_and(|text| text.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).any(|row| row["type"] == "card.end"))).unwrap();
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child", "--nocapture"]).env("O_LEAD_SUCCESS_REPLY", "1").env("O_STREAM_INDEX", index).env("O_STREAM_SHA", &hash);
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|event| matches!(event, lead::LeadEvent::Reply { .. })));
    assert!(!events.iter().any(|event| matches!(event, lead::LeadEvent::Error { .. })));
}

#[cfg(windows)]
#[test]
fn unsafe_output_folder_is_reported_without_blocking_the_lead() {
    use std::io::Write;
    let (home, project) = fixture();
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let run = o_core::run::store::run_dir(&home, "one");
    std::fs::create_dir_all(&run).unwrap();
    std::fs::write(run.join("events.jsonl"), "{\"t\":\"run.end\",\"at\":\"now\",\"runId\":\"one\",\"status\":\"failed\"}\n").unwrap();
    let log = lead::thread_dir(&home, &thread).join("events.jsonl");
    writeln!(std::fs::OpenOptions::new().append(true).open(log).unwrap(), "{}", serde_json::json!({"t":"lead.dispatch", "at":"now", "planId":"p", "runId":"one", "clean":false})).unwrap();
    let target = home.parent().unwrap().join("untrusted");
    std::fs::create_dir(&target).unwrap();
    let root = lead::files::files_root(&home);
    let mut junction = Command::new("cmd");
    junction.args(["/C", "mklink", "/J"]).arg(root).arg(&target).stdout(Stdio::null()).stderr(Stdio::null());
    assert!(o_core::sys::proc::hide_window(&mut junction).status().unwrap().success());
    let spawner = |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<Command> {
        assert!(spec.stdin.as_ref().unwrap().contains("원문을 안전하게 제공할 수 없습니다"));
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["--exact", "fake_child"]);
        Ok(command)
    };
    let events = lead::send(&home, &request(&project), &spawner).unwrap();
    assert!(events.iter().any(|e| matches!(e, lead::LeadEvent::Warning { text, .. } if text.contains("안전하게 제공할 수 없습니다"))));
    assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
}
