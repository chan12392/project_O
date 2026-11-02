//! `o context` and `o tokens` with the real `o` against a throwaway O home
//! (LOCALAPPDATA/project_O): JSON shapes, the default project, card read-only.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{json, Value};

fn o_in(base: &Path, cwd: &Path, card: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o"))
        .args(args)
        .current_dir(cwd)
        .env("LOCALAPPDATA", base)
        .env("O_RUN_ID", if card.is_empty() { "" } else { "r1" })
        .env("O_CARD_ID", card)
        .output()
        .unwrap()
}

fn ok_json(out: Output) -> Value {
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn setup(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("o-context-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let home = base.join("project_O");
    let project = base.join("game");
    fs::create_dir_all(project.join("src")).unwrap();
    let project_text = project.to_string_lossy().into_owned();
    for (run_id, at, clean, card, provider, result, tokens) in [
        ("r1", "2026-11-01T10:00:00.000Z", false, "art", "claude", "캐릭터 스프라이트를 32px로 바꿨습니다", [100, 50, 10]),
        ("r2", "2026-11-02T10:00:00.000Z", true, "ui", "codex", "메뉴 버튼 색을 바꿨습니다", [40, 10, 5]),
    ] {
        let dir = home.join("runs").join(run_id);
        fs::create_dir_all(&dir).unwrap();
        let mut events = vec![json!({ "t": "run.start", "at": at, "runId": run_id, "title": "t", "project": project_text, "cards": [{ "id": card, "provider": provider }] })];
        if clean {
            fs::write(dir.join("clean"), "").unwrap();
            events.push(json!({ "t": "run.context", "at": at, "runId": run_id, "cards": 1, "chars": 120, "cardIds": [card] }));
        }
        events.push(json!({ "t": "card.usage", "at": at, "cardId": card, "input": tokens[0], "cached": tokens[1], "output": tokens[2] }));
        events.push(json!({ "t": "card.end", "at": at, "cardId": card, "status": "done", "result": result, "exitCode": 0 }));
        let text: String = events.iter().map(|e| format!("{e}\n")).collect();
        fs::write(dir.join("events.jsonl"), text).unwrap();
    }
    (base, home, project)
}

#[test]
fn context_search_add_and_the_card_side_are_read_only() {
    let (base, _home, project) = setup("search");
    let p = project.to_string_lossy().replace('\\', "/");

    // From a folder inside the project, the project is found on its own.
    let found = ok_json(o_in(&base, &project.join("src"), "", &["context", "캐릭터를", "--json"]));
    assert_eq!(found["project"], p.as_str());
    assert_eq!(found["query"], "캐릭터를");
    let hits = found["hits"].as_array().unwrap();
    assert_eq!(hits.len(), 1);
    let keys: Vec<&str> = hits[0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["kind", "runId", "cardId", "title", "at", "project", "files", "excerpt", "score"]);
    assert_eq!((hits[0]["kind"].as_str(), hits[0]["runId"].as_str(), hits[0]["cardId"].as_str()), (Some("result"), Some("r1"), Some("art")));

    let added = ok_json(o_in(&base, &base, "", &["context", "add", "--project", &p, "배포는", "금요일에", "하지", "않는다", "--json"]));
    assert_eq!(added, json!({ "added": true, "project": p }));
    let notes = ok_json(o_in(&base, &base, "", &["context", "배포", "--project", &p, "--json", "--limit", "1"]));
    assert_eq!(notes["hits"][0]["kind"], "note");
    // Outside any known project, with no --project, the search covers every project.
    assert_eq!(ok_json(o_in(&base, &base, "", &["context", "메뉴", "--json"]))["project"], Value::Null);

    // A card reads (without re-indexing) and cannot add.
    let text = o_in(&base, &project, "art", &["context", "메뉴", "버튼"]);
    assert_eq!(text.status.code(), Some(0));
    let printed = String::from_utf8_lossy(&text.stdout);
    assert!(printed.starts_with("1. [결과 · r2/ui · "), "{printed}");
    let refused = o_in(&base, &project, "art", &["context", "add", "메모"]);
    assert_eq!(refused.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("o msg send"));

    assert_eq!(o_in(&base, &base, "", &["context"]).status.code(), Some(1));
    assert_eq!(o_in(&base, &base, "", &["context", "x", "--limit", "0"]).status.code(), Some(1));
}

#[test]
fn tokens_report_and_compare() {
    let (base, _home, project) = setup("tokens");
    let report = ok_json(o_in(&base, &base, "", &["tokens", "--json"]));
    let keys: Vec<&str> = report.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(keys, ["groups", "compare", "runs"]);
    assert_eq!(report["groups"].as_array().unwrap().iter().map(|g| g["key"].as_str().unwrap()).collect::<Vec<_>>(), ["normal", "clean+context"]);
    let group_keys: Vec<&str> = report["groups"][0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(group_keys, ["key", "label", "clean", "context", "runs", "cards", "tokens", "perCard", "byProvider"]);
    assert_eq!(report["compare"]["baseline"]["perCard"], json!({ "input": 100, "cached": 50, "output": 10, "total": 160 }));
    assert_eq!(report["compare"]["shared"]["perCard"]["total"], 55);
    let run_keys: Vec<&str> = report["runs"][0].as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(run_keys, ["runId", "title", "project", "startedAt", "clean", "context", "cards", "tokens"]);
    assert_eq!(report["runs"][0]["cards"][0], json!({ "cardId": "ui", "provider": "codex", "context": true, "tokens": { "input": 40, "cached": 10, "output": 5, "total": 55 } }));

    let since = ok_json(o_in(&base, &base, "", &["tokens", "--json", "--since", "2026-11-02"]));
    assert_eq!(since["runs"].as_array().unwrap().len(), 1);
    assert_eq!(since["compare"]["baseline"], Value::Null);
    let other = ok_json(o_in(&base, &base, "", &["tokens", "--json", "--project", "C:/nowhere"]));
    assert_eq!(other["runs"], json!([]));
    let mine = ok_json(o_in(&base, &base, "", &["tokens", "--json", "--project", &project.to_string_lossy()]));
    assert_eq!(mine["runs"].as_array().unwrap().len(), 2);

    let compare = o_in(&base, &base, "", &["tokens", "--compare"]);
    assert_eq!(compare.status.code(), Some(0));
    let text = String::from_utf8_lossy(&compare.stdout);
    assert!(text.contains("기본 하네스") && text.contains("깨끗+세션 공유"), "{text}");
    assert!(text.contains("카드당 합계") && text.contains("160") && text.contains("-66%"), "{text}");
    assert_eq!(o_in(&base, &base, "", &["tokens", "--since", "어제"]).status.code(), Some(1));
}
