//! The lead chat with a fake lead CLI. The fake is this test binary itself: started
//! with O_FAKE_LEAD_LINES it prints those lines and exits, so no real (paid) CLI runs.

use std::path::{Path, PathBuf};
mod support;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::io::{BufRead, Write};

use chrono::Utc;
use o_core::lead::files::{self, MAX_BYTES};
use o_core::lead::prompt::{build, split_plan, PromptInput, Verdict};
use o_core::lead::{self, DispatchError, LeadEvent, SendRequest};
use o_core::providers::LaunchSpec;
use o_core::run::plan::ProviderId;
use o_core::run::mode::{Profile, RunMode};
use o_core::run::runner::Spawner;
use serde_json::{json, Value};

const FAKE: &str = "O_FAKE_LEAD_LINES";
const FAKE_APP_SERVER: &str = "O_FAKE_LEAD_APP_SERVER";
const FAKE_LINGER: &str = "O_FAKE_LEAD_LINGER_MS";
const FAKE_SLEEP: &str = "O_FAKE_LEAD_SLEEP_MS";
const FAKE_PAUSE: &str = "O_FAKE_LEAD_PAUSE_EACH_MS";
/// Set for a fake CLI: the file it makes when it runs at all.
const FAKE_RAN: &str = "O_FAKE_LEAD_RAN";
/// Set for the Codex app-server probe: "1" prints the app-server help, "0" the top-level help
/// an older CLI prints for `app-server --help` (it reads `app-server` as a prompt).
const FAKE_PROBE: &str = "O_FAKE_LEAD_PROBE";
/// Set with FAKE_PROBE: a file the probe adds a line to each time it runs.
const FAKE_PROBE_LOG: &str = "O_FAKE_LEAD_PROBE_LOG";
/// Set for this binary as a turn's owner in its own process (the app, standing in): a JSON job.
const OWNER: &str = "O_FAKE_LEAD_OWNER";

fn main() {
    if let Ok(probe) = std::env::var(FAKE_PROBE) {
        if let Some(log) = std::env::var_os(FAKE_PROBE_LOG) {
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(log).unwrap();
            writeln!(file, "probe").unwrap();
        }
        if probe == "1" {
            println!("[experimental] Run the app server or related tooling

Usage: codex app-server [OPTIONS] [COMMAND]");
        } else {
            println!("Codex CLI

Usage: codex [OPTIONS] [PROMPT]
       codex [OPTIONS] <COMMAND> [ARGS]");
        }
        return;
    }
    if let Ok(lines) = std::env::var(FAKE) {
        let lines: Vec<String> = serde_json::from_str(&lines).expect("fake lines");
        if std::env::var_os(FAKE_APP_SERVER).is_some() { return fake_app_server(lines); }
        if let Ok(ran) = std::env::var(FAKE_RAN) {
            std::fs::write(ran, "ran").unwrap();
        }
        // A slow CLI (a turn still answering) waits before it answers.
        if let Some(ms) = std::env::var(FAKE_SLEEP).ok().and_then(|v| v.parse().ok()) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        // "{keys}" in a line becomes the API key variables this process inherited.
        let watched = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CODEX_API_KEY"];
        let keys: Vec<String> = std::env::vars().map(|(k, _)| k).filter(|k| watched.iter().any(|n| n.eq_ignore_ascii_case(k))).collect();
        for line in lines {
            println!("{}", line.replace("{keys}", &keys.join(",")).replace("{agent-role}", &std::env::var("O_AGENT_ROLE").unwrap_or_default()));
            if let Some(ms) = std::env::var(FAKE_PAUSE).ok().and_then(|v| v.parse().ok()) {
                std::thread::sleep(std::time::Duration::from_millis(ms));
            }
        }
        return;
    }
    if let Ok(job) = std::env::var(OWNER) {
        return owner_process(&job);
    }
    // Normal-mode turns find `o` beside the running program for their approvals; whether a
    // built `o` happens to sit beside this test binary differs by OS, so name one for every test.
    lead::approvals::set_program(Some(std::env::current_exe().unwrap()));
    let tests: [(&str, fn()); 41] = [
        ("follow-ups only target cards dispatched by this thread", followup_routing),
        ("a plan for a non-Git project can be dispatched", dispatch_non_git),
        ("tool progress is visible during the turn and cleared afterwards", live_tool_progress),
        ("design snapshots persist and malformed ones do not dispatch", design_snapshots),
        ("design notes persist, reach the lead and preserve concurrent edits", design_notes),
        ("only the latest plan in an open thread can dispatch", stale_plan_refused),
        ("automatic dispatch respects mode, running work and the daily budget", autorun),
        ("only scheduled dispatch records scheduled in the runner's mode event", scheduled_run_mode_recorded),
        ("an automatic dispatch that fails to start uses up no auto-run limit", autorun_failures_not_counted),
        ("opening another tab preserves the live tab's automatic dispatch", autorun_new_thread_cancels),
        ("an automatic dispatch never starts a plan a newer request replaced", autorun_newer_request_wins),
        ("lead child receives its agent marker", lead_child_marker),
        ("the subscription billing mode refuses a lead turn whose CLI would get an API key", lead_subscription_mode),
        ("a plan block is read: none, one, malformed, two", plan_blocks),
        ("the first turn tells the role and schema; a resumed one only the news", prompts),
        ("a turn records the message, session, tokens, reply and a checked plan", send_records_a_plan),
        ("a Markdown reply keeps every line break from the CLI stream to the thread file", reply_keeps_lines),
        ("a Codex reply keeps its reported file changes", reply_reported_files),
        ("the plan's project is forced to the thread's", project_forced),
        ("a plan the policy refuses is stored with its violations", policy_refusal_stored),
        ("the next turn resumes the CLI session", resumes_session),
        ("a plan is dispatched once, after the policy check again", dispatch_once),
        ("a thread persists per project; new starts another", threads),
        ("legacy current state migrates without changing the writer", thread_migration),
        ("concurrent tab creation respects the four-tab limit", concurrent_open_limit),
        ("different tabs answer together and each excludes a second turn", parallel_tabs),
        ("design-only tabs remain read-only even in YOLO", nonwriter_read_only),
        ("tab titles and project state aggregate waiting before working", tab_states),
        ("attachments: type, size, name and link refusals", attachments),
        ("sends started together run one turn; the others are refused", busy),
        ("a dead turn's marker remains unknown, even with its pid reused", stale_marker),
        ("bad requests start nothing", refused_requests),
        ("the lead CLI inherits API key variables", api_keys_inherited),
        ("no project hook runs before a plan is approved, first turn or resumed", project_hooks_off),
        ("an attachment reached through a linked parent folder is refused", attachment_parent_links),
        ("a stop while the turn starts keeps the CLI from running and never ends the owner", stop_while_starting),
        ("a stop ends a running CLI but does not trust an orphan's pid", stop_running_and_orphans),
        ("a turn that outlasts its time limit is ended and says so", turn_time_limit),
        ("back from another agent, a resumed session hears the turns it missed", round_trip),
        ("the thread list is written whole and a broken one is never overwritten", current_list),
        ("the lead's CLI sessions are listed from a small index, not every thread's events", session_index),
    ];
    let extra: [(&str, fn()); 13] = [
        ("the overall lead designs and proposes; the person sends requests on to listed projects, once each", overall_forwards),
        ("a request whose forward did not start can be sent again, alone and once (#96)", overall_forward_retries),
        ("stopped or closed overall threads do not deliver pending requests", overall_cancelled_forwards),
        ("the overall lead's approval runs the plan; a revision goes back at most twice", overall_reviews),
        ("an approved plan still keeps to the auto-run limits and a newer request", overall_review_limits),
        ("stop targets one tab, one project, or all open tabs", stop_scopes),
        ("questions stand in for the answer; the next turn carries the picks", ask_questions),
        ("a Codex CLI without app-server leads through read-only exec, probed once, resuming across transports", codex_exec_fallback),
        ("Codex reports an interrupted turn rather than its partial answer", codex_interrupted),
        ("Codex asks the person, declines what they refuse and continues", codex_approval),
        ("Codex shows partial prose while keeping the plan hidden", codex_partial),
        ("Codex refuses other server requests and the turn continues", codex_other_requests),
        ("a finished Codex turn survives a slow app-server shutdown", codex_slow_shutdown),
    ];
    let total = tests.len() + extra.len();
    let mut failed = 0;
    for (name, test) in tests.into_iter().chain(extra) {
        let ok = std::panic::catch_unwind(test).is_ok();
        println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
        failed += usize::from(!ok);
    }
    println!("\nlead: {} passed; {failed} failed", total - failed);
    if failed > 0 {
        std::process::exit(1);
    }
}

fn followup_routing() {
    let (home, project) = setup("followup-routing");
    let started = Mutex::new(Vec::new());
    let resume = |id: &str| { started.lock().unwrap().push(id.to_string()); Ok(()) };
    // Nothing dispatched in this thread yet: a follow-up is not a way around "실행".
    let early = "```o-followup\n[{\"run\":\"20260101-000000-abcd\",\"card\":\"docs\",\"task\":\"수정\"}]\n```";
    let (spawner, _) = fake(claude_answer("s", early));
    let refused = lead::send_with_followups(&home, &request(&project, "수정해줘"), spawner.as_ref(), &resume).unwrap();
    assert!(refused.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("이 대화"))));
    assert!(started.lock().unwrap().is_empty());
    let (spawner, _) = fake(claude_answer("s", &plan_answer(good_plan())));
    let events = lead::send(&home, &request(&project, "설계"), spawner.as_ref()).unwrap();
    let original = lead::current_thread(&home, &project).unwrap();
    let plan_id = plans(&events).remove(0).0;
    let dispatched = lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &|_| Ok(())).ok().unwrap();
    let answer = format!("수정 요청을 전달합니다.\n```o-followup\n[{{\"run\":\"{}\",\"card\":\"docs\",\"task\":\"제목 수정\"}}]\n```", dispatched.run_id);
    let (entered, release) = (home.join("followup-entered"), home.join("followup-release"));
    let spawner = gated(claude_answer("s", &answer), entered.clone(), release.clone());
    let req = SendRequest { thread: Some(&original), ..request(&project, "제목 수정해줘") };
    let result = std::thread::scope(|scope| {
        let turn = scope.spawn(|| lead::send_with_followups(&home, &req, spawner.as_ref(), &resume));
        wait_until("the follow-up turn", || entered.exists());
        lead::new_thread(&home, &project).unwrap();
        std::fs::write(&release, "").unwrap();
        turn.join().unwrap().unwrap()
    });
    assert!(result.iter().any(|e| matches!(e, LeadEvent::Followup { source_run_id, card_id, .. } if source_run_id == &dispatched.run_id && card_id == "docs")));
    assert_eq!(started.lock().unwrap().len(), 1);
    let (bad, _) = fake(claude_answer("s", "```o-followup\n[{\"run\":\"foreign\",\"card\":\"docs\",\"task\":\"수정\"}]\n```"));
    let refused = lead::send_with_followups(&home, &SendRequest { thread: Some(&original), ..request(&project, "수정해줘") }, bad.as_ref(), &resume).unwrap();
    assert!(refused.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("이 대화"))));
    assert_eq!(started.lock().unwrap().len(), 1);
    let followup_run = started.lock().unwrap()[0].clone();
    assert_eq!(lead::stop_all(&home, &project).unwrap(), 2);
    assert!(o_core::run::store::stop_requested(&home, &followup_run));
    // Open tabs keep their own dispatch scope; another tab still cannot borrow it.
    lead::new_thread(&home, &project).unwrap();
    let (spawner, _) = fake(claude_answer("s2", &answer));
    let refused = lead::send_with_followups(&home, &request(&project, "제목 수정해줘"), spawner.as_ref(), &resume).unwrap();
    assert!(refused.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("이 대화"))));
    assert_eq!(started.lock().unwrap().len(), 1);
    lead::close(&home, &original).unwrap();
    let (spawner, seen) = fake(claude_answer("s", &answer));
    assert!(lead::send_with_followups(&home, &SendRequest { thread: Some(&original), ..request(&project, "닫힌 탭") }, spawner.as_ref(), &resume).is_err());
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(started.lock().unwrap().len(), 1);
    let example = "````markdown\n```o-followup\n[]\n```\n````";
    assert!(o_core::lead::prompt::split_followups(example).1.unwrap().is_none());
}

/// A home beside a projects root holding "alpha" and "beta"; the root is the overall lead's folder.
fn setup_overall(name: &str) -> (PathBuf, String, String, String) {
    let base = temp(name);
    let home = base.join("project_O");
    let root = base.join("projects");
    for dir in [&home, &root.join("alpha"), &root.join("beta")] { std::fs::create_dir_all(dir).unwrap(); }
    let listing = o_core::projects::set_root(&home, &o_core::run::store::user_home(), Some(&root.to_string_lossy())).unwrap();
    let path = |name: &str| listing.projects.iter().find(|p| p.name == name).unwrap().path.clone();
    (home, listing.root.clone(), path("alpha"), path("beta"))
}

fn forward_block(items: &[(&str, &str)]) -> String {
    let items: Vec<Value> = items.iter().map(|(project, text)| json!({"project": project, "text": text})).collect();
    format!("두 프로젝트에 넘깁니다.\n```o-forward\n{}\n```", Value::from(items))
}

/// The overall lead's latest proposal of requests (`lead.forward-plan`).
fn proposal(events: &[LeadEvent]) -> String {
    events.iter().rev().find_map(|e| match e { LeadEvent::ForwardPlan { proposal_id, .. } => Some(proposal_id.clone()), _ => None }).expect("a proposal")
}

fn review_answer(verdict: &str, notes: &str) -> String {
    format!("검토했습니다.\n```o-review\n{}\n```", json!({ "verdict": verdict, "notes": notes }))
}

/// A fake lead CLI answering by the folder it runs in: `projects` (the overall lead) or a project's name.
fn fake_by_folder(answers: Vec<(&'static str, Vec<String>)>) -> (Arc<Spawner>, Arc<Mutex<Vec<LaunchSpec>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let keep = seen.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        if let Some(command) = probe_command(spec, cwd, &APP_SERVER) { return command; }
        keep.lock().unwrap().push(spec.clone());
        let lines = answers.iter().find(|(name, _)| Path::new(cwd).file_name().is_some_and(|n| n == *name)).map(|(_, lines)| lines.clone()).unwrap_or_default();
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap()).env(FAKE_SLEEP, "0");
        Ok(command)
    });
    (spawner, seen)
}

/// What `o _lead-forward` would start next: runs, and requests passed on again.
#[derive(Default)]
struct Started {
    runs: Mutex<Vec<String>>,
    forwards: Mutex<Vec<(String, usize)>>,
}

impl Started {
    fn run(&self, id: &str) -> std::io::Result<()> { self.runs.lock().unwrap().push(id.to_string()); Ok(()) }
    fn forward(&self, id: &str, n: usize) -> std::io::Result<()> { self.forwards.lock().unwrap().push((id.to_string(), n)); Ok(()) }
}

/// `overall::forward` with its starts recorded in `started`.
fn forward_with(home: &Path, thread: &str, index: usize, spawner: &Spawner, started: &Started) -> Result<Vec<LeadEvent>, String> {
    let run = |id: &str| started.run(id);
    let forward = |id: &str, n: usize| started.forward(id, n);
    lead::overall::forward(home, thread, index, spawner, &lead::overall::Starts { run: &run, forward: &forward })
}

fn overall_forwards() {
    let (home, root, alpha, beta) = setup_overall("overall");
    assert!(lead::overall::is_overall(&home, &root) && !lead::overall::is_overall(&home, &alpha));
    // The overall lead sees every project, never runs in YOLO, designs and proposes; nothing goes on yet.
    let design = "설계했습니다.\n```o-design\n{\"goal\":\"둘 다 정리\",\"decisions\":[\"alpha 먼저\"],\"questions\":[]}\n```";
    let answer = format!("{design}\n{}", forward_block(&[(&alpha, "로그인 화면 정리"), (&beta, "테스트 보강")]));
    let (spawner, seen) = fake(claude_answer("o1", &answer));
    let events = lead::send(&home, &SendRequest { yolo: true, ..request(&root, "둘 다 진행해") }, spawner.as_ref()).unwrap();
    let prompt = seen.lock().unwrap()[0].stdin.clone().unwrap();
    assert!(prompt.contains("[O 전체 총괄]") && prompt.contains("alpha") && prompt.contains("beta") && !prompt.contains("YOLO"));
    assert!(events.iter().any(|e| matches!(e, LeadEvent::User { yolo: false, origin: None, .. })));
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Design { design, .. } if design["goal"] == "둘 다 정리")));
    assert!(!events.iter().any(|e| matches!(e, LeadEvent::Forward { .. })));
    let thread = lead::current_thread(&home, &root).unwrap();
    let first = proposal(&events);
    assert_eq!(first, format!("{thread}-f1"));
    // The next turn hears the saved design; the tab waits for the person.
    let (spawner, seen) = fake(claude_answer("o1", "네"));
    let later = lead::send(&home, &request(&root, "설계 다시 보여줘"), spawner.as_ref()).unwrap();
    assert!(seen.lock().unwrap()[0].stdin.as_deref().unwrap().contains("둘 다 정리"));
    // A word from the person after the proposal makes it stale; a new proposal is sent on, once.
    assert!(lead::overall::send_on(&home, &first, &|_, _| Ok(())).unwrap_err().contains("바뀌었습니다"));
    assert!(!later.iter().any(|e| matches!(e, LeadEvent::ForwardPlan { .. })));
    let (spawner, _) = fake(claude_answer("o1", &answer));
    let events = lead::send(&home, &request(&root, "다시 제안해"), spawner.as_ref()).unwrap();
    let second = proposal(&events);
    let shown = lead::show(&home, Some(&root), None).unwrap();
    assert_eq!(shown["threads"][0]["state"], "waiting");
    let started = Started::default();
    let sent = lead::overall::send_on(&home, &second, &|id, n| started.forward(id, n)).unwrap();
    let forwards: Vec<(String, String, Option<String>)> = sent.iter().filter_map(|e| match e { LeadEvent::Forward { project, text, proposal, .. } => Some((project.clone(), text.clone(), proposal.clone())), _ => None }).collect();
    assert_eq!(forwards, [(alpha.clone(), "로그인 화면 정리".into(), Some(second.clone())), (beta.clone(), "테스트 보강".into(), Some(second.clone()))]);
    assert_eq!(*started.forwards.lock().unwrap(), [(thread.clone(), 0), (thread.clone(), 1)]);
    assert!(lead::overall::send_on(&home, &second, &|_, _| Ok(())).unwrap_err().contains("이미"));
    assert!(lead::overall::send_on(&home, "not-a-proposal", &|_, _| Ok(())).is_err());

    // The project lead gets it marked as the overall lead's, once. A review without its block
    // approves nothing: the plan waits for "실행".
    let (spawner, seen) = fake_by_folder(vec![("alpha", claude_answer("p1", &plan_answer(good_plan()))), ("projects", claude_answer("o1", "좋아 보입니다"))]);
    let sent = forward_with(&home, &thread, 0, spawner.as_ref(), &started).unwrap();
    let prompts: Vec<String> = seen.lock().unwrap().iter().map(|s| s.stdin.clone().unwrap()).collect();
    assert!(prompts[0].contains("[전체 총괄이 넘긴 요청"));
    assert!(prompts[1].contains("[O 검토 요청]") && prompts[1].contains("README 한 줄") && prompts[1].contains("로그인 화면 정리"));
    assert!(sent.iter().any(|e| matches!(e, LeadEvent::User { origin: Some(o), text, .. } if o == "overall" && text == "로그인 화면 정리")));
    assert_eq!(plans(&sent).len(), 1);
    let overall_events = lead::read_thread(&home, &thread);
    assert!(overall_events.iter().any(|e| matches!(e, LeadEvent::User { origin: Some(o), .. } if o == "review")));
    assert!(overall_events.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("o-review"))));
    assert!(!overall_events.iter().any(|e| matches!(e, LeadEvent::Review { .. })));
    assert!(started.runs.lock().unwrap().is_empty());
    assert!(forward_with(&home, &thread, 0, spawner.as_ref(), &started).unwrap_err().contains("이미"));
    assert!(forward_with(&home, &thread, 5, spawner.as_ref(), &started).is_err());
    // The overview shows the plan waiting for the person on the next overall turn.
    assert!(lead::overall::overview(&home).contains("실행하지 않은 계획"));

    // What the code refuses, whatever the model writes.
    let refused = |answer: String, word: &str| {
        let (spawner, _) = fake(claude_answer("o1", &answer));
        let events = lead::send(&home, &request(&root, "진행"), spawner.as_ref()).unwrap();
        assert!(!events.iter().any(|e| matches!(e, LeadEvent::Forward { .. } | LeadEvent::ForwardPlan { .. } | LeadEvent::Plan { .. } | LeadEvent::Review { .. })), "{answer}");
        assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains(word))), "{answer}: {events:?}");
    };
    refused(plan_answer(good_plan()), "계획");
    refused(review_answer("approve", "좋음"), "검토 블록");
    refused(forward_block(&[("C:/elsewhere", "일")]), "목록에 없는");
    refused(forward_block(&[(&root, "일")]), "목록에 없는");
    refused(forward_block(&[(&alpha, "하나"), (&alpha, "둘")]), "하나만");
    refused(forward_block(&[(&alpha, "1"), (&beta, "2"), (&alpha, "3"), (&beta, "4")]), "1~3");
    assert!(forward_with(&home, &lead::current_thread(&home, &alpha).unwrap(), 0, spawner.as_ref(), &started).unwrap_err().contains("전체 총괄 대화가 아닙니다"));
}

fn overall_forward_retries() {
    let (home, root, alpha, beta) = setup_overall("overall-retry");
    let (spawner, _) = fake(claude_answer("o1", &forward_block(&[(&alpha, "로그인 화면 정리"), (&beta, "테스트 보강")])));
    let events = lead::send(&home, &request(&root, "둘 다 진행해"), spawner.as_ref()).unwrap();
    let id = proposal(&events);
    let thread = lead::current_thread(&home, &root).unwrap();
    let state = || lead::show(&home, Some(&root), None).unwrap()["threads"][0]["state"].clone();
    let failures = |events: &[LeadEvent]| -> Vec<usize> { events.iter().filter_map(|e| match e { LeadEvent::ForwardStart { index, error: Some(_), .. } => Some(*index), _ => None }).collect() };

    // The first request starts, the second does not: it alone is recorded as not started.
    let started = Started::default();
    let second_fails = |id: &str, n: usize| if n == 1 { Err(std::io::Error::other("o CLI가 맞지 않습니다")) } else { started.forward(id, n) };
    let sent = lead::overall::send_on(&home, &id, &second_fails).unwrap();
    assert_eq!(failures(&sent), [1]);
    assert_eq!(*started.forwards.lock().unwrap(), [(thread.clone(), 0)]);
    assert_eq!(lead::overall::unstarted(&lead::read_thread(&home, &thread)).into_iter().collect::<Vec<_>>(), [1]);
    assert_eq!(state(), "waiting");
    // Still failing: it stays as it was. The cause gone: the second goes, alone, once.
    assert_eq!(failures(&lead::overall::send_on(&home, &id, &second_fails).unwrap()), [1]);
    let again = lead::overall::send_on(&home, &id, &|id, n| started.forward(id, n)).unwrap();
    assert!(matches!(again.as_slice(), [LeadEvent::ForwardStart { index: 1, error: None, .. }]), "{again:?}");
    assert_eq!(*started.forwards.lock().unwrap(), [(thread.clone(), 0), (thread.clone(), 1)]);
    assert!(lead::overall::unstarted(&lead::read_thread(&home, &thread)).is_empty());
    assert!(lead::overall::send_on(&home, &id, &|id, n| started.forward(id, n)).unwrap_err().contains("이미"));
    assert_eq!(started.forwards.lock().unwrap().len(), 2);
    assert_eq!(lead::read_thread(&home, &thread).iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).count(), 2);

    // Every start failing: all go again, at once from two sides each is started once.
    let (spawner, _) = fake(claude_answer("o1", &forward_block(&[(&alpha, "하나"), (&beta, "둘")])));
    let events = lead::send(&home, &request(&root, "다시 제안해"), spawner.as_ref()).unwrap();
    let id = proposal(&events);
    assert_eq!(failures(&lead::overall::send_on(&home, &id, &|_, _| Err(std::io::Error::other("없음"))).unwrap()), [2, 3]);
    let started = Started::default();
    let results: Vec<bool> = std::thread::scope(|s| {
        let retries: Vec<_> = (0..2).map(|_| s.spawn(|| lead::overall::send_on(&home, &id, &|id, n| started.forward(id, n)).is_ok())).collect();
        retries.into_iter().map(|retry| retry.join().unwrap()).collect()
    });
    assert_eq!(results.iter().filter(|ok| **ok).count(), 1, "{results:?}");
    let mut forwards = started.forwards.lock().unwrap().clone();
    forwards.sort();
    assert_eq!(forwards, [(thread.clone(), 2), (thread.clone(), 3)]);

    // A request `o _lead-forward` already took is not started again, whatever the record says.
    let (spawner, _) = fake(claude_answer("o1", &forward_block(&[(&alpha, "셋")])));
    let events = lead::send(&home, &request(&root, "하나 더"), spawner.as_ref()).unwrap();
    let id = proposal(&events);
    lead::overall::send_on(&home, &id, &|_, _| Err(std::io::Error::other("없음"))).unwrap();
    let (project, _) = fake_by_folder(vec![("alpha", claude_answer("p1", "네"))]);
    forward_with(&home, &thread, 4, project.as_ref(), &started).unwrap();
    assert!(lead::overall::send_on(&home, &id, &|id, n| started.forward(id, n)).unwrap_err().contains("이미"));
    assert_eq!(started.forwards.lock().unwrap().len(), 2);
}

fn overall_reviews() {
    let (home, root, alpha, beta) = setup_overall("overall-review");
    init_git(&alpha);
    let (source, _) = fake(claude_answer("o", &forward_block(&[(&alpha, "README 정리"), (&beta, "테스트 보강")])));
    let events = lead::send(&home, &request(&root, "진행"), source.as_ref()).unwrap();
    lead::overall::send_on(&home, &proposal(&events), &|_, _| Ok(())).unwrap();
    let thread = lead::current_thread(&home, &root).unwrap();

    // Approved: the plan runs at once, with YOLO off.
    let started = Started::default();
    let (spawner, _) = fake_by_folder(vec![("alpha", claude_answer("p", &plan_answer(good_plan()))), ("projects", claude_answer("o", &review_answer("approve", "설계와 맞음")))]);
    let sent = forward_with(&home, &thread, 0, spawner.as_ref(), &started).unwrap();
    let plan_id = plans(&sent).remove(0).0;
    assert_eq!(started.runs.lock().unwrap().len(), 1);
    let project = lead::read_thread(&home, &lead::current_thread(&home, &alpha).unwrap());
    assert!(project.iter().any(|e| matches!(e, LeadEvent::Review { plan_id: id, verdict: Verdict::Approve, notes, .. } if *id == plan_id && notes == "설계와 맞음")));
    assert!(project.iter().any(|e| matches!(e, LeadEvent::Dispatch { plan_id: id, yolo: false, .. } if *id == plan_id)));
    assert!(lead::read_thread(&home, &thread).iter().any(|e| matches!(e, LeadEvent::Review { verdict: Verdict::Approve, .. })));

    // Revise: the notes go back to the thread that made the plan, at most twice.
    let (spawner, seen) = fake_by_folder(vec![("beta", claude_answer("p", &plan_answer(good_plan()))), ("projects", claude_answer("o", &review_answer("revise", "테스트 범위를 넓혀 주세요")))]);
    forward_with(&home, &thread, 1, spawner.as_ref(), &started).unwrap();
    assert_eq!(*started.forwards.lock().unwrap(), [(thread.clone(), 2)]);
    let revision = lead::read_thread(&home, &thread).into_iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).nth(2).unwrap();
    assert!(matches!(&revision, LeadEvent::Forward { text, revision: Some(_), proposal: Some(_), .. } if text == "테스트 범위를 넓혀 주세요"));
    forward_with(&home, &thread, 2, spawner.as_ref(), &started).unwrap();
    assert!(seen.lock().unwrap().iter().any(|s| s.stdin.as_deref().unwrap().contains("[전체 총괄이 넘긴 요청") && s.stdin.as_deref().unwrap().contains("테스트 범위를 넓혀")));
    forward_with(&home, &thread, 3, spawner.as_ref(), &started).unwrap();
    assert_eq!(started.forwards.lock().unwrap().len(), 2);
    assert!(lead::read_thread(&home, &thread).iter().any(|e| matches!(e, LeadEvent::Warning { text, .. } if text.contains("2번을 넘어"))));
    let beta_thread = lead::current_thread(&home, &beta).unwrap();
    let beta_events = lead::read_thread(&home, &beta_thread);
    assert_eq!(beta_events.iter().filter(|e| matches!(e, LeadEvent::Review { verdict: Verdict::Revise, .. })).count(), 3);
    assert!(!beta_events.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    assert_eq!(started.runs.lock().unwrap().len(), 1);
}

fn overall_review_limits() {
    let (home, root, alpha, _) = setup_overall("overall-review-limits");
    init_git(&alpha);
    let (source, _) = fake(claude_answer("o", &forward_block(&[(&alpha, "README 정리")])));
    let send_on = |text: &str| {
        let events = lead::send(&home, &request(&root, text), source.as_ref()).unwrap();
        lead::overall::send_on(&home, &proposal(&events), &|_, _| Ok(())).unwrap();
        lead::read_thread(&home, &lead::current_thread(&home, &root).unwrap()).iter().filter(|e| matches!(e, LeadEvent::Forward { .. })).count() - 1
    };
    let thread = |home: &Path| lead::current_thread(home, &root).unwrap();
    let started = Started::default();
    let (spawner, _) = fake_by_folder(vec![("alpha", claude_answer("p", &plan_answer(good_plan()))), ("projects", claude_answer("o", &review_answer("approve", "")))]);
    // The daily budget off: approved, yet not run.
    o_core::usage::budget::write_enabled(&home, false).unwrap();
    let index = send_on("진행");
    forward_with(&home, &thread(&home), index, spawner.as_ref(), &started).unwrap();
    assert!(started.runs.lock().unwrap().is_empty());
    let project = lead::read_thread(&home, &lead::current_thread(&home, &alpha).unwrap());
    assert!(auto_reason(&project, "예산"));
    assert!(lead::read_thread(&home, &thread(&home)).iter().any(|e| matches!(e, LeadEvent::Warning { text, .. } if text.contains("승인한 계획을 실행하지 않았습니다"))));
    // A stopped overall thread applies no verdict.
    o_core::usage::budget::write_enabled(&home, true).unwrap();
    let index = send_on("다시");
    let overall = thread(&home);
    let stop = lead::thread_dir(&home, &overall).join("stop-all-at");
    let (stopper, _) = fake_by_folder(vec![("alpha", claude_answer("p", &plan_answer(good_plan()))), ("projects", claude_answer("o", &review_answer("approve", "")))]);
    let stopping: Arc<Spawner> = {
        let stop = stop.clone();
        Arc::new(move |spec: &LaunchSpec, cwd: &str| {
            if Path::new(cwd).file_name().is_some_and(|n| n == "projects") { std::fs::write(&stop, (Utc::now() + chrono::Duration::seconds(1)).to_rfc3339()).unwrap(); }
            stopper(spec, cwd)
        })
    };
    forward_with(&home, &overall, index, stopping.as_ref(), &started).unwrap();
    assert!(started.runs.lock().unwrap().is_empty());
    assert!(lead::read_thread(&home, &overall).iter().any(|e| matches!(e, LeadEvent::Warning { text, .. } if text.contains("중지"))));
}

fn overall_cancelled_forwards() {
    for replace in [false, true] {
        let (home, root, alpha, _) = setup_overall(if replace { "forward-replaced" } else { "forward-stopped" });
        let (source, _) = fake(claude_answer("o", &forward_block(&[(&alpha, "작업")])));
        let events = lead::send(&home, &request(&root, "진행"), source.as_ref()).unwrap();
        lead::overall::send_on(&home, &proposal(&events), &|_, _| Ok(())).unwrap();
        let thread = lead::current_thread(&home, &root).unwrap();
        let target = lead::new_thread(&home, &alpha).unwrap();
        let (spawner, seen) = fake(claude_answer("p", "완료"));
        let started = Started::default();

        if replace {
            lead::new_thread(&home, &root).unwrap();
            let sent = forward_with(&home, &thread, 0, spawner.as_ref(), &started).unwrap();
            assert!(sent.iter().any(|e| matches!(e, LeadEvent::User { origin: Some(origin), .. } if origin == "overall")));
            let events = lead::send(&home, &SendRequest { thread: Some(&thread), ..request(&root, "다음 전달") }, source.as_ref()).unwrap();
            lead::overall::send_on(&home, &proposal(&events), &|_, _| Ok(())).unwrap();
            lead::close(&home, &thread).unwrap();
            let (spawner, seen) = fake(claude_answer("p", "must not run"));
            assert!(forward_with(&home, &thread, 1, spawner.as_ref(), &started).is_err());
            assert!(seen.lock().unwrap().is_empty());
            continue;
        } else {
            let lock = std::fs::OpenOptions::new().read(true).write(true).open(home.join("leads/current.lock")).unwrap();
            lock.lock().unwrap();
            std::thread::scope(|scope| {
                let (ready, wait) = std::sync::mpsc::channel();
                let (home, thread, spawner, started) = (&home, &thread, &spawner, &started);
                let forward = scope.spawn(move || {
                    ready.send(()).unwrap();
                    forward_with(home, thread, 0, spawner.as_ref(), started)
                });
                wait.recv().unwrap();
                // Cancellation reaches the queued forward before it can acquire current.lock.
                std::fs::write(lead::thread_dir(home, thread).join("stop-all-at"), Utc::now().to_rfc3339()).unwrap();
                drop(lock);
                assert!(forward.join().unwrap().is_err());
            });
        }
        assert!(seen.lock().unwrap().is_empty());
        assert!(!lead::read_thread(&home, &target).iter().any(|e| matches!(e, LeadEvent::User { origin: Some(_), .. })));

        // A later turn remains deliverable after stopping the preceding one.
        if !replace {
            let stopped = chrono::DateTime::parse_from_rfc3339(&std::fs::read_to_string(lead::thread_dir(&home, &thread).join("stop-all-at")).unwrap()).unwrap();
            wait_until("the next turn's timestamp after cancellation", || Utc::now().timestamp_millis() > stopped.timestamp_millis());
            let events = lead::send(&home, &request(&root, "다시 진행"), source.as_ref()).unwrap();
            lead::overall::send_on(&home, &proposal(&events), &|_, _| Ok(())).unwrap();
            let sent = forward_with(&home, &thread, 1, spawner.as_ref(), &started).unwrap();
            assert_eq!(sent.iter().filter(|e| matches!(e, LeadEvent::User { origin: Some(o), .. } if o == "overall")).count(), 1);
            assert_eq!(seen.lock().unwrap().len(), 1);
        }
    }
}

fn design_snapshots() {
    let (home, project) = setup("design");
    let answer = "설계했습니다.\n```o-design\n{\"goal\":\"게임\",\"decisions\":[\"2D\"],\"questions\":[\"점수\"]}\n```";
    let (spawner, _) = fake(claude_answer("s", answer));
    let events = lead::send(&home, &request(&project, "설계"), spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Design { design, .. } if design["goal"] == "게임")));
    let (next, seen) = fake(claude_answer("s", "확인"));
    lead::send(&home, &request(&project, "계속"), next.as_ref()).unwrap();
    assert!(seen.lock().unwrap()[0].stdin.as_deref().unwrap().contains("게임"));
    let broken = format!("```o-design\n{{}}\n```\n{}", plan_answer(good_plan()));
    let (spawner, _) = fake(claude_answer("s", &broken));
    let events = lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { .. })));
    assert!(!events.iter().any(|e| matches!(e, LeadEvent::Plan { .. } | LeadEvent::Design { .. })));
}

fn ask_questions() {
    let (home, project) = setup("ask");
    let ask = r#"{"questions":[{"header":"형태","question":"스킬의 형태는?","options":[{"label":"MCP 서버 (추천)","description":"여러 CLI에서 같이"},{"label":"SKILL.md"}]},{"question":"언어는?","options":[{"label":"Python"},{"label":"C#"}],"multi":true}]}"#;
    // A plan beside the questions is not taken: the person has not chosen yet.
    let answer = format!("먼저 확인할 것이 있습니다.\n```o-ask\n{ask}\n```\n{}", plan_answer(good_plan()));
    let (spawner, _) = fake(claude_answer("s", &answer));
    let events = lead::send(&home, &request(&project, "스킬 만들어 줘"), spawner.as_ref()).unwrap();
    let asked = events.iter().find_map(|e| match e { LeadEvent::Ask { ask_id, intro, questions, .. } => Some((ask_id.clone(), intro.clone(), questions.clone())), _ => None }).expect("질문");
    assert!(asked.0.ends_with("-q1"));
    assert!(asked.1.starts_with("먼저 확인할 것이 있습니다.") && !asked.1.contains("o-plan"), "{}", asked.1);
    assert_eq!(asked.2.len(), 2);
    assert!(asked.2[1].multi);
    assert!(!events.iter().any(|e| matches!(e, LeadEvent::Reply { .. } | LeadEvent::Plan { .. })));
    // The thread file keeps it, and the next turn's history (a fresh agent) names the questions.
    let thread = lead::current_thread(&home, &project).unwrap();
    assert!(lead::read_thread(&home, &thread).iter().any(|e| matches!(e, LeadEvent::Ask { .. })));
    let codex_lines = vec![json!({ "type": "thread.started", "thread_id": "th-ask" }).to_string(), json!({ "type": "turn.completed", "usage": {} }).to_string()];
    let (codex, seen) = fake_without_app_server(codex_lines, "ask", home.join("ask-probe.log"));
    lead::send(&home, &SendRequest { provider: ProviderId::Codex, ..request(&project, "[질문 답변]\n- 형태: MCP 서버") }, codex.as_ref()).unwrap();
    // The app-server probe comes first and has no prompt; the turn's own launch does.
    let stdin = seen.lock().unwrap().iter().rev().find_map(|spec| spec.stdin.clone()).unwrap_or_default();
    assert!(stdin.contains("스킬의 형태는?") && stdin.contains("o-ask"), "{stdin}");
    // Malformed questions are refused with the prose kept.
    for broken in [r#"{"questions":[]}"#, r#"{"questions":[{"question":"하나뿐","options":[{"label":"A"}]}]}"#, "not json"] {
        let (spawner, _) = fake(claude_answer("s", &format!("물어볼게요\n```o-ask\n{broken}\n```")));
        let events = lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap();
        assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { .. })), "{broken}");
        assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "물어볼게요")), "{broken}");
        assert!(!events.iter().any(|e| matches!(e, LeadEvent::Ask { .. })), "{broken}");
    }
    // An example inside another fence stays prose.
    let example = "````markdown\n```o-ask\n[]\n```\n````";
    assert!(o_core::lead::prompt::split_ask(example).1.unwrap().is_none());
}

fn design_notes() {
    use o_core::lead::notes::{self, Change, Note};
    let (home, project) = setup("notes");
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let original = Note { id: "note-1".into(), quote: "게임".into(), anchor: "게임".into(), occurrence: 0, tag: "주석".into(), text: "점수 추가".into(), sent: false };
    notes::change(&home, &thread, Change::Add { note: original.clone() }).unwrap();
    assert_eq!(notes::read(&home, &thread).unwrap().as_slice(), std::slice::from_ref(&original));
    assert!(notes::change(&home, "../other", Change::Delete { id: "note-1".into() }).is_err());
    let (spawner, seen) = fake(claude_answer("note-session", "확인"));
    lead::send(&home, &request(&project, "설계 수정"), spawner.as_ref()).unwrap();
    let prompt = seen.lock().unwrap()[0].stdin.clone().unwrap();
    assert!(prompt.contains("점수 추가") && prompt.contains("게임"));
    assert!(notes::read(&home, &thread).unwrap()[0].sent);
    let changed = Note { text: "점수 다시 수정".into(), sent: false, ..original.clone() };
    notes::change(&home, &thread, Change::Update { note: changed.clone() }).unwrap();
    notes::change(&home, &thread, Change::Sent { notes: vec![original] }).unwrap();
    assert_eq!(notes::read(&home, &thread).unwrap(), [changed]);
    notes::change(&home, &thread, Change::Delete { id: "note-1".into() }).unwrap();
    assert!(notes::read(&home, &thread).unwrap().is_empty());
}

fn stale_plan_refused() {
    let (home, project) = setup("stale-plan");
    let (spawner, _) = fake(claude_answer("s", &plan_answer(good_plan())));
    let first = plans(&lead::send(&home, &request(&project, "first"), spawner.as_ref()).unwrap()).remove(0).0;
    let second = plans(&lead::send(&home, &request(&project, "second"), spawner.as_ref()).unwrap()).remove(0).0;
    let start = |_: &str| -> std::io::Result<()> { panic!("stale plans must not start") };
    assert!(matches!(lead::dispatch(&home, &first, RunMode::of(Profile::Own), &start), Err(DispatchError::Other(m)) if m.contains("설계가 변경")));
    let (reply, _) = fake(claude_answer("s", "답변"));
    lead::send(&home, &request(&project, "추가 질문"), reply.as_ref()).unwrap();
    assert!(matches!(lead::dispatch(&home, &second, RunMode::of(Profile::Own), &start), Err(DispatchError::Other(m)) if m.contains("설계가 변경")));
    let latest = plans(&lead::send(&home, &request(&project, "실행할 계획"), spawner.as_ref()).unwrap()).remove(0).0;
    let original = lead::current_thread(&home, &project).unwrap();
    lead::new_thread(&home, &project).unwrap();
    assert!(lead::dispatch(&home, &latest, RunMode::of(Profile::Own), &|_| Ok(())).is_ok());
    let latest = plans(&lead::send(&home, &SendRequest { thread: Some(&original), ..request(&project, "다음 계획") }, spawner.as_ref()).unwrap()).remove(0).0;
    lead::close(&home, &original).unwrap();
    assert!(matches!(lead::dispatch(&home, &latest, RunMode::of(Profile::Own), &start), Err(DispatchError::Other(_))));
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
    fake_slow(lines, 0)
}

/// Responds to the real stdio handshake; input lines are existing Codex turn fixtures.
fn fake_app_server(lines: Vec<String>) {
    let watched = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CODEX_API_KEY"];
    let keys: Vec<String> = std::env::vars().map(|(k, _)| k).filter(|k| watched.iter().any(|n| n.eq_ignore_ascii_case(k))).collect();
    let events: Vec<Value> = lines.iter().filter_map(|line| serde_json::from_str(&line.replace("{keys}", &keys.join(","))).ok()).collect();
    let mut input = std::io::stdin().lock().lines();
    let read = |input: &mut std::io::Lines<std::io::StdinLock<'_>>| -> Value {
        serde_json::from_str(&input.next().unwrap().unwrap()).unwrap()
    };
    let send = |value: Value| { println!("{value}"); std::io::stdout().flush().unwrap(); };
    assert_eq!(read(&mut input)["method"], "initialize");
    send(json!({"id":1,"result":{}}));
    assert_eq!(read(&mut input)["method"], "initialized");
    let thread = read(&mut input);
    let id = thread["params"]["threadId"].as_str().or_else(|| events.iter().find_map(|e| e["thread_id"].as_str())).unwrap_or("fake-codex").to_string();
    send(json!({"id":2,"result":{"thread":{"id":id}}}));
    let turn = read(&mut input);
    assert_eq!(turn["method"], "turn/start");
    if let Some(path) = std::env::var_os("O_FAKE_LEAD_INPUT_FILE") {
        std::fs::write(path, json!({"thread":thread,"turn":turn}).to_string()).unwrap();
    }
    send(json!({"id":3,"result":{"turn":{"id":"fake-turn"}}}));
    for event in events {
        match event["type"].as_str().unwrap_or("") {
            "o.delta" => send(json!({"method":"item/agentMessage/delta","params":{"threadId":id,"turnId":"fake-turn","delta":event["delta"]}})),
            "o.approval" => {
                send(json!({"id":42,"method":"item/commandExecution/requestApproval","params":{"threadId":id,"turnId":"fake-turn"}}));
                assert_eq!(read(&mut input), json!({"id":42,"result":{"decision":"decline"}}));
            }
            "o.requests" => {
                send(json!({"id":43,"method":"item/permissions/requestApproval","params":{"threadId":id,"turnId":"fake-turn","permissions":{"network":{"enabled":true}}}}));
                assert_eq!(read(&mut input), json!({"id":43,"result":{"permissions":{},"scope":"turn"}}));
                send(json!({"id":44,"method":"mcpServer/elicitation/request","params":{"threadId":id,"serverName":"s","mode":"url"}}));
                assert_eq!(read(&mut input), json!({"id":44,"result":{"action":"decline"}}));
                send(json!({"id":45,"method":"item/tool/requestUserInput","params":{"threadId":id,"turnId":"fake-turn","questions":[]}}));
                assert_eq!(read(&mut input), json!({"id":45,"result":{"answers":{}}}));
                send(json!({"id":46,"method":"item/tool/call","params":{"threadId":id,"turnId":"fake-turn"}}));
                let reply = read(&mut input);
                assert!(reply["error"].is_object() || reply["result"]["success"] == false, "{reply}");
            }
            "item.completed" => {
                let mut item = event["item"].clone();
                let kind = match item["type"].as_str().unwrap_or("") {
                    "agent_message" => "agentMessage", "file_change" => "fileChange",
                    "command_execution" => "commandExecution", "web_search" => "webSearch", _ => continue,
                };
                item["type"] = json!(kind);
                send(json!({"method":"item/completed","params":{"threadId":id,"turnId":"fake-turn","item":item}}));
            }
            "o.interrupted" => send(json!({"method":"turn/completed","params":{"threadId":id,"turn":{"id":"fake-turn","status":"interrupted"}}})),
            "turn.completed" => {
                let u = &event["usage"];
                let usage = json!({"inputTokens":u["input_tokens"].as_u64().unwrap_or(0),
                    "cachedInputTokens":u["cached_input_tokens"].as_u64().unwrap_or(0),
                    "outputTokens":u["output_tokens"].as_u64().unwrap_or(0)});
                send(json!({"method":"thread/tokenUsage/updated","params":{"threadId":id,"turnId":"fake-turn","tokenUsage":{"last":usage,"total":usage}}}));
                send(json!({"method":"turn/completed","params":{"threadId":id,"turn":{"id":"fake-turn","status":"completed"}}}));
            }
            _ => {}
        }
        if let Some(ms) = std::env::var(FAKE_PAUSE).ok().and_then(|v| v.parse().ok()) {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
    // A slow shutdown after the turn: still running when O stops waiting, then a failing exit.
    if let Some(ms) = std::env::var(FAKE_LINGER).ok().and_then(|v| v.parse().ok()) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        std::process::exit(3);
    }
}

/// A fake lead CLI that waits `ms` before printing `lines`.
fn fake_slow(lines: Vec<String>, ms: u64) -> (Arc<Spawner>, Arc<Mutex<Vec<LaunchSpec>>>) {
    fake_cli(lines, ms, APP_SERVER)
}

/// What the fake Codex answers to the app-server probe. `tag` names the fake CLI: O keeps one
/// probe result per CLI command line, so a different tag is a different (unprobed) CLI.
struct FakeProbe {
    app_server: bool,
    tag: &'static str,
    log: Option<PathBuf>,
}

/// A fake Codex CLI without app-server (an older one), printing `lines` from `codex exec`.
fn fake_without_app_server(lines: Vec<String>, tag: &'static str, log: PathBuf) -> (Arc<Spawner>, Arc<Mutex<Vec<LaunchSpec>>>) {
    fake_cli(lines, 0, FakeProbe { app_server: false, tag, log: Some(log) })
}

/// The fake Codex every app-server test uses.
const APP_SERVER: FakeProbe = FakeProbe { app_server: true, tag: "app-server", log: None };

/// The fake CLI's answer when `spec` is the app-server probe.
fn probe_command(spec: &LaunchSpec, cwd: &str, probe: &FakeProbe) -> Option<std::io::Result<o_core::sys::launch::Launch>> {
    if spec.bin != "codex" || spec.args != o_core::providers::codex::APP_SERVER_PROBE { return None; }
    Some(support::fake_worker_launch().map(|mut command| {
        command.current_dir(cwd).arg(format!("probe-{}", probe.tag)).env(FAKE_PROBE, if probe.app_server { "1" } else { "0" });
        if let Some(log) = &probe.log { command.env(FAKE_PROBE_LOG, log); }
        command
    }))
}

/// The probe's launch is answered by `probe` and not kept with the turn launches.
fn fake_cli(lines: Vec<String>, ms: u64, probe: FakeProbe) -> (Arc<Spawner>, Arc<Mutex<Vec<LaunchSpec>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let keep = seen.clone();
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        if let Some(command) = probe_command(spec, cwd, &probe) { return command; }
        keep.lock().unwrap().push(spec.clone());
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap()).env(FAKE_SLEEP, ms.to_string());
        if spec.bin == "codex" && spec.args.first().is_some_and(|arg| arg == "app-server") {
            command.env(FAKE_APP_SERVER, "1").env("O_FAKE_LEAD_INPUT_FILE", Path::new(cwd).join("fake-codex-input.json"));
        }
        Ok(command)
    });
    (spawner, seen)
}

/// A process with nothing to do with the lead (this binary, idling), standing in for one
/// that reused a dead turn's pid.
fn bystander() -> std::process::Child {
    Command::new(std::env::current_exe().unwrap()).env(FAKE, "[]").env(FAKE_SLEEP, "30000").spawn().unwrap()
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
    json!({ "title": "README 한 줄", "cards": [{ "id": "docs", "provider": "claude", "model": "test-model", "effort": "low", "task": "README.md 끝에 한 줄 추가" }] })
}

fn init_git(project: &str) {
    for args in [vec!["init"], vec!["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "fixture"]] {
        let mut command = Command::new("git");
        command.current_dir(project).args(args);
        let out = o_core::sys::proc::hide_window(&mut command).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
}

fn lead_child_marker() {
    let (home, project) = setup("lead-marker");
    let (spawner, _) = fake(claude_answer("session", "{agent-role}"));
    let events = lead::send(&home, &request(&project, "확인"), spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "lead")));
}

/// ADR-0035 개정 1: a lead turn in 구독만 with an authentication variable on its launch does not
/// start and says which by name; in 경고만 it answers. The variable is a placeholder on the launch only.
fn lead_subscription_mode() {
    use o_core::agents::billing;
    let (home, project) = setup("lead-billing");
    let (inner, seen) = fake(claude_answer("session", "답"));
    let spawner: Arc<Spawner> = Arc::new(move |spec: &LaunchSpec, cwd: &str| {
        let mut command = inner(spec, cwd)?;
        command.env("ANTHROPIC_AUTH_TOKEN", "placeholder-not-a-key");
        Ok(command)
    });
    billing::write_mode(&home, "subscription").unwrap();
    let events = lead::send(&home, &request(&project, "확인"), spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("구독만") && text.contains("ANTHROPIC_AUTH_TOKEN"))), "{events:?}");
    assert!(!events.iter().any(|e| matches!(e, LeadEvent::Reply { .. })));
    assert!(!serde_json::to_string(&events).unwrap().contains("placeholder-not-a-key"));
    billing::write_mode(&home, "warn").unwrap();
    let events = lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "답")), "{events:?}");
    assert!(!seen.lock().unwrap().is_empty());
}

fn live_tool_progress() {
    let (home, project) = setup("tool-progress");
    let mut lines = vec![
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Read", "input": { "file_path": "src/demo.py" } }] } }).to_string(),
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Edit", "input": { "file_path": "src/demo.py" } }] } }).to_string(),
        json!({ "type": "assistant", "message": { "content": [{ "type": "tool_use", "name": "Bash", "input": { "command": "git status" } }] } }).to_string(),
    ];
    lines.extend(claude_answer("session", "완료"));
    let worker_home = home.clone();
    let worker_project = project.clone();
    let worker = std::thread::spawn(move || {
        let spawner = move |spec: &LaunchSpec, cwd: &str| {
            if let Some(probe) = probe_command(spec, cwd, &APP_SERVER) { return probe; }
            let mut command = support::fake_worker_launch()?;
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap()).env(FAKE_PAUSE, "500");
            Ok(command)
        };
        lead::send(&worker_home, &request(&worker_project, "읽어줘"), &spawner).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let view = lead::show(&home, Some(&project), None).unwrap();
        if view["thread"]["progressCounts"] == json!({ "reads": 1, "searches": 0, "edits": 1, "commands": 1 }) {
            assert!(view["thread"]["progress"].as_array().unwrap().iter().any(|item| item["name"] == "Read" && item["summary"] == "src/demo.py"));
            break;
        }
        assert!(std::time::Instant::now() < deadline, "tool progress never became visible: {view}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    worker.join().unwrap();
    let view = lead::show(&home, Some(&project), None).unwrap();
    assert_eq!(view["thread"]["progress"], json!([]));
    assert!(view["thread"]["progressCounts"].is_null());
    assert!(view["thread"]["events"].as_array().unwrap().iter().all(|event| event["t"] != "lead.tool"));
}

fn codex_interrupted() {
    let (home, project) = setup("codex-interrupted");
    let (spawner, _) = fake(vec![
        json!({"type":"item.completed","item":{"type":"agent_message","text":"unfinished answer"}}).to_string(),
        json!({"type":"o.interrupted"}).to_string(),
    ]);
    let req = SendRequest { provider: ProviderId::Codex, ..request(&project, "확인") };
    let events = lead::send(&home, &req, spawner.as_ref()).unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { text, .. } if text.contains("Codex 응답이 중단됐습니다"))));
    assert!(!events.iter().any(|event| matches!(event, LeadEvent::Reply { .. })));
}

fn codex_approval() {
    let (home, project) = setup("codex-approval");
    let lines = vec![json!({"type":"o.approval"}).to_string(),
        json!({"type":"item.completed","item":{"type":"file_change","status":"declined","changes":[{"path":"should-not-change.txt","kind":"update"}]}}).to_string(),
        json!({"type":"item.completed","item":{"type":"agent_message","text":"승인이 필요한 작업은 하지 않았습니다."}}).to_string(),
        json!({"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}).to_string()];
    let (worker_home, worker_project) = (home.clone(), project.clone());
    let worker = std::thread::spawn(move || {
        let spawner = move |spec: &LaunchSpec, cwd: &str| {
            if let Some(probe) = probe_command(spec, cwd, &APP_SERVER) { return probe; }
            let mut command = support::fake_worker_launch()?;
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap())
                .env(FAKE_APP_SERVER, "1").env(FAKE_PAUSE, "500");
            Ok(command)
        };
        let req = SendRequest { provider: ProviderId::Codex, ..request(&worker_project, "확인") };
        lead::send(&worker_home, &req, &spawner).unwrap()
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let view = lead::show(&home, Some(&project), None).unwrap();
        // Normal mode asks the person (ADR-0008 개정 3); here they refuse.
        if let Some(id) = view["thread"]["approvals"][0]["id"].as_str() { let _ = lead::decide(&home, id, false); }
        if view["thread"]["progress"].as_array().is_some_and(|steps| steps.iter().any(|step| step["name"] == "권한" && step["summary"].as_str().is_some_and(|text| text.contains("거절")))) { break; }
        assert!(std::time::Instant::now() < deadline, "approval refusal was not visible: {view}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let events = worker.join().unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Reply { text, reported_files, .. } if text.contains("하지 않았습니다") && reported_files.is_empty())));
    assert!(!events.iter().any(|event| matches!(event, LeadEvent::Error { .. })));
}

fn codex_other_requests() {
    let (home, project) = setup("codex-requests");
    let (spawner, _) = fake(vec![json!({"type":"o.requests"}).to_string(),
        json!({"type":"item.completed","item":{"type":"agent_message","text":"요청 없이 답했습니다."}}).to_string(),
        json!({"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}).to_string()]);
    let req = SendRequest { provider: ProviderId::Codex, ..request(&project, "확인") };
    let events = lead::send(&home, &req, spawner.as_ref()).unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Reply { text, .. } if text.contains("요청 없이"))));
    assert!(!events.iter().any(|event| matches!(event, LeadEvent::Error { .. })), "{events:?}");
}

fn codex_slow_shutdown() {
    let (home, project) = setup("codex-slow-shutdown");
    let lines = vec![json!({"type":"item.completed","item":{"type":"agent_message","text":"끝났습니다."}}).to_string(),
        json!({"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}).to_string()];
    let spawner = move |spec: &LaunchSpec, cwd: &str| {
        if let Some(probe) = probe_command(spec, cwd, &APP_SERVER) { return probe; }
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap())
            .env(FAKE_APP_SERVER, "1").env(FAKE_LINGER, "20000");
        Ok(command)
    };
    let started = std::time::Instant::now();
    let req = SendRequest { provider: ProviderId::Codex, ..request(&project, "확인") };
    let events = lead::send(&home, &req, &spawner).unwrap();
    assert!(started.elapsed() < std::time::Duration::from_secs(15), "the process was not ended");
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Reply { text, .. } if text.contains("끝났습니다"))));
    assert!(!events.iter().any(|event| matches!(event, LeadEvent::Error { .. })), "{events:?}");
}

fn codex_partial() {
    let (home, project) = setup("codex-partial");
    let answer = plan_answer(good_plan());
    let lines = vec![
        json!({"type":"o.delta","delta":answer}).to_string(),
        json!({"type":"item.completed","item":{"type":"agent_message","text":answer}}).to_string(),
        json!({"type":"turn.completed","usage":{"input_tokens":10,"output_tokens":5}}).to_string(),
    ];
    let (worker_home, worker_project) = (home.clone(), project.clone());
    let worker = std::thread::spawn(move || {
        let spawner = move |spec: &LaunchSpec, cwd: &str| {
            if let Some(probe) = probe_command(spec, cwd, &APP_SERVER) { return probe; }
            let mut command = support::fake_worker_launch()?;
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap())
                .env(FAKE_APP_SERVER, "1").env(FAKE_PAUSE, "500");
            Ok(command)
        };
        let req = SendRequest { provider: ProviderId::Codex, ..request(&worker_project, "계획") };
        lead::send(&worker_home, &req, &spawner).unwrap()
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let view = lead::show(&home, Some(&project), None).unwrap();
        if let Some(partial) = view["thread"]["partial"].as_str().filter(|text| !text.is_empty()) {
            assert!(partial.contains("카드 한 장"));
            assert!(!partial.contains("o-plan") && !partial.contains("README"));
            assert!(view["thread"]["events"].as_array().unwrap().iter().all(|event| event["t"] != "lead.reply"));
            break;
        }
        assert!(std::time::Instant::now() < deadline, "Codex partial never became visible: {view}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let events = worker.join().unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Reply { text, .. } if text.contains("카드 한 장"))));
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Plan { .. })));
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["thread"]["partial"], "");
}

fn autorun() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (home, project) = setup("autorun");
    init_git(&project);
    let (spawner, _) = fake(claude_answer("session", &plan_answer(good_plan())));
    let starts = AtomicUsize::new(0);
    let start = |_: &str| { starts.fetch_add(1, Ordering::SeqCst); Ok(()) };
    let normal = lead::send_with_autorun(&home, &request(&project, "계획"), spawner.as_ref(), &start).unwrap();
    assert!(normal.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(_), .. })));
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    let (scheduled_home, scheduled_project) = setup("autorun-scheduled");
    init_git(&scheduled_project);
    let scheduled = lead::send_with_scheduled_autorun(&scheduled_home, &request(&scheduled_project, "예약"), spawner.as_ref(), &start).unwrap();
    assert!(scheduled.iter().any(|e| matches!(e, LeadEvent::Dispatch { yolo: false, .. })));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let blocked_schedule = lead::send_with_scheduled_autorun(&scheduled_home, &request(&scheduled_project, "예약"), spawner.as_ref(), &start).unwrap();
    assert!(blocked_schedule.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains("진행 중"))));
    let (agy_home, agy_project) = setup("autorun-agi-policy");
    init_git(&agy_project);
    let mut agy_plan = good_plan();
    agy_plan["cards"][0]["provider"] = json!("agy");
    agy_plan["cards"][0].as_object_mut().unwrap().remove("effort");
    let (agy_spawner, _) = fake(claude_answer("session", &plan_answer(agy_plan)));
    let agy = lead::send_with_scheduled_autorun(&agy_home, &request(&agy_project, "예약"), agy_spawner.as_ref(), &start).unwrap();
    assert!(agy.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains("Antigravity"))));
    assert!(!agy.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    o_core::agents::agy::set(&agy_home, o_core::agents::agy::Change { workers: Some(true), ack: Some(true), ..Default::default() }).unwrap();
    let agy_enabled = lead::send_with_scheduled_autorun(&agy_home, &request(&agy_project, "예약"), agy_spawner.as_ref(), &start).unwrap();
    assert!(agy_enabled.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains("예약에서는 Antigravity"))));
    assert!(!agy_enabled.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    let agy_lead = SendRequest { provider: ProviderId::Agy, ..request(&agy_project, "예약") };
    assert!(lead::send_with_scheduled_autorun(&agy_home, &agy_lead, agy_spawner.as_ref(), &start).is_err());
    assert!(lead::send(&agy_home, &agy_lead, agy_spawner.as_ref()).unwrap_err().contains("Antigravity"));
    starts.store(0, Ordering::SeqCst);
    let yolo = SendRequest { yolo: true, ..request(&project, "진행") };
    let manual = lead::send(&home, &yolo, spawner.as_ref()).unwrap();
    assert!(manual.iter().any(|e| matches!(e, LeadEvent::Plan { .. })));
    assert!(!manual.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    let run = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(run.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let blocked = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(blocked.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains("진행 중"))));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    let run_id = run.iter().find_map(|e| match e { LeadEvent::Dispatch { run_id, .. } => Some(run_id), _ => None }).unwrap();
    assert_eq!(lead::stop_all(&home, &project).unwrap(), 1);
    assert!(o_core::run::store::stop_requested(&home, run_id));
    o_core::run::store::append_event(&home, run_id, &o_core::run::events::OEvent::RunEnd {
        at: Utc::now().to_rfc3339(), run_id: run_id.clone(), status: o_core::run::events::CardStatus::Done,
    }).unwrap();
    let duplicate = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(duplicate.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains("같은 계획"))));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    o_core::usage::budget::write_enabled(&home, false).unwrap();
    let blocked = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(blocked.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains("예산"))));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
}

fn scheduled_run_mode_recorded() {
    use o_core::run::{events::CardStatus, runner, store};
    for scheduled in [false, true] {
        let (home, project) = setup(if scheduled { "scheduled-mode" } else { "ordinary-mode" });
        init_git(&project);
        let (lead_cli, _) = fake(claude_answer("lead", &plan_answer(good_plan())));
        let (worker_cli, _) = fake(claude_answer("worker", "완료"));
        let start = |run_id: &str| {
            assert_eq!(store::is_scheduled(&home, run_id), scheduled);
            let plan = serde_json::from_value(store::read_plan(&home, run_id)?).unwrap();
            assert_eq!(runner::run_plan(&home, run_id, &plan, None, Arc::clone(&worker_cli))?, CardStatus::Done);
            Ok(())
        };
        let req = SendRequest { yolo: !scheduled, ..request(&project, "실행") };
        let events = if scheduled {
            lead::send_with_scheduled_autorun(&home, &req, lead_cli.as_ref(), &start)
        } else {
            lead::send_with_autorun(&home, &req, lead_cli.as_ref(), &start)
        }.unwrap();
        let run_id = events.iter().find_map(|event| match event {
            LeadEvent::Dispatch { run_id, .. } => Some(run_id),
            _ => None,
        }).expect("the plan started");
        let recorded = store::read_event_values(&home, run_id);
        let mode = recorded.iter().find(|event| event["t"] == "run.mode").expect("the runner recorded its mode");
        assert_eq!(mode.get("scheduled"), scheduled.then_some(&json!(true)));
        assert_eq!(mode["yolo"], !scheduled);
    }
}

fn auto_reason(events: &[LeadEvent], part: &str) -> bool {
    events.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains(part)))
}

fn titled_plan(title: &str) -> Value {
    json!({ "title": title, "cards": [{ "id": "docs", "provider": "claude", "model": "test-model", "effort": "low", "task": "README.md 끝에 한 줄 추가" }] })
}

fn autorun_failures_not_counted() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let (home, project) = setup("autorun-failures");
    init_git(&project);
    let (spawner, _) = fake(claude_answer("session", &plan_answer(good_plan())));
    let (attempts, fail) = (AtomicUsize::new(0), AtomicBool::new(true));
    // `o` missing or the start callback failing both come back as the start's error.
    let start = |_: &str| {
        attempts.fetch_add(1, Ordering::SeqCst);
        if fail.load(Ordering::SeqCst) { Err(std::io::Error::new(std::io::ErrorKind::NotFound, "o 실행 파일 없음")) } else { Ok(()) }
    };
    let yolo = SendRequest { yolo: true, ..request(&project, "진행") };
    // More failures of the same plan than the hourly limit: each one is tried again.
    for round in 1..=3 {
        let failed = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
        assert!(auto_reason(&failed, "시작하지 못했습니다"), "round {round}: {failed:?}");
        assert!(!failed.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
        assert_eq!(attempts.load(Ordering::SeqCst), round);
    }
    // The last check before the start refusing (uncommitted work in the project) counts no more.
    let untracked = Path::new(&project).join("uncommitted.txt");
    std::fs::write(&untracked, "user edit").unwrap();
    let refused = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(auto_reason(&refused, "시작하지 못했습니다"), "{refused:?}");
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    std::fs::remove_file(untracked).unwrap();
    // Fixed, the same plan runs, and then a different one still fits in the hour.
    fail.store(false, Ordering::SeqCst);
    let run = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    let run_id = run.iter().find_map(|e| match e { LeadEvent::Dispatch { run_id, .. } => Some(run_id.clone()), _ => None }).expect("the fixed plan starts");
    let end = |run_id: &str| o_core::run::store::append_event(&home, run_id, &o_core::run::events::OEvent::RunEnd {
        at: Utc::now().to_rfc3339(), run_id: run_id.to_string(), status: o_core::run::events::CardStatus::Done,
    }).unwrap();
    end(&run_id);
    let (other, _) = fake(claude_answer("session", &plan_answer(titled_plan("다른 계획"))));
    let second = lead::send_with_autorun(&home, &yolo, other.as_ref(), &start).unwrap();
    let second_id = second.iter().find_map(|e| match e { LeadEvent::Dispatch { run_id, .. } => Some(run_id.clone()), _ => None }).expect("a different plan starts");
    end(&second_id);
    assert_eq!(attempts.load(Ordering::SeqCst), 5);
    // A plan that did start is still not run again within the duplicate window.
    let duplicate = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(auto_reason(&duplicate, "같은 계획") || auto_reason(&duplicate, "이번 시간"), "{duplicate:?}");
    assert_eq!(attempts.load(Ordering::SeqCst), 5);
}

/// A fake lead CLI whose start waits until `release` exists, after making `entered`.
fn gated(lines: Vec<String>, entered: PathBuf, release: PathBuf) -> Arc<Spawner> {
    Arc::new(move |_spec: &LaunchSpec, cwd: &str| {
        std::fs::write(&entered, "").unwrap();
        wait_until("the fake turn's release", || release.exists());
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap());
        Ok(command)
    })
}

fn autorun_new_thread_cancels() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let (home, project) = setup("autorun-new-thread");
    init_git(&project);
    let (entered, release) = (home.join("entered"), home.join("release"));
    let spawner = gated(claude_answer("session", &plan_answer(good_plan())), entered.clone(), release.clone());
    let starts = AtomicUsize::new(0);
    let start = |_: &str| { starts.fetch_add(1, Ordering::SeqCst); Ok(()) };
    let original = lead::start(&home, &project).unwrap();
    let yolo = SendRequest { thread: Some(&original), yolo: true, ..request(&project, "진행") };
    let events = std::thread::scope(|scope| {
        let turn = scope.spawn(|| lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start));
        wait_until("the lead to start answering", || entered.exists());
        let next = lead::start(&home, &project).unwrap();
        assert_ne!(next, original);
        assert!(lead::is_open(&home, &project, &original));
        std::fs::write(&release, "").unwrap();
        turn.join().unwrap().unwrap()
    });
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Plan { ok: true, .. })));
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    // A design-only tab still auto-runs its own plan under the existing card policy.
    let designer = lead::new_thread(&home, &project).unwrap();
    let (spawner, _) = fake(claude_answer("designer", &plan_answer(titled_plan("별도 설계"))));
    let result = lead::send_with_autorun(&home, &SendRequest { thread: Some(&designer), yolo: true, ..request(&project, "다른 작업") }, spawner.as_ref(), &start).unwrap();
    assert!(result.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })), "{result:?}");
    assert_eq!(starts.load(Ordering::SeqCst), 2);
    // The existing dispatch prevents another automatic run while it is active.
    let (spawner, _) = fake(claude_answer("session", &plan_answer(good_plan())));
    let run = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert!(auto_reason(&run, "진행 중"));
    assert_eq!(starts.load(Ordering::SeqCst), 2);
}

/// Another request lands right as the lead's answer ends: the auto path either starts the plan
/// while it is still the latest, or gives up; it never starts it after the newer request.
fn autorun_newer_request_wins() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for round in 0..3 {
        let (home, project) = setup(&format!("autorun-newer{round}"));
        init_git(&project);
        let (entered, release) = (home.join("entered"), home.join("release"));
        let spawner = gated(claude_answer("session", &plan_answer(good_plan())), entered.clone(), release.clone());
        let (later, _) = fake_slow(claude_answer("session", "새 요청에 답합니다"), 500);
        let starts = AtomicUsize::new(0);
        let start = |_: &str| { starts.fetch_add(1, Ordering::SeqCst); Ok(()) };
        let yolo = SendRequest { yolo: true, ..request(&project, "진행") };
        let events = std::thread::scope(|scope| {
            let turn = scope.spawn(|| lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start));
            wait_until("the lead to start answering", || entered.exists());
            let newer = scope.spawn(|| loop {
                if lead::send(&home, &request(&project, "새 요청"), later.as_ref()).is_ok() { break; }
            });
            std::fs::write(&release, "").unwrap();
            let events = turn.join().unwrap().unwrap();
            newer.join().unwrap();
            events
        });
        let thread = lead::read_thread(&home, &lead::current_thread(&home, &project).unwrap());
        let users: Vec<usize> = thread.iter().enumerate().filter(|(_, e)| matches!(e, LeadEvent::User { .. })).map(|(i, _)| i).collect();
        let dispatched = thread.iter().position(|e| matches!(e, LeadEvent::Dispatch { .. }));
        match dispatched {
            Some(at) => {
                assert!(at < users[1], "round {round}: started after the newer request");
                assert_eq!(starts.load(Ordering::SeqCst), 1);
            }
            None => {
                assert!(auto_reason(&events, "취소"), "round {round}: {events:?}");
                assert_eq!(starts.load(Ordering::SeqCst), 0);
            }
        }
    }
}

fn request<'a>(project: &'a str, text: &'a str) -> SendRequest<'a> {
    SendRequest { project, thread: None, provider: ProviderId::Claude, model: None, effort: Some("low"), text, attach: &[], fresh: false, clean: None, yolo: false }
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
    let (_, tagged) = split_plan("```o-plan\ntitle: 게임\n[[codex]] id=logic\n게임 로직\n[[claude]] id=score after=logic\n점수 로직\n```");
    let tagged = tagged.unwrap().unwrap();
    assert_eq!(tagged["cards"][0]["provider"], "codex");
    assert_eq!(tagged["cards"][1]["depends_on"], serde_json::json!(["logic"]));
    let (_, inline) = split_plan("```o-plan\ntitle: 게임\n[[codex]] = 로직 구현\n```");
    assert_eq!(inline.unwrap().unwrap()["cards"][0]["task"], "로직 구현");
    assert!(split_plan("```o-plan\ntitle: 게임\n[[codex]] id=a\n내용").1.is_err());
    assert!(split_plan("```o-plan\ntitle: 게임\n[[codex]] id=a command=oops\n내용\n```").1.is_err());
    let example = "````markdown\n```o-plan\ntitle: 예시\n[[codex]] = 실행하면 안 됨\n```\n````";
    assert_eq!(split_plan(example).1, Ok(None));
    let indented = "    ```o-plan\n    title: 예시\n    [[codex]] = 실행하면 안 됨\n    ```";
    assert_eq!(split_plan(indented).1, Ok(None));
}

fn prompts() {
    let input = PromptInput { project: "C:/p", text: "할 일", attachments: &[], agy_workers_allowed: false, budget: &["codex 여유 있음".into()], unready: &[], runs: &[], runs_file: None, runs_error: None, result_tools: false, resumed: false, history: &[], missed: &[], design: None, yolo: false, approvals: false, overview: None, forwarded: false, review: None };
    let first = build(&input);
    for part in ["[O 총괄]", "프로젝트: C:/p", "```o-plan", "읽기만", "codex 여유 있음", "사용자:\n할 일"] {
        assert!(first.contains(part), "{part}");
    }
    let next = build(&PromptInput { resumed: true, budget: &[], ..input });
    let enabled = build(&PromptInput { agy_workers_allowed: true, ..input });
    assert!(enabled.contains("claude|codex|agy|opencode|devin") && enabled.contains("완료 파일") && enabled.contains("후속 지시를 받지"));
    assert!(first.contains("작업 에이전트가 꺼져"));
    assert!(!next.contains("```o-plan") && next.ends_with("사용자:\n할 일"));
    assert!(!first.contains(o_core::lead::prompt::UNREADY_NOTE) && !next.contains(o_core::lead::prompt::UNREADY_NOTE));
    // A provider whose project_O profile is not ready is named on every turn, fresh or resumed (ADR-0040).
    let unready = [o_core::providers::codex::SANDBOX_NOT_SET.to_string()];
    for turn in [build(&PromptInput { unready: &unready, ..input }), build(&PromptInput { resumed: true, unready: &unready, ..input })] {
        assert!(turn.contains(&format!("{}\n- {}", o_core::lead::prompt::UNREADY_NOTE, unready[0])), "{turn}");
        assert!(turn.ends_with("사용자:\n할 일"));
    }
    let (home, project) = setup("plan-notice");
    let mut plan = good_plan();
    plan["cards"][0]["provider"] = json!("devin");
    let (plan, violations, notices) = lead::check_lead_plan(&home, &project, plan, RunMode::of(Profile::ProjectO));
    assert!(violations.is_empty());
    let event = LeadEvent::Plan { at: Utc::now().to_rfc3339(), plan_id: "p".into(), plan, ok: true, violations, notices };
    let stored = serde_json::to_value(event).unwrap();
    assert_eq!(stored["notices"][0], o_core::providers::devin::OWN_PROFILE_NOTICE);
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
            LeadEvent::Usage { .. } | LeadEvent::UsageSettled { .. } => "usage",
            LeadEvent::Reply { .. } => "reply",
            LeadEvent::Plan { .. } => "plan",
            LeadEvent::Error { .. } => "error",
            LeadEvent::Seen { .. } => "seen",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["user", "session", "usage", "seen", "reply", "plan"]);
    let spec = &seen.lock().unwrap()[0];
    assert!(spec.stdin.as_deref().unwrap().contains("README에 한 줄 추가"));
    assert!(!spec.args.contains(&"--resume".to_string()));
    let [(plan_id, plan, ok, violations)] = <[_; 1]>::try_from(plans(&events)).unwrap();
    assert!(ok, "{violations:?}");
    assert!(plan_id.ends_with("-p1"));
    assert_eq!(plan["project"], project.as_str());
    assert!(events.iter().any(|e| matches!(e, LeadEvent::UsageSettled { usage, .. } if (usage.input, usage.cached, usage.output) == (15.0, 100.0, 20.0) && !usage.complete)));
    // Saved, and counted as planning overhead.
    let thread = lead::current_thread(&home, &project).unwrap();
    assert_eq!(lead::read_thread(&home, &thread).len(), 7);
    assert_eq!(lead::tokens(&home, Some(&project), None).total, 135);
    // Its CLI session is the lead's, not separate work for `o runs --sessions`.
    assert_eq!(lead::session_ids(&home), ["sess-1"]);
}

/// A table's rows are separate lines; joined, it shows as one line of pipes.
fn reply_reported_files() {
    let (home, project) = setup("reply-files");
    let lines = vec![
        json!({ "type": "thread.started", "thread_id": "files" }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "file_change", "status": "completed", "changes": [{"path":"src/a.rs"}, {"path":"src/b.rs"}] } }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "file_change", "status": "failed", "changes": [{"path":"failed.rs"}] } }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "agent_message", "text": "완료" } }).to_string(),
        json!({ "type": "turn.completed", "usage": {} }).to_string(),
    ];
    let (spawner, _) = fake(lines);
    let req = SendRequest { provider: ProviderId::Codex, ..request(&project, "수정") };
    let events = lead::send(&home, &req, spawner.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { reported_files, .. } if reported_files == &["src/a.rs", "src/b.rs"])));
}

fn reply_keeps_lines() {
    let (home, project) = setup("lines");
    let prose = "정리했습니다.\n\n## 진행 상황 요약\n\n### ✅ 끝난 것\n\n| 영역 | 내용 | 근거 |\n|---|---|---|\n| 기술 스택 | **B 확인 필요** | `docs/decisions/0001.md` |\n| Rust 이관 | 끝 | `crates/` |\n\n- 하나\n  - 둘";
    let (spawner, _) = fake(claude_answer("sess-lines", &format!("{prose}\n\n```o-plan\n{}\n```", good_plan())));
    lead::send(&home, &request(&project, "진행 상황"), spawner.as_ref()).unwrap();
    let thread = lead::current_thread(&home, &project).unwrap();
    let replies: Vec<String> = lead::read_thread(&home, &thread)
        .into_iter()
        .filter_map(|e| match e {
            LeadEvent::Reply { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(replies, [prose]);
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
    let refused = lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &|_| panic!("a refused plan must not start"));
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
    assert_eq!(spec.args[0], "app-server");
    let sent: Value = serde_json::from_slice(&std::fs::read(Path::new(&project).join("fake-codex-input.json")).unwrap()).unwrap();
    assert_eq!(sent["thread"]["method"], "thread/start");
    // Normal mode: Codex asks the person before anything outside its read-only sandbox (ADR-0023 개정).
    for at in ["thread", "turn"] {
        assert_eq!(sent[at]["params"]["approvalPolicy"], "on-request");
        assert_eq!(sent[at]["params"]["approvalsReviewer"], "user");
    }
    assert_eq!(sent["turn"]["params"]["sandboxPolicy"]["type"], "readOnly");
    assert!(sent["turn"]["params"]["input"][0]["text"].as_str().unwrap().contains("어느 파일인가요?"));
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "네")));
}

fn dispatch_once() {
    let (home, project) = setup("dispatch");
    init_git(&project);
    let (spawner, _) = fake(claude_answer("s", &plan_answer(good_plan())));
    let events = lead::send(&home, &request(&project, "해 줘"), spawner.as_ref()).unwrap();
    let plan_id = plans(&events).remove(0).0;
    let started = Mutex::new(Vec::new());
    let start = |run: &str| -> std::io::Result<()> {
        started.lock().unwrap().push(run.to_string());
        Ok(())
    };
    let untracked = Path::new(&project).join("uncommitted.txt");
    std::fs::write(&untracked, "user edit").unwrap();
    assert!(matches!(lead::dispatch(&home, &plan_id, RunMode::of(Profile::ProjectO), &start), Err(DispatchError::Other(m)) if m.contains("커밋하지 않은")));
    assert!(started.lock().unwrap().is_empty());
    std::fs::remove_file(untracked).unwrap();
    let done = lead::dispatch(&home, &plan_id, RunMode::of(Profile::ProjectO), &start).ok().unwrap();
    assert_eq!(*started.lock().unwrap(), vec![done.run_id.clone()]);
    assert_eq!(done.cards, ["docs"]);
    assert!(home.join("runs").join(&done.run_id).join("plan.json").exists());
    assert!(home.join("runs").join(&done.run_id).join("clean").exists());
    assert!(home.join("runs").join(&done.run_id).join("isolated-worktrees").exists());
    let again = lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &start);
    assert!(matches!(again, Err(DispatchError::Other(m)) if m.contains("이미 실행")));
    assert_eq!(started.lock().unwrap().len(), 1);
    use o_core::run::events::{CardStatus, OEvent};
    let at = Utc::now().to_rfc3339();
    o_core::run::store::append_event(&home, &done.run_id, &OEvent::RunStart { at: at.clone(), run_id: done.run_id.clone(), title: "test".into(), project: project.clone(), cards: vec![] }).unwrap();
    o_core::run::store::append_event(&home, &done.run_id, &OEvent::RunEnd { at, run_id: done.run_id.clone(), status: CardStatus::Failed }).unwrap();
    let thread = lead::current_thread(&home, &project).unwrap();
    let retry_mark = home.join("leads").join(thread).join("dispatched").join(format!("{plan_id}.retry"));
    std::fs::write(&retry_mark, "unrecorded-run").unwrap();
    assert!(matches!(lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &start), Err(DispatchError::Other(m)) if m.contains("확인할 수 없어")));
    std::fs::remove_file(retry_mark).unwrap();
    let retried = lead::dispatch(&home, &plan_id, RunMode::of(Profile::ProjectO), &start).ok().expect("failed run can be retried");
    assert_ne!(retried.run_id, done.run_id);
    assert_eq!(started.lock().unwrap().len(), 2);
    assert!(matches!(lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &start), Err(DispatchError::Other(m)) if m.contains("이미 실행")));
    assert!(matches!(lead::dispatch(&home, "nope", RunMode::of(Profile::Own), &start), Err(DispatchError::Other(_))));
    // The next turn hears how the dispatched run is doing.
    let (spawner, seen) = fake(claude_answer("s", "확인했습니다"));
    lead::send(&home, &request(&project, "어떻게 됐어?"), spawner.as_ref()).unwrap();
    assert!(seen.lock().unwrap()[0].stdin.as_deref().unwrap().contains(&done.run_id));
}

fn dispatch_non_git() {
    let (home, project) = setup("dispatch-non-git");
    let (spawner, _) = fake(claude_answer("s", &plan_answer(good_plan())));
    let events = lead::send(&home, &request(&project, "해 줘"), spawner.as_ref()).unwrap();
    let plan_id = plans(&events).remove(0).0;
    let done = lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &|_| Ok(())).ok().expect("non-Git plan dispatch");
    assert!(home.join("runs").join(done.run_id).join("isolated-worktrees").exists());
    assert!(!Path::new(&project).join(".git").exists());
}

fn threads() {
    let (home, project) = setup("threads");
    let first = lead::select_thread(&home, &project, None, false).unwrap();
    assert_eq!(lead::select_thread(&home, &project, None, false).unwrap(), first);
    // The same folder written another way is the same project.
    assert_eq!(lead::current_thread(&home, &format!("{}/", project.replace('\\', "/"))), Some(first.clone()));
    let second = lead::start(&home, &project).unwrap();
    assert_ne!(first, second);
    assert_eq!(lead::current_thread(&home, &project), Some(second.clone()));
    let shown = lead::show(&home, Some(&project), None).unwrap();
    assert_eq!(shown["thread"]["threadId"], second.as_str());
    assert_eq!(shown["projects"][0]["project"], project.as_str());
    assert_eq!(lead::open_threads(&home, &project), [first.clone(), second.clone()]);
    assert_eq!(shown["threads"][0]["writer"], true);
    assert_eq!(shown["threads"][1]["writer"], false);
    assert!(lead::show(&home, Some("C:/no/such"), None).unwrap()["thread"].is_null());
    assert_eq!(lead::show(&home, None, Some(&first)).unwrap()["thread"]["threadId"], first);
    assert!(lead::show(&home, Some("C:/no/such"), Some(&first)).is_err());
    assert!(lead::select_thread(&home, &project, Some("../outside"), false).is_err());
    assert!(lead::select_thread(&home, "C:/no/such", Some(&first), false).is_err());
    assert!(lead::select_thread(&home, &project, Some(&first), true).is_err());
    let third = lead::new_thread(&home, &project).unwrap();
    lead::set_writer(&home, &third).unwrap();
    let shown = lead::show(&home, Some(&project), Some(&first)).unwrap();
    assert_eq!(shown["thread"]["threadId"], first);
    assert_eq!(shown["threads"].as_array().unwrap().iter().map(|tab| tab["writer"].as_bool().unwrap()).collect::<Vec<_>>(), [false, false, true]);
    lead::select_thread(&home, &project, Some(&third), false).unwrap();
    let history = std::fs::read(lead::thread_dir(&home, &third).join("events.jsonl")).unwrap();
    lead::close(&home, &third).unwrap();
    assert_eq!(lead::current_thread(&home, &project), Some(second.clone()));
    assert!(!lead::is_open(&home, &project, &third));
    assert_eq!(std::fs::read(lead::thread_dir(&home, &third).join("events.jsonl")).unwrap(), history);
    assert!(lead::set_writer(&home, &third).is_err());
    assert!(lead::select_thread(&home, &project, Some(&third), false).is_err());
    assert_eq!(lead::show(&home, Some(&project), Some(&third)).unwrap()["thread"]["threadId"], second);
    assert!(lead::stop(&home, Some(&project), Some(&third)).is_err());
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["writer"], true);
    lead::close(&home, &first).unwrap();
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["writer"], true);
    lead::close(&home, &second).unwrap();
    assert!(lead::open_threads(&home, &project).is_empty());
    assert!(lead::current_thread(&home, &project).is_none());
    assert!(lead::show(&home, Some(&project), None).unwrap()["thread"].is_null());
    let replacement = lead::select_thread(&home, &project, None, false).unwrap();
    assert_ne!(replacement, second);
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["writer"], true);
}

fn thread_migration() {
    let (home, project) = setup("legacy-tabs");
    let first = lead::start(&home, &project).unwrap();
    std::fs::remove_file(home.join("leads/open.json")).unwrap();
    assert_eq!(lead::open_threads(&home, &project), std::slice::from_ref(&first));
    assert!(lead::is_open(&home, &project, &first));
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["writer"], true);
    let second = lead::new_thread(&home, &project).unwrap();
    assert_eq!(lead::open_threads(&home, &project), [first.clone(), second]);
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["writer"], true);

    let (home, root, _, _) = setup_overall("overall-tabs");
    let first = lead::start(&home, &root).unwrap();
    std::fs::remove_file(home.join("leads/open.json")).unwrap();
    assert_eq!(lead::show(&home, Some(&root), None).unwrap()["threads"][0]["writer"], false);
    let second = lead::new_thread(&home, &root).unwrap();
    assert_eq!(lead::open_threads(&home, &root), [first.clone(), second.clone()]);
    assert!(lead::set_writer(&home, &first).is_err());
    assert!(lead::set_writer(&home, &second).is_err());
    assert!(lead::show(&home, Some(&root), None).unwrap()["threads"].as_array().unwrap().iter().all(|tab| tab["writer"] == false));
}

fn concurrent_open_limit() {
    let (home, project) = setup("tab-limit");
    assert_eq!(lead::MAX_OPEN, 4);
    let barrier = std::sync::Barrier::new(8);
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8).map(|_| scope.spawn(|| {
            barrier.wait();
            lead::start(&home, &project)
        })).collect();
        handles.into_iter().map(|handle| handle.join().unwrap()).collect::<Vec<_>>()
    });
    let made: std::collections::BTreeSet<_> = results.iter().filter_map(|result| result.as_ref().ok().cloned()).collect();
    assert_eq!(made.len(), lead::MAX_OPEN, "{results:?}");
    let open = lead::open_threads(&home, &project);
    assert_eq!(open.len(), lead::MAX_OPEN);
    assert_eq!(open.iter().cloned().collect::<std::collections::BTreeSet<_>>(), made);
    assert!(lead::start(&home, &project).is_err());
    let shown = lead::show(&home, Some(&project), None).unwrap();
    assert_eq!(shown["threads"].as_array().unwrap().iter().filter(|tab| tab["writer"] == true).count(), 1);
    lead::close(&home, &open[1]).unwrap();
    let replacement = lead::start(&home, &project).unwrap();
    assert!(lead::is_open(&home, &project, &replacement));
    assert_eq!(lead::open_threads(&home, &project).len(), lead::MAX_OPEN);
}

fn parallel_tabs() {
    let (home, project) = setup("parallel-tabs");
    let first = lead::start(&home, &project).unwrap();
    let second = lead::new_thread(&home, &project).unwrap();
    let (entered_a, release_a) = (home.join("entered-a"), home.join("release-a"));
    let (entered_b, release_b) = (home.join("entered-b"), home.join("release-b"));
    let a = gated(claude_answer("a", "first answer"), entered_a.clone(), release_a.clone());
    let b = gated(claude_answer("b", "second answer"), entered_b.clone(), release_b.clone());
    let first_request = SendRequest { thread: Some(&first), ..request(&project, "first") };
    let second_request = SendRequest { thread: Some(&second), ..request(&project, "second") };
    let (refused, seen) = fake(claude_answer("unused", "must not run"));
    std::thread::scope(|scope| {
        let first_turn = scope.spawn(|| lead::send(&home, &first_request, a.as_ref()));
        wait_until("the first tab", || entered_a.exists());
        assert!(lead::set_writer(&home, &second).is_err(), "the active writer cannot be transferred");
        let second_turn = scope.spawn(|| lead::send(&home, &second_request, b.as_ref()));
        wait_until("both tabs to enter their spawners", || entered_b.exists());
        assert_eq!(lead::active_count(&home).unwrap(), 2);
        for req in [&first_request, &second_request] {
            assert!(lead::send(&home, req, refused.as_ref()).unwrap_err().contains("답하는 중"));
            assert!(lead::close(&home, req.thread.unwrap()).is_err());
        }
        assert!(seen.lock().unwrap().is_empty());
        let shown = lead::show(&home, Some(&project), None).unwrap();
        assert!(shown["threads"].as_array().unwrap().iter().all(|tab| tab["state"] == "working" && !tab["busy"].is_null()));
        std::fs::write(&release_a, "").unwrap();
        assert!(first_turn.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "first answer")));
        assert!(lead::set_writer(&home, &second).is_err(), "the receiving tab is still active");
        std::fs::write(&release_b, "").unwrap();
        assert!(second_turn.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "second answer")));
    });
    assert_eq!(lead::active_count(&home).unwrap(), 0);
    lead::set_writer(&home, &second).unwrap();
    assert!(lead::read_thread(&home, &first).iter().all(|e| !matches!(e, LeadEvent::Reply { text, .. } if text == "second answer")));
    assert!(lead::read_thread(&home, &second).iter().all(|e| !matches!(e, LeadEvent::Reply { text, .. } if text == "first answer")));
}

fn nonwriter_read_only() {
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        let (home, project) = setup(&format!("readonly-{}", provider.as_str()));
        lead::start(&home, &project).unwrap();
        let designer = lead::new_thread(&home, &project).unwrap();
        let lines = if provider == ProviderId::Claude { claude_answer("designer", "읽었습니다") } else { vec![
            json!({"type":"thread.started","thread_id":"designer"}).to_string(),
            json!({"type":"item.completed","item":{"type":"agent_message","text":"읽었습니다"}}).to_string(),
            json!({"type":"turn.completed","usage":{}}).to_string(),
        ] };
        let (spawner, seen) = fake(lines);
        for yolo in [false, true] {
            let req = SendRequest { provider, thread: Some(&designer), yolo, ..request(&project, "읽어 줘") };
            let events = lead::send(&home, &req, spawner.as_ref()).unwrap();
            assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { .. })), "{events:?}");
        }
        let specs = seen.lock().unwrap();
        assert_eq!(specs.len(), 2);
        for spec in specs.iter() {
            assert!(!spec.args.iter().any(|arg| arg.contains("dangerously") || arg == "--permission-prompt-tool"), "{:?}", spec.args);
            if provider == ProviderId::Claude {
                assert_eq!(arg_after(&spec.args, "--tools"), Some("Read,Grep,Glob"));
            } else if spec.args.first().is_some_and(|arg| arg == "app-server") {
                let sent: Value = serde_json::from_slice(&std::fs::read(Path::new(&project).join("fake-codex-input.json")).unwrap()).unwrap();
                assert_eq!(sent["thread"]["params"]["sandbox"], "read-only");
                assert_eq!(sent["thread"]["params"]["approvalPolicy"], "never");
            } else {
                assert_eq!(arg_after(&spec.args, "-s"), Some("read-only"));
            }
        }
    }
}

fn tab_states() {
    let (home, project) = setup("tab-states");
    let (spawner, _) = fake(claude_answer("title", &plan_answer(good_plan())));
    let text = "가나다라마바사아자차카타파하😀ABCDEFGHIJKLMNOP\n둘째 줄은 제목이 아닙니다";
    let events = lead::send(&home, &request(&project, text), spawner.as_ref()).unwrap();
    let first = lead::current_thread(&home, &project).unwrap();
    let second = lead::new_thread(&home, &project).unwrap();
    let shown = lead::show(&home, Some(&project), None).unwrap();
    assert_eq!(shown["threads"][0]["title"], "가나다라마바사아자차카타파하😀ABCDEFGHI");
    assert_eq!(shown["threads"][0]["state"], "waiting");
    assert!(shown["threads"][0]["startedAt"].as_str().is_some_and(|at| !at.is_empty()));
    assert_eq!(shown["threads"][1]["title"], "");
    assert_eq!(shown["threads"][1]["state"], "idle");
    assert_eq!(shown["projects"][0]["state"], "waiting");
    let plan = plans(&events).remove(0).0;
    let dispatched = lead::dispatch(&home, &plan, RunMode::of(Profile::Own), &|_| Ok(())).ok().unwrap();
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["threads"][0]["state"], "working");
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["projects"][0]["state"], "working");
    let (spawner, _) = fake(claude_answer("title", "```o-ask\n{\"questions\":[{\"question\":\"어느 쪽?\",\"options\":[{\"label\":\"A\"},{\"label\":\"B\"}]}]}\n```"));
    lead::send(&home, &SendRequest { thread: Some(&first), ..request(&project, "다음 단계") }, spawner.as_ref()).unwrap();
    let shown = lead::show(&home, Some(&project), Some(&second)).unwrap();
    assert_eq!(shown["threads"][0]["state"], "waiting", "an unanswered question outranks its live run");
    assert_eq!(shown["projects"][0]["state"], "waiting");
    let (spawner, _) = fake(claude_answer("title", "알겠습니다"));
    lead::send(&home, &SendRequest { thread: Some(&first), ..request(&project, "A") }, spawner.as_ref()).unwrap();
    o_core::run::store::append_event(&home, &dispatched.run_id, &o_core::run::events::OEvent::RunEnd {
        at: Utc::now().to_rfc3339(), run_id: dispatched.run_id.clone(), status: o_core::run::events::CardStatus::Done,
    }).unwrap();
    let shown = lead::show(&home, Some(&project), Some(&second)).unwrap();
    assert_eq!(shown["threads"][0]["title"], "가나다라마바사아자차카타파하😀ABCDEFGHI");
    assert_eq!(shown["projects"][0]["state"], "idle");
    std::fs::write(lead::thread_dir(&home, &first).join("running.json"), "{broken").unwrap();
    let shown = lead::show(&home, Some(&project), Some(&second)).unwrap();
    assert_eq!(shown["threads"][0]["busy"], "unknown");
    assert_eq!(shown["threads"][0]["state"], "waiting");
    assert_eq!(shown["projects"][0]["state"], "waiting");
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

/// Sends that start together: exactly one turn runs, every other is refused as busy, and
/// while it runs the thread shows as busy. Repeated, since the old marker lost this race
/// only sometimes (an empty or half-written marker read as "no turn").
fn busy() {
    // A sender that holds the turn (its OS lock on `turn.lock`) but has not finished writing
    // its marker yet: an empty or half-written marker means "starting", not "no turn".
    let (home, project) = setup("busy-mid");
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let dir = lead::thread_dir(&home, &thread);
    let held = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("turn.lock")).unwrap();
    held.try_lock().unwrap();
    assert_eq!(lead::active_count(&home).unwrap(), 1);
    let next = lead::start(&home, &project).unwrap();
    assert_ne!(next, thread);
    assert!(lead::close(&home, &thread).is_err());
    assert!(lead::set_writer(&home, &next).is_err());
    assert_eq!(lead::active_count(&home).unwrap(), 1);
    // Continue the remaining busy checks on the same thread.
    drop(held);
    let thread = lead::select_thread(&home, &project, Some(&thread), false).unwrap();
    let dir = lead::thread_dir(&home, &thread);
    let held = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("turn.lock")).unwrap();
    held.try_lock().unwrap();
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    for partial in ["", "{\"provider\":\"cla"] {
        std::fs::write(dir.join("running.json"), partial).unwrap();
        let refused = lead::send(&home, &SendRequest { thread: Some(&thread), ..request(&project, "또") }, spawner.as_ref()).unwrap_err();
        assert!(refused.contains("답하는 중"), "{partial:?}: {refused}");
        assert!(!lead::show(&home, Some(&project), Some(&thread)).unwrap()["thread"]["busy"].is_null(), "{partial:?}");
        assert!(dir.join("running.json").exists(), "another sender removed the owner's marker");
    }
    assert!(seen.lock().unwrap().is_empty());
    // A missing owner does not prove that its child process tree has exited.
    drop(held);
    assert!(lead::send(&home, &SendRequest { thread: Some(&thread), ..request(&project, "다시") }, spawner.as_ref()).unwrap_err().contains("확인할 수 없어"));
    assert!(seen.lock().unwrap().is_empty());
    assert!(dir.join("running.json").exists());

    const SENDERS: usize = 8;
    for round in 0..4 {
        let (home, project) = setup(&format!("busy{round}"));
        lead::thread_for(&home, &project, false).unwrap();
        let (entered, release) = (home.join("entered"), home.join("release"));
        let spawner = gated(claude_answer("s", "ok"), entered.clone(), release.clone());
        let refused_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let gate = Arc::new(std::sync::Barrier::new(SENDERS + 1));
        let handles: Vec<_> = (0..SENDERS)
            .map(|_| {
                let (home, project, spawner, gate, refused_count) = (home.clone(), project.clone(), spawner.clone(), gate.clone(), refused_count.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    let result = lead::send(&home, &request(&project, "동시에"), spawner.as_ref());
                    if result.is_err() { refused_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }
                    result
                })
            })
            .collect();
        gate.wait();
        // While the one turn answers, the thread is busy.
        wait_until("the same-tab contenders to be refused", || entered.exists() && refused_count.load(std::sync::atomic::Ordering::SeqCst) == SENDERS - 1);
        assert_eq!(lead::show(&home, Some(&project), None).unwrap()["thread"]["busy"], "claude", "round {round}");
        std::fs::write(&release, "").unwrap();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let started = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!(started, 1, "round {round}: {results:?}");
        for refused in results.iter().filter_map(|r| r.as_ref().err()) {
            assert!(refused.contains("답하는 중"), "{refused}");
        }
        assert!(lead::show(&home, Some(&project), None).unwrap()["thread"]["busy"].is_null());
    }
}

/// A dead owner's marker cannot prove that its process tree exited.
fn stale_marker() {
    let (home, project) = setup("stale");
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    let mut other = bystander();
    let pid = other.id();
    let stale = json!({ "pid": pid, "provider": "claude", "owner": pid, "token": "dead", "child": { "pid": pid, "started": 1 } });
    std::fs::write(&marker, stale.to_string()).unwrap();
    let approvals = lead::thread_dir(&home, &thread).join("approvals");
    std::fs::create_dir_all(&approvals).unwrap();
    std::fs::write(approvals.join("old.json"), json!({"id":"old","token":"dead","at":"","expires":"","provider":"claude","kind":"file","tool":"Write","summary":"","detail":"","cwd":null,"reason":null,"files":[]}).to_string()).unwrap();
    assert_eq!(lead::show(&home, Some(&project), None).unwrap()["thread"]["busy"], "unknown");
    assert!(lead::show(&home, Some(&project), None).unwrap()["thread"]["approvals"].as_array().unwrap().is_empty());
    assert!(!lead::stop(&home, Some(&project), None).unwrap());
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(other.try_wait().unwrap().is_none(), "stop ended a process that only reused the dead turn's pid");
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    assert!(lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap_err().contains("종료를 확인"));
    let next = lead::new_thread(&home, &project).unwrap();
    assert!(lead::is_open(&home, &project, &thread));
    assert!(lead::close(&home, &thread).is_err());
    assert!(lead::set_writer(&home, &next).is_err());
    assert!(seen.lock().unwrap().is_empty());
    assert!(marker.exists());
    let _ = other.kill();
    let _ = other.wait();
}

fn refused_requests() {
    let (home, project) = setup("refuse");
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    let home_text = home.to_string_lossy().into_owned();
    let cases = [
        SendRequest { provider: ProviderId::Agy, effort: Some("minimal"), ..request(&project, "a") },
        SendRequest { effort: Some("minimal"), ..request(&project, "a") },
        // Codex's models start at `low`.
        SendRequest { provider: ProviderId::Codex, effort: Some("minimal"), ..request(&project, "a") },
        // The lead's model takes the shape a card's model does (policy's model rule).
        SendRequest { model: Some("-opus"), ..request(&project, "a") },
        SendRequest { model: Some("opus 5"), ..request(&project, "a") },
        SendRequest { model: Some(".hidden"), ..request(&project, "a") },
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
    // "새 대화" on a folder that may not have a lead starts nothing.
    assert!(lead::start(&home, &home_text).is_err());
    assert!(lead::start(&home, &user_home.to_string_lossy()).is_err());
    assert!(lead::start(&home, &project).is_ok());
    let config = Path::new(&project).join(".codex");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("config.toml"), "[mcp_servers.fixture]\ncommand = \"never-run\"\n").unwrap();
    let codex = SendRequest { provider: ProviderId::Codex, ..request(&project, "a") };
    let error = lead::send(&home, &codex, spawner.as_ref()).unwrap_err();
    assert!(error.contains("프로젝트 설정·MCP"));
    assert!(seen.lock().unwrap().is_empty(), "even the app-server probe must not start");
    assert!(lead::check_request(&home, &SendRequest { yolo: true, ..codex }).is_ok());
    std::fs::write(config.join("config.toml"), "include_collaboration_mode_instructions=false\n[agents]\nenabled=false\n[features]\nmulti_agent=false\n").unwrap();
    assert!(lead::check_request(&home, &codex).is_ok(), "collaboration settings do not prevent the representative's lead");
}

fn api_keys_inherited() {
    let (home, project) = setup("keys");
    // Tests here run one after another on this thread, so changing this process's variables is safe.
    let planted = ["anthropic_api_key", "ANTHROPIC_AUTH_TOKEN", "CODEX_API_KEY"];
    for name in planted {
        std::env::set_var(name, "planted");
    }
    let reply = |provider: ProviderId, lines: Vec<String>| {
        let (spawner, _) = fake(lines);
        let req = SendRequest { provider, effort: None, ..request(&project, "키?") };
        let events = lead::send(&home, &req, spawner.as_ref()).unwrap();
        events.iter().find_map(|e| match e {
            LeadEvent::Reply { text, .. } => Some(text.clone()),
            _ => None,
        })
    };
    let claude = reply(ProviderId::Claude, claude_answer("s", "keys:{keys}"));
    let codex = reply(
        ProviderId::Codex,
        vec![
            json!({ "type": "item.completed", "item": { "type": "agent_message", "text": "keys:{keys}" } }).to_string(),
            json!({ "type": "turn.completed", "usage": {} }).to_string(),
        ],
    );
    for name in planted {
        std::env::remove_var(name);
    }
    for answer in [claude.unwrap(), codex.unwrap()] {
        for name in planted {
            assert!(answer.to_ascii_lowercase().contains(&name.to_ascii_lowercase()), "{answer}");
        }
    }
}

/// A Windows junction (no special rights needed), made with the system's own mklink.
#[cfg(windows)]
fn junction(target: &Path, link: &Path) -> bool {
    Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().is_ok_and(|o| o.status.success())
}

/// A folder link to a sign-in folder hides `.claude` from the path as written: an ordinary
/// file under it must still be refused, whether the alias is a symlink or a junction.
fn attachment_parent_links() {
    let (home, project) = setup("parents");
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let private = PathBuf::from(&project).join(".claude");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::write(private.join("notes.txt"), "private").unwrap();
    // The same file named directly is refused already.
    assert!(files::import(&home, &thread, &private.join("notes.txt")).is_err());

    let mut aliases = Vec::new();
    let symlink = PathBuf::from(&project).join("alias-link");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(&private, &symlink).is_ok();
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&private, &symlink).is_ok();
    if made {
        aliases.push(symlink);
    } else {
        println!("  (symlinks not permitted here: symlink case skipped)");
    }
    #[cfg(windows)]
    {
        let joined = PathBuf::from(&project).join("alias-junction");
        assert!(junction(&private, &joined), "mklink /J failed");
        aliases.push(joined);
    }
    for alias in aliases {
        let through = alias.join("notes.txt");
        assert_eq!(std::fs::read_to_string(&through).unwrap(), "private", "the alias reaches the file");
        let refused = files::import(&home, &thread, &through);
        assert!(refused.is_err(), "{} was attached: {refused:?}", through.display());
    }
    // A plain nested folder still works.
    let plain = PathBuf::from(&project).join("docs");
    std::fs::create_dir_all(&plain).unwrap();
    std::fs::write(plain.join("spec.md"), "# spec").unwrap();
    assert!(files::import(&home, &thread, &plain.join("spec.md")).is_ok());
    // Any link on the way is refused, even one to an ordinary folder.
    #[cfg(windows)]
    {
        let to_plain = PathBuf::from(&project).join("docs-junction");
        assert!(junction(&plain, &to_plain));
        assert!(files::import(&home, &thread, &to_plain.join("spec.md")).unwrap_err().contains("링크"));
    }
}

/// The value after `flag` in an argv, if the flag is there.
fn arg_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(String::as_str)
}

/// A repository's own settings may hold hooks that `claude -p` runs before anyone trusted it
/// (code.claude.com/docs/en/permissions#what-runs-before-you-trust-a-folder): the lead reads
/// user settings only and turns hooks off, on the first turn and on every resumed one.
/// Codex turns its hooks off too; agy, which has no such switch, refuses a project with hooks.
fn project_hooks_off() {
    let (home, project) = setup("hooks");
    let (spawner, seen) = fake(claude_answer("sess-h", "네"));
    lead::send(&home, &request(&project, "처음"), spawner.as_ref()).unwrap();
    lead::send(&home, &request(&project, "이어서"), spawner.as_ref()).unwrap();
    let specs = seen.lock().unwrap().clone();
    assert_eq!(arg_after(&specs[1].args, "--resume"), Some("sess-h"));
    for spec in &specs {
        assert_eq!(arg_after(&spec.args, "--setting-sources"), Some("user"), "{:?}", spec.args);
        let settings: Value = serde_json::from_str(arg_after(&spec.args, "--settings").expect("--settings")).unwrap();
        // Hooks off either way; with O's approval server the asked tools come with it (ADR-0008 개정 3).
        assert_eq!(settings["disableAllHooks"], true);
        assert!(settings.as_object().unwrap().keys().all(|k| k == "disableAllHooks" || k == "permissions"), "{settings}");
        assert!(settings.get("hooks").is_none() && settings["permissions"].get("allow").is_none());
    }

    let codex_lines = vec![
        json!({ "type": "thread.started", "thread_id": "th-h" }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "agent_message", "text": "네" } }).to_string(),
        json!({ "type": "turn.completed", "usage": {} }).to_string(),
    ];
    let (codex, seen) = fake(codex_lines);
    let req = SendRequest { provider: ProviderId::Codex, ..request(&project, "codex 처음") };
    lead::send(&home, &req, codex.as_ref()).unwrap();
    lead::send(&home, &SendRequest { text: "codex 이어서", ..req }, codex.as_ref()).unwrap();
    let specs = seen.lock().unwrap().clone();
    let sent: Value = serde_json::from_slice(&std::fs::read(Path::new(&project).join("fake-codex-input.json")).unwrap()).unwrap();
    assert_eq!(sent["thread"]["method"], "thread/resume");
    assert_eq!(sent["thread"]["params"]["threadId"], "th-h");
    for spec in &specs {
        let at = spec.args.iter().position(|a| a == "--disable").expect("--disable");
        assert_eq!(spec.args[at + 1], "hooks");
    }

    let hooks = PathBuf::from(&project).join(".agents");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::write(hooks.join("hooks.json"), "{}").unwrap();
    let (agy, seen) = fake(vec![]);
    let refused = lead::send(&home, &SendRequest { provider: ProviderId::Agy, ..request(&project, "agy") }, agy.as_ref()).unwrap_err();
    assert!(refused.contains("Antigravity"), "{refused}");
    assert!(seen.lock().unwrap().is_empty());
}

/// A spawner that takes `delay_ms` before handing over the command (a turn still setting
/// up: attachments, budget), whose fake CLI makes `ran` if it ever runs.
fn starting_spawner(lines: Vec<String>, ran: PathBuf, delay_ms: u64) -> Arc<Spawner> {
    Arc::new(move |_spec: &LaunchSpec, cwd: &str| {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        let mut command = support::fake_worker_launch()?;
        command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap()).env(FAKE_RAN, &ran);
        Ok(command)
    })
}

/// What a turn's events say, as `kind: text` lines.
fn summary(events: &[LeadEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            LeadEvent::Reply { text, .. } => format!("reply: {text}"),
            LeadEvent::Error { text, .. } => format!("error: {text}"),
            LeadEvent::User { .. } => "user".into(),
            _ => "other".into(),
        })
        .collect()
}

/// This binary as the turn's owner in a process of its own (the app): it sends, and its
/// spawner takes a while, so a stop from the test process lands while the turn starts.
fn owner_process(job: &str) {
    let job: Value = serde_json::from_str(job).unwrap();
    let text = |k: &str| job[k].as_str().unwrap().to_string();
    let (home, project) = (PathBuf::from(text("home")), text("project"));
    let spawner = starting_spawner(claude_answer("s", "다 했습니다"), PathBuf::from(text("ran")), 2_000);
    let result = lead::send(&home, &request(&project, "오래 걸리는 시작"), spawner.as_ref()).map(|events| summary(&events));
    std::fs::write(text("out"), serde_json::to_string(&result).unwrap()).unwrap();
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !done() {
        assert!(std::time::Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn is_busy(home: &Path, project: &str) -> bool {
    !lead::show(home, Some(project), None).unwrap()["thread"]["busy"].is_null()
}

fn stop_scopes() {
    let (home, project) = setup("stop-scopes");
    let other = home.parent().unwrap().join("other");
    std::fs::create_dir_all(&other).unwrap();
    let other = other.to_string_lossy().into_owned();
    let first = lead::start(&home, &project).unwrap();
    let second = lead::new_thread(&home, &project).unwrap();
    let third = lead::start(&home, &other).unwrap();
    let (entered_a, release_a) = (home.join("entered-a"), home.join("release-a"));
    let (entered_b, release_b) = (home.join("entered-b"), home.join("release-b"));
    let (entered_c, release_c) = (home.join("entered-c"), home.join("release-c"));
    let (entered_d, release_d) = (home.join("entered-d"), home.join("release-d"));
    let (entered_e, release_e) = (home.join("entered-e"), home.join("release-e"));
    let a = gated(claude_answer("a", "unused"), entered_a.clone(), release_a.clone());
    let b = gated(claude_answer("b", "unused"), entered_b.clone(), release_b.clone());
    let c = gated(claude_answer("c", "unused"), entered_c.clone(), release_c.clone());
    let d = gated(claude_answer("d", "unused"), entered_d.clone(), release_d.clone());
    let e = gated(claude_answer("e", "unused"), entered_e.clone(), release_e.clone());
    let req_a = SendRequest { thread: Some(&first), ..request(&project, "first") };
    let req_b = SendRequest { thread: Some(&second), ..request(&project, "second") };
    let req_c = SendRequest { thread: Some(&third), ..request(&other, "third") };
    std::thread::scope(|scope| {
        let turn_a = scope.spawn(|| lead::send(&home, &req_a, a.as_ref()));
        let turn_b = scope.spawn(|| lead::send(&home, &req_b, b.as_ref()));
        let turn_c = scope.spawn(|| lead::send(&home, &req_c, c.as_ref()));
        wait_until("all three tabs to start", || entered_a.exists() && entered_b.exists() && entered_c.exists());
        assert!(lead::stop(&home, Some(&other), Some(&first)).is_err());
        assert!(lead::stop(&home, None, Some(&first)).unwrap());
        std::fs::write(&release_a, "").unwrap();
        assert!(turn_a.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))));
        assert_eq!(lead::active_count(&home).unwrap(), 2);
        let turn_d = scope.spawn(|| lead::send(&home, &req_a, d.as_ref()));
        wait_until("the restarted first tab", || entered_d.exists());
        assert!(lead::stop(&home, Some(&project), None).unwrap());
        std::fs::write(&release_b, "").unwrap();
        std::fs::write(&release_d, "").unwrap();
        assert!(turn_b.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))));
        assert!(turn_d.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))));
        assert_eq!(lead::active_count(&home).unwrap(), 1);
        assert!(!lead::stop(&home, Some(&project), None).unwrap());
        let turn_e = scope.spawn(|| lead::send(&home, &req_a, e.as_ref()));
        wait_until("two projects to be active again", || entered_e.exists());
        assert!(lead::stop(&home, None, None).unwrap());
        std::fs::write(&release_c, "").unwrap();
        std::fs::write(&release_e, "").unwrap();
        assert!(turn_c.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))));
        assert!(turn_e.join().unwrap().unwrap().iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("중지"))));
    });
    assert_eq!(lead::active_count(&home).unwrap(), 0);
    assert!(!lead::stop(&home, None, None).unwrap());
}

/// "중지" pressed while the turn still sets up (before its CLI starts): the CLI never runs,
/// the turn ends as stopped, and the stop never ends the turn's owner — whether the stop
/// comes from the owner's own process or from another one (`o lead stop` beside the app).
fn stop_while_starting() {
    // Another process owns the turn.
    let (home, project) = setup("stop-other");
    lead::thread_for(&home, &project, false).unwrap();
    let base = home.parent().unwrap().to_path_buf();
    let (ran, out) = (base.join("ran"), base.join("out.json"));
    let job = json!({ "home": home, "project": project, "ran": ran, "out": out });
    let mut owner = Command::new(std::env::current_exe().unwrap()).env(OWNER, job.to_string()).spawn().unwrap();
    wait_until("the owner to take the turn", || is_busy(&home, &project));
    assert!(lead::stop(&home, Some(&project), None).unwrap());
    let status = owner.wait().unwrap();
    assert!(status.success(), "the stop ended the turn's owner: {status:?}");
    let result: Result<Vec<String>, String> = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let lines = result.unwrap();
    assert!(lines.iter().any(|l| l.contains("중지")), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("reply")), "{lines:?}");
    assert!(!ran.exists(), "the CLI ran after the stop");

    // The owner's own process stops it (the app's 중지 button).
    let (home, project) = setup("stop-same");
    lead::thread_for(&home, &project, false).unwrap();
    let ran = home.parent().unwrap().join("ran");
    let spawner = starting_spawner(claude_answer("s", "다 했습니다"), ran.clone(), 1_500);
    let turn = {
        let (home, project) = (home.clone(), project.clone());
        std::thread::spawn(move || lead::send(&home, &request(&project, "시작 중 중지"), spawner.as_ref()).map(|e| summary(&e)))
    };
    wait_until("the turn to start", || is_busy(&home, &project));
    assert!(lead::stop(&home, Some(&project), None).unwrap());
    let lines = turn.join().unwrap().unwrap();
    assert!(lines.iter().any(|l| l.contains("중지")) && !lines.iter().any(|l| l.starts_with("reply")), "{lines:?}");
    assert!(!ran.exists(), "the CLI ran after the stop");
    // A stop left from that turn does not stop the next one.
    let (spawner, _) = fake(claude_answer("s", "다음 턴"));
    let lines = summary(&lead::send(&home, &request(&project, "다음"), spawner.as_ref()).unwrap());
    assert!(lines.contains(&"reply: 다음 턴".to_string()), "{lines:?}");
}

/// A stop ends the live owner's CLI; an orphan's PID alone never authorizes termination.
fn stop_running_and_orphans() {
    let (home, project) = setup("stop-run");
    let (spawner, seen) = fake_slow(claude_answer("s", "늦은 답"), 20_000);
    let turn = {
        let (home, project, spawner) = (home.clone(), project.clone(), spawner.clone());
        std::thread::spawn(move || lead::send(&home, &request(&project, "오래"), spawner.as_ref()).map(|e| summary(&e)))
    };
    wait_until("the CLI to start", || !seen.lock().unwrap().is_empty());
    std::thread::sleep(std::time::Duration::from_millis(200));
    let at = std::time::Instant::now();
    assert!(lead::stop(&home, Some(&project), None).unwrap());
    let lines = turn.join().unwrap().unwrap();
    assert!(at.elapsed() < std::time::Duration::from_secs(10), "the CLI kept running after the stop");
    assert!(lines.iter().any(|l| l.contains("중지")), "{lines:?}");

    let thread = lead::current_thread(&home, &project).unwrap();
    let marker = lead::thread_dir(&home, &thread).join("running.json");
    let mut orphan = bystander();
    let pid = orphan.id();
    let started = o_core::sys::proc::started_at(pid).expect("start time");
    let mark = |started: u64| json!({ "provider": "claude", "owner": 4_000_000_000u32, "token": "gone", "child": { "pid": pid, "started": started } });
    std::fs::write(&marker, mark(started + 1).to_string()).unwrap();
    assert!(!lead::stop(&home, Some(&project), None).unwrap());
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(orphan.try_wait().unwrap().is_none(), "a process whose start time differs was ended");
    std::fs::write(&marker, mark(started).to_string()).unwrap();
    assert!(!lead::stop(&home, Some(&project), None).unwrap());
    assert!(orphan.try_wait().unwrap().is_none());
    assert!(marker.exists());
    orphan.kill().unwrap();
    orphan.wait().unwrap();
}

/// A CLI that hangs does not hold the thread forever: past the turn's limit it is ended and
/// the thread says so (the desktop had no limit; the dev server stopped waiting at 15 minutes).
fn turn_time_limit() {
    let (home, project) = setup("limit");
    let (spawner, _) = fake_slow(claude_answer("s", "너무 늦은 답"), 30_000);
    let at = std::time::Instant::now();
    let events = lead::send_within(&home, &request(&project, "멈춘 CLI"), spawner.as_ref(), std::time::Duration::from_secs(1)).unwrap();
    assert!(at.elapsed() < std::time::Duration::from_secs(15), "waited {:?}", at.elapsed());
    let lines = summary(&events);
    assert!(lines.iter().any(|l| l.contains("시간")), "{lines:?}");
    assert!(!lines.iter().any(|l| l.starts_with("reply")), "{lines:?}");
    assert!(!is_busy(&home, &project));
    assert_eq!(lead::TURN_LIMIT, std::time::Duration::from_secs(15 * 60));
}
/// Claude → Codex → Claude: the resumed Claude session never saw the Codex turns, so its
/// prompt carries them (here a constraint the person gave Codex); turns it saw are not repeated.
fn round_trip() {
    let (home, project) = setup("round-trip");
    let (claude, seen) = fake(claude_answer("sess-r", "어느 파일인가요?"));
    lead::send(&home, &request(&project, "README 고쳐 줘"), claude.as_ref()).unwrap();
    let (codex, _) = fake(vec![
        json!({ "type": "thread.started", "thread_id": "th-r" }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "agent_message", "text": "알겠습니다. main 브랜치는 건드리지 않겠습니다" } }).to_string(),
        json!({ "type": "turn.completed", "usage": {} }).to_string(),
    ]);
    lead::send(&home, &SendRequest { provider: ProviderId::Codex, ..request(&project, "main 브랜치는 절대 건드리지 마") }, codex.as_ref()).unwrap();
    lead::send(&home, &request(&project, "이제 계획 세워 줘"), claude.as_ref()).unwrap();
    let specs = seen.lock().unwrap().clone();
    assert_eq!(arg_after(&specs[1].args, "--resume"), Some("sess-r"));
    let prompt = specs[1].stdin.clone().unwrap();
    assert!(prompt.contains("main 브랜치는 절대 건드리지 마"), "{prompt}");
    assert!(prompt.contains("main 브랜치는 건드리지 않겠습니다"), "{prompt}");
    assert!(!prompt.contains("README 고쳐 줘"), "a turn the session saw is sent again: {prompt}");
    // Once it has heard them, the next resumed turn does not repeat them.
    lead::send(&home, &request(&project, "좋아"), claude.as_ref()).unwrap();
    let prompt = seen.lock().unwrap()[2].stdin.clone().unwrap();
    assert!(!prompt.contains("main 브랜치는 절대"), "{prompt}");
    // A resumed CLI that never started heard nothing: the next turn tells it that message too.
    let (dead, _) = fake(Vec::new());
    let events = lead::send(&home, &request(&project, "테스트도 넣어 줘"), dead.as_ref()).unwrap();
    assert!(!events.iter().any(|e| matches!(e, LeadEvent::Seen { .. })), "{events:?}");
    lead::send(&home, &request(&project, "다시 보냅니다"), claude.as_ref()).unwrap();
    let prompt = seen.lock().unwrap()[3].stdin.clone().unwrap();
    assert!(prompt.contains("테스트도 넣어 줘"), "{prompt}");
}
/// `leads/current.json` (project → its thread) is replaced whole. One that cannot be read is
/// not taken as empty: a new thread is not started over it (which dropped every other
/// project's thread), and starting threads for many projects at once loses none.
fn current_list() {
    let (home, project) = setup("current");
    let first = lead::thread_for(&home, &project, false).unwrap();
    let list = home.join("leads").join("current.json");
    for broken in ["", "{\"c:/x\": \"20"] {
        std::fs::write(&list, broken).unwrap();
        assert!(lead::thread_for(&home, &project, false).is_err(), "{broken:?}");
        assert!(lead::start(&home, &project).is_err(), "{broken:?}");
        assert_eq!(std::fs::read_to_string(&list).unwrap(), broken, "a broken list was overwritten");
        assert!(lead::show(&home, Some(&project), None).is_err());
    }
    std::fs::remove_file(&list).unwrap();
    let open = home.join("leads/open.json");
    let saved = std::fs::read(&open).unwrap();
    for broken in ["", "{\"project\":{"] {
        std::fs::write(&open, broken).unwrap();
        assert!(lead::start(&home, &project).is_err());
        assert!(lead::close(&home, &first).is_err());
        assert!(lead::set_writer(&home, &first).is_err());
        assert_eq!(std::fs::read_to_string(&open).unwrap(), broken);
    }
    std::fs::write(&open, saved).unwrap();
    std::fs::remove_file(&open).unwrap();
    // Many projects at once: every one keeps its thread.
    let base = home.parent().unwrap().to_path_buf();
    let folders: Vec<String> = (0..8)
        .map(|i| {
            let dir = base.join(format!("p{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            dir.to_string_lossy().into_owned()
        })
        .collect();
    let gate = Arc::new(std::sync::Barrier::new(folders.len()));
    let handles: Vec<_> = folders
        .iter()
        .map(|folder| {
            let (home, folder, gate) = (home.clone(), folder.clone(), gate.clone());
            std::thread::spawn(move || {
                gate.wait();
                lead::thread_for(&home, &folder, false).unwrap()
            })
        })
        .collect();
    let made: Vec<String> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for (folder, thread) in folders.iter().zip(&made) {
        assert_eq!(lead::current_thread(&home, folder).as_ref(), Some(thread), "{folder} lost its thread");
        assert_eq!(lead::open_threads(&home, folder), std::slice::from_ref(thread));
    }
    assert_ne!(lead::thread_for(&home, &project, false).unwrap(), first, "the list was removed above, so a new thread starts");
}
/// `o runs --sessions` asks every 2 seconds which CLI sessions are the lead's: that comes from
/// each thread's small `sessions.txt`, not from re-reading every thread's events. A thread
/// from before the index gets one from its events, once.
fn session_index() {
    let (home, project) = setup("sessions");
    let (spawner, _) = fake(claude_answer("sess-ix", "네"));
    lead::send(&home, &request(&project, "안녕"), spawner.as_ref()).unwrap();
    let thread = lead::current_thread(&home, &project).unwrap();
    let dir = lead::thread_dir(&home, &thread);
    // The events are not what is read: replaced, the answer stays.
    std::fs::write(dir.join("events.jsonl"), "not events\n").unwrap();
    assert_eq!(lead::session_ids(&home), ["sess-ix"]);

    // An older thread with no index: read from its events, then indexed.
    let old = lead::thread_for(&home, &PathBuf::from(&project).join("old").to_string_lossy(), false).unwrap();
    let old_dir = lead::thread_dir(&home, &old);
    let _ = std::fs::remove_file(old_dir.join("sessions.txt"));
    let line = json!({ "t": "lead.session", "at": "2026-11-01T00:00:00.000Z", "provider": "codex", "sessionId": "th-old" });
    std::fs::OpenOptions::new().append(true).open(old_dir.join("events.jsonl")).and_then(|mut f| std::io::Write::write_all(&mut f, format!("{line}\n").as_bytes())).unwrap();
    let mut ids = lead::session_ids(&home);
    ids.sort();
    assert_eq!(ids, ["sess-ix", "th-old"]);
    assert!(std::fs::read_to_string(old_dir.join("sessions.txt")).unwrap().contains("th-old"));
}

/// An older Codex CLI (no app-server) leads through the earlier read-only `codex exec`, probed
/// once per CLI. The conversation id is the same in both transports, so a thread that started
/// on exec resumes on app-server (after an update) and back again (after a downgrade).
fn codex_exec_fallback() {
    let (home, project) = setup("codex-exec-fallback");
    let log = home.join("probes.log");
    let lines = vec![
        json!({ "type": "thread.started", "thread_id": "th-old" }).to_string(),
        json!({ "type": "item.completed", "item": { "type": "agent_message", "text": "네 {keys}" } }).to_string(),
        json!({ "type": "turn.completed", "usage": { "input_tokens": 3, "cached_input_tokens": 1, "output_tokens": 2 } }).to_string(),
    ];
    let (old, seen) = fake_without_app_server(lines.clone(), "exec-fallback", log.clone());
    let req = SendRequest { provider: ProviderId::Codex, effort: None, ..request(&project, "codex 처음") };
    let events = lead::send(&home, &req, old.as_ref()).unwrap();
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text.starts_with("네"))), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Session { session_id, .. } if session_id == "th-old")));
    lead::send(&home, &SendRequest { text: "codex 이어서", ..req }, old.as_ref()).unwrap();
    let specs = seen.lock().unwrap().clone();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].args, ["exec", "-s", "read-only", "-C", project.as_str(), "--skip-git-repo-check", "--disable", "hooks", "--json", "-"]);
    assert!(specs[0].stdin.as_deref().unwrap().contains("codex 처음"));
    assert_eq!(specs[1].args, ["exec", "-s", "read-only", "-C", project.as_str(), "--skip-git-repo-check", "--disable", "hooks", "resume", "--json", "th-old", "-"]);
    // Probed once for this CLI, not on every turn.
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1);

    // Updated to a CLI with app-server: the same conversation resumes there.
    let (new, seen) = fake(lines.clone());
    lead::send(&home, &SendRequest { text: "업데이트 후", ..req }, new.as_ref()).unwrap();
    assert_eq!(seen.lock().unwrap()[0].args, ["app-server", "--disable", "hooks"]);
    let sent: Value = serde_json::from_slice(&std::fs::read(Path::new(&project).join("fake-codex-input.json")).unwrap()).unwrap();
    assert_eq!(sent["thread"]["method"], "thread/resume");
    assert_eq!(sent["thread"]["params"]["threadId"], "th-old");

    // And back on a CLI without it, exec resumes the conversation app-server kept.
    let (back, seen) = fake_without_app_server(lines, "exec-fallback-again", log.clone());
    lead::send(&home, &SendRequest { text: "다시 예전 CLI", ..req }, back.as_ref()).unwrap();
    assert_eq!(arg_after(&seen.lock().unwrap()[0].args, "--json"), Some("th-old"));
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 2);
}
