//! Runner behaviour with a fake provider CLI. The fake is this test binary
//! itself: started with O_FAKE_CLI_LINES it prints those lines and exits, so
//! the tests need no node, shell or provider CLI.

mod support;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{Duration as Span, Utc};
use o_core::run::events::{CardStatus, OEvent};
use o_core::sys::js::{iso, parse_time};
use o_core::run::plan::{Card, Plan, ProviderId};
use o_core::policy::LIMITS;
use o_core::providers::LaunchSpec;
use o_core::run::runner::{run_detached, run_plan, run_plan_in, Spawner, Workdir};
use o_core::run::store::{
    active_markers, clear_active, cooldown_times, create_run, mark_active, read_events, record_start, request_stop, set_cooldown, started_on,
    ActiveMarker,
};

const FAKE: &str = "O_FAKE_CLI_LINES";
/// Milliseconds the fake waits before printing: a CLI that is still working.
const FAKE_SLEEP: &str = "O_FAKE_CLI_SLEEP_MS";
/// `--dump-env <file>`: the fake writes its environment there as JSON and succeeds.
const DUMP_ENV: &str = "--dump-env";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--fake-raw") {
        std::io::stdout().write_all(b"\xff\r\n{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}\r\n").unwrap();
        return;
    }
    if args.get(1).map(String::as_str) == Some("--fake-devin") {
        std::fs::write(&args[2], r#"{"session_id":"fake-session","steps":[{"source":"agent","tool_calls":[{"function_name":"read","arguments":{"path":"demo.txt"}}]}]}"#).unwrap();
        println!("작업 완료");
        return;
    }
    if args.get(1).map(String::as_str) == Some(DUMP_ENV) {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        std::fs::write(&args[2], serde_json::to_string(&env).unwrap()).unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"ok"}}"#);
        return;
    }
    if let Ok(lines) = std::env::var(FAKE) {
        if let Some(ms) = std::env::var(FAKE_SLEEP).ok().and_then(|ms| ms.parse().ok()) {
            std::thread::sleep(Duration::from_millis(ms));
        }
        let lines: Vec<String> = serde_json::from_str(&lines).expect("fake lines");
        for line in lines {
            println!("{line}");
        }
        return;
    }
    let tests: [(&str, fn()); 32] = [
        ("non-Git handoffs deliver changes and refuse unsafe copies", non_git_handoffs),
        ("unsafe workspace handoffs never start the dependent", unsafe_handoffs),
        ("a failed log stops the worker and its dependent", failed_log),
        ("raw output is preserved byte for byte", raw_output_preserved),
        ("dependency results remain readable when they are large", dependency_result_limit),
        ("queued follow-ups stop safely and never run in parallel for one session", followup_queue_and_stop),
        ("stopping a card cancels the follow-up queued behind it", followup_cancelled_with_source),
        ("follow-up resumes the saved session in the original isolated workspace", followup_resumes_workspace),
        ("non-Git projects run in independent copies without changing the source", isolated_copies),
        ("lead graph keeps each worker in its own worktree and refuses a dirty base", isolated_worktrees),
        ("a repository with a submodule entry is refused before any worker starts", isolated_worktrees_refuse_submodules),
        ("dependencies wait for success and never start after failure", dependency_order),
        ("devin transcript and answer become card events", devin_transcript),
        ("a rejected plan records its reason before the run ends", rejected_plan_records_reason),
        ("budget off keeps effort and refuses nothing", budget_off_keeps_effort_and_refuses_nothing),
        ("cards start in the folder the policy checked, not the link the plan names", cards_run_in_the_checked_folder),
        ("a project link pointed elsewhere after the check refuses the card", a_link_swapped_after_the_check_is_refused),
        ("a stop that comes while a card's CLI is starting still ends it", stop_while_starting),
        ("runs dispatched together never take more slots than the caps between them", runners_share_slots),
        ("runs dispatched together never start more cards than the day allows", runners_share_the_days_last_start),
        ("a card whose CLI cannot start gives back its slot and its start", failed_start_gives_back),
        ("behind today's budget, cards start with lower effort and say so", budget_lowers_effort),
        ("a provider over today's budget starts no card", budget_over_refuses),
        ("runs cards in parallel, records a limit as a provider cooldown", parallel_and_limit),
        ("a card whose provider is cooling down ends without launching", cooling_down),
        ("the day's cap is checked again when a card starts, not only at dispatch", daily_cap_at_start),
        ("a stop request ends the cards still waiting", stop_request),
        ("a card waits in the queue while its provider is full, then runs", queue_then_run),
        ("a clean run starts its cards with only the clean environment", clean_run),
        ("cards of one provider in one plan never take more than its slots", same_provider_slots_in_one_plan),
        ("a card's CLI never inherits the API key variables it prefers over its sign-in", api_keys_dropped),
        ("an OpenCode card without YOLO does not start under OpenCode project config", opencode_project_config_refused),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("\nrunner: {} passed; {failed} failed", tests.len() - failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

fn followup_queue_and_stop() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut initial = plan(vec![card("a", ProviderId::Claude, "first")]);
    initial.project = project.to_string_lossy().into_owned();
    create_run(&home, "original", &initial).unwrap();
    let first = o_core::run::followup::start(&home, "original", "a", "수정 1", &|_| Ok(())).unwrap();
    let second = o_core::run::followup::start(&home, "original", "a", "수정 2", &|_| Ok(())).unwrap();
    let worker_home = home.clone();
    let worker_id = first.clone();
    let worker = std::thread::spawn(move || run_detached(&worker_home, &worker_id));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !read_events(&home, &first).iter().any(|e| matches!(e, OEvent::CardQueued { .. })) {
        assert!(std::time::Instant::now() < deadline, "후속 대기 상태가 나타나지 않았습니다");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(run_detached(&home, &second).is_err());
    assert!(read_events(&home, &second).iter().any(|e| matches!(e, OEvent::CardEnd { status: CardStatus::Failed, result: Some(text), .. } if text.contains("이미"))));
    request_stop(&home, &first).unwrap();
    assert!(worker.join().unwrap().is_err());
    assert!(read_events(&home, &first).iter().any(|e| matches!(e, OEvent::CardEnd { status: CardStatus::Stopped, .. })));
    assert!(active_markers(&home).is_empty());
}

fn followup_cancelled_with_source() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut initial = plan(vec![card("a", ProviderId::Claude, "first")]);
    initial.project = project.to_string_lossy().into_owned();
    create_run(&home, "original", &initial).unwrap();
    let queued = o_core::run::followup::start(&home, "original", "a", "수정", &|_| Ok(())).unwrap();
    let worker_home = home.clone();
    let worker_id = queued.clone();
    let worker = std::thread::spawn(move || run_detached(&worker_home, &worker_id));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !read_events(&home, &queued).iter().any(|e| matches!(e, OEvent::CardQueued { .. })) {
        assert!(std::time::Instant::now() < deadline, "후속 대기 상태가 나타나지 않았습니다");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(o_core::run::runner::stop_run(&home, "original").unwrap());
    assert!(worker.join().unwrap().is_err());
    let events = read_events(&home, &queued);
    assert!(events.iter().any(|e| matches!(e, OEvent::CardEnd { status: CardStatus::Stopped, result: Some(text), .. } if text.contains("취소"))), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, OEvent::RunEnd { status: CardStatus::Stopped, .. })));
    assert!(active_markers(&home).is_empty());
}

fn followup_resumes_workspace() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("file.txt"), "before").unwrap();
    let mut initial = plan(vec![card("a", ProviderId::Claude, "first")]);
    initial.project = project.to_string_lossy().into_owned();
    create_run(&home, "original", &initial).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "original").join("isolated-worktrees"), "").unwrap();
    let original: Arc<Spawner> = Arc::new(|spec, cwd| {
        std::fs::write(PathBuf::from(cwd).join("file.txt"), "changed").unwrap();
        fake_cli(|_| vec![r#"{"type":"system","subtype":"init","session_id":"s1"}"#, r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#])(spec, cwd)
    });
    assert_eq!(run_plan(&home, "original", &initial, None, original).unwrap(), CardStatus::Done);
    mark_active(&home, &ActiveMarker { run_id: "original".into(), card_id: "a".into(), provider: ProviderId::Claude, pid: std::process::id() }).unwrap();
    assert!(o_core::run::followup::target(&home, "original", "a").is_err());
    clear_active(&home, "original", "a");
    let id = o_core::run::followup::start(&home, "original", "a", "다시 확인해 주세요", &|_| Ok(())).unwrap();
    let follow: Plan = serde_json::from_value(o_core::run::store::read_plan(&home, &id).unwrap()).unwrap();
    let resume: Arc<Spawner> = Arc::new(|spec, cwd| {
        // The resumed cwd is the plain path the first run used, never the extended `\\?\` form.
        #[cfg(windows)]
        assert!(!cwd.starts_with(r"\\?\"), "{cwd}");
        assert_eq!(std::fs::read_to_string(PathBuf::from(cwd).join("file.txt")).unwrap(), "changed");
        assert!(spec.args.windows(2).any(|pair| pair == ["--resume", "s1"]));
        assert!(spec.stdin.as_ref().unwrap().contains("다시 확인해 주세요"));
        quiet()(spec, cwd)
    });
    let result = run_plan(&home, &id, &follow, None, resume).unwrap();
    assert_eq!(result, CardStatus::Done, "{:?}", read_events(&home, &id));
    assert_eq!(std::fs::read_to_string(project.join("file.txt")).unwrap(), "before");
    assert!(o_core::run::followup::start(&home, "original", "a", "다시", &|_| Err(std::io::Error::other("no runner"))).is_err());
    let failed = std::fs::read_dir(home.join("runs")).unwrap().flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).find(|run| run != "original" && run != &id).unwrap();
    assert!(!o_core::run::store::run_is_live(&home, &failed));
    assert!(read_events(&home, &failed).iter().any(|event| matches!(event, OEvent::RunEnd { status: CardStatus::Failed, .. })));
    let retry = o_core::run::followup::target(&home, &failed, "a").unwrap();
    assert_eq!(retry.session, "s1");
    #[cfg(windows)]
    assert!(!retry.workspace.to_string_lossy().starts_with(r"\\?\"), "{}", retry.workspace.display());
    assert_eq!(std::fs::read_to_string(retry.workspace.join("file.txt")).unwrap(), "changed");
    let interrupted = o_core::run::followup::start(&home, "original", "a", "중단 테스트", &|_| Ok(())).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, &interrupted).join("runner.json"), r#"{"pid":2147483647}"#).unwrap();
    let runs = o_core::run::store::read_runs(&home, &o_core::run::index::RunQuery { limit: 50, project: None }).unwrap();
    let record = runs.iter().find(|run| run.run_id == interrupted).unwrap();
    assert!(record.events.iter().any(|event| event["t"] == "card.end" && event["status"] == "failed"));
    assert!(record.events.iter().any(|event| event["t"] == "run.end" && event["status"] == "failed"));
    assert!(o_core::run::followup::target(&home, &interrupted, "a").is_err());
}

fn isolated_copies() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src").join("game.py"), "original\n").unwrap();
    let mut p = plan(vec![card("a", ProviderId::Claude, "first"), card("b", ProviderId::Claude, "second")]);
    p.project = project.to_string_lossy().into_owned();
    create_run(&home, "copies", &p).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "copies").join("isolated-worktrees"), "").unwrap();
    let paths = Arc::new(Mutex::new(BTreeMap::<String, String>::new()));
    let observed = Arc::clone(&paths);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
        let id = spec.env["O_CARD_ID"].clone();
        let cwd = PathBuf::from(cwd);
        assert_eq!(std::fs::read_to_string(cwd.join("src/game.py")).unwrap(), "original\n");
        assert!(cwd.join(".git").is_dir());
        std::fs::write(cwd.join("src/game.py"), &id).unwrap();
        observed.lock().unwrap().insert(id, cwd.to_string_lossy().into_owned());
        inner(spec, &cwd.to_string_lossy())
    });
    assert_eq!(run_plan(&home, "copies", &p, None, spawner).unwrap(), CardStatus::Done);
    let paths = paths.lock().unwrap();
    assert_ne!(paths["a"], paths["b"]);
    assert_eq!(std::fs::read_to_string(project.join("src/game.py")).unwrap(), "original\n");
    assert!(!project.join(".git").exists());
    assert_eq!(std::fs::read_to_string(PathBuf::from(&paths["a"]).join("src/game.py")).unwrap(), "a");
    drop(paths);

    std::fs::create_dir(project.join("src/.git")).unwrap();
    create_run(&home, "nested", &p).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "nested").join("isolated-worktrees"), "").unwrap();
    let never: Arc<Spawner> = Arc::new(|_, _| panic!("nested repository must not start"));
    assert_eq!(run_plan(&home, "nested", &p, None, never).unwrap(), CardStatus::Failed);
    assert!(read_events(&home, "nested").iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(reason), .. } if reason.contains("중첩 Git"))));
}

fn isolated_worktrees() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command.current_dir(&project).args(args);
        let out = o_core::sys::proc::hide_window(&mut command).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    git(&["init"]);
    std::fs::write(project.join(".gitattributes"), "* -text\n").unwrap();
    std::fs::write(project.join("shared.txt"), "base\n").unwrap();
    std::fs::write(project.join("remove.txt"), "remove me\n").unwrap();
    std::fs::write(project.join("binary.dat"), [0, 1, 2]).unwrap();
    git(&["add", "."]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture"]);
    let mut a = card("a", ProviderId::Claude, "first");
    let b = card("b", ProviderId::Claude, "second");
    a.depends_on = vec!["b".into()];
    let mut p = plan(vec![a, b]);
    p.project = project.to_string_lossy().into_owned();
    create_run(&home, "isolated", &p).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "isolated").join("isolated-worktrees"), "").unwrap();
    let paths = Arc::new(Mutex::new(BTreeMap::<String, String>::new()));
    let observed = Arc::clone(&paths);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
        let id = spec.env["O_CARD_ID"].clone();
        if id == "a" {
            let previous = observed.lock().unwrap()["b"].clone();
            assert_eq!(std::fs::read_to_string(PathBuf::from(&previous).join("shared.txt")).unwrap(), "b\n");
            assert_eq!(std::fs::read_to_string(PathBuf::from(cwd).join("shared.txt")).unwrap(), "b\n");
            assert!(!PathBuf::from(cwd).join("remove.txt").exists());
            assert_eq!(std::fs::read(PathBuf::from(cwd).join("binary.dat")).unwrap(), [0, 255, 3]);
            assert_eq!(std::fs::read(PathBuf::from(cwd).join("new.bin")).unwrap(), [0, 254, 4]);
        }
        std::fs::write(PathBuf::from(cwd).join("shared.txt"), format!("{id}\n")).unwrap();
        if id == "b" {
            let run_git = |args: &[&str]| {
                let mut command = Command::new("git");
                command.current_dir(cwd).args(args);
                let result = o_core::sys::proc::hide_window(&mut command).output().unwrap();
                assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
            };
            run_git(&["add", "shared.txt"]);
            run_git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "worker"]);
            std::fs::remove_file(PathBuf::from(cwd).join("remove.txt")).unwrap();
            std::fs::write(PathBuf::from(cwd).join("binary.dat"), [0, 255, 3]).unwrap();
            std::fs::write(PathBuf::from(cwd).join("new.bin"), [0, 254, 4]).unwrap();
        }
        observed.lock().unwrap().insert(id, cwd.into());
        inner(spec, cwd)
    });
    assert_eq!(run_plan(&home, "isolated", &p, None, spawner).unwrap(), CardStatus::Done);
    let paths = paths.lock().unwrap();
    assert_ne!(paths["a"], paths["b"]);
    assert_eq!(std::fs::read_to_string(project.join("shared.txt")).unwrap(), "base\n");
    assert_eq!(std::fs::read_to_string(PathBuf::from(&paths["a"]).join("shared.txt")).unwrap(), "a\n");
    drop(paths);

    std::fs::write(project.join("shared.txt"), "user change\n").unwrap();
    create_run(&home, "dirty", &p).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "dirty").join("isolated-worktrees"), "").unwrap();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let never: Arc<Spawner> = Arc::new(move |_, _| { seen.store(true, Ordering::SeqCst); Err(std::io::Error::other("must not start")) });
    assert_eq!(run_plan(&home, "dirty", &p, None, never).unwrap(), CardStatus::Failed);
    assert!(!launched.load(Ordering::SeqCst));
    assert_eq!(std::fs::read_to_string(project.join("shared.txt")).unwrap(), "user change\n");
    assert!(read_events(&home, "dirty").iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(reason), .. } if reason.contains("커밋하지 않은"))));
}

fn unsafe_handoffs() {
    for scenario in ["conflict", "target-change", "base-change", "handoff-file", "ignored", "outside", "staged-overlap", "eol-only", "eol-changed", "non-git-unchanged"] {
        let fixture = temp_home();
        let home = fixture.join("state");
        let root = fixture.join("project");
        let project = if scenario.starts_with("non-git") { root.clone() } else { root.join("sub") };
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("shared.txt"), "base\n").unwrap();
        if !scenario.starts_with("non-git") {
            std::fs::write(project.join(".gitignore"), "ignored.dat\n").unwrap();
            let git = |args: &[&str]| {
                let mut command = Command::new("git");
                command.current_dir(&root).args(args);
                let output = o_core::sys::proc::hide_window(&mut command).output().unwrap();
                assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            };
            git(&["init"]);
            if scenario.starts_with("eol-") { git(&["config", "core.autocrlf", "true"]); }
            git(&["add", "."]);
            git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "base"]);
        }
        let first = card("a", ProviderId::Claude, "first");
        let mut dependent = card("c", ProviderId::Claude, "dependent");
        dependent.depends_on = if scenario == "conflict" { vec!["a".into(), "b".into()] } else { vec!["a".into()] };
        let cards = if scenario == "conflict" { vec![first, card("b", ProviderId::Claude, "second"), dependent] } else { vec![first, dependent] };
        let mut p = plan(cards);
        p.project = project.to_string_lossy().into_owned();
        create_run(&home, "unsafe", &p).unwrap();
        std::fs::write(o_core::run::store::run_dir(&home, "unsafe").join("isolated-worktrees"), "").unwrap();
        let launched = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&launched);
        let original = project.clone();
        let handoff = o_core::run::store::run_dir(&home, "unsafe").join("handoff");
        let inner = quiet();
        let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
            let id = &spec.env["O_CARD_ID"];
            if id == "c" { observed.store(true, Ordering::SeqCst); }
            let cwd = PathBuf::from(cwd);
            if scenario == "conflict" { std::fs::write(cwd.join("shared.txt"), format!("{id}\n")).unwrap(); }
            if scenario == "non-git" { std::fs::write(cwd.join("shared.txt"), "changed\n").unwrap(); }
            if scenario == "eol-only" { std::fs::write(cwd.join("shared.txt"), "base\n").unwrap(); }
            if scenario == "eol-changed" { std::fs::write(cwd.join("shared.txt"), "changed\n").unwrap(); }
            if scenario == "staged-overlap" {
                std::fs::write(cwd.join("shared.txt"), "staged\n").unwrap();
                let mut command = Command::new("git");
                command.current_dir(&cwd).args(["add", "shared.txt"]);
                assert!(o_core::sys::proc::hide_window(&mut command).status().unwrap().success());
                std::fs::write(cwd.join("shared.txt"), "working\n").unwrap();
            }
            if scenario == "ignored" { std::fs::write(cwd.join("ignored.dat"), "ignored").unwrap(); }
            if scenario == "outside" { std::fs::write(cwd.parent().unwrap().join("outside.txt"), "outside").unwrap(); }
            if scenario == "base-change" { std::fs::write(original.join("user.txt"), "human").unwrap(); }
            if scenario == "handoff-file" { std::fs::write(&handoff, "human").unwrap(); }
            if scenario == "target-change" {
                let target = cwd.parent().unwrap().parent().unwrap().join("2").join("sub");
                std::fs::write(target.join("shared.txt"), "human\n").unwrap();
            }
            inner(spec, &cwd.to_string_lossy())
        });
        let unchanged = scenario == "non-git-unchanged";
        assert_eq!(run_plan(&home, "unsafe", &p, None, spawner).unwrap(), if unchanged { CardStatus::Done } else { CardStatus::Failed }, "{scenario}");
        assert_eq!(launched.load(Ordering::SeqCst), unchanged, "{scenario}");
        if !unchanged { assert!(read_events(&home, "unsafe").iter().any(|event| matches!(event, OEvent::CardEnd { card_id, status: CardStatus::Failed, result: Some(reason), .. } if card_id == "c" && reason.contains("인계 실패"))), "{scenario}"); }
        if scenario == "base-change" { assert_eq!(std::fs::read_to_string(project.join("user.txt")).unwrap(), "human"); }
        if scenario == "handoff-file" { assert_eq!(std::fs::read_to_string(o_core::run::store::run_dir(&home, "unsafe").join("handoff")).unwrap(), "human"); }
        if scenario == "target-change" {
            let target = home.with_file_name("state-worktrees").join("unsafe/2/sub/shared.txt");
            assert_eq!(std::fs::read_to_string(target).unwrap(), "human\n");
        }
    }
}

fn non_git_handoffs() {
    for scenario in ["changes", "independent", "conflict", "target", "target-git", "original", "base", "link", "staged"] {
        let fixture = temp_home();
        let home = fixture.join("state");
        let project = fixture.join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("edit"), b"base").unwrap();
        std::fs::write(project.join("delete"), b"base").unwrap();
        let first = card("a", ProviderId::Claude, "first");
        let mut last = card("c", ProviderId::Claude, "last");
        last.depends_on = if matches!(scenario, "conflict" | "independent") { vec!["a".into(), "b".into()] } else { vec!["a".into()] };
        let mut p = plan(if matches!(scenario, "conflict" | "independent") { vec![first, card("b", ProviderId::Claude, "second"), last] } else { vec![first, last] });
        p.project = project.to_string_lossy().into_owned();
        create_run(&home, "copy-handoff", &p).unwrap();
        std::fs::write(o_core::run::store::run_dir(&home, "copy-handoff").join("isolated-worktrees"), "").unwrap();
        let run = home.with_file_name("state-worktrees").join("copy-handoff");
        let launched = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&launched);
        let original = project.clone();
        let copies = run.clone();
        let inner = quiet();
        let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
            let id = &spec.env["O_CARD_ID"];
            let cwd = PathBuf::from(cwd);
            if id == "c" {
                seen.store(true, Ordering::SeqCst);
                assert_eq!(std::fs::read(cwd.join("edit")).unwrap(), b"changed");
                if scenario == "changes" {
                    assert!(!cwd.join("delete").exists());
                    assert_eq!(std::fs::read(cwd.join("new/file")).unwrap(), [0, 255]);
                } else { assert_eq!(std::fs::read(cwd.join("other")).unwrap(), b"second"); }
            } else {
                if scenario == "independent" && id == "b" {
                    std::fs::write(cwd.join("other"), "second").unwrap();
                } else { std::fs::write(cwd.join("edit"), if id == "a" { "changed" } else { "conflict" }).unwrap(); }
                if scenario == "changes" {
                    std::fs::remove_file(cwd.join("delete")).unwrap();
                    std::fs::create_dir(cwd.join("new")).unwrap();
                    std::fs::write(cwd.join("new/file"), [0, 255]).unwrap();
                }
                if scenario == "target" { std::fs::write(copies.join("2/edit"), "human").unwrap(); }
                if scenario == "target-git" { std::fs::write(copies.join("2/.git/human"), "human").unwrap(); }
                if scenario == "original" { std::fs::write(original.join("edit"), "human").unwrap(); }
                if scenario == "base" { std::fs::write(copies.join("base/edit"), "tampered").unwrap(); }
                if scenario == "staged" {
                    let mut command = Command::new("git");
                    command.current_dir(&cwd).args(["add", "edit"]);
                    assert!(o_core::sys::proc::hide_window(&mut command).status().unwrap().success());
                    std::fs::write(cwd.join("edit"), "unstaged").unwrap();
                }
                #[cfg(unix)]
                if scenario == "link" { std::os::unix::fs::symlink(original.join("edit"), cwd.join("link")).unwrap(); }
                #[cfg(windows)]
                if scenario == "link" && std::os::windows::fs::symlink_file(original.join("edit"), cwd.join("link")).is_err() {
                    std::fs::create_dir(cwd.join("nested")).unwrap();
                    std::fs::create_dir(cwd.join("nested/.git")).unwrap();
                }
            }
            inner(spec, &cwd.to_string_lossy())
        });
        let succeeds = matches!(scenario, "changes" | "independent");
        assert_eq!(run_plan(&home, "copy-handoff", &p, None, spawner).unwrap(), if succeeds { CardStatus::Done } else { CardStatus::Failed }, "{scenario}");
        assert_eq!(launched.load(Ordering::SeqCst), succeeds, "{scenario}");
        if scenario == "changes" {
            assert_eq!(std::fs::read(run.join("handoff-2/edit")).unwrap(), b"base");
            assert_eq!(std::fs::read(run.join("handoff-2/delete")).unwrap(), b"base");
        }
        if scenario == "target" { assert_eq!(std::fs::read(run.join("2/edit")).unwrap(), b"human"); }
        if scenario == "target-git" { assert_eq!(std::fs::read(run.join("2/.git/human")).unwrap(), b"human"); }
        if scenario == "original" { assert_eq!(std::fs::read(project.join("edit")).unwrap(), b"human"); }
    }
}

fn isolated_worktrees_refuse_submodules() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(project.join("vendor")).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new("git");
        command.current_dir(&project).args(args);
        let out = o_core::sys::proc::hide_window(&mut command).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init"]);
    std::fs::write(project.join("shared.txt"), "base
").unwrap();
    git(&["add", "shared.txt"]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture"]);
    // A gitlink (mode 160000) is what a submodule leaves in the index; the empty folder is its uninitialized checkout.
    let commit = git(&["rev-parse", "HEAD"]);
    git(&["update-index", "--add", "--cacheinfo", &format!("160000,{commit},vendor/lib")]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "submodule"]);
    std::fs::create_dir_all(project.join("vendor/lib")).unwrap();
    assert_eq!(git(&["status", "--porcelain=v1", "--untracked-files=all"]), "");
    let mut p = plan(vec![card("a", ProviderId::Claude, "first")]);
    p.project = project.to_string_lossy().into_owned();
    create_run(&home, "submodule", &p).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "submodule").join("isolated-worktrees"), "").unwrap();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let never: Arc<Spawner> = Arc::new(move |_, _| { seen.store(true, Ordering::SeqCst); Err(std::io::Error::other("must not start")) });
    assert_eq!(run_plan(&home, "submodule", &p, None, never).unwrap(), CardStatus::Failed);
    assert!(!launched.load(Ordering::SeqCst));
    assert!(read_events(&home, "submodule").iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(reason), .. } if reason.contains("서브모듈") && reason.contains("vendor/lib"))));
    assert!(!home.with_file_name("state-worktrees").join("submodule").join("1").exists());

    // A project in another folder of the same repository never reads the submodule: it starts.
    std::fs::create_dir_all(project.join("app")).unwrap();
    std::fs::write(project.join("app").join("main.txt"), "app\n").unwrap();
    git(&["add", "app/main.txt"]);
    git(&["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "app"]);
    let mut app = plan(vec![card("a", ProviderId::Claude, "first")]);
    app.project = project.join("app").to_string_lossy().into_owned();
    create_run(&home, "beside-submodule", &app).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "beside-submodule").join("isolated-worktrees"), "").unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&started);
    let spawner: Arc<Spawner> = Arc::new(move |_, _| { seen.store(true, Ordering::SeqCst); Err(std::io::Error::other("fixture stops here")) });
    let _ = run_plan(&home, "beside-submodule", &app, None, spawner);
    assert!(started.load(Ordering::SeqCst));
}

fn failed_log() {
    for blocked_log in ["first.raw.jsonl", "first.err.log"] {
        let home = temp_home();
        let first = card("first", ProviderId::Claude, "first");
        let mut second = card("second", ProviderId::Claude, "second");
        second.depends_on = vec!["first".into()];
        let p = plan(vec![first, second]);
        create_run(&home, "bad-log", &p).unwrap();
        std::fs::create_dir(o_core::run::store::run_dir(&home, "bad-log").join("cards").join(blocked_log)).unwrap();
        let launched = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&launched);
        let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
            if spec.env["O_CARD_ID"] == "second" { observed.store(true, Ordering::SeqCst); }
            let mut command = support::fake_worker_launch()?;
            command.current_dir(cwd).env(FAKE, "[]").env(FAKE_SLEEP, "30000");
            Ok(command)
        });
        let start = std::time::Instant::now();
        assert_eq!(run_plan(&home, "bad-log", &p, None, spawner).unwrap(), CardStatus::Failed);
        assert!(start.elapsed() < Duration::from_secs(10));
        assert!(!launched.load(Ordering::SeqCst));
        assert!(active_markers(&home).is_empty());
    }
}

fn raw_output_preserved() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Claude, "raw")]);
    create_run(&home, "raw", &p).unwrap();
    let spawner: Arc<Spawner> = Arc::new(|_, cwd| {
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).arg("--fake-raw");
        Ok(command)
    });
    assert_eq!(run_plan(&home, "raw", &p, None, spawner).unwrap(), CardStatus::Done);
    let raw = std::fs::read(o_core::run::store::run_dir(&home, "raw").join("cards/a.raw.jsonl")).unwrap();
    assert_eq!(raw, b"\xff\r\n{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}\r\n");
}

fn dependency_result_limit() {
    for (run, size, expected) in [("within", 2000, CardStatus::Done), ("large", 13000, CardStatus::Done)] {
        let home = temp_home();
        let first = card("first", ProviderId::Claude, "first");
        let mut second = card("second", ProviderId::Claude, "second");
        second.depends_on = vec!["first".into()];
        let p = plan(vec![first, second]);
        create_run(&home, run, &p).unwrap();
        let seen = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&seen);
        let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
            if spec.env["O_CARD_ID"] == "second" {
                let prompt = spec.stdin.as_deref().unwrap();
                if size <= 2000 { assert!(prompt.contains(&format!("{}END", "x".repeat(size)))); }
                else { assert!(prompt.contains("o context --result first") && prompt.contains(&o_core::run::result::digest(&format!("{}END", "x".repeat(size))))); }
                observed.store(true, Ordering::SeqCst);
            }
            let line = serde_json::json!({"type":"result", "subtype":"success", "is_error":false, "result":format!("{}END", "x".repeat(size))}).to_string();
            let mut command = support::fake_worker_launch()?;
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&vec![line]).unwrap());
            Ok(command)
        });
        assert_eq!(run_plan(&home, run, &p, None, spawner).unwrap(), expected);
        assert_eq!(seen.load(Ordering::SeqCst), expected == CardStatus::Done);
    }
}

fn dependency_order() {
    let home = temp_home();
    let first = card("first", ProviderId::Claude, "first");
    let mut second = card("second", ProviderId::Claude, "second");
    second.depends_on = vec!["first".into()];
    let p = plan(vec![second, first]);
    create_run(&home, "graph-ok", &p).unwrap();
    let received = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&received);
    let spawner = fake_cli(move |spec| {
        if spec.stdin.as_deref().is_some_and(|s| s.contains("[선행 작업 결과") && s.contains("first")) { observed.store(true, Ordering::SeqCst); }
        vec![r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#]
    });
    assert_eq!(run_plan(&home, "graph-ok", &p, None, spawner).unwrap(), CardStatus::Done);
    assert!(received.load(Ordering::SeqCst));
    let events = read_events(&home, "graph-ok");
    let first_end = events.iter().position(|e| matches!(e, OEvent::CardEnd { card_id, status: CardStatus::Done, .. } if card_id == "first")).unwrap();
    let second_start = events.iter().position(|e| matches!(e, OEvent::CardStart { card_id, .. } if card_id == "second")).unwrap();
    assert!(first_end < second_start);

    create_run(&home, "graph-fail", &p).unwrap();
    let fail: Arc<Spawner> = Arc::new(|_, _| Err(std::io::Error::other("fake failure")));
    assert_eq!(run_plan(&home, "graph-fail", &p, None, fail).unwrap(), CardStatus::Failed);
    let events = read_events(&home, "graph-fail");
    assert!(!events.iter().any(|e| matches!(e, OEvent::CardStart { card_id, .. } if card_id == "second")));
    assert!(events.iter().any(|e| matches!(e, OEvent::CardEnd { card_id, status: CardStatus::Failed, .. } if card_id == "second")));
}

fn devin_transcript() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Devin, "x")]);
    create_run(&home, "devin", &p).unwrap();
    let spawner: Arc<Spawner> = Arc::new(|spec, cwd| {
        let export = spec.args.iter().position(|arg| arg == "--export").unwrap();
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).args(["--fake-devin", &spec.args[export + 1]]);
        Ok(command)
    });
    assert_eq!(run_plan(&home, "devin", &p, None, spawner).unwrap(), CardStatus::Done);
    let events = read_events(&home, "devin");
    assert!(events.iter().any(|e| matches!(e, OEvent::CardTool { name, .. } if name == "read")));
    assert!(events.iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(result), .. } if result == "작업 완료")));
}

/// Stands in for a provider CLI: prints the given stream lines, then exits.
fn fake_cli(lines_for: impl Fn(&LaunchSpec) -> Vec<&'static str> + Send + Sync + 'static) -> Arc<Spawner> {
    Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines_for(spec)).unwrap());
        Ok(command)
    })
}

fn temp_home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-test-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn card(id: &str, provider: ProviderId, task: &str) -> Card {
    Card { id: id.into(), provider, role: None, task: task.into(), model: None, effort: None, depends_on: vec![], resources: vec![] }
}

fn plan(cards: Vec<Card>) -> Plan {
    Plan { title: "t".into(), project: temp_home().to_string_lossy().into_owned(), cards }
}

fn ends_of(home: &std::path::Path, run_id: &str) -> BTreeMap<String, CardStatus> {
    read_events(home, run_id)
        .into_iter()
        .filter_map(|e| match e {
            OEvent::CardEnd { card_id, status, .. } => Some((card_id, status)),
            _ => None,
        })
        .collect()
}

fn quiet() -> Arc<Spawner> {
    fake_cli(|_| vec![r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#])
}

fn rejected_plan_records_reason() {
    let home = temp_home();
    let mut p = plan(vec![card("a", ProviderId::Claude, "test")]);
    p.project = home.join("missing-project").to_string_lossy().into_owned();
    create_run(&home, "rejected", &p).unwrap();
    assert!(run_detached(&home, "rejected").is_err());
    let events = read_events(&home, "rejected");
    assert!(matches!(events.as_slice(), [OEvent::RunStart { cards, .. }, OEvent::RunBlocked { violations, .. }, OEvent::RunEnd { status: CardStatus::Failed, .. }] if cards.is_empty() && !violations.is_empty()));
}

fn parallel_and_limit() {
    let home = temp_home();
    let mut p = plan(vec![card("plan", ProviderId::Claude, "설계해"), card("code", ProviderId::Codex, "구현해")]);
    p.title = "테스트".into();
    p.cards[0].role = Some("설계".into());
    p.cards[1].role = Some("코딩".into());
    create_run(&home, "r1", &p).unwrap();

    let status = run_plan(
        &home,
        "r1",
        &p,
        None,
        fake_cli(|spec| {
            if spec.bin == "claude" {
                vec![
                    r#"{"type":"system","subtype":"init","session_id":"s1"}"#,
                    r#"{"type":"assistant","message":{"content":[{"type":"text","text":"설계 끝"}]}}"#,
                    r#"{"type":"result","subtype":"success","is_error":false,"result":"설계 끝"}"#,
                ]
            } else {
                vec![r#"{"type":"thread.started","thread_id":"t1"}"#, r#"{"type":"turn.failed","error":{"message":"usage limit reached"}}"#]
            }
        }),
    )
    .unwrap();

    assert_eq!(status, CardStatus::Failed);
    let events = read_events(&home, "r1");
    assert_eq!(ends_of(&home, "r1"), BTreeMap::from([("plan".into(), CardStatus::Done), ("code".into(), CardStatus::Failed)]));
    assert_eq!(events.last().map(OEvent::kind), Some("run.end"));
    assert!(events.iter().any(|e| matches!(e, OEvent::CardLimit { card_id, .. } if card_id == "code")));
    let until = cooldown_times(&home).unwrap().get(&ProviderId::Codex).cloned().unwrap_or_default();
    assert!(parse_time(&until).is_some_and(|t| t > Utc::now()));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 2);
    assert!(active_markers(&home).is_empty());
    // The run start carries labels, and every card start a pid.
    assert!(events.iter().any(|e| matches!(e, OEvent::RunStart { cards, .. } if cards[0].label.as_deref() == Some("설계"))));
    assert!(events.iter().filter(|e| matches!(e, OEvent::CardStart { pid: Some(_), .. })).count() == 2);
}

/// #37: OpenCode loads `opencode.json(c)` and `.opencode/` (plugins too) from the folder up to the
/// root, so a card without YOLO is refused when the project or a parent has one.
fn opencode_project_config_refused() {
    let fixture = temp_home();
    let home = fixture.join("state");
    let project = fixture.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let plugins = fixture.join(".opencode").join("plugins");
    std::fs::create_dir_all(&plugins).unwrap();
    std::fs::write(plugins.join("marker.js"), "// harmless fixture, never loaded\n").unwrap();
    let mut p = plan(vec![card("a", ProviderId::OpenCode, "x")]);
    p.project = project.to_string_lossy().into_owned();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let spawner: Arc<Spawner> = Arc::new(move |_: &LaunchSpec, _: &str| {
        seen.store(true, Ordering::SeqCst);
        Err(std::io::Error::other("fake start"))
    });
    create_run(&home, "oc-parent", &p).unwrap();
    assert_eq!(run_plan(&home, "oc-parent", &p, None, Arc::clone(&spawner)).unwrap(), CardStatus::Failed);
    assert!(!launched.load(Ordering::SeqCst));
    assert!(read_events(&home, "oc-parent").iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(reason), .. } if reason.contains(".opencode") && reason.contains("OpenCode 카드를 시작하지 않았습니다"))));

    std::fs::remove_dir_all(fixture.join(".opencode")).unwrap();
    std::fs::write(project.join("opencode.jsonc"), "{}\n").unwrap();
    create_run(&home, "oc-project", &p).unwrap();
    assert_eq!(run_plan(&home, "oc-project", &p, None, Arc::clone(&spawner)).unwrap(), CardStatus::Failed);
    assert!(!launched.load(Ordering::SeqCst));

    std::fs::remove_file(project.join("opencode.jsonc")).unwrap();
    create_run(&home, "oc-clean", &p).unwrap();
    run_plan(&home, "oc-clean", &p, None, spawner).unwrap();
    assert!(launched.load(Ordering::SeqCst));
}

fn cooling_down() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Codex, "x")]);
    create_run(&home, "r2", &p).unwrap();
    set_cooldown(&home, ProviderId::Codex, &iso(Utc::now() + Span::seconds(60)), Utc::now()).unwrap();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        seen.store(true, Ordering::SeqCst);
        inner(spec, cwd)
    });
    run_plan(&home, "r2", &p, None, spawner).unwrap();
    assert!(!launched.load(Ordering::SeqCst));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);
    assert_eq!(ends_of(&home, "r2"), BTreeMap::from([("a".into(), CardStatus::Failed)]));
}

fn daily_cap_at_start() {
    let home = temp_home();
    for i in 0..LIMITS.max_cards_per_day {
        record_start(&home, "earlier", &format!("c{i}"), ProviderId::Codex, Utc::now()).unwrap();
    }
    let p = plan(vec![card("a", ProviderId::Claude, "x")]);
    create_run(&home, "r3", &p).unwrap();
    assert_eq!(run_plan(&home, "r3", &p, None, quiet()).unwrap(), CardStatus::Failed);
    assert_eq!(ends_of(&home, "r3"), BTreeMap::from([("a".into(), CardStatus::Failed)]));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), LIMITS.max_cards_per_day);
}

fn stop_request() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Codex, "x")]);
    create_run(&home, "r4", &p).unwrap();
    request_stop(&home, "r4").unwrap();
    assert_eq!(run_plan(&home, "r4", &p, None, quiet()).unwrap(), CardStatus::Stopped);
    assert_eq!(ends_of(&home, "r4"), BTreeMap::from([("a".into(), CardStatus::Stopped)]));
}

fn queue_then_run() {
    let home = temp_home();
    // Live markers from another run fill agy's slots (this test process stands in as their pid).
    let slots = LIMITS.max_active_per_provider(ProviderId::Agy);
    for i in 0..slots {
        let marker = ActiveMarker { run_id: "other".into(), card_id: format!("b{i}"), provider: ProviderId::Agy, pid: std::process::id() };
        mark_active(&home, &marker).unwrap();
    }
    let p = plan(vec![card("a", ProviderId::Agy, "x")]);
    create_run(&home, "r5", &p).unwrap();
    let freeing = home.clone();
    let freer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        for i in 0..slots {
            clear_active(&freeing, "other", &format!("b{i}"));
        }
    });
    run_plan(&home, "r5", &p, None, fake_cli(|_| vec![r#"{"event":"result","result":{"status":"SUCCESS","response":"ok"}}"#])).unwrap();
    freer.join().unwrap();
    let kinds: Vec<&str> = read_events(&home, "r5").iter().map(OEvent::kind).collect();
    let at = |k: &str| kinds.iter().position(|x| *x == k).unwrap();
    assert!(at("card.queued") < at("card.start"));
    assert_eq!(ends_of(&home, "r5"), BTreeMap::from([("a".into(), CardStatus::Done)]));
}

fn clean_run() {
    let home = temp_home();
    let root = home.join("agents");
    let dump = home.join("env.json");
    std::env::set_var("ORCA_TERMINAL_ID", "t1");
    std::env::set_var("BEAKHO_RELEASE", "pinned");
    let p = plan(vec![card("a", ProviderId::Claude, "x")]);
    create_run(&home, "r6", &p).unwrap();
    let target = dump.clone();
    let spawner: Arc<Spawner> = Arc::new(move |_: &LaunchSpec, cwd: &str| {
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).arg(DUMP_ENV).arg(&target);
        Ok(command)
    });
    assert_eq!(run_plan(&home, "r6", &p, Some(&root), spawner).unwrap(), CardStatus::Done);
    let env: BTreeMap<String, String> = serde_json::from_str(&std::fs::read_to_string(&dump).unwrap()).unwrap();
    let get = |name: &str| env.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    let profile = root.join("claude").join("home");
    assert_eq!(get("O_RUN_ID").as_deref(), Some("r6"));
    assert_eq!(get("O_CARD_ID").as_deref(), Some("a"));
    assert_eq!(get("USERPROFILE"), Some(profile.to_string_lossy().into_owned()));
    assert_eq!(get("CLAUDE_CONFIG_DIR"), Some(profile.join(".claude").to_string_lossy().into_owned()));
    assert!(get("PATH").is_some());
    assert!(get("ORCA_TERMINAL_ID").is_none() && get("BEAKHO_RELEASE").is_none());
    assert!(profile.join(".claude").join("settings.json").is_file());
}

/// One plan with more cards of a provider than its slots: the extra card waits even
/// while the first ones are still starting (their slot markers not yet written).
fn same_provider_slots_in_one_plan() {
    let home = temp_home();
    let max = LIMITS.max_active_per_provider(ProviderId::Claude);
    let p = plan((0..=max).map(|i| {
        let mut card = card(&format!("c{i}"), ProviderId::Claude, "x");
        card.resources = vec![o_core::run::plan::Resource { kind: o_core::run::plan::ResourceKind::External, key: format!("test-{i}"), mode: o_core::run::plan::ResourceMode::Write }];
        card
    }).collect());
    create_run(&home, "r10", &p).unwrap();
    let inner = quiet();
    // A slow start (PATH lookup, process creation) leaves the window the runner must not miss.
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        std::thread::sleep(Duration::from_millis(300));
        inner(spec, cwd)
    });
    assert_eq!(run_plan(&home, "r10", &p, None, spawner).unwrap(), CardStatus::Done);
    let queued = read_events(&home, "r10").iter().filter(|e| matches!(e, OEvent::CardQueued { .. })).count();
    assert_eq!(queued, 1);
    assert!(active_markers(&home).is_empty());
}

/// What the spawner has in flight, per CLI and in total, and the most it ever had.
#[derive(Default)]
struct InFlight {
    now: BTreeMap<String, usize>,
    peak: BTreeMap<String, usize>,
    total: usize,
    peak_total: usize,
}

/// A spawner that takes 300 ms to start each card (PATH lookup, process creation) and counts
/// how many starts overlap: the window in which another runner must already see the slot taken.
fn slow_counting_spawner(seen: Arc<Mutex<InFlight>>) -> Arc<Spawner> {
    let inner = quiet();
    Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        {
            let mut f = seen.lock().unwrap();
            *f.now.entry(spec.bin.clone()).or_default() += 1;
            f.total += 1;
            let now = f.now[&spec.bin];
            let peak = f.peak.entry(spec.bin.clone()).or_default();
            *peak = (*peak).max(now);
            f.peak_total = f.peak_total.max(f.total);
        }
        std::thread::sleep(Duration::from_millis(300));
        {
            let mut f = seen.lock().unwrap();
            *f.now.get_mut(&spec.bin).unwrap() -= 1;
            f.total -= 1;
        }
        inner(spec, cwd)
    })
}

/// Runs each plan on its own thread, all released at once, like runners of separate dispatches.
fn run_together(home: &std::path::Path, plans: Vec<(&'static str, Plan)>, spawner: Arc<Spawner>) {
    let barrier = Arc::new(std::sync::Barrier::new(plans.len()));
    let handles: Vec<_> = plans
        .into_iter()
        .map(|(run_id, p)| {
            create_run(home, run_id, &p).unwrap();
            let (home, barrier, spawner) = (home.to_path_buf(), Arc::clone(&barrier), Arc::clone(&spawner));
            std::thread::spawn(move || {
                barrier.wait();
                run_plan(&home, run_id, &p, None, spawner).unwrap();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

/// Two runs dispatched together, each filling every slot: neither may start a card in a slot
/// the other is still starting a CLI in (its marker is not on disk yet).
fn runners_share_slots() {
    let home = temp_home();
    let full = |prefix: &str| {
        let claude = LIMITS.max_active_per_provider(ProviderId::Claude);
        let codex = LIMITS.max_active_total - claude;
        let mut cards: Vec<Card> = (0..claude).map(|i| card(&format!("{prefix}c{i}"), ProviderId::Claude, "x")).collect();
        cards.extend((0..codex).map(|i| card(&format!("{prefix}x{i}"), ProviderId::Codex, "x")));
        for card in &mut cards {
            card.resources = vec![o_core::run::plan::Resource { kind: o_core::run::plan::ResourceKind::External, key: format!("test-{}", card.id), mode: o_core::run::plan::ResourceMode::Write }];
        }
        plan(cards)
    };
    let seen = Arc::new(Mutex::new(InFlight::default()));
    run_together(&home, vec![("ra", full("a")), ("rb", full("b"))], slow_counting_spawner(Arc::clone(&seen)));
    let seen = seen.lock().unwrap();
    assert!(seen.peak["claude"] <= LIMITS.max_active_per_provider(ProviderId::Claude), "claude {:?}", seen.peak);
    assert!(seen.peak["codex"] <= LIMITS.max_active_per_provider(ProviderId::Codex), "codex {:?}", seen.peak);
    assert!(seen.peak_total <= LIMITS.max_active_total, "total {}", seen.peak_total);
    assert_eq!(ends_of(&home, "ra").len() + ends_of(&home, "rb").len(), 2 * LIMITS.max_active_total);
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 2 * LIMITS.max_active_total);
    assert!(active_markers(&home).is_empty());
}

/// Two runs that each want the day's last start: one gets it, the other ends without starting.
fn runners_share_the_days_last_start() {
    let home = temp_home();
    for i in 0..LIMITS.max_cards_per_day - 1 {
        record_start(&home, "earlier", &format!("c{i}"), ProviderId::Codex, Utc::now()).unwrap();
    }
    let seen = Arc::new(Mutex::new(InFlight::default()));
    let one = || plan(vec![card("a", ProviderId::Claude, "x")]);
    run_together(&home, vec![("rc", one()), ("rd", one())], slow_counting_spawner(seen));
    assert_eq!(started_on(&home, Utc::now()).unwrap(), LIMITS.max_cards_per_day);
    let mut ends: Vec<CardStatus> = ends_of(&home, "rc").into_values().chain(ends_of(&home, "rd").into_values()).collect();
    ends.sort_by_key(|s| format!("{s:?}"));
    assert_eq!(ends, [CardStatus::Done, CardStatus::Failed]);
}

/// A card whose CLI cannot be started gives its slot and its start of the day back.
fn failed_start_gives_back() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Claude, "x")]);
    create_run(&home, "re", &p).unwrap();
    let spawner: Arc<Spawner> = Arc::new(|_: &LaunchSpec, _: &str| Err(std::io::Error::other("없음")));
    assert_eq!(run_plan(&home, "re", &p, None, spawner).unwrap(), CardStatus::Failed);
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);
    assert!(o_core::run::store::policy_context(&home, Utc::now()).unwrap().active_by_provider.values().all(|n| *n == 0));
}

/// `o stop` arrives while a card's CLI is being started (slot taken, no marker yet): the stop
/// kills nothing it can see, so the runner must stop the CLI itself as soon as it is up.
fn stop_while_starting() {
    let home = temp_home();
    let p = plan(vec![card("a", ProviderId::Claude, "x")]);
    create_run(&home, "rs", &p).unwrap();
    let stopping = home.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        assert!(o_core::run::runner::stop_run(&stopping, "rs").unwrap());
        let mut command = quiet()(spec, cwd)?;
        command.env(FAKE_SLEEP, "20000");
        Ok(command)
    });
    let started = std::time::Instant::now();
    assert_eq!(run_plan(&home, "rs", &p, None, spawner).unwrap(), CardStatus::Stopped);
    assert!(started.elapsed() < Duration::from_secs(10), "the CLI ran on for {:?} after the stop", started.elapsed());
    assert_eq!(ends_of(&home, "rs"), BTreeMap::from([("a".into(), CardStatus::Stopped)]));
    assert!(active_markers(&home).is_empty());
}

/// The plan names the project through a link; the policy passed the folder it resolved to.
/// Cards start in that checked folder, not wherever the link points later.
fn cards_run_in_the_checked_folder() {
    let home = temp_home();
    let real = home.join("project");
    std::fs::create_dir_all(&real).unwrap();
    let checked = real.to_string_lossy().into_owned();
    let mut p = plan(vec![card("a", ProviderId::Codex, "x")]);
    p.project = home.join("link-to-project").to_string_lossy().into_owned();
    create_run(&home, "rw", &p).unwrap();
    let seen = Arc::new(Mutex::new(Vec::<(String, Vec<String>)>::new()));
    let log = Arc::clone(&seen);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        log.lock().unwrap().push((cwd.to_string(), spec.args.clone()));
        inner(spec, cwd)
    });
    let to = checked.clone();
    let workdir = Workdir { checked: checked.clone(), resolve: Arc::new(move |_: &str| to.clone()) };
    run_plan_in(&home, "rw", &p, None, spawner, workdir).unwrap();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, checked);
    // Codex is told the same folder (`-C`), not the link.
    let at = seen[0].1.iter().position(|a| a == "-C").unwrap();
    assert_eq!(seen[0].1[at + 1], checked);
}

/// The link is pointed elsewhere after the check: the card is refused, its CLI never starts.
fn a_link_swapped_after_the_check_is_refused() {
    let home = temp_home();
    let mut p = plan(vec![card("a", ProviderId::Claude, "x")]);
    p.project = home.join("link-to-project").to_string_lossy().into_owned();
    create_run(&home, "rx", &p).unwrap();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        seen.store(true, Ordering::SeqCst);
        inner(spec, cwd)
    });
    let checked = home.join("project").to_string_lossy().into_owned();
    let elsewhere = std::env::temp_dir().to_string_lossy().into_owned();
    let workdir = Workdir { checked, resolve: Arc::new(move |_: &str| elsewhere.clone()) };
    assert_eq!(run_plan_in(&home, "rx", &p, None, spawner, workdir).unwrap(), CardStatus::Failed);
    assert!(!launched.load(Ordering::SeqCst));
    let refused = read_events(&home, "rx").into_iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(r), .. } if r.contains("정책 검사 뒤")));
    assert!(refused);
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);

    create_run(&home, "rx-isolated", &p).unwrap();
    std::fs::write(o_core::run::store::run_dir(&home, "rx-isolated").join("isolated-worktrees"), "").unwrap();
    let checked = home.join("project").to_string_lossy().into_owned();
    let elsewhere = std::env::temp_dir().to_string_lossy().into_owned();
    let workdir = Workdir { checked, resolve: Arc::new(move |_: &str| elsewhere.clone()) };
    assert_eq!(run_plan_in(&home, "rx-isolated", &p, None, quiet(), workdir).unwrap(), CardStatus::Failed);
    assert!(read_events(&home, "rx-isolated").iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(r), .. } if r.contains("정책 검사 뒤"))));
}

/// A key left in the person's environment must not reach the card's CLI, in any case spelling.
fn api_keys_dropped() {
    let home = temp_home();
    let vars = [("ANTHROPIC_API_KEY", "placeholder"), ("anthropic_auth_token", "placeholder"), ("CODEX_API_KEY", "placeholder"), ("ANTHROPIC_API_KEY_HELPER", "kept")];
    for (name, value) in vars {
        std::env::set_var(name, value);
    }
    let p = plan(vec![card("a", ProviderId::Claude, "x"), card("b", ProviderId::Codex, "y")]);
    create_run(&home, "r9", &p).unwrap();
    let dumps = home.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).arg(DUMP_ENV).arg(dumps.join(format!("{}.json", spec.bin)));
        Ok(command)
    });
    // The fake answers in Claude's stream format, so only the Claude card ends done; both dump their environment.
    run_plan(&home, "r9", &p, None, spawner).unwrap();
    for (name, _) in vars {
        std::env::remove_var(name);
    }
    assert_eq!(ends_of(&home, "r9").get("a"), Some(&CardStatus::Done));
    let env_of = |bin: &str| -> BTreeMap<String, String> { serde_json::from_str(&std::fs::read_to_string(home.join(format!("{bin}.json"))).unwrap()).unwrap() };
    let has = |env: &BTreeMap<String, String>, name: &str| env.keys().any(|k| k.eq_ignore_ascii_case(name));
    let (claude, codex) = (env_of("claude"), env_of("codex"));
    assert!(!has(&claude, "ANTHROPIC_API_KEY") && !has(&claude, "ANTHROPIC_AUTH_TOKEN"));
    assert!(has(&claude, "ANTHROPIC_API_KEY_HELPER") && has(&claude, "CODEX_API_KEY") && has(&claude, "PATH"));
    assert!(!has(&codex, "CODEX_API_KEY") && has(&codex, "ANTHROPIC_API_KEY"));
    assert_eq!(claude.get("O_CARD_ID").map(String::as_str), Some("a"));
}

use std::sync::Mutex;

use o_core::usage::budget::{local_day_of, record};
use o_core::usage::UsageWindow;

/// Codex's weekly window: 20% at the start of today, `today` more now.
fn seed_codex_spent(home: &std::path::Path, today: f64) {
    let now = Utc::now();
    let resets_at = iso(now + Span::days(3));
    let weekly = |used: f64| UsageWindow { provider: ProviderId::Codex, window: "seven_day".into(), group: None, used, resets_at: resets_at.clone() };
    record(home, &[weekly(0.20)], local_day_of(now).start - Span::minutes(1)).unwrap();
    record(home, &[weekly(0.20 + today)], now).unwrap();
}

fn budget_lowers_effort() {
    let home = temp_home();
    // 12.99 of 13 points spent: far behind pace until the last minutes of the day.
    seed_codex_spent(&home, 0.1299);
    let mut p = plan(vec![card("hi", ProviderId::Codex, "x"), card("plain", ProviderId::Codex, "y"), card("low", ProviderId::Codex, "z"), card("other", ProviderId::Claude, "w")]);
    p.cards[0].effort = Some("xhigh".into());
    p.cards[2].effort = Some("low".into());
    p.cards[3].effort = Some("high".into());
    create_run(&home, "r7", &p).unwrap();
    let launched = Arc::new(Mutex::new(BTreeMap::<String, Vec<String>>::new()));
    let log = Arc::clone(&launched);
    // Claude reports its windows in the stream; the runner keeps them as budget snapshots.
    let inner = fake_cli(|spec| {
        let mut lines = vec![r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#];
        if spec.bin == "claude" {
            lines.insert(0, r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":{"seven_day":{"utilization":0.31,"resetsAt":4102444800}}}}"#);
        }
        lines
    });
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        log.lock().unwrap().insert(spec.env["O_CARD_ID"].clone(), spec.args.clone());
        inner(spec, cwd)
    });
    run_plan(&home, "r7", &p, None, spawner).unwrap();

    let adjusted: BTreeMap<String, (Option<String>, String)> = read_events(&home, "r7")
        .into_iter()
        .filter_map(|e| match e {
            OEvent::CardBudget { card_id, effort_from, effort_to, spent, allowance, .. } => {
                assert!((spent - 12.99).abs() < 0.01 && allowance == 13.0, "{spent}/{allowance}");
                Some((card_id, (effort_from, effort_to)))
            }
            _ => None,
        })
        .collect();
    // Two steps down, never below low; Claude has no weekly reading and is left alone.
    assert_eq!(adjusted, BTreeMap::from([("hi".into(), (Some("xhigh".into()), "medium".into())), ("plain".into(), (None, "low".into()))]));
    let launched = launched.lock().unwrap();
    let has = |id: &str, arg: &str| launched[id].iter().any(|a| a == arg);
    assert!(has("hi", "model_reasoning_effort=medium") && has("plain", "model_reasoning_effort=low") && has("low", "model_reasoning_effort=low"));
    assert!(has("other", "high"));
    // The plan stays as written; the run's view shows the effort the card ran with.
    assert_eq!(o_core::run::store::read_plan(&home, "r7").unwrap()["cards"][0]["effort"], "xhigh");
    let view = o_core::run::snapshot::snapshot(&read_events(&home, "r7")).unwrap();
    let hi = view.cards.iter().find(|c| c.id == "hi").unwrap();
    assert_eq!(hi.effort.as_deref(), Some("medium"));
    assert!(hi.log.iter().any(|l| l.kind == o_core::run::snapshot::LogKind::Budget && l.text.contains("xhigh → medium")));
    let claude: Vec<f64> = o_core::usage::budget::weekly_snapshots(&home, ProviderId::Claude).unwrap().iter().map(|s| s.used).collect();
    assert_eq!(claude, [0.31]);
}

fn budget_over_refuses() {
    let home = temp_home();
    seed_codex_spent(&home, 0.14);
    let p = plan(vec![card("a", ProviderId::Codex, "x")]);
    create_run(&home, "r8", &p).unwrap();
    let launched = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&launched);
    let inner = quiet();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        seen.store(true, Ordering::SeqCst);
        inner(spec, cwd)
    });
    assert_eq!(run_plan(&home, "r8", &p, None, spawner).unwrap(), CardStatus::Failed);
    assert!(!launched.load(Ordering::SeqCst));
    let refused = read_events(&home, "r8").into_iter().any(|e| matches!(e, OEvent::CardEnd { result: Some(r), .. } if r.contains("사용 예산을 다 썼습니다")));
    assert!(refused);
    assert_eq!(started_on(&home, Utc::now()).unwrap(), 0);
}

fn budget_off_keeps_effort_and_refuses_nothing() {
    let home = temp_home();
    seed_codex_spent(&home, 0.1299);
    o_core::usage::budget::write_enabled(&home, false).unwrap();
    let mut p = plan(vec![card("a", ProviderId::Codex, "x")]);
    p.cards[0].effort = Some("xhigh".into());
    create_run(&home, "off", &p).unwrap();
    let launched = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&launched);
    let inner = fake_cli(|_| vec![r#"{"type":"turn.completed"}"#]);
    let spawner: Arc<Spawner> = Arc::new(move |spec, cwd| {
        *seen.lock().unwrap() = spec.args.clone();
        inner(spec, cwd)
    });
    let status = run_plan(&home, "off", &p, None, spawner).unwrap();
    assert_eq!(status, CardStatus::Done, "{:?}", read_events(&home, "off"));
    assert!(launched.lock().unwrap().iter().any(|a| a == "model_reasoning_effort=xhigh"));
    assert!(!read_events(&home, "off").iter().any(|e| matches!(e, OEvent::CardBudget { .. })));

    // The day's budget used up: with the switch off the card still starts.
    let spent_home = temp_home();
    seed_codex_spent(&spent_home, 0.14);
    o_core::usage::budget::write_enabled(&spent_home, false).unwrap();
    create_run(&spent_home, "spent", &p).unwrap();
    let inner = fake_cli(|_| vec![r#"{"type":"turn.completed"}"#]);
    assert_eq!(run_plan(&spent_home, "spent", &p, None, inner).unwrap(), CardStatus::Done, "{:?}", read_events(&spent_home, "spent"));
}
