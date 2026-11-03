//! Port of tests/snapshot.test.ts and tests/liveRuns.test.ts, plus the event
//! contract: every event the TS port wrote reads back and writes out unchanged.

use std::path::PathBuf;

use o_core::run::events::OEvent;
use o_core::run::snapshot::{describe_tool, snapshot, CardState, LogKind, RunStatus};
use o_core::run::store::{newest_first, parse_event_values, parse_events, RunRecord};
use serde_json::{json, Value};

/// The app's demo fixture: real end-to-end runs, paths sanitised.
fn demo() -> Vec<Vec<Value>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app/src/data/demo-runs.json");
    let data: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    data["runs"].as_array().unwrap().iter().map(|r| r["events"].as_array().unwrap().clone()).collect()
}

fn typed(events: &[Value]) -> Vec<OEvent> {
    events.iter().map(|e| serde_json::from_value(e.clone()).unwrap_or_else(|err| panic!("{err}: {e}"))).collect()
}

fn by_id(run_id: &str) -> Vec<OEvent> {
    let run = demo().into_iter().find(|events| events[0]["t"] == "run.start" && events[0]["runId"] == run_id).expect("fixture run");
    typed(&run)
}

const FIXTURE: &str = "20261101-100916-402x";

#[test]
fn every_fixture_event_reads_back_and_writes_out_unchanged() {
    let runs = demo();
    assert!(!runs.is_empty());
    for events in runs {
        for (value, event) in events.iter().zip(typed(&events)) {
            // Same fields and numbers (the fixture was hand-sanitised, so its key order is not the writer's).
            assert_eq!(&serde_json::to_value(&event).unwrap(), value);
        }
    }
}

#[test]
fn status_states_of_the_fixture_run() {
    let run = snapshot(&by_id(FIXTURE)).unwrap();
    assert_eq!(run.status, RunStatus::Failed);
    let states: Vec<(&str, CardState)> = run.cards.iter().map(|c| (c.id.as_str(), c.state)).collect();
    assert_eq!(states, [("code", CardState::Done), ("design", CardState::Failed), ("research", CardState::Done)]);
    let design = run.cards.iter().find(|c| c.id == "design").unwrap();
    assert!(design.activity.contains("RunCommand"));
    assert!(run.cards.iter().all(|c| c.started_at.is_some() && c.ended_at.is_some()));
    assert!(run.cards.iter().find(|c| c.id == "code").unwrap().log.len() > 20);
}

#[test]
fn real_team_runs_carry_their_messages_between_codex_claude_and_agy() {
    let pairs = |id: &str| -> Vec<String> { snapshot(&by_id(id)).unwrap().messages.iter().map(|m| format!("{}>{}", m.from, m.to)).collect() };
    assert_eq!(pairs("20261101-145813-t353"), ["c>d", "d>c"]);
    assert_eq!(pairs("20261101-145332-q3eg"), ["a>b", "b>a", "a>lead"]);
    let run = snapshot(&by_id("20261101-145332-q3eg")).unwrap();
    assert_eq!(run.cards.iter().find(|c| c.id == "a").unwrap().label, "a");
    assert_eq!(run.cards.iter().find(|c| c.id == "b").unwrap().label, "답장 담당");
}

#[test]
fn a_prefix_of_the_stream_shows_cards_still_running() {
    let events = by_id(FIXTURE);
    let first = events.iter().position(|e| matches!(e, OEvent::CardSession { .. })).unwrap();
    let run = snapshot(&events[..=first]).unwrap();
    assert_eq!(run.status, RunStatus::Running);
    assert!(run.cards.iter().all(|c| c.state == CardState::Running));
}

#[test]
fn tool_activity_reads_as_an_act_the_log_keeps_the_raw_event() {
    assert_eq!(describe_tool("shell", r#"ego-browser nodejs -e \"const task\""#), r#"명령 실행: ego-browser nodejs -e "const task""#);
    assert_eq!(describe_tool("read_file", "src/app.js"), "파일 읽기: src/app.js");
    assert_eq!(describe_tool("Edit", ""), "파일 수정");
    assert_eq!(
        describe_tool("edit", r#"[{"path":"C:/p\\app\\align.py","kind":"add"},{"path":"src/app.js","kind":"update"}]"#),
        "파일 수정: align.py 추가, app.js 수정"
    );
    assert_eq!(
        describe_tool("edit", r#"[{"path":"C:/projects/todo-app/index.html","kind":"update"},{"path":"C:/projects/to"#),
        "파일 수정: index.html 수정 외"
    );
    assert_eq!(describe_tool("edit", r#"[{"path":"C:/projects/todo-app"#), "파일 수정");
    assert_eq!(describe_tool("todo_write", "x"), "todo_write: x");
    let run = snapshot(&by_id(FIXTURE)).unwrap();
    let design = run.cards.iter().find(|c| c.id == "design").unwrap();
    assert!(design.log.iter().any(|l| l.kind == LogKind::Tool && l.text.starts_with("run_command")));
}

#[test]
fn team_messages_land_on_the_run_and_in_each_partys_log() {
    let start = by_id(FIXTURE).remove(0);
    let msg = |at: &str, from: &str, to: &str, text: &str| OEvent::Msg { at: at.into(), from: from.into(), to: to.into(), text: text.into(), id: None, kind: None };
    let run = snapshot(&[
        start,
        msg("2026-11-01T10:10:00.000Z", "code", "research", "app.js 끝났어요"),
        msg("2026-11-01T10:11:00.000Z", "lead", "all", "모두 마무리해 주세요"),
    ])
    .unwrap();
    assert_eq!(run.messages.len(), 2);
    let log = |id: &str| -> Vec<String> {
        run.cards.iter().find(|c| c.id == id).unwrap().log.iter().filter(|l| l.kind == LogKind::Team).map(|l| l.text.clone()).collect()
    };
    assert_eq!(log("code"), ["app.js 끝났어요", "모두 마무리해 주세요"]);
    assert_eq!(log("research"), ["app.js 끝났어요", "모두 마무리해 주세요"]);
    assert_eq!(log("design"), ["모두 마무리해 주세요"]);
    assert_eq!(run.cards[0].label, "코딩");
}

#[test]
fn question_metadata_survives_typed_event_and_snapshot() {
    let mut events = by_id(FIXTURE);
    events.push(serde_json::from_str(r#"{"t":"msg","at":"now","from":"lead","to":"all","text":"help?","id":"m1","kind":"question"}"#).unwrap());
    let view = o_core::run::snapshot::snapshot(&events).unwrap();
    let msg = view.messages.last().unwrap();
    assert_eq!(msg.id.as_deref(), Some("m1"));
    assert_eq!(msg.kind.as_deref(), Some("question"));
}

#[test]
fn no_run_start_means_nothing_to_show() {
    assert!(snapshot(&[]).is_none());
}

fn start(run_id: &str, at: &str) -> Value {
    json!({ "t": "run.start", "at": at, "runId": run_id, "title": run_id, "project": "C:/work/demo", "cards": [] })
}

#[test]
fn a_half_written_last_line_of_a_live_run_is_skipped_not_fatal() {
    let whole = start("r1", "2026-11-01T10:00:00.000Z").to_string();
    let message = json!({ "t": "msg", "at": "2026-11-01T10:00:01.000Z", "from": "lead", "to": "all", "text": "시작" }).to_string();
    let text = format!("{whole}\n\n{message}\r\n{{\"t\":\"card.tool\",\"at\":\"2026-11-01T1");
    let kinds: Vec<&str> = parse_events(&text).iter().map(OEvent::kind).collect();
    assert_eq!(kinds, ["run.start", "msg"]);
    assert_eq!(parse_event_values(&text).len(), 2);
    assert!(parse_events("").is_empty());
}

#[test]
fn runs_come_newest_first_by_their_start_and_runs_without_a_start_are_left_out() {
    let record = |run_id: &str, events: Vec<Value>| RunRecord { run_id: run_id.into(), events };
    let runs = newest_first(vec![
        record("20261101-090000-aaaa", vec![start("20261101-090000-aaaa", "2026-11-01T09:00:00.000Z")]),
        record("20261101-120000-cccc", vec![]),
        record("20261101-110000-bbbb", vec![start("20261101-110000-bbbb", "2026-11-01T11:00:00.000Z")]),
    ]);
    let ids: Vec<&str> = runs.iter().map(|r| r.run_id.as_str()).collect();
    assert_eq!(ids, ["20261101-110000-bbbb", "20261101-090000-aaaa"]);
    assert_eq!(
        serde_json::to_string(&runs[1]).unwrap(),
        format!(r#"{{"runId":"20261101-090000-aaaa","events":[{}]}}"#, start("20261101-090000-aaaa", "2026-11-01T09:00:00.000Z"))
    );
}

/// Same events as tests/snapshot.test.ts "a repeated card id is one card, kept by the first".
#[test]
fn a_repeated_card_id_is_one_card_kept_by_the_first() {
    let events = typed(&[
        json!({ "t": "run.start", "at": "2026-11-02T10:00:00.000Z", "runId": "r", "title": "t", "project": "C:/p", "cards": [{ "id": "a1", "provider": "claude", "label": "첫" }, { "id": "b2", "provider": "claude", "label": "b" }, { "id": "a1", "provider": "claude", "label": "둘" }] }),
        json!({ "t": "card.start", "at": "2026-11-02T10:00:01.000Z", "cardId": "a1" }),
        json!({ "t": "card.end", "at": "2026-11-02T10:00:02.000Z", "cardId": "a1", "status": "done", "result": "결과" }),
    ]);
    let run = snapshot(&events).unwrap();
    let cards: Vec<(&str, &str, CardState, Option<&str>)> = run.cards.iter().map(|c| (c.id.as_str(), c.label.as_str(), c.state, c.result.as_deref())).collect();
    assert_eq!(cards, [("a1", "첫", CardState::Done, Some("결과")), ("b2", "b", CardState::Waiting, None)]);
}

/// Runs this machine already recorded (read only): every event line the TS port wrote is still an event.
#[test]
fn existing_local_runs_still_read() {
    let home = o_core::run::store::default_home();
    for run_id in o_core::run::store::list_runs(&home) {
        for value in o_core::run::store::read_event_values(&home, &run_id) {
            let event: OEvent = serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{run_id}: {e}: {value}"));
            // Written back byte for byte: same keys, same order, same numbers.
            assert_eq!(serde_json::to_string(&event).unwrap(), value.to_string(), "{run_id}");
        }
    }
}
