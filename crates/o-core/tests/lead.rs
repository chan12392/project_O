//! The lead chat with a fake lead CLI. The fake is this test binary itself: started
//! with O_FAKE_LEAD_LINES it prints those lines and exits, so no real (paid) CLI runs.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::io::{BufRead, Write};

use chrono::Utc;
use o_core::lead::files::{self, MAX_BYTES};
use o_core::lead::prompt::{build, split_plan, PromptInput};
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
    let tests: [(&str, fn()); 34] = [
        ("follow-ups only target cards dispatched by this thread", followup_routing),
        ("a plan for a non-Git project can be dispatched", dispatch_non_git),
        ("tool progress is visible during the turn and cleared afterwards", live_tool_progress),
        ("design snapshots persist and malformed ones do not dispatch", design_snapshots),
        ("design notes persist, reach the lead and preserve concurrent edits", design_notes),
        ("only the latest plan in the current thread can dispatch", stale_plan_refused),
        ("automatic dispatch respects mode, running work and the daily budget", autorun),
        ("an automatic dispatch that fails to start uses up no auto-run limit", autorun_failures_not_counted),
        ("a new thread cannot replace a live lead", autorun_new_thread_cancels),
        ("an automatic dispatch never starts a plan a newer request replaced", autorun_newer_request_wins),
        ("lead child receives its agent marker", lead_child_marker),
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
        ("attachments: type, size, name and link refusals", attachments),
        ("sends started together run one turn; the others are refused", busy),
        ("a dead turn's marker remains unknown, even with its pid reused", stale_marker),
        ("bad requests start nothing", refused_requests),
        ("the lead CLI never inherits an API key variable of its provider", api_keys_removed),
        ("no project hook runs before a plan is approved, first turn or resumed", project_hooks_off),
        ("an attachment reached through a linked parent folder is refused", attachment_parent_links),
        ("a stop while the turn starts keeps the CLI from running and never ends the owner", stop_while_starting),
        ("a stop ends a running CLI but does not trust an orphan's pid", stop_running_and_orphans),
        ("a turn that outlasts its time limit is ended and says so", turn_time_limit),
        ("back from another agent, a resumed session hears the turns it missed", round_trip),
        ("the thread list is written whole and a broken one is never overwritten", current_list),
        ("the lead's CLI sessions are listed from a small index, not every thread's events", session_index),
    ];
    let extra: [(&str, fn()); 6] = [
        ("a Codex CLI without app-server leads through read-only exec, probed once, resuming across transports", codex_exec_fallback),
        ("Codex reports an interrupted turn rather than its partial answer", codex_interrupted),
        ("Codex declines read-only approval requests and continues", codex_approval),
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
    let plan_id = plans(&events).remove(0).0;
    let dispatched = lead::dispatch(&home, &plan_id, RunMode::of(Profile::Own), &|_| Ok(())).ok().unwrap();
    let answer = format!("수정 요청을 전달합니다.\n```o-followup\n[{{\"run\":\"{}\",\"card\":\"docs\",\"task\":\"제목 수정\"}}]\n```", dispatched.run_id);
    let (spawner, _) = fake(claude_answer("s", &answer));
    let result = lead::send_with_followups(&home, &request(&project, "제목 수정해줘"), spawner.as_ref(), &resume).unwrap();
    assert!(result.iter().any(|e| matches!(e, LeadEvent::Followup { source_run_id, card_id, .. } if source_run_id == &dispatched.run_id && card_id == "docs")));
    assert_eq!(started.lock().unwrap().len(), 1);
    let (bad, _) = fake(claude_answer("s", "```o-followup\n[{\"run\":\"foreign\",\"card\":\"docs\",\"task\":\"수정\"}]\n```"));
    let refused = lead::send_with_followups(&home, &request(&project, "수정해줘"), bad.as_ref(), &resume).unwrap();
    assert!(refused.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("이 대화"))));
    assert_eq!(started.lock().unwrap().len(), 1);
    let followup_run = started.lock().unwrap()[0].clone();
    assert_eq!(lead::stop_all(&home, &project).unwrap(), 2);
    assert!(o_core::run::store::stop_requested(&home, &followup_run));
    // A new design is a new thread: work dispatched by the old one is not continued from it.
    lead::new_thread(&home, &project).unwrap();
    let (spawner, _) = fake(claude_answer("s2", &answer));
    let refused = lead::send_with_followups(&home, &request(&project, "제목 수정해줘"), spawner.as_ref(), &resume).unwrap();
    assert!(refused.iter().any(|e| matches!(e, LeadEvent::Error { text, .. } if text.contains("이 대화"))));
    assert_eq!(started.lock().unwrap().len(), 1);
    let example = "````markdown\n```o-followup\n[]\n```\n````";
    assert!(o_core::lead::prompt::split_followups(example).1.unwrap().is_none());
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
    lead::new_thread(&home, &project).unwrap();
    assert!(matches!(lead::dispatch(&home, &second, RunMode::of(Profile::Own), &start), Err(DispatchError::Other(m)) if m.contains("설계가 변경")));
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
                assert!(reply["error"].is_object() && reply.get("result").is_none(), "{reply}");
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
fn probe_command(spec: &LaunchSpec, cwd: &str, probe: &FakeProbe) -> Option<std::io::Result<Command>> {
    if spec.bin != "codex" || spec.args != o_core::providers::codex::APP_SERVER_PROBE { return None; }
    Some(std::env::current_exe().map(|exe| {
        let mut command = Command::new(exe);
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
        let mut command = Command::new(std::env::current_exe()?);
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
    json!({ "title": "README 한 줄", "cards": [{ "id": "docs", "provider": "claude", "effort": "low", "task": "README.md 끝에 한 줄 추가" }] })
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
            let mut command = Command::new(std::env::current_exe()?);
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap()).env(FAKE_PAUSE, "500");
            Ok(command)
        };
        lead::send(&worker_home, &request(&worker_project, "읽어줘"), &spawner).unwrap();
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let view = lead::show(&home, Some(&project));
        if view["thread"]["progressCounts"] == json!({ "reads": 1, "searches": 0, "edits": 1, "commands": 1 }) {
            assert!(view["thread"]["progress"].as_array().unwrap().iter().any(|item| item["name"] == "Read" && item["summary"] == "src/demo.py"));
            break;
        }
        assert!(std::time::Instant::now() < deadline, "tool progress never became visible: {view}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    worker.join().unwrap();
    let view = lead::show(&home, Some(&project));
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
            let mut command = Command::new(std::env::current_exe()?);
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap())
                .env(FAKE_APP_SERVER, "1").env(FAKE_PAUSE, "500");
            Ok(command)
        };
        let req = SendRequest { provider: ProviderId::Codex, ..request(&worker_project, "확인") };
        lead::send(&worker_home, &req, &spawner).unwrap()
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let view = lead::show(&home, Some(&project));
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
        let mut command = Command::new(std::env::current_exe()?);
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
            let mut command = Command::new(std::env::current_exe()?);
            command.current_dir(cwd).env(FAKE, serde_json::to_string(&lines).unwrap())
                .env(FAKE_APP_SERVER, "1").env(FAKE_PAUSE, "500");
            Ok(command)
        };
        let req = SendRequest { provider: ProviderId::Codex, ..request(&worker_project, "계획") };
        lead::send(&worker_home, &req, &spawner).unwrap()
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        let view = lead::show(&home, Some(&project));
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
    assert_eq!(lead::show(&home, Some(&project))["thread"]["partial"], "");
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

fn auto_reason(events: &[LeadEvent], part: &str) -> bool {
    events.iter().any(|e| matches!(e, LeadEvent::AutoRun { reason: Some(reason), .. } if reason.contains(part)))
}

fn titled_plan(title: &str) -> Value {
    json!({ "title": title, "cards": [{ "id": "docs", "provider": "claude", "effort": "low", "task": "README.md 끝에 한 줄 추가" }] })
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
        while !release.exists() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let mut command = Command::new(std::env::current_exe()?);
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
    let yolo = SendRequest { yolo: true, ..request(&project, "진행") };
    let events = std::thread::scope(|scope| {
        let turn = scope.spawn(|| lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start));
        wait_until("the lead to start answering", || entered.exists());
        // Another client cannot replace a lead that is still answering.
        assert!(lead::start(&home, &project).is_err());
        std::fs::write(&release, "").unwrap();
        turn.join().unwrap().unwrap()
    });
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Plan { ok: true, .. })));
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Dispatch { .. })));
    assert_eq!(starts.load(Ordering::SeqCst), 1);
    // On the current thread, the latest plan still runs exactly once.
    let (spawner, _) = fake(claude_answer("session", &plan_answer(good_plan())));
    let run = lead::send_with_autorun(&home, &yolo, spawner.as_ref(), &start).unwrap();
    assert_eq!(run.iter().filter(|e| matches!(e, LeadEvent::Dispatch { .. })).count(), 1);
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
    SendRequest { project, provider: ProviderId::Claude, model: None, effort: Some("low"), text, attach: &[], fresh: false, clean: None, yolo: false }
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
    let input = PromptInput { project: "C:/p", text: "할 일", attachments: &[], budget: &["codex 여유 있음".into()], runs: &[], runs_file: None, runs_error: None, result_tools: false, resumed: false, history: &[], missed: &[], design: None, yolo: false };
    let first = build(&input);
    for part in ["[O 총괄]", "프로젝트: C:/p", "```o-plan", "읽기만", "codex 여유 있음", "사용자:\n할 일"] {
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
            LeadEvent::Seen { .. } => "seen",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["user", "session", "seen", "usage", "reply", "plan"]);
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
    for at in ["thread", "turn"] {
        assert_eq!(sent[at]["params"]["approvalPolicy"], "never");
        assert_eq!(sent[at]["params"]["approvalsReviewer"], "user");
    }
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
    lead::start(&home, &project).unwrap();
    assert_eq!(lead::active_count(&home).unwrap(), 1, "a superseded thread still owns its running turn");
    // Return to the original thread for the remaining busy checks.
    drop(held);
    let thread = lead::thread_for(&home, &project, false).unwrap();
    let dir = lead::thread_dir(&home, &thread);
    let held = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("turn.lock")).unwrap();
    held.try_lock().unwrap();
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    for partial in ["", "{\"provider\":\"cla"] {
        std::fs::write(dir.join("running.json"), partial).unwrap();
        let refused = lead::send(&home, &request(&project, "또"), spawner.as_ref()).unwrap_err();
        assert!(refused.contains("답하는 중"), "{partial:?}: {refused}");
        assert!(!lead::show(&home, Some(&project))["thread"]["busy"].is_null(), "{partial:?}");
        assert!(dir.join("running.json").exists(), "another sender removed the owner's marker");
    }
    assert!(seen.lock().unwrap().is_empty());
    // Its owner gone (the OS lets go of the lock with the process), the thread is free.
    drop(held);
    lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);

    const SENDERS: usize = 8;
    for round in 0..4 {
        let (home, project) = setup(&format!("busy{round}"));
        lead::thread_for(&home, &project, false).unwrap();
        let (spawner, seen) = fake_slow(claude_answer("s", "ok"), 1_500);
        let gate = Arc::new(std::sync::Barrier::new(SENDERS + 1));
        let handles: Vec<_> = (0..SENDERS)
            .map(|_| {
                let (home, project, spawner, gate) = (home.clone(), project.clone(), spawner.clone(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    lead::send(&home, &request(&project, "동시에"), spawner.as_ref())
                })
            })
            .collect();
        gate.wait();
        // While the one turn answers, the thread is busy.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while seen.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(lead::show(&home, Some(&project))["thread"]["busy"], "claude", "round {round}");
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let started = results.iter().filter(|r| r.is_ok()).count();
        assert_eq!((started, seen.lock().unwrap().len()), (1, 1), "round {round}: {results:?}");
        for refused in results.iter().filter_map(|r| r.as_ref().err()) {
            assert!(refused.contains("답하는 중"), "{refused}");
        }
        assert!(lead::show(&home, Some(&project))["thread"]["busy"].is_null());
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
    assert_eq!(lead::show(&home, Some(&project))["thread"]["busy"], "unknown");
    assert!(!lead::stop(&home, Some(&project)));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(other.try_wait().unwrap().is_none(), "stop ended a process that only reused the dead turn's pid");
    let (spawner, seen) = fake(claude_answer("s", "ok"));
    assert!(lead::send(&home, &request(&project, "다시"), spawner.as_ref()).unwrap_err().contains("종료를 확인"));
    assert!(lead::new_thread(&home, &project).is_err());
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
}

fn api_keys_removed() {
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
    // Each CLI loses its own keys (any spelling) and keeps the other's, which it never reads.
    assert_eq!(claude.as_deref(), Some("keys:CODEX_API_KEY"));
    let codex = codex.unwrap();
    assert!(codex.contains("anthropic_api_key") && codex.contains("ANTHROPIC_AUTH_TOKEN") && !codex.contains("CODEX_API_KEY"), "{codex}");
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
        assert_eq!(settings, json!({ "disableAllHooks": true }));
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
        let mut command = Command::new(std::env::current_exe()?);
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
    !lead::show(home, Some(project))["thread"]["busy"].is_null()
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
    assert!(lead::stop(&home, Some(&project)));
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
    assert!(lead::stop(&home, Some(&project)));
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
    assert!(lead::stop(&home, Some(&project)));
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
    assert!(!lead::stop(&home, Some(&project)));
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(orphan.try_wait().unwrap().is_none(), "a process whose start time differs was ended");
    std::fs::write(&marker, mark(started).to_string()).unwrap();
    assert!(!lead::stop(&home, Some(&project)));
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
        assert!(lead::show(&home, Some(&project))["thread"].is_null());
    }
    std::fs::remove_file(&list).unwrap();
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
    // The API key of the shell never reaches the lead ("{keys}" stays empty).
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Reply { text, .. } if text == "네")), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, LeadEvent::Session { session_id, .. } if session_id == "th-old")));
    lead::send(&home, &SendRequest { text: "codex 이어서", ..req }, old.as_ref()).unwrap();
    let specs = seen.lock().unwrap().clone();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].args, ["exec", "-s", "read-only", "-C", project.as_str(), "--skip-git-repo-check", "--disable", "hooks", "--json", "-"]);
    assert!(specs[0].stdin.as_deref().unwrap().contains("codex 처음"));
    assert_eq!(specs[1].args, ["exec", "-s", "read-only", "-C", project.as_str(), "--skip-git-repo-check", "--disable", "hooks", "resume", "--json", "th-old", "-"]);
    for spec in &specs {
        assert_eq!(spec.env_remove, ["CODEX_API_KEY"]);
    }
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
