//! The lead chat with a fake lead CLI. The fake is this test binary itself: started
//! with O_FAKE_LEAD_LINES it prints those lines and exits, so no real (paid) CLI runs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::Utc;
use o_core::lead::files::{self, MAX_BYTES};
use o_core::lead::prompt::{build, split_plan, PromptInput};
use o_core::lead::{self, DispatchError, LeadEvent, SendRequest};
use o_core::providers::LaunchSpec;
use o_core::run::plan::ProviderId;
use o_core::run::runner::Spawner;
use serde_json::{json, Value};

const FAKE: &str = "O_FAKE_LEAD_LINES";

fn main() {
    if let Ok(lines) = std::env::var(FAKE) {
        let lines: Vec<String> = serde_json::from_str(&lines).expect("fake lines");
        for line in lines {
            println!("{line}");
        }
        return;
    }
    let tests: [(&str, fn()); 11] = [
        ("a plan block is read: none, one, malformed, two", plan_blocks),
        ("the first turn tells the role and schema; a resumed one only the news", prompts),
        ("a turn records the message, session, tokens, reply and a checked plan", send_records_a_plan),
        ("the plan's project is forced to the thread's", project_forced),
        ("a plan the policy refuses is stored with its violations", policy_refusal_stored),
        ("the next turn resumes the CLI session", resumes_session),
        ("a plan is dispatched once, after the policy check again", dispatch_once),
        ("a thread persists per project; new starts another", threads),
        ("attachments: type, size, name and link refusals", attachments),
        ("a second message while the lead answers is refused", busy),
        ("bad requests start nothing", refused_requests),
    ];
    let mut failed = 0;
    for (name, test) in tests {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("\nlead: {} passed; {failed} failed", tests.len() - failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-lead-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A home and a project folder beside it (never inside O's state folder).
fn setup(name: &str) -> (PathBuf, String) {
    let base = temp(name);
    let home = base.join("project_O");
    let project = base.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    (home, project.to_string_lossy().into_owned())
}

/// A fake lead CLI printing `lines`; every spec it was asked to launch is kept.
fn fake(lines: Vec<String>) -> (Arc<Spawner>, Arc<Mutex<Vec<LaunchSpec>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let keep = seen.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        keep.lock().unwrap().push(spec.clone());
        let mut command = Command::new(std::env::current_exe()?);
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap());
        Ok(command)
    });
    (spawner, seen)
}

/// Claude stream lines: a session, an answer and its result.
fn claude_answer(session: &str, answer: &str) -> Vec<String> {
    vec![
        json!({ "type": "system", "subtype": "init", "session_id": session }).to_string(),
        json!({ "type": "assistant", "message": { "content": [{ "type": "text", "text": answer }] } }).to_string(),
        json!({ "type": "result", "subtype": "success", "is_error": false, "result": answer,
                "usage": { "input_tokens": 10, "cache_creation_input_tokens": 5, "cache_read_input_tokens": 100, "output_tokens": 20 } })
        .to_string(),
    ]
}

fn plan_answer(plan: Value) -> String {
    format!("카드 한 장으로 나눴습니다.\n- docs: claude, 짧은 문서 작업\n\n```o-plan\n{plan}\n```")
}

fn good_plan() -> Value {
    json!({ "title": "README 한 줄", "cards": [{ "id": "docs", "provider": "claude", "effort": "low", "task": "README.md 끝에 한 줄 추가" }] })
}

fn request<'a>(project: &'a str, text: &'a str) -> SendRequest<'a> {
    SendRequest { project, provider: ProviderId::Claude, model: None, effort: Some("low"), text, attach: &[], fresh: false, clean: None }
}

fn plans(events: &[LeadEvent]) -> Vec<(String, Value, bool, Vec<String>)> {
    events
        .iter()
        .filter_map(|e| match e {
            LeadEvent::Plan { plan_id, plan, ok, violations, .. } => Some((plan_id.clone(), plan.clone(), *ok, violations.iter().map(|v| v.rule.clone()).collect())),
            _ => None,
        })
        .collect()
}

fn plan_blocks() {
    assert_eq!(split_plan("그냥 답입니다").1, Ok(None));
    let (prose, plan) = split_plan(&plan_answer(good_plan()));
    assert_eq!(prose, "카드 한 장으로 나눴습니다.\n- docs: claude, 짧은 문서 작업");
    assert_eq!(plan, Ok(Some(good_plan())));
    assert!(split_plan("```o-plan\n{ not json\n```").1.is_err());
    assert!(split_plan("```o-plan\n[1, 2]\n```").1.is_err());
    let two = format!("{}\n\n```o-plan\n{}\n```", plan_answer(good_plan()), good_plan());
    assert!(split_plan(&two).1.unwrap_err().contains("2개"));
    // Other code blocks are prose.
    let (prose, plan) = split_plan("예:\n```json\n{}\n```");
    assert_eq!((prose.as_str(), plan), ("예:\n```json\n{}\n```", Ok(None)));
}

fn prompts() {
    let input = PromptInput { project: "C:/p", text: "할 일", attachments: &[], budget: &["codex 여유 있음".into()], runs: &[], resumed: false, history: &[] };
    let first = build(&input);
    for part in ["[O 총괄] 프로젝트: C:/p", "```o-plan", "읽기만", "codex 여유 있음", "사용자:\n할 일"] {
        assert!(first.contains(part), "{part}");
    }
    let next = build(&PromptInput { resumed: true, budget: &[], ..input });
    assert!(!next.contains("```o-plan") && next.ends_with("사용자:\n할 일"));
}

fn send_records_a_plan() {
    let (home, project) = setup("send");
    let (spawner, seen) = fake(claude_answer("sess-1", &plan_answer(good_plan())));
    let events = lead::send(&home, &request(&project, "README에 한 줄 추가"), spawner.as_ref()).unwrap();
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            LeadEvent::User { .. } => "user",
            LeadEvent::Session { .. } => "session",
            LeadEvent::Usage { .. } => "usage",
            LeadEvent::Reply { .. } => "reply",
            LeadEvent::Plan { .. } => "plan",
            LeadEvent::Error { .. } => "error",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["user", "session", "usage", "reply", "plan"]);
    let spec = &seen.lock().unwrap()[0];
    assert!(spec.stdin.as_deref().unwrap().contains("README에 한 줄 추가"));
    assert!(!spec.args.contains(&"--resume".to_string()));
    let [(plan_id, plan, ok, violations)] = <[_; 1]>::try_from(plans(&events)).unwrap();
    assert!(ok, "{violations:?}");
    assert!(plan_id.ends_with("-p1"));
    assert_eq!(plan["project"], project.as_str());
    if let Some(LeadEvent::Usage { input, cached, output, .. }) = events.iter().find(|e| matches!(e, LeadEvent::Usage { .. })) {
        assert_eq!((*input, *cached, *output), (15.0, 100.0, 20.0));
    }
    // Saved, and counted as planning overhead.
    let thread = lead::current_thread(&home, &project).unwrap();
    assert_eq!(lead::read_thread(&home, &thread).len(), 6);
    assert_eq!(lead::tokens(&home, Some(&project), None).total, 135);
}

fn project_forced() {
    let (home, project) = setup("forced");
    let mut plan = good_plan();
    plan["project"] = json!("C:/somewhere/else");
    let (spawner, _) = fake(claude_answer("s", &plan_answer(plan)));
    let events = lead::send(&home, &request(&project, "해 줘"), spawner.as_ref()).unwrap();
    assert_eq!(plans(&events)[0].1["project"], project.as_str());
}

fn policy_refusal_stored() {
    let (home, project) = setup("refused");
    let plan = json!({ "title": "t", "cards": [{ "id": "x", "provider": "openclaw", "task": "a", "command": "rm" }] });
    let (spawner, _) = fake(claude_answer("s", &plan_answer(plan)));
    let events = lead::send(&home, &request(&project, "해 줘"), spawner.as_ref()).unwrap();
    let (plan_id, _, ok, rules) = plans(&events).remove(0);
    assert!(!ok);
    assert!(rules.contains(&"unsupported-field".to_string()) && rules.contains(&"unknown-provider".to_string()), "{rules:?}");
    let refused = lead::dispatch(&home, &plan_id, false, &|_| panic!("a refused plan must not start"));
    assert!(matches!(refused, Err(DispatchError::Blocked(v)) if !v.is_empty()));
    // A malformed block is an error on record, not a plan.
    let (spawner, _) = fake(claude_answer("s", "```o-plan\n{ broken\n```"));
    let events = lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("JSON"))));
    assert!(plans(&events).is_empty());
}

fn resumes_session() {
    let (home, project) = setup("resume");
    let (spawner, seen) = fake(claude_answer("sess-7", "어느 파일인가요?"));
    lead::send(&home, &request(&project, "고쳐 줘"), spawner.as_ref()).unwrap();
    lead::send(&home, &request(&project, "README요"), spawner.as_ref()).unwrap();
    let specs = seen.lock().unwrap();
    let args = &specs[1].args;
    assert_eq!(args[args.iter().position(|a| a == "--resume").unwrap() + 1], "sess-7");
    // A resumed session already has the role: only the news and the message go.
    assert!(!specs[1].stdin.as_deref().unwrap().contains("```o-plan"));
    // Another CLI in the same thread starts fresh, with the conversation so far.
    let (codex, seen) = fake(vec![
        json!({ "type": "thread.started", "thread_id": "th-1" }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "agent_message", "text": "네" } }).to_string(),
        json!({ "type": "turn.completed", "usage": { "input_tokens": 3, "cached_input_tokens": 1, "output_tokens": 2 } }).to_string(),
    ]);
    let req = SendRequest { provider: ProviderId::Codex, effort: Some("low"), ..request(&project, "codex로") };
    let events = lead::send(&home, &req, codex.as_ref()).unwrap();
    let spec = &seen.lock().unwrap()[0];
    assert!(!spec.args.contains(&"resume".to_string()));
    assert!(spec.stdin.as_deref().unwrap().contains("어느 파일인가요?"));
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "네")));
}

fn dispatch_once() {
    let (home, project) = setup("dispatch");
    let (spawner, _) = fake(claude_answer("s", &plan_answer(good_plan())));
    let events = lead::send(&home, &request(&project, "해 줘"), spawner.as_ref()).unwrap();
    let plan_id = plans(&events).remove(0).0;
    let started = Mutex::new(Vec::new());
    let start = |run: &str| -> std::io::Result<()> {
        started.lock().unwrap().push(run.to_string());
        Ok(())
    };
    let done = lead::dispatch(&home, &plan_id, true, &start).ok().unwrap();
    assert_eq!(*started.lock().unwrap(), vec![done.run_id.clone()]);
    assert_eq!(done.cards, ["docs"]);
    assert!(home.join("runs").join(&done.run_id).join("plan.json").exists());
    assert!(home.join("runs").join(&done.run_id).join("clean").exists());
    let again = lead::dispatch(&home, &plan_id, false, &start);
    assert!(matches!(again, Err(DispatchError::Other(m)) if m.contains("이미 실행")));
    assert_eq!(started.lock().unwrap().len(), 1);
    assert!(matches!(lead::dispatch(&home, "nope", false, &start), Err(DispatchError::Other(_))));
    // The next turn hears how the dispatched run is doing.
    let (spawner, seen) = fake(claude_answer("s", "확인했습니다"));
    lead::send(&home, &request(&project, "어떻게 됐어?"), spawner.as_ref()).unwrap();
    assert!(seen.lock().unwrap()[0].stdin.as_deref().unwrap().contains(&done.run_id));
}

fn threads() {
    let (home, project) = setup("threads");
    let first = lead::thread_for(&home, &project, false).unwrap();
    assert_eq!(lead::thread_for(&home, &project, false).unwrap(), first);
    // The same folder written another way is the same project.
    assert_eq!(lead::current_thread(&home, &format!("{}/", project.replace('\\', "/"))), Some(first.clone()));
    let second = lead::thread_for(&home, &project, true).unwrap();
    assert_ne!(first, second);
    assert_eq!(lead::current_thread(&home, &project), Some(second.clone()));
    let shown = lead::show(&home, Some(&project));
    assert_eq!(shown["thread"]["threadId"], second.as_str());
    assert_eq!(shown["projects"][0]["project"], project.as_str());
    assert!(lead::show(&home, Some("C:/no/such")).get("thread").unwrap().is_null());
}

fn attachments() {
    let (home, project) = setup("files");
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let saved = files::save(&home, &thread, "../../화면 1.png", b"png").unwrap();
    assert_eq!(saved.name, "01-화면_1.png");
    assert!(saved.image);
    assert!(Path::new(&saved.path).starts_with(files::thread_files(&home, &thread)));
    assert!(!Path::new(&saved.path).starts_with(&home), "attachments stay outside O's state folder");
    assert_eq!(files::save(&home, &thread, "화면 1.png", b"png").unwrap().name, "02-화면_1.png");
    assert!(!files::save(&home, &thread, "notes.md", b"# hi").unwrap().image);
    for bad in ["run.exe", "script.ps1", "noext", ".png", ""] {
        assert!(files::save(&home, &thread, bad, b"x").is_err(), "{bad}");
    }
    assert!(files::save(&home, &thread, "big.png", &vec![0u8; MAX_BYTES as usize + 1]).unwrap_err().contains("10MB"));
    let secret = concat!(".creden", "tials.json");
    assert!(files::save(&home, &thread, secret, b"{}").unwrap_err().contains("로그인"));
    // A file given by path is copied in; one from a sign-in folder never is.
    let outside = PathBuf::from(&project).join("spec.txt");
    std::fs::write(&outside, "spec").unwrap();
    let copied = files::import(&home, &thread, &outside).unwrap();
    assert_ne!(copied.path, outside.to_string_lossy());
    assert_eq!(files::import(&home, &thread, Path::new(&copied.path)).unwrap().path, copied.path);
    let private = PathBuf::from(&project).join(".codex");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("notes.txt"), "x").unwrap();
    assert!(files::import(&home, &thread, &private.join("notes.txt")).is_err());
    // A link is never followed (skipped where this machine may not make links).
    let link = PathBuf::from(&project).join("link.txt");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&outside, &link).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, &link).is_ok();
    if made {
        assert!(files::import(&home, &thread, &link).unwrap_err().contains("링크"));
    }
    // A message carries at most five.
    let six: Vec<PathBuf> = (0..6).map(|_| outside.clone()).collect();
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    let req = SendRequest { attach: &six, ..request(&project, "봐 줘") };
    assert!(lead::send(&home, &req, spawner.as_ref()).is_err());
    let two = [outside.clone(), PathBuf::from(&saved.path)];
    let req = SendRequest { attach: &two, ..request(&project, "봐 줘") };
    lead::send(&home, &req, spawner.as_ref()).unwrap();
    let spec = &seen.lock().unwrap()[0];
    assert!(spec.stdin.as_deref().unwrap().contains(&saved.path));
    assert!(spec.args.contains(&files::thread_files(&home, &thread).to_string_lossy().into_owned()));
}

fn busy() {
    let (home, project) = setup("busy");
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    std::fs::write(&marker, json!({ "pid": std::process::id(), "provider": "claude" }).to_string()).unwrap();
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    assert!(lead::send(&home, &request(&project, "또"), spawner.as_ref()).unwrap_err().contains("답하는 중"));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(lead::show(&home, Some(&project))["thread"]["busy"], "claude");
    // A marker whose process is gone does not hold the thread.
    std::fs::write(&marker, json!({ "pid": 4_000_000_000u32, "provider": "claude" }).to_string()).unwrap();
    lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap();
    assert!(!marker.exists());
}

fn refused_requests() {
    let (home, project) = setup("refuse");
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    let home_text = home.to_string_lossy().into_owned();
    let cases = [
        SendRequest { provider: ProviderId::Agy, effort: None, ..request(&project, "a") },
        SendRequest { effort: Some("minimal"), ..request(&project, "a") },
        request(&project, "   "),
        request("relative/dir", "a"),
        request(&home_text, "a"),
    ];
    for req in cases {
        assert!(lead::send(&home, &req, spawner.as_ref()).is_err());
    }
    let user_home = o_core::run::store::user_home();
    assert!(lead::send(&home, &request(&user_home.to_string_lossy(), "a"), spawner.as_ref()).is_err());
    assert!(seen.lock().unwrap().is_empty());
}
