//! `o tokens` (ADR-0005): the token report that compares the default harness
//! with clean agents plus the shared session context.

use std::path::Path;

use o_core::context::tokens::{self, Group, Report, TokenQuery};
use o_core::sys::js::parse_time;
use serde_json::json;

use super::{fail, Command};

pub const TOKENS: Command = Command {
    name: "tokens",
    usage: "  o tokens [--project <폴더>] [--since 날짜] [--compare] [--json]
                             카드별 토큰(새 입력·캐시·출력)과 모드별 평균
                             --compare: 기본 하네스 vs 깨끗한 에이전트+세션 공유
  o tokens reconcile --run 실행ID|--project 폴더 [--apply] [--json]
                             과거 사용량 보정 미리보기; --apply는 사용자가 직접 실행",
    run: tokens_main,
};

/// `o tokens [--project P] [--since DATE] [--json] [--compare]`.
fn tokens_main(home: &Path, args: &[String]) -> i32 {
    if args.first().is_some_and(|arg| arg == "reconcile") { return reconcile(home, &args[1..]); }
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

fn reconcile(home: &Path, args: &[String]) -> i32 {
    let (mut run, mut project, mut apply) = (None, None, false);
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--run" => { run = items.next().map(String::as_str); if run.is_none() { return fail("실행 ID가 필요합니다"); } }
            "--project" => { project = items.next().map(String::as_str); if project.is_none() { return fail("프로젝트 경로가 필요합니다"); } }
            "--apply" => apply = true,
            "--json" => {}
            _ => return fail("o tokens reconcile --run <ID>|--project <폴더> [--apply] [--json]"),
        }
    }
    if apply && super::inside_agent() { return fail("과거 기록 적용은 사용자가 직접 실행해야 합니다"); }
    match o_core::usage::reconcile::run(home, run, project, apply) {
        Ok(changes) => { println!("{}", json!({"applied":apply,"changes":changes})); 0 }
        Err(error) => fail(&error),
    }
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
    let lead = &report.lead;
    if lead.turns > 0 {
        let partial = if lead.incomplete > 0 { " (일부 누락 또는 불명 포함)" } else { "" };
        println!("총괄 대화(계획 부대 비용) {}회  {} / {} / {} = {}{partial}\n", lead.turns, n(lead.input), n(lead.cached), n(lead.output), n(lead.total));
    }
    print_coverage(report);
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
        let partial = if run.incomplete_cards.is_empty() { "" } else { " (일부 누락 또는 불명 포함)" };
        println!("  {}  {} {}  합계 {}{partial}", run.run_id, left(mode, 10), run.title, n(run.tokens.total));
        for c in &run.cards {
            let t = &c.tokens;
            let shared = if c.context { " +컨텍스트" } else { "" };
            println!("    {:<12} {:<6} {} / {} / {} = {}{shared}", c.card_id, c.provider, n(t.input), n(t.cached), n(t.output), n(t.total));
        }
        if !run.unreported_cards.is_empty() {
            println!("    사용량 미보고: {}", run.unreported_cards.join(", "));
        }
        if !run.incomplete_cards.is_empty() {
            println!("    사용량 일부 누락: {}", run.incomplete_cards.join(", "));
        }
    }
}

fn print_coverage(report: &Report) {
    let s = &report.summary;
    let cache = s.cache_percent.map_or_else(|| "미측정".to_string(), |p| format!("{p:.1}%"));
    println!("완전하게 보고된 총괄·카드 합계 {}  입력 캐시 비율 {cache}", n(s.reported.total));
    println!("사용량 보고 카드 {} · 미보고 {} · 일부 누락 {}", s.reported_cards, s.unreported_cards, s.incomplete_cards);
    println!("총괄 요청 {} · 사용량 이벤트 없음 {} · 일부 누락 또는 불명 {} (실행 전 실패·진행 중 포함)", report.lead.requested, report.lead.unreported, report.lead.incomplete);
    println!("미보고에는 대기 중 카드가 포함됩니다. 불완전·불명 기록은 평균과 요약 합계에서 제외합니다.\n");
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
    println!("\n실행 방식의 기술 통계이며 같은 작업·모델·캐시 조건을 보장하지 않습니다. 총괄과 미보고 사용량은 표의 카드 평균에 포함되지 않습니다.");
    print_coverage(report);
}
