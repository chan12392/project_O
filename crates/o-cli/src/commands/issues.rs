use std::{fs, path::Path};
use o_core::{issues::{self, Finding, IssueRef}, review::OfficialGh};
use super::{fail, Command};

pub const ISSUES: Command = Command {
    name: "issues",
    usage: "  o issues search <owner/repo> <검색어> | record <finding.json> | list <owner/repo>\n  o issues classify <id> <분류> [<이슈번호> <updatedAt>]\n                             GitHub 이슈 조회와 로컬 발견 기록 (외부 쓰기 없음; ADR-0027)",
    run,
};

fn run(home: &Path, args: &[String]) -> i32 {
    let result = match args {
        [cmd, repo, query] if cmd == "search" => std::env::current_dir().map_err(|e| e.to_string()).and_then(|cwd| issues::search(&OfficialGh, &cwd, repo, query)).and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())),
        [cmd, path] if cmd == "record" => fs::read_to_string(path).map_err(|e| e.to_string()).and_then(|s| serde_json::from_str::<Finding>(&s).map_err(|e| e.to_string())).and_then(|v| std::env::current_dir().map_err(|e| e.to_string()).and_then(|cwd| issues::record(home, &OfficialGh, &cwd, &v))).map(|id| serde_json::json!({"id": id})),
        [cmd, repo] if cmd == "list" => issues::list(home, repo).map(|v| serde_json::json!(v)),
        [cmd, id, classification] if cmd == "classify" => classify(home, id, classification, None),
        [cmd, id, classification, number, updated_at] if cmd == "classify" => number.parse::<u64>().map_err(|_| "이슈 번호가 올바르지 않습니다".to_string()).and_then(|number| classify(home, id, classification, Some(IssueRef { number, updated_at: updated_at.clone() }))),
        _ => Err("사용법: o issues search <owner/repo> <검색어> | record <finding.json> | list <owner/repo>".into()),
    };
    match result { Ok(value) => { println!("{value}"); 0 }, Err(error) => fail(&error) }
}

fn classify(home: &Path, id: &str, classification: &str, issue: Option<IssueRef>) -> Result<serde_json::Value, String> {
    let id = id.parse::<i64>().map_err(|_| "발견 ID가 올바르지 않습니다".to_string())?;
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    issues::classify(home, &OfficialGh, &cwd, id, classification, issue)?;
    Ok(serde_json::json!({"id": id}))
}
