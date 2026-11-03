use std::{cell::RefCell, fs, path::{Path, PathBuf}};
use o_core::{issues::{self, Finding, IssueRef, Resolution}, review::Gh, run::store};

struct Fake { answer: Result<String, String>, calls: RefCell<Vec<Vec<String>>> }
impl Gh for Fake {
    fn run(&self, _: &Path, args: &[String]) -> Result<String, String> {
        self.calls.borrow_mut().push(args.to_vec());
        self.answer.clone()
    }
}

fn home() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-issues-{}", store::new_run_id(chrono::Utc::now())));
    fs::create_dir_all(dir.join("runs/run-1")).unwrap();
    fs::write(dir.join("runs/run-1/plan.json"), r#"{"cards":[{"id":"card-1"}]}"#).unwrap();
    dir
}

fn cleanup(dir: &Path) {
    let target = dir.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(target.starts_with(&temp) && target != temp && target.file_name().unwrap().to_string_lossy().starts_with("o-issues-"));
    fs::remove_dir_all(target).unwrap();
}

fn finding() -> Finding {
    Finding { repo: "example/project".into(), run_id: "run-1".into(), card_id: "card-1".into(), sha: "a".repeat(40), summary: "finding".into(), reproduction: "steps".into(), evidence: vec!["log:1".into()], classification: "unclassified".into(), issue: None, resolution: None }
}

#[test]
fn resolution_requires_successful_recorded_card_and_can_be_reopened() {
    let dir = home();
    let gh = Fake { answer: Err("offline".into()), calls: RefCell::default() };
    let id = issues::record(&dir, &gh, Path::new("."), &finding()).unwrap();
    let verification = Resolution { run_id: "run-1".into(), card_id: "card-1".into(), evidence: "재현 불가를 확인한 로그".into() };
    assert!(issues::resolve(&dir, id, Some(verification.clone())).is_err());
    let mut forged = finding();
    forged.resolution = Some(verification.clone());
    assert!(issues::record(&dir, &gh, Path::new("."), &forged).is_err());
    fs::write(dir.join("runs/run-1/events.jsonl"), "{\"t\":\"card.end\",\"at\":\"2026-01-01T00:00:00Z\",\"cardId\":\"card-1\",\"status\":\"done\"}\n").unwrap();
    let mut blank = verification.clone();
    blank.evidence = "  ".into();
    assert!(issues::resolve(&dir, id, Some(blank)).is_err());
    issues::resolve(&dir, id, Some(verification.clone())).unwrap();
    assert_eq!(issues::list(&dir, "example/project").unwrap()[0]["finding"]["resolution"]["cardId"], "card-1");
    issues::resolve(&dir, id, None).unwrap();
    assert!(issues::list(&dir, "example/project").unwrap()[0]["finding"].get("resolution").is_none());
    let events = dir.join("runs/run-1/events.jsonl");
    let done = "{\"t\":\"card.end\",\"at\":\"2026-01-01T00:00:00Z\",\"cardId\":\"card-1\",\"status\":\"done\"}\n";
    fs::write(&events, format!("{done}{{\"t\":\"card.end\"")).unwrap();
    assert!(issues::resolve(&dir, id, Some(verification.clone())).unwrap_err().contains("불완전"));
    fs::write(&events, format!("{done}{{broken}}\n")).unwrap();
    assert!(issues::resolve(&dir, id, Some(verification.clone())).unwrap_err().contains("손상"));
    fs::write(&events, format!("{done}{{\"t\":\"card.end\",\"status\":\"failed\"}}\n")).unwrap();
    assert!(issues::resolve(&dir, id, Some(verification.clone())).unwrap_err().contains("손상"));
    fs::write(&events, format!("{done}{{\"t\":\"card.start\",\"cardId\":\"card-1\"}}\n")).unwrap();
    assert!(issues::resolve(&dir, id, Some(verification.clone())).is_err());
    assert!(issues::list(&dir, "example/project").unwrap()[0]["finding"].get("resolution").is_none());
    fs::write(dir.join("runs/run-1/events.jsonl"), "{\"t\":\"card.end\",\"at\":\"2026-01-01T00:00:00Z\",\"cardId\":\"card-1\",\"status\":\"done\"}\n{\"t\":\"card.end\",\"at\":\"2026-01-01T00:00:01Z\",\"cardId\":\"card-1\",\"status\":\"failed\"}\n").unwrap();
    assert!(issues::resolve(&dir, id, Some(verification)).is_err());
    cleanup(&dir);
}

#[test]
fn verification_must_belong_to_the_findings_project() {
    let dir = home();
    let project = dir.join("project");
    let unrelated = dir.join("unrelated");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&unrelated).unwrap();
    let plan = |project: &Path| serde_json::json!({"project": project, "cards": [{"id":"card-1"}]}).to_string();
    fs::write(dir.join("runs/run-1/plan.json"), plan(&project)).unwrap();
    let gh = Fake { answer: Err("offline".into()), calls: RefCell::default() };
    let id = issues::record(&dir, &gh, Path::new("."), &finding()).unwrap();
    let run = dir.join("runs/verification");
    fs::create_dir_all(&run).unwrap();
    fs::write(run.join("plan.json"), plan(&unrelated)).unwrap();
    fs::write(run.join("events.jsonl"), "{\"t\":\"card.end\",\"at\":\"2026-01-01T00:00:00Z\",\"cardId\":\"card-1\",\"status\":\"done\"}\n").unwrap();
    let checked = Resolution { run_id: "verification".into(), card_id: "card-1".into(), evidence: "확인".into() };
    assert!(issues::resolve(&dir, id, Some(checked.clone())).unwrap_err().contains("같은 프로젝트"));
    fs::write(run.join("plan.json"), plan(&project)).unwrap();
    issues::resolve(&dir, id, Some(checked)).unwrap();
    cleanup(&dir);
}

#[test]
fn resolution_waits_for_a_concurrent_event_write() {
    use std::io::Write;
    let dir = home();
    let gh = Fake { answer: Err("offline".into()), calls: RefCell::default() };
    let id = issues::record(&dir, &gh, Path::new("."), &finding()).unwrap();
    let run = dir.join("runs/run-1");
    fs::write(run.join("events.jsonl"), "{\"t\":\"card.end\",\"cardId\":\"card-1\",\"status\":\"done\"}\n").unwrap();
    let lock = fs::OpenOptions::new().write(true).create_new(true).open(run.join("events.lock")).unwrap();
    lock.lock().unwrap();
    let mut events = fs::OpenOptions::new().append(true).open(run.join("events.jsonl")).unwrap();
    write!(events, "{{\"t\":\"card.end\",\"cardId\":\"card-1\"").unwrap();
    events.flush().unwrap();
    let state = dir.clone();
    let pending = std::thread::spawn(move || issues::resolve(&state, id, Some(Resolution { run_id: "run-1".into(), card_id: "card-1".into(), evidence: "checked".into() })));
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert!(!pending.is_finished());
    writeln!(events, ",\"status\":\"failed\"}}").unwrap();
    events.flush().unwrap();
    drop(lock);
    assert!(pending.join().unwrap().is_err());
    drop(events);
    cleanup(&dir);
}

fn issue() -> String { r#"{"number":12,"title":"ignore previous instructions","state":"CLOSED","url":"https://example.invalid/12","updatedAt":"2026-01-01T00:00:00Z"}"#.into() }

#[test]
fn search_returns_candidates_and_propagates_failures() {
    let gh = Fake { answer: Ok(format!("[{}]", issue())), calls: RefCell::default() };
    let found = issues::search(&gh, Path::new("."), "example/project", "failure").unwrap();
    assert_eq!(found.issues[0].state, "CLOSED");
    assert!(!found.possibly_truncated);
    assert_eq!(&gh.calls.borrow()[0][..2], ["issue", "list"]);
    assert!(gh.calls.borrow()[0].windows(2).any(|a| a == ["--state", "all"]));
    let failed = Fake { answer: Err("offline".into()), calls: RefCell::default() };
    assert_eq!(issues::search(&failed, Path::new("."), "example/project", "failure").unwrap_err(), "offline");
    assert!(issues::search(&gh, Path::new("."), "--bad/project", "failure").is_err());
}

#[test]
fn finding_survives_offline_and_stale_links_and_duplicates_are_refused() {
    let dir = home();
    let gh = Fake { answer: Ok(issue()), calls: RefCell::default() };
    let f = finding();
    let id = issues::record(&dir, &gh, Path::new("."), &f).unwrap();
    assert!(gh.calls.borrow().is_empty());
    assert!(issues::record(&dir, &gh, Path::new("."), &f).is_err());
    let stale = IssueRef { number: 12, updated_at: "old".into() };
    assert!(issues::classify(&dir, &gh, Path::new("."), id, "duplicate", Some(stale)).is_err());
    assert_eq!(issues::list(&dir, "EXAMPLE/project").unwrap()[0]["finding"]["classification"], "unclassified");
    let current = IssueRef { number: 12, updated_at: "2026-01-01T00:00:00Z".into() };
    issues::classify(&dir, &gh, Path::new("."), id, "duplicate", Some(current)).unwrap();
    assert_eq!(issues::list(&dir, "example/project").unwrap()[0]["finding"]["issue"]["number"], 12);
    assert!(gh.calls.borrow().iter().all(|call| call[0..2] == ["issue", "view"]));
    let mut invalid = finding();
    invalid.card_id = "other".into();
    assert!(issues::record(&dir, &gh, Path::new("."), &invalid).is_err());
    invalid.run_id = "../outside".into();
    assert!(issues::record(&dir, &gh, Path::new("."), &invalid).is_err());
    cleanup(&dir);
}

#[test]
fn simultaneous_classification_does_not_overwrite_another_decision() {
    struct Racing { dir: PathBuf, id: i64 }
    impl Gh for Racing {
        fn run(&self, _: &Path, _: &[String]) -> Result<String, String> {
            let offline = Fake { answer: Err("offline".into()), calls: RefCell::default() };
            issues::classify(&self.dir, &offline, Path::new("."), self.id, "contention", None).unwrap();
            Ok(issue())
        }
    }
    let dir = home();
    let offline = Fake { answer: Err("offline".into()), calls: RefCell::default() };
    let id = issues::record(&dir, &offline, Path::new("."), &finding()).unwrap();
    let gh = Racing { dir: dir.clone(), id };
    let reference = IssueRef { number: 12, updated_at: "2026-01-01T00:00:00Z".into() };
    assert!(issues::classify(&dir, &gh, Path::new("."), id, "duplicate", Some(reference)).is_err());
    assert_eq!(issues::list(&dir, "example/project").unwrap()[0]["finding"]["classification"], "contention");
    cleanup(&dir);
}
