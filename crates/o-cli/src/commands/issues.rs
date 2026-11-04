use std::{fs, io::Read, path::Path};
use o_core::{issues::{self, Finding, IssueRef, Resolution}, review::OfficialGh};
use super::{fail, Command};

pub const ISSUES: Command = Command {
    name: "issues",
    usage: "  o issues search <owner/repo> <검색어> | record <finding.json> | list <owner/repo>\n  o issues classify <id> <분류> [<이슈번호> <updatedAt>]\n  o issues resolve <id> <검증실행> <카드> <증거> | reopen <id>\n                             GitHub 이슈 조회와 로컬 발견 기록 (외부 쓰기 없음; ADR-0027)",
    run,
};

fn run(home: &Path, args: &[String]) -> i32 {
    let result = match args {
        [cmd, repo, query] if cmd == "search" => std::env::current_dir().map_err(|e| e.to_string()).and_then(|cwd| issues::search(&OfficialGh, &cwd, repo, query)).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        [cmd, path] if cmd == "record" => read_finding(path).and_then(|s| serde_json::from_str::<Finding>(&s).map_err(|e| e.to_string())).and_then(|v| std::env::current_dir().map_err(|e| e.to_string()).and_then(|cwd| issues::record(home, &OfficialGh, &cwd, &v))).map(|id| serde_json::json!({"id": id})),
        [cmd, repo] if cmd == "list" => issues::list(home, repo).map(|v| serde_json::json!(v)),
        [cmd, id, classification] if cmd == "classify" => classify(home, id, classification, None),
        [cmd, id, classification, number, updated_at] if cmd == "classify" => number.parse::<u64>().map_err(|_| "이슈 번호가 올바르지 않습니다".to_string()).and_then(|number| classify(home, id, classification, Some(IssueRef { number, updated_at: updated_at.clone() }))),
        [cmd, id, run_id, card_id, evidence] if cmd == "resolve" => resolve(home, id, Some(Resolution { run_id: run_id.clone(), card_id: card_id.clone(), evidence: evidence.clone() })),
        [cmd, id] if cmd == "reopen" => resolve(home, id, None),
        _ => Err("사용법: o issues search <owner/repo> <검색어> | record <finding.json> | list <owner/repo>".into()),
    };
    match result { Ok(value) => { println!("{value}"); 0 }, Err(error) => fail(&error) }
}

fn resolve(home: &Path, id: &str, resolution: Option<Resolution>) -> Result<serde_json::Value, String> {
    let id = id.parse::<i64>().map_err(|_| "발견 ID가 올바르지 않습니다".to_string())?;
    issues::resolve(home, id, resolution)?;
    Ok(serde_json::json!({"id": id}))
}

fn read_finding(path: &str) -> Result<String, String> {
    if path != "-" { return fs::read_to_string(path).map_err(|e| e.to_string()); }
    let mut input = String::new();
    std::io::stdin().take(1_048_577).read_to_string(&mut input).map_err(|e| e.to_string())?;
    if input.len() > 1_048_576 { return Err("발견 기록은 1MB까지입니다".into()); }
    Ok(input)
}

fn classify(home: &Path, id: &str, classification: &str, issue: Option<IssueRef>) -> Result<serde_json::Value, String> {
    let id = id.parse::<i64>().map_err(|_| "발견 ID가 올바르지 않습니다".to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    issues::classify(home, &OfficialGh, &cwd, id, classification, issue)?;
    Ok(serde_json::json!({"id": id}))
}
