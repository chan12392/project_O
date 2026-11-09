//! The `o` commands: one file per command, listed once in [`COMMANDS`]. `o help`
//! is built from that table, in its order.

use std::path::Path;
use std::sync::LazyLock;

use o_core::policy::Violation;
use o_core::run::store::is_run_id;

mod about;
mod agents;
mod budget;
mod context;
mod dispatch;
mod events;
mod followup;
mod guard;
mod graph;
mod images;
mod issues;
mod lead;
mod mirror;
mod msg;
mod projects;
mod review;
mod resources;
mod runs;
mod schedule;
mod sessions;
mod status;
mod stop;
mod tokens;
mod usage;
mod yolo;

pub struct Command {
    pub name: &'static str,
    /// Its lines in `o help`; empty for the internal commands `o` starts itself.
    pub usage: &'static str,
    pub run: fn(home: &Path, args: &[String]) -> i32,
}

/// Every command, in the order `o help` lists them.
pub const COMMANDS: &[Command] = &[
    dispatch::CHECK,
    dispatch::DISPATCH,
    followup::FOLLOWUP,
    status::STATUS,
    status::CHANGES,
    events::EVENTS,
    stop::STOP,
    msg::MSG,
    lead::LEAD,
    images::IMAGE,
    issues::ISSUES,
    review::REVIEW,
    resources::RESOURCES,
    context::CONTEXT,
    tokens::TOKENS,
    usage::USAGE,
    budget::BUDGET,
    yolo::YOLO,
    runs::RUNS,
    sessions::SESSIONS,
    schedule::SCHEDULE,
    projects::PROJECTS,
    graph::GRAPH,
    guard::HOOK,
    guard::SCAN,
    mirror::SYNC_MIRROR,
    agents::AGENTS,
    about::ABOUT,
    dispatch::RUN,
    lead::WATCH,
    lead::APPROVE,
    lead::FORWARD,
    followup::RUN_FOLLOWUP,
    review::REVIEW_BACKGROUND,
];

pub fn find(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

static HELP: LazyLock<String> = LazyLock::new(|| {
    let lines: Vec<&str> = COMMANDS.iter().map(|c| c.usage).filter(|u| !u.is_empty()).collect();
    format!(
        "O CLI — 작업카드를 공식 에이전트 CLI에 병렬로 전달합니다.\n\n{}\n\n정책 위반이 하나라도 있으면 계획 전체를 실행하지 않고 종료 코드 2로 끝납니다.",
        lines.join("\n")
    )
});

/// `o help`: what every listed command takes.
pub fn help() -> &'static str {
    &HELP
}

fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
}

fn print_violations(violations: &[Violation]) {
    eprintln!("차단됨 — 정책 위반으로 이 계획은 실행되지 않았습니다:");
    for v in violations {
        let card = v.card_id.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
        eprintln!("  - [{}]{card} {}", v.rule, v.message);
    }
    eprintln!("위반을 우회하지 말고 사용자에게 이 내용을 그대로 보고하세요.");
}

/// `--since <hours>`: a positive number of hours.
fn since_arg(value: Option<&String>) -> Result<f64, i32> {
    match value.and_then(|h| h.parse::<f64>().ok()).filter(|h| h.is_finite() && *h > 0.0) {
        Some(hours) => Ok(hours),
        None => Err(fail("--since 뒤에 0보다 큰 시간(시간 단위)이 필요합니다")),
    }
}

/// Run ids name folders, so a malformed one stops here.
fn run_id_arg(value: Option<&str>) -> Option<&str> {
    match value {
        Some(id) if is_run_id(id) => Some(id),
        Some(id) => {
            eprintln!("실행 id 형식이 아닙니다: {id}");
            None
        }
        None => {
            eprintln!("{}", help());
            None
        }
    }
}
