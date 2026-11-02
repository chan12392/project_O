//! `o context` and `o tokens` (ADR-0005): the shared session context and the
//! token report that compares the default harness with clean agents plus it.

use std::path::Path;

use o_core::context::tokens::{self, Group, Report, TokenQuery};
use o_core::context::{self, Hit};
use o_core::sys::js::parse_time;
use serde_json::json;

const DEFAULT_LIMIT: usize = 5;

/// Inside a card (`O_CARD_ID` set by the runner) the context is read-only.
fn in_card() -> bool {
    std::env::var("O_CARD_ID").is_ok_and(|v| !v.is_empty())
}

fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
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
pub fn context_main(home: &Path, args: &[String]) -> i32 {
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

/// `o tokens [--project P] [--since DATE] [--json] [--compare]`.
pub fn tokens_main(home: &Path, args: &[String]) -> i32 {
    let (mut project, mut since, mut as_json, mut compare) = (None, None, false, false);
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--compare" => compare = true,
            "--project" => match items.next() {
                Some(p) => project = Some(p.clone()),
                None => return fail("--project 뒤에 폴더 경로가 필요합니다"),
            },
            "--since" => match items.next().and_then(|d| since_arg(d)) {
                Some(d) => since = Some(d),
                None => return fail("--since 뒤에 날짜(2026-11-02)나 ISO 시각이 필요합니다"),
            },
            other => return fail(&format!("알 수 없는 인자: {other}")),
        }
    }
    let report = match tokens::report(home, &TokenQuery { project: project.as_deref(), since: since.as_deref() }) {
        Ok(report) => report,
        Err(error) => return fail(&error.to_string()),
    };
    if as_json {
        println!("{}", json!(report));
    } else if compare {
        print_compare(&report);
    } else {
        print_report(&report);
    }
    0
}

/// A date is the start of that day (UTC, as run times are stored); a time is taken as given.
fn since_arg(text: &str) -> Option<String> {
    if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(format!("{date}T00:00:00.000Z"));
    }
    parse_time(text).map(o_core::sys::js::iso)
}

fn n(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn print_report(report: &Report) {
    if report.runs.is_empty() {
        println!("토큰 기록이 있는 실행이 없습니다.");
        return;
    }
    println!("모드별 카드당 평균 (새 입력 / 캐시 / 출력 = 합계)");
    for g in &report.groups {
        let t = &g.per_card;
        println!("  {} 실행 {:>3} 카드 {:>3}  {} / {} / {} = {}", left(&g.label, 18), g.runs, g.cards, n(t.input), n(t.cached), n(t.output), n(t.total));
        for p in &g.by_provider {
            println!("    {:<6} 카드 {:>3}  합계 {}", p.provider, p.cards, n(p.per_card.total));
        }
    }
    println!("\n실행별 (최근 순)");
    for run in &report.runs {
        let mode = match (run.clean, run.context) {
            (false, false) => "기본",
            (false, true) => "기본+공유",
            (true, false) => "깨끗",
            (true, true) => "깨끗+공유",
        };
        println!("  {}  {} {}  합계 {}", run.run_id, left(mode, 10), run.title, n(run.tokens.total));
        for c in &run.cards {
            let t = &c.tokens;
            let shared = if c.context { " +컨텍스트" } else { "" };
            println!("    {:<12} {:<6} {} / {} / {} = {}{shared}", c.card_id, c.provider, n(t.input), n(t.cached), n(t.output), n(t.total));
        }
    }
}

fn change(a: u64, b: u64) -> String {
    if a == 0 {
        return "-".into();
    }
    let pct = (b as f64 - a as f64) / a as f64 * 100.0;
    format!("{pct:+.0}%")
}

/// Columns as a terminal shows them: Hangul and other wide characters take two cells.
fn cells(text: &str) -> usize {
    text.chars().map(|c| if c >= '\u{1100}' { 2 } else { 1 }).sum()
}

fn left(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(cells(text))))
}

fn right(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(cells(text))))
}

fn print_compare(report: &Report) {
    let (a, b) = (&report.compare.baseline, &report.compare.shared);
    let col = |g: &Option<Group>, f: &dyn Fn(&Group) -> u64| g.as_ref().map_or("-".to_string(), |g| n(f(g)));
    println!("{}{}{}{}", left("", 20), right("기본 하네스", 14), right("깨끗+세션 공유", 18), right("차이", 8));
    let row = |name: &str, f: &dyn Fn(&Group) -> u64| {
        let diff = match (a, b) {
            (Some(a), Some(b)) => change(f(a), f(b)),
            _ => "-".into(),
        };
        println!("{}{}{}{}", left(name, 20), right(&col(a, f), 14), right(&col(b, f), 18), right(&diff, 8));
    };
    row("실행", &|g| g.runs as u64);
    row("카드", &|g| g.cards as u64);
    row("카드당 새 입력", &|g| g.per_card.input);
    row("카드당 캐시 입력", &|g| g.per_card.cached);
    row("카드당 출력", &|g| g.per_card.output);
    row("카드당 합계", &|g| g.per_card.total);
    let mut providers: Vec<&str> = a.iter().chain(b.iter()).flat_map(|g| g.by_provider.iter().map(|p| p.provider.as_str())).collect();
    providers.sort_unstable();
    providers.dedup();
    for provider in providers {
        let per = |g: &Group| g.by_provider.iter().find(|p| p.provider == provider).map_or(0, |p| p.per_card.total);
        row(&format!("{provider} 카드당 합계"), &per);
    }
    if a.is_none() || b.is_none() {
        println!("\n한쪽 기록이 없습니다. 같은 계획을 o dispatch --no-context 와 o dispatch --clean 으로 한 번씩 실행하세요 (docs/token-benchmark.md).");
    }
    let others: Vec<String> = report.groups.iter().filter(|g| g.key != "normal" && g.key != "clean+context").map(|g| format!("{} 카드 {}", g.label, g.cards)).collect();
    if !others.is_empty() {
        println!("비교에서 뺀 기록: {}", others.join(", "));
    }
}
