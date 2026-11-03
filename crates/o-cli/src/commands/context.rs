//! `o context` (ADR-0005): search the shared session context, or add a note to it.

use std::path::Path;

use o_core::context::{self, Hit};
use serde_json::json;

use super::{fail, Command};

pub const CONTEXT: Command = Command {
    name: "context",
    usage: "  o context <질문> [--project <폴더>] [--limit N] [--json]
                             이전 작업 결과·팀 대화·메모에서 관련 내용 찾기 (카드도 읽기 전용으로 사용)
  o context add [--project <폴더>] \"내용\"   프로젝트 컨텍스트에 메모 추가
  o context --result <선행 카드 ID> <SHA-256> <바이트 위치>   같은 실행의 선행 결과를 검증해 나누어 읽기",
    run: context_main,
};

const DEFAULT_LIMIT: usize = 5;

/// Inside a card (`O_CARD_ID` set by the runner) the context is read-only.
fn in_card() -> bool {
    std::env::var("O_CARD_ID").is_ok_and(|v| !v.is_empty())
}

struct Args {
    words: Vec<String>,
    project: Option<String>,
    limit: usize,
    json: bool,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args { words: Vec::new(), project: None, limit: DEFAULT_LIMIT, json: false };
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => out.json = true,
            "--project" => out.project = Some(items.next().ok_or("--project 뒤에 폴더 경로가 필요합니다")?.clone()),
            "--limit" => out.limit = items.next().and_then(|n| n.parse().ok()).filter(|n| *n > 0).ok_or("--limit 뒤에 1 이상의 정수가 필요합니다")?,
            other if other.starts_with("--") => return Err(format!("알 수 없는 인자: {other}")),
            other => out.words.push(other.to_string()),
        }
    }
    Ok(out)
}

/// `o context <질문> | add "내용"`.
fn context_main(home: &Path, args: &[String]) -> i32 {
    if args.first().is_some_and(|arg| arg == "--result") {
        let [_, source, hash, offset] = args else { return fail("사용법: o context --result <선행 카드 ID> <SHA-256> <바이트 위치>") };
        let (Ok(run), Ok(card), Ok(offset)) = (std::env::var("O_RUN_ID"), std::env::var("O_CARD_ID"), offset.parse::<usize>()) else { return fail("실행 중인 카드와 올바른 위치가 필요합니다") };
        return match o_core::run::result::read(home, &run, &card, source, hash, offset) {
            Ok(chunk) => { println!("{}", json!(chunk)); 0 }
            Err(error) => fail(&error),
        };
    }
    let mut parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => return fail(&message),
    };
    if parsed.words.first().map(String::as_str) == Some("add") {
        parsed.words.remove(0);
        return add(home, parsed);
    }
    let query = parsed.words.join(" ");
    if query.trim().is_empty() {
        return fail("찾을 내용을 적으세요: o context \"질문\"");
    }
    let conn = if in_card() { context::open_read_only(home) } else { context::refresh(home).map(Some) };
    let conn = match conn {
        Ok(conn) => conn,
        Err(error) => return fail(&error.to_string()),
    };
    let (project, hits) = match &conn {
        Some(conn) => {
            // A card searches its own run's project only, never another one or all of them.
            let project = if in_card() {
                let run_id = std::env::var("O_RUN_ID").unwrap_or_default();
                match context::card_project(conn, &run_id, parsed.project.as_deref()) {
                    Ok(project) => Some(project),
                    Err(message) => return fail(&message),
                }
            } else {
                context::default_project(conn, parsed.project.as_deref())
            };
            match context::search(conn, &query, project.as_deref(), parsed.limit) {
                Ok(hits) => (project, hits),
                Err(error) => return fail(&error.to_string()),
            }
        }
        None => (parsed.project.map(|p| context::normalize_project(&p)), Vec::new()),
    };
    if parsed.json {
        println!("{}", json!({ "query": query, "project": project, "hits": hits }));
        return 0;
    }
    print_hits(project.as_deref(), &hits);
    0
}

fn print_hits(project: Option<&str>, hits: &[Hit]) {
    if hits.is_empty() {
        println!("관련 컨텍스트가 없습니다{}.", project.map(|p| format!(" (프로젝트 {p})")).unwrap_or_default());
        println!("이전 작업 결과, 팀 대화와 추가한 메모에서 찾습니다. 프로젝트 파일 전체를 검색하지는 않습니다.");
        return;
    }
    for (i, hit) in hits.iter().enumerate() {
        println!("{}. [{}] {}", i + 1, context::source(hit), hit.title);
        println!("   {}", hit.excerpt);
        if !hit.files.is_empty() {
            println!("   파일: {}", hit.files);
        }
    }
}

fn add(home: &Path, parsed: Args) -> i32 {
    if in_card() {
        return fail("카드 안에서는 컨텍스트를 읽기만 합니다. 다른 카드가 알아야 할 것은 o msg send로 보내세요.");
    }
    let text = parsed.words.join(" ");
    let conn = match context::refresh(home) {
        Ok(conn) => conn,
        Err(error) => return fail(&error.to_string()),
    };
    let Some(project) = context::default_project(&conn, parsed.project.as_deref()) else {
        return fail("프로젝트를 정하지 못했습니다: --project <폴더>를 주거나 실행 기록이 있는 프로젝트 폴더에서 실행하세요");
    };
    match context::add_note(&conn, &project, &text) {
        Ok(()) => {
            if parsed.json {
                println!("{}", json!({ "added": true, "project": project }));
            } else {
                println!("메모 추가: {project}");
            }
            0
        }
        Err(error) => fail(&error.to_string()),
    }
}
