use std::fs;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;

fn o(base: &std::path::Path, args: &[&str], input: Option<&str>, card: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_o"))
        .args(args)
        .env("LOCALAPPDATA", base.join("local"))
        .env("HOME", base.join("user"))
        .env("USERPROFILE", base.join("user"))
        .env("O_AGENT_ROLE", "")
        .env("O_CARD_ID", card)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input.unwrap_or_default().as_bytes()).unwrap();
    child.wait_with_output().unwrap()
}

fn output(base: &std::path::Path, args: &[&str], input: Option<&str>) -> Value {
    let result = o(base, args, input, "");
    assert!(result.status.success(), "{args:?}: {}", String::from_utf8_lossy(&result.stderr));
    serde_json::from_slice(&result.stdout).unwrap()
}

#[test]
fn materials_go_in_are_read_wired_and_come_out() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-dream-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&base).unwrap();

    assert_eq!(output(&base, &["dream", "list", "--json"], None)["docs"].as_array().unwrap().len(), 0);
    let first = output(&base, &["dream", "add", "-"], Some(r##"{"kind":"text","title":"배포 점검표","text":"# 점검\n\n본문"}"##));
    let second = output(&base, &["dream", "add", "-"], Some(r#"{"kind":"text","text":"메모\n\n[[배포 점검표]]를 봅니다."}"#));
    let (first, second) = (first["path"].as_str().unwrap(), second["path"].as_str().unwrap());
    assert!(first.starts_with("외부 자료/"), "{first}");

    let list = output(&base, &["dream", "list"], None);
    assert_eq!(list["docs"].as_array().unwrap().len(), 2);
    let page = output(&base, &["dream", "read", second, "--json"], None);
    assert_eq!(page["title"], "메모");
    assert_eq!(page["links"][0], first);
    let graph = output(&base, &["dream", "graph", "--json"], None);
    assert_eq!(graph["nodes"][0]["id"], "dream");
    assert!(graph["links"].as_array().unwrap().iter().any(|l| l["kind"] == "references" && l["target"] == format!("f:{first}")));

    // A card neither adds nor removes.
    assert_eq!(o(&base, &["dream", "add", "-"], Some(r#"{"kind":"text","text":"x"}"#), "card-1").status.code(), Some(1));
    assert_eq!(o(&base, &["dream", "remove", first], None, "card-1").status.code(), Some(1));
    for refused in [vec!["dream"], vec!["dream", "write"], vec!["dream", "read"], vec!["dream", "read", "외부 자료/없음.md"], vec!["dream", "list", "--extra"], vec!["dream", "remove", "작업 성공/x.md"], vec!["dream", "add"]] {
        assert_eq!(o(&base, &refused, None, "").status.code(), Some(1), "{refused:?}");
    }
    assert_eq!(o(&base, &["dream", "add", "-"], Some("not json"), "").status.code(), Some(1));

    // Dreaming's changes: none yet, and only a person takes one back.
    assert_eq!(output(&base, &["dream", "changes", "--json"], None).as_array().unwrap().len(), 0);
    assert_eq!(o(&base, &["dream", "revert", "0000000000000000"], None, "").status.code(), Some(1));
    assert_eq!(o(&base, &["dream", "revert", "0000000000000000"], None, "card-1").status.code(), Some(1));

    assert_eq!(output(&base, &["dream", "remove", first], None)["ok"], true);
    assert_eq!(output(&base, &["dream", "list"], None)["docs"].as_array().unwrap().len(), 1);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn documents_are_found_by_tag_and_a_card_reads_only_its_projects() {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-cli-dream-tags-{}-{nonce}", std::process::id()));
    let runs = base.join("local").join("project_O").join("runs");
    let run = |id: &str, project: &str, note: &str| {
        fs::create_dir_all(runs.join(id)).unwrap();
        // The run id opens with its day.
        let at = |minute: u8| format!("{}-{}-{}T00:0{minute}:00.000Z", &id[..4], &id[4..6], &id[6..8]);
        let events = [
            serde_json::json!({ "t": "run.start", "at": at(0), "runId": id, "title": "배포", "project": project, "cards": [{ "id": "build", "provider": "claude", "label": "구현" }] }),
            serde_json::json!({ "t": "msg", "at": at(1), "from": "build", "to": "wiki", "text": note }),
            serde_json::json!({ "t": "card.end", "at": at(2), "cardId": "build", "status": "done", "result": "끝", "exitCode": 0 }),
        ];
        fs::write(runs.join(id).join("events.jsonl"), events.iter().map(|e| format!("{e}\n")).collect::<String>()).unwrap();
    };
    run("20260101-000000-aaaa", "C:/work/blog", "서명 키는 환경 변수로만 읽는다 #서명\n둘째 줄");
    run("20260102-000000-bbbb", "D:/other/shop", "결제 메모 #서명 #결제");
    output(&base, &["dream", "add", "-"], Some(r#"{"kind":"text","title":"공용 안내","text":"본문","tags":"서명, 배포"}"#));

    let found = output(&base, &["dream", "find", "--tag", "#서명", "--kind", "note"], None);
    let paths: Vec<&str> = found["docs"].as_array().unwrap().iter().map(|d| d["path"].as_str().unwrap()).collect();
    assert_eq!(paths, ["배운 것/shop/20260102-000000-bbbb-build-1.md", "배운 것/blog/20260101-000000-aaaa-build-1.md"]);
    assert_eq!(found["docs"][0]["tags"], serde_json::json!(["서명", "결제"]));
    assert!(found["docs"][0].get("text").is_none());
    assert_eq!(output(&base, &["dream", "find", "서명", "결제", "--limit", "1"], None)["docs"].as_array().unwrap().len(), 1);
    assert_eq!(output(&base, &["dream", "tags"], None)[0], serde_json::json!({ "tag": "서명", "count": 5 }));
    assert_eq!(output(&base, &["dream", "tags", "--project", "D:/other/shop"], None).as_array().unwrap().len(), 2);
    for refused in [vec!["dream", "find", "--kind", "all"], vec!["dream", "find", "--tag"], vec!["dream", "find", "a/b"], vec!["dream", "tags", "--limit", "2"]] {
        assert_eq!(o(&base, &refused, None, "").status.code(), Some(1), "{refused:?}");
    }

    // The same through `o context`, the one command a card may run: titles first, one text at a time.
    let said = |args: &[&str], card: Option<&str>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        command.args(args).env("LOCALAPPDATA", base.join("local")).env("USERPROFILE", base.join("user")).env("HOME", base.join("user")).env("O_AGENT_ROLE", "");
        match card {
            Some(run_id) => command.env("O_CARD_ID", "build").env("O_RUN_ID", run_id),
            None => command.env("O_CARD_ID", "").env("O_RUN_ID", ""),
        };
        let out = command.output().unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr))
    };
    // A person's search brings the index up to date; a card only reads it.
    assert_eq!(said(&["context", "서명"], None).0, Some(0));
    let card = Some("20260101-000000-aaaa");
    let (code, tags) = said(&["context", "--wiki"], card);
    assert!(code == Some(0) && tags.contains("#서명 3") && !tags.contains("결제"), "{tags}");
    let (_, listed) = said(&["context", "--wiki", "서명"], card);
    assert!(listed.contains("배운 것/blog/20260101-000000-aaaa-build-1.md — 서명 키는 환경 변수로만 읽는다 #서명 (#서명)") && listed.contains("공용 안내") && !listed.contains("shop") && !listed.contains("둘째 줄"), "{listed}");
    let (code, text) = said(&["context", "--wiki-read", "배운 것/blog/20260101-000000-aaaa-build-1.md"], card);
    assert!(code == Some(0) && text.contains("둘째 줄") && text.contains("참고 데이터"), "{text}");
    assert_eq!(said(&["context", "--wiki-read", "배운 것/shop/20260102-000000-bbbb-build-1.md"], card).0, Some(1));
    assert!(said(&["context", "--wiki", "없는태그"], card).1.contains("쓰이는 태그: #서명 3"));
    // `o dream` inside a card keeps to the same documents, and draws no graph.
    let (_, own) = said(&["dream", "list"], card);
    assert!(own.contains("blog") && !own.contains("shop"), "{own}");
    assert_eq!(said(&["dream", "read", "배운 것/shop/20260102-000000-bbbb-build-1.md"], card).0, Some(1));
    assert_eq!(said(&["dream", "graph"], card).0, Some(1));
    fs::remove_dir_all(base).unwrap();
}
