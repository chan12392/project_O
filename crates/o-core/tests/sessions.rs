//! Claude Code transcripts read as runs: synthetic fixtures only (no real transcript).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Duration, TimeZone, Utc};
use o_core::run::events::OEvent;
use o_core::sys::js::iso;
use o_core::run::index::RunQuery;
use o_core::sessions::{read_sessions, transcript_files, with_sessions, with_sessions_for};
use o_core::run::snapshot::{snapshot, CardState, RunStatus, RunView};
use o_core::run::store::RunRecord;
use serde_json::{json, Value};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-sessions-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lines(values: &[Value]) -> String {
    values.iter().map(|v| format!("{v}\n")).collect()
}

fn at(base: DateTime<Utc>, seconds: i64) -> String {
    iso(base + Duration::seconds(seconds))
}

fn view(record: &RunRecord) -> RunView {
    let events: Vec<OEvent> = record.events.iter().map(|e| serde_json::from_value(e.clone()).unwrap()).collect();
    snapshot(&events).unwrap()
}

fn user(at: &str, content: Value) -> Value {
    json!({ "parentUuid": null, "isSidechain": false, "type": "user", "message": { "role": "user", "content": content }, "uuid": format!("u{at}"), "timestamp": at, "cwd": "C:\\work\\app\\.claude\\worktrees\\fix", "sessionId": "s1" })
}

fn assistant(at: &str, content: Value) -> Value {
    json!({ "parentUuid": null, "isSidechain": false, "type": "assistant", "message": { "role": "assistant", "content": content }, "uuid": format!("a{at}"), "timestamp": at, "sessionId": "s1" })
}

fn side(kind: &str, agent: &str, at: &str, content: Value) -> Value {
    json!({ "isSidechain": true, "agentId": agent, "type": kind, "message": { "role": kind, "content": content }, "timestamp": at, "sessionId": "s1" })
}

/// A main thread that starts two subagents: one in front that finishes, one in the background that fails.
fn write_session(root: &Path, base: DateTime<Utc>) -> PathBuf {
    let folder = root.join("projects").join("C--work-app");
    let subagents = folder.join("s1").join("subagents");
    fs::create_dir_all(&subagents).unwrap();
    let notice = "<task-notification>\n<task-id>bbb2</task-id>\n<tool-use-id>toolu_B</tool-use-id>\n<status>failed</status>\n<summary>테스트 돌리기 failed: tests crashed</summary>\n</task-notification>";
    let main = folder.join("s1.jsonl");
    fs::write(
        &main,
        lines(&[
            json!({ "type": "ai-title", "aiTitle": "로그인 고치기", "sessionId": "s1" }),
            user(&at(base, 0), json!("로그인을 고쳐 줘")),
            json!({ "type": "api-request-blob", "hash": "h", "blob": "x".repeat(10_000) }),
            assistant(
                &at(base, 10),
                json!([
                    { "type": "text", "text": "둘로 나눠 봅니다" },
                    { "type": "tool_use", "id": "toolu_A", "name": "Agent", "input": { "description": "코드 찾기", "subagent_type": "Explore", "prompt": "find login" } },
                    { "type": "tool_use", "id": "toolu_B", "name": "Agent", "input": { "description": "테스트 돌리기", "subagent_type": "general-purpose", "prompt": "run tests", "run_in_background": true } }
                ]),
            ),
            json!({ "type": "user", "isSidechain": false, "timestamp": at(base, 11), "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_B", "content": "launched" }] }, "toolUseResult": { "status": "async_launched", "agentId": "bbb2" } }),
            json!({ "type": "user", "isSidechain": false, "timestamp": at(base, 60), "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_A", "content": [{ "type": "text", "text": "found it in src/login.rs" }] }] }, "toolUseResult": { "status": "completed", "agentId": "aaa1" } }),
            json!({ "type": "attachment", "timestamp": at(base, 299), "attachment": { "type": "queued_command", "prompt": notice } }),
            user(&at(base, 300), json!(notice)),
            assistant(&at(base, 310), json!([{ "type": "text", "text": "테스트가 깨졌습니다" }])),
        ]),
    )
    .unwrap();
    fs::write(
        subagents.join("agent-aaa1.jsonl"),
        lines(&[
            side("user", "aaa1", &at(base, 12), json!("find login")),
            side("assistant", "aaa1", &at(base, 20), json!([{ "type": "tool_use", "id": "t1", "name": "Read", "input": { "file_path": "src/login.rs" } }])),
            side("assistant", "aaa1", &at(base, 50), json!([{ "type": "text", "text": "찾았습니다" }])),
        ]),
    )
    .unwrap();
    fs::write(subagents.join("agent-aaa1.meta.json"), json!({ "agentType": "Explore", "description": "코드 찾기", "toolUseId": "toolu_A", "finished": true }).to_string()).unwrap();
    fs::write(
        subagents.join("agent-bbb2.jsonl"),
        lines(&[side("assistant", "bbb2", &at(base, 30), json!([{ "type": "tool_use", "id": "t2", "name": "Bash", "input": { "command": "cargo test", "description": "Run tests" } }]))]),
    )
    .unwrap();
    fs::write(subagents.join("agent-bbb2.meta.json"), json!({ "agentType": "general-purpose", "description": "테스트 돌리기", "toolUseId": "toolu_B", "finished": true }).to_string()).unwrap();
    main
}

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 1, 10, 0, 0).unwrap()
}

/// Files were just written, so a window measured from the real clock finds them.
fn recent() -> (DateTime<Utc>, Duration) {
    (Utc::now(), Duration::hours(24))
}

#[test]
fn main_thread_and_two_subagents_one_failed() {
    let root = temp("two");
    write_session(&root, base());
    let (now, since) = recent();
    let runs = read_sessions(std::slice::from_ref(&root), None, now, since);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, "session-s1");
    let run = view(&runs[0]);
    assert_eq!(run.title, "로그인 고치기");
    assert_eq!(run.project, "C:/work/app", "a worktree session belongs to its repository");
    assert_eq!(run.started_at, at(base(), 0));
    // Quiet for hours: the session ended, and a failed subagent fails it.
    assert_eq!(run.status, RunStatus::Failed);
    assert_eq!(run.ended_at.as_deref(), Some(at(base(), 310).as_str()));

    let ids: Vec<&str> = run.cards.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["aaa1", "bbb2"]);
    let (found, tests) = (&run.cards[0], &run.cards[1]);
    assert_eq!((found.label.as_str(), found.role.as_deref()), ("코드 찾기", Some("Explore")));
    assert_eq!(found.state, CardState::Done);
    assert_eq!(found.result.as_deref(), Some("found it in src/login.rs"));
    assert_eq!(found.started_at.as_deref(), Some(at(base(), 12).as_str()));
    assert!(found.log.iter().any(|l| l.tool.as_ref().is_some_and(|t| t.word == "파일 읽기" && t.detail == "src/login.rs")));
    assert!(found.log.iter().any(|l| l.text == "찾았습니다"));
    assert_eq!((tests.label.as_str(), tests.role.as_deref()), ("테스트 돌리기", Some("general-purpose")));
    assert_eq!(tests.state, CardState::Failed);
    assert_eq!(tests.result.as_deref(), Some("테스트 돌리기 failed: tests crashed"));
    assert_eq!(tests.ended_at.as_deref(), Some(at(base(), 300).as_str()), "the notice as the prompt it became wins over the queued copy");

    // The main thread speaks as the lead.
    let said: Vec<(&str, &str)> = run.messages.iter().map(|m| (m.from.as_str(), m.text.as_str())).collect();
    assert_eq!(said, [("lead", "둘로 나눠 봅니다"), ("lead", "테스트가 깨졌습니다")]);
}

/// Claude Code resumes a subagent with a new Agent call whose result names the same agentId.
fn resume_aaa1(main: &Path, base: DateTime<Utc>, finished: bool) {
    let mut text = fs::read_to_string(main).unwrap();
    text.push_str(&lines(&[assistant(
        &at(base, 320),
        json!([
            { "type": "tool_use", "id": "toolu_C", "name": "Agent", "input": { "description": "코드 더 찾기", "subagent_type": "Explore", "prompt": "keep looking", "resume": "aaa1" } }
        ]),
    )]));
    if finished {
        text.push_str(&lines(&[
            json!({ "type": "user", "isSidechain": false, "timestamp": at(base, 400), "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "toolu_C", "content": "also in src/session.rs" }] }, "toolUseResult": { "status": "completed", "agentId": "aaa1" } }),
        ]));
    }
    fs::write(main, text).unwrap();
}

#[test]
fn a_resumed_subagent_stays_one_card_with_its_latest_call() {
    let root = temp("resumed");
    let main = write_session(&root, base());
    resume_aaa1(&main, base(), true);
    let (now, since) = recent();
    let runs = read_sessions(std::slice::from_ref(&root), None, now, since);
    let start = runs[0].events.iter().find(|e| e["t"] == "run.start").unwrap();
    let ids: Vec<&str> = start["cards"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["aaa1", "bbb2"], "one card per agentId");
    let run = view(&runs[0]);
    let found = &run.cards[0];
    assert_eq!(found.label, "코드 찾기", "the first call names the card");
    assert_eq!(found.state, CardState::Done);
    assert_eq!(found.result.as_deref(), Some("also in src/session.rs"), "the latest call's result");
    assert_eq!(found.ended_at.as_deref(), Some(at(base(), 400).as_str()));
    let ends = runs[0].events.iter().filter(|e| e["t"] == "card.end" && e["cardId"] == "aaa1").count();
    assert_eq!(ends, 1);

    // Resumed and still working: the card runs again until the session goes quiet.
    let live = temp("resumed-live");
    let now = Utc::now();
    let main = write_session(&live, now - Duration::seconds(400));
    resume_aaa1(&main, now - Duration::seconds(400), false);
    let run = view(&read_sessions(&[live], None, now, Duration::hours(24))[0]);
    assert_eq!(run.cards.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["aaa1", "bbb2"]);
    assert_eq!(run.cards[0].state, CardState::Running);
    assert_eq!(run.status, RunStatus::Running);
}

#[test]
fn older_sidechains_in_the_main_file_are_subagents_too() {
    let root = temp("sidechain");
    let folder = root.join("projects").join("p");
    fs::create_dir_all(&folder).unwrap();
    let b = base();
    let chain = |uuid: &str, parent: Option<&str>, kind: &str, at: &str, content: Value| {
        json!({ "uuid": uuid, "parentUuid": parent, "isSidechain": true, "type": kind, "message": { "role": kind, "content": content }, "timestamp": at, "sessionId": "old" })
    };
    fs::write(
        folder.join("old.jsonl"),
        lines(&[
            json!({ "type": "summary", "summary": "예전 세션" }),
            json!({ "type": "user", "isSidechain": false, "uuid": "m1", "timestamp": at(b, 0), "cwd": "/home/u/site", "message": { "content": "go" } }),
            json!({ "type": "assistant", "isSidechain": false, "uuid": "m2", "timestamp": at(b, 5), "message": { "content": [
                { "type": "tool_use", "id": "toolu_1", "name": "Task", "input": { "description": "문서 읽기", "subagent_type": "Explore", "prompt": "read docs" } },
                { "type": "tool_use", "id": "toolu_2", "name": "Task", "input": { "description": "빌드", "subagent_type": "general-purpose", "prompt": "build it" } }
            ] } }),
            chain("c1", None, "user", &at(b, 6), json!("read docs")),
            chain("c2", Some("c1"), "assistant", &at(b, 8), json!([{ "type": "tool_use", "id": "x", "name": "Grep", "input": { "pattern": "TODO" } }])),
            chain("d1", None, "user", &at(b, 7), json!("build it")),
            chain("d2", Some("d1"), "assistant", &at(b, 9), json!([{ "type": "text", "text": "build broke" }])),
            json!({ "type": "user", "isSidechain": false, "timestamp": at(b, 20), "message": { "content": [
                { "type": "tool_result", "tool_use_id": "toolu_1", "content": "docs say hi" },
                { "type": "tool_result", "tool_use_id": "toolu_2", "content": "build failed", "is_error": true }
            ] } }),
        ]),
    )
    .unwrap();
    let (now, since) = recent();
    let runs = read_sessions(&[root], None, now, since);
    let run = view(&runs[0]);
    assert_eq!(run.title, "예전 세션");
    assert_eq!(run.project, "/home/u/site");
    let cards: Vec<(&str, &str, CardState)> = run.cards.iter().map(|c| (c.id.as_str(), c.label.as_str(), c.state)).collect();
    assert_eq!(cards, [("c1", "문서 읽기", CardState::Done), ("d1", "빌드", CardState::Failed)]);
    assert_eq!(run.cards[1].log.iter().filter(|l| l.text == "build broke").count(), 1);
    assert_eq!(run.status, RunStatus::Failed);
}

#[test]
fn a_session_in_progress_stays_running() {
    let root = temp("running");
    let now = Utc::now();
    write_session(&root, now - Duration::seconds(400));
    // The background subagent has not ended yet: drop its notice and its finished mark.
    let main = root.join("projects").join("C--work-app").join("s1.jsonl");
    let text = fs::read_to_string(&main).unwrap();
    let kept: String = text.lines().filter(|l| !l.contains("task-notification")).map(|l| format!("{l}\n")).collect();
    fs::write(&main, kept).unwrap();
    let meta = root.join("projects").join("C--work-app").join("s1").join("subagents").join("agent-bbb2.meta.json");
    fs::write(&meta, json!({ "agentType": "general-purpose", "description": "테스트 돌리기", "toolUseId": "toolu_B", "finished": false }).to_string()).unwrap();

    let run = view(&read_sessions(&[root], None, now, Duration::hours(24))[0]);
    assert_eq!(run.status, RunStatus::Running);
    assert_eq!(run.ended_at, None);
    assert_eq!(run.cards[0].state, CardState::Done);
    assert_eq!(run.cards[1].state, CardState::Running);
    assert_eq!(run.cards[1].activity, "명령 실행: Run tests");
}

#[test]
fn quiet_sessions_stop_their_open_subagents() {
    let root = temp("quiet");
    write_session(&root, base());
    let meta = root.join("projects").join("C--work-app").join("s1").join("subagents").join("agent-bbb2.meta.json");
    fs::write(&meta, json!({ "description": "테스트 돌리기", "toolUseId": "toolu_B" }).to_string()).unwrap();
    let main = root.join("projects").join("C--work-app").join("s1.jsonl");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.lines().filter(|l| !l.contains("task-notification")).map(|l| format!("{l}\n")).collect::<String>()).unwrap();
    let (now, since) = recent();
    let run = view(&read_sessions(&[root], None, now, since)[0]);
    assert_eq!(run.cards[1].state, CardState::Stopped);
    assert_eq!(run.status, RunStatus::Done);
}

#[test]
fn reads_only_what_was_appended() {
    let root = temp("incremental");
    let cache = root.join("state").join("sessions-cache.json");
    let main = write_session(&root, base());
    let (now, since) = recent();
    let first = view(&read_sessions(std::slice::from_ref(&root), Some(&cache), now, since)[0]);
    assert_eq!(first.title, "로그인 고치기");
    assert!(cache.is_file());

    // Change an old line in place (same length) and append one: only the new line is read.
    let text = fs::read_to_string(&main).unwrap().replace("로그인 고치기", "로그아웃 고침");
    fs::write(&main, format!("{text}{}", lines(&[assistant(&at(base(), 400), json!([{ "type": "text", "text": "다 고쳤습니다" }]))]))).unwrap();
    let second = view(&read_sessions(std::slice::from_ref(&root), Some(&cache), now, since)[0]);
    assert_eq!(second.title, "로그인 고치기", "lines before the cached offset are not read again");
    assert_eq!(second.messages.last().map(|m| m.text.as_str()), Some("다 고쳤습니다"));

    // A half-written last line waits; once finished it is read.
    let partial = assistant(&at(base(), 500), json!([{ "type": "text", "text": "마지막 말" }])).to_string();
    let (start, rest) = partial.split_at(20);
    let grown = fs::read_to_string(&main).unwrap();
    fs::write(&main, format!("{grown}{start}")).unwrap();
    assert_eq!(view(&read_sessions(std::slice::from_ref(&root), Some(&cache), now, since)[0]).messages.len(), 3);
    fs::write(&main, format!("{grown}{start}{rest}\n")).unwrap();
    assert_eq!(view(&read_sessions(std::slice::from_ref(&root), Some(&cache), now, since)[0]).messages.last().unwrap().text, "마지막 말");

    // A rewritten (shorter) file is read again from the start.
    fs::write(&main, lines(&[json!({ "type": "ai-title", "aiTitle": "새로", "sessionId": "s1" }), user(&at(base(), 0), json!("hi"))])).unwrap();
    let third = view(&read_sessions(std::slice::from_ref(&root), Some(&cache), now, since)[0]);
    assert_eq!(third.title, "새로");
    assert!(third.messages.is_empty());
}

#[test]
fn opens_nothing_outside_the_transcript_folders() {
    let root = temp("privacy");
    write_session(&root, base());
    let folder = root.join("projects").join("C--work-app");
    // Things that sit beside transcripts and must never be read.
    fs::write(root.join("settings.json"), "{}").unwrap();
    fs::write(root.join("history.jsonl"), lines(&[user(&at(base(), 0), json!("secret prompt"))])).unwrap();
    fs::write(root.join("projects").join("loose.jsonl"), lines(&[user(&at(base(), 0), json!("not in a project folder"))])).unwrap();
    fs::write(folder.join("notes.txt"), "x").unwrap();
    fs::write(folder.join("s1.jsonl.bak"), "x").unwrap();
    fs::write(folder.join("bad id.jsonl"), lines(&[user(&at(base(), 0), json!("x"))])).unwrap();
    fs::create_dir_all(folder.join("deeper")).unwrap();
    fs::write(folder.join("deeper").join("s9.jsonl"), lines(&[user(&at(base(), 0), json!("x"))])).unwrap();
    fs::create_dir_all(folder.join("s1").join("other")).unwrap();
    fs::write(folder.join("s1").join("other").join("agent-zz.jsonl"), "x").unwrap();
    fs::write(folder.join("s1").join("subagents").join("readme.md"), "x").unwrap();
    fs::write(folder.join("s1").join("subagents").join("agent-cc.txt"), "x").unwrap();

    let found = transcript_files(&root, UNIX_EPOCH);
    assert_eq!(found.len(), 1);
    let projects = root.join("projects");
    let mut opened: Vec<String> = found[0].paths().iter().map(|p| p.strip_prefix(&projects).unwrap().to_string_lossy().replace('\\', "/")).collect();
    opened.sort();
    assert_eq!(
        opened,
        [
            "C--work-app/s1.jsonl",
            "C--work-app/s1/subagents/agent-aaa1.jsonl",
            "C--work-app/s1/subagents/agent-aaa1.meta.json",
            "C--work-app/s1/subagents/agent-bbb2.jsonl",
            "C--work-app/s1/subagents/agent-bbb2.meta.json",
        ]
    );
    // And the cache records only those files.
    let cache = temp("privacy-state").join("cache.json");
    let (now, since) = recent();
    assert_eq!(read_sessions(std::slice::from_ref(&root), Some(&cache), now, since).len(), 1);
    let saved: Value = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
    let mut cached: Vec<String> = saved["files"].as_object().unwrap().keys().map(|k| Path::new(k).strip_prefix(&projects).unwrap().to_string_lossy().replace('\\', "/")).collect();
    cached.sort();
    assert_eq!(cached, ["C--work-app/s1.jsonl", "C--work-app/s1/subagents/agent-aaa1.jsonl", "C--work-app/s1/subagents/agent-bbb2.jsonl"]);
}

/// A folder link, or None where this machine does not allow making one.
fn link_dir(target: &Path, link: &Path) -> Option<()> {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    made.ok()
}

fn link_file(target: &Path, link: &Path) -> Option<()> {
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    made.ok()
}

/// A Windows junction (no special rights needed), made with the system's own mklink.
#[cfg(windows)]
fn junction(target: &Path, link: &Path) -> bool {
    std::process::Command::new("cmd").arg("/C").arg("mklink").arg("/J").arg(link).arg(target).output().is_ok_and(|o| o.status.success())
}

fn ids(root: &Path) -> Vec<String> {
    transcript_files(root, UNIX_EPOCH).into_iter().map(|s| s.session_id).collect()
}

#[test]
fn links_anywhere_below_the_config_root_are_not_followed() {
    // Outside: a real transcript folder that must never be read through a link.
    let outside = temp("links-outside");
    write_session(&outside, base());
    let outside_projects = outside.join("projects");
    let outside_folder = outside_projects.join("C--work-app");

    // `projects` itself is a link.
    let root = temp("links-projects");
    if link_dir(&outside_projects, &root.join("projects")).is_none() {
        eprintln!("skipped: this machine does not allow symlinks");
        return;
    }
    assert!(ids(&root).is_empty(), "projects link");

    // A project folder is a link.
    let root = temp("links-folder");
    fs::create_dir_all(root.join("projects")).unwrap();
    link_dir(&outside_folder, &root.join("projects").join("C--evil")).unwrap();
    assert!(ids(&root).is_empty(), "folder link");

    // The session's folder or its subagents folder is a link: the main file is read, no subagent.
    let root = temp("links-subagents");
    write_session(&root, base());
    let folder = root.join("projects").join("C--work-app");
    fs::remove_dir_all(folder.join("s1")).unwrap();
    link_dir(&outside_folder.join("s1"), &folder.join("s1")).unwrap();
    let found = transcript_files(&root, UNIX_EPOCH);
    assert_eq!(found.len(), 1);
    assert!(found[0].subagents.is_empty(), "session folder link");

    // A transcript or meta file is a link.
    let root = temp("links-file");
    fs::create_dir_all(root.join("projects").join("C--work-app")).unwrap();
    link_file(&outside_folder.join("s1.jsonl"), &root.join("projects").join("C--work-app").join("s7.jsonl")).unwrap();
    assert!(ids(&root).is_empty(), "file link");

    #[cfg(windows)]
    {
        let root = temp("links-junction");
        assert!(junction(&outside_projects, &root.join("projects")));
        assert!(ids(&root).is_empty(), "projects junction");
        let root = temp("links-junction-folder");
        fs::create_dir_all(root.join("projects")).unwrap();
        assert!(junction(&outside_folder, &root.join("projects").join("C--evil")));
        assert!(ids(&root).is_empty(), "folder junction");
    }

    // A config root that is itself a link is the user's choice and still read.
    let linked_root = temp("links-root").join("claude");
    link_dir(&outside, &linked_root).unwrap();
    assert_eq!(ids(&linked_root), ["s1"]);
}

#[test]
fn only_sessions_inside_the_window_are_read() {
    let root = temp("window");
    write_session(&root, base());
    // Seen from two days ahead, a file written now is older than a day.
    assert!(read_sessions(std::slice::from_ref(&root), None, Utc::now() + Duration::hours(48), Duration::hours(24)).is_empty());
    assert_eq!(read_sessions(&[root], None, Utc::now() + Duration::hours(48), Duration::hours(72)).len(), 1);
}

#[test]
fn o_cards_own_sessions_are_not_listed_twice() {
    let record = |id: &str, events: Vec<Value>| RunRecord { run_id: id.into(), events };
    let start = |id: &str, at: &str| json!({ "t": "run.start", "at": at, "runId": id, "title": "t", "project": "C:/p", "cards": [] });
    let runs = vec![record("20261101-100000-aaaa", vec![start("20261101-100000-aaaa", "2026-11-01T10:00:00.000Z"), json!({ "t": "card.session", "at": "2026-11-01T10:00:01.000Z", "cardId": "c1", "sessionId": "s1" })])];
    let sessions = vec![
        record("session-s1", vec![start("session-s1", "2026-11-01T10:00:01.000Z")]),
        record("session-s2", vec![start("session-s2", "2026-11-01T11:00:00.000Z")]),
    ];
    let ids: Vec<String> = with_sessions(runs, sessions).into_iter().map(|r| r.run_id).collect();
    assert_eq!(ids, ["session-s2", "20261101-100000-aaaa"]);
}

#[test]
fn sessions_follow_the_runs_project_and_limit() {
    let record = |id: &str, project: &str, at: &str| RunRecord { run_id: id.into(), events: vec![json!({ "t": "run.start", "at": at, "runId": id, "title": "t", "project": project, "cards": [] })] };
    let mut o_run = record("20261101-100000-aaaa", "C:/A", "2026-11-01T10:00:00.000Z");
    o_run.events.push(json!({ "t": "card.session", "at": "2026-11-01T10:00:01.000Z", "cardId": "c1", "sessionId": "s1" }));
    let sessions = || {
        vec![
            record("session-s1", "C:/A", "2026-11-01T10:00:01.000Z"),
            record("session-a2", if cfg!(windows) { "c:\\a\\" } else { "C:/A/" }, "2026-11-01T11:00:00.000Z"),
            record("session-b1", "C:/B", "2026-11-01T12:00:00.000Z"),
        ]
    };
    let ids = |query: &RunQuery| -> Vec<String> { with_sessions_for(vec![o_run.clone()], sessions(), query).into_iter().map(|r| r.run_id).collect() };
    // --project A: B's session is left out, and the O card's own session is not listed twice.
    assert_eq!(ids(&RunQuery { limit: 50, project: Some("C:/A") }), ["session-a2", "20261101-100000-aaaa"]);
    // --limit applies to the merged list.
    assert_eq!(ids(&RunQuery { limit: 1, project: Some("C:/A") }), ["session-a2"]);
    assert_eq!(ids(&RunQuery { limit: 1, project: None }), ["session-b1"]);
    assert_eq!(ids(&RunQuery { limit: 50, project: None }).len(), 3);
}
