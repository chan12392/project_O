use std::fs;
use std::path::PathBuf;

use o_core::run::plan::ProviderId;
use o_core::usage::{accounting::{self, Collector}, fallback};
use serde_json::json;

fn home(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("o-usage-{label}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
    fs::create_dir_all(&path).unwrap();
    path
}

fn report(input: u64, cached: u64, output: u64) -> String {
    json!({"type":"result","session_id":"shared","is_error":true,"modelUsage":{
        "model-a":{"inputTokens":input,"cacheCreationInputTokens":4,"cacheReadInputTokens":cached,"outputTokens":output},
        "model-b":{"inputTokens":6,"cacheCreationInputTokens":0,"cacheReadInputTokens":10,"outputTokens":2}
    }}).to_string()
}

#[test]
fn failed_result_sums_all_models_and_resumes_from_durable_profile_baseline() {
    let home = home("resume");
    let profile = home.join("profile");
    let mut first = Collector::new(ProviderId::Claude, None, false, "2026-11-12T01:00:00.000Z".into());
    first.observe(&report(90, 190, 8));
    let one = first.settle(&home, &profile, "one", "raw", "2026-11-12T01:01:00.000Z", true, vec![]).unwrap().unwrap();
    assert_eq!((one.input, one.cached, one.output), (100.0, 200.0, 10.0));
    assert!(one.complete && one.reported);
    assert_eq!(one.models, ["model-a", "model-b"]);
    // No database is needed, even when its path cannot be opened.
    fs::create_dir(home.join("o.db")).unwrap();
    let mut next = Collector::new(ProviderId::Claude, Some("shared".into()), false, "2026-11-12T02:00:00.000Z".into());
    next.observe(&report(120, 240, 10));
    let two = next.settle(&home, &profile, "two", "raw", "2026-11-12T02:01:00.000Z", true, vec![]).unwrap().unwrap();
    assert_eq!((two.input, two.cached, two.output), (30.0, 50.0, 2.0));
    assert_eq!(next.settle(&home, &profile, "two", "raw", "2026-11-12T02:01:00.000Z", true, vec![]).unwrap().unwrap(), two);
    assert_eq!(accounting::read_journal(&home).unwrap().len(), 2);
    let other_profile = next.calculate(&accounting::read_journal(&home).unwrap(), &home.join("other-profile"), "three", "raw", "2026-11-12T02:01:00.000Z", true, vec![]).unwrap().unwrap();
    assert!(!other_profile.reported && !other_profile.complete);
}

#[test]
fn parent_only_and_interrupted_messages_remain_partial_and_do_not_duplicate_a_message() {
    let home = home("partial");
    let mut parent = Collector::new(ProviderId::Claude, None, false, "2026-11-12T01:00:00Z".into());
    let message = json!({"type":"assistant","session_id":"partial","message":{"id":"m","model":"observed-model","usage":{"input_tokens":10,"cache_creation_input_tokens":2,"cache_read_input_tokens":20,"output_tokens":3}}}).to_string();
    parent.observe(&message); parent.observe(&message);
    let partial = parent.calculate(&[], &home, "p", "raw", "2026-11-12T01:01:00Z", false, vec![]).unwrap().unwrap();
    assert_eq!((partial.input, partial.cached, partial.output), (12.0, 20.0, 3.0));
    assert!(!partial.complete && partial.reported);
    parent.observe(&json!({"type":"result","session_id":"partial","usage":{"input_tokens":15,"cache_creation_input_tokens":2,"cache_read_input_tokens":20,"output_tokens":8}}).to_string());
    let partial = parent.calculate(&[], &home, "p", "raw", "2026-11-12T01:01:00Z", true, vec![]).unwrap().unwrap();
    assert_eq!(partial.input, 17.0);
    assert!(!partial.complete);
}

#[test]
fn projection_matches_the_typescript_contract() {
    let cases: serde_json::Value = serde_json::from_str(include_str!("../../../tests/fixtures/usage-settlements.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let visible: Vec<_> = accounting::project(case["events"].as_array().unwrap()).iter().filter(|e| e["t"] == "card.usage").map(|e| json!({"cardId":e["cardId"],"input":e["input"],"at":e["at"],"complete":e["complete"]})).collect();
        assert_eq!(json!(visible), case["expected"], "{}", case["name"]);
        let mut stable = case["events"].as_array().unwrap().clone();
        let references = accounting::legacy_references(&stable);
        for event in &mut stable {
            if let Some(replaces) = event["replaces"].as_array_mut() { for reference in replaces { *reference = json!(references[reference.as_u64().unwrap() as usize]); } }
        }
        stable.retain(|e| e["t"] != "future.event");
        let visible: Vec<_> = accounting::project(&stable).iter().filter(|e| e["t"] == "card.usage").map(|e| json!({"cardId":e["cardId"],"input":e["input"],"at":e["at"],"complete":e["complete"]})).collect();
        assert_eq!(json!(visible), case["expected"]);
    }
}

#[test]
fn app_server_turn_deltas_are_not_subtracted_from_a_session_baseline() {
    let home = home("app-server");
    let mut turn = Collector::new(ProviderId::Codex, Some("session".into()), true, "2026-11-12T02:00:00Z".into());
    turn.observe(r#"{"type":"turn.usage","usage":{"input_tokens":20,"cached_input_tokens":5,"output_tokens":3}}"#);
    let result = turn.calculate(&[], &home, "app-server-turn", "raw", "2026-11-12T02:01:00Z", true, vec![]).unwrap().unwrap();
    assert_eq!((result.input, result.cached, result.output), (15.0, 5.0, 3.0));
    assert!(result.reported);
}

#[test]
fn a_fallback_claim_is_one_attempt_and_an_unknown_limit_clears_subscription_evidence() {
    let home = home("fallback");
    assert!(!fallback::read(&home).enabled);
    let target = fallback::Target { provider: ProviderId::Codex, model: "chosen-model".into() };
    // Write fixture settings directly; the real setter rejects agent-owned processes.
    fs::write(home.join("settings.json"), json!({"subscriptionFallback":{"enabled":true,"target":target}}).to_string()).unwrap();
    fallback::claim(&home, "logical-task", &target).unwrap();
    assert!(fallback::claim(&home, "logical-task", &target).is_err());
    fallback::record_limit(&home, ProviderId::Claude, "").unwrap();
    assert!(fallback::confirmed(&home, ProviderId::Claude).unwrap());
    o_core::run::store::set_cooldown(&home, ProviderId::Claude, "", chrono::Utc::now()).unwrap();
    assert!(!fallback::confirmed(&home, ProviderId::Claude).unwrap());
    fs::write(home.join("settings.json"), "broken").unwrap();
    assert!(!fallback::read(&home).enabled);
    assert!(fallback::claim(&home, "another-task", &target).is_err());
}

#[test]
fn reconciliation_preview_writes_nothing() {
    let home = home("preview");
    let path = home.join("runs/r/cards");
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("a.raw.jsonl"), format!("{}\n", report(90, 190, 8))).unwrap();
    let events = [
        json!({"t":"run.start","at":"2026-11-12T01:00:00Z","project":"C:/fixture","cards":[{"id":"a","provider":"claude"}]}),
        json!({"t":"card.start","at":"2026-11-12T01:00:00Z","cardId":"a"}),
        json!({"t":"card.usage","at":"2026-11-12T01:01:00Z","cardId":"a","input":30,"cached":0,"output":0,"complete":true}),
        json!({"t":"card.end","at":"2026-11-12T01:01:00Z","cardId":"a"}),
        json!({"t":"run.end","at":"2026-11-12T01:01:00Z"})
    ].map(|e| e.to_string()).join("\n");
    fs::write(home.join("runs/r/events.jsonl"), &events).unwrap();
    let result = o_core::usage::reconcile::run(&home, Some("r"), None, false).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!((result[0].before, result[0].after, result[0].applied), (30.0, 310.0, false));
    assert_eq!(fs::read_to_string(home.join("runs/r/events.jsonl")).unwrap(), events);
    assert!(!home.join("usage-journal.jsonl").exists() && !home.join("o.db").exists());
}
