//! `o runs --json [--limit N] [--project <folder>] [--sessions [--since H]]`: always JSON, the app's shape.

use std::path::Path;

use o_core::run::index::RunQuery;
use o_core::run::store;
use o_core::sessions;
use serde_json::json;

use super::{fail, help, since_arg, Command};

pub const RUNS: Command = Command {
    name: "runs",
    usage: "  o runs --json [--limit N] [--project <폴더>] [--sessions [--since H]]
                             실행별 이벤트(최근 시작 순, 기본 50개) — 앱 화면용
                             --sessions: 최근 H시간(기본 24) Claude Code 세션도 실행으로 함께",
    run: runs,
};

fn runs(home: &Path, args: &[String]) -> i32 {
    let mut query = RunQuery { limit: 50, project: None };
    let mut with_sessions = false;
    let mut since = sessions::DEFAULT_SINCE_HOURS;
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => {}
            "--sessions" => with_sessions = true,
            "--since" => match since_arg(items.next()) {
                Ok(hours) => since = hours,
                Err(code) => return code,
            },
            "--limit" => match items.next().and_then(|n| n.parse().ok()) {
                Some(limit) => query.limit = limit,
                None => return fail("--limit 뒤에 0 이상의 정수가 필요합니다"),
            },
            "--project" => match items.next() {
                Some(project) => query.project = Some(project),
                None => return fail("--project 뒤에 폴더 경로가 필요합니다"),
            },
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{}", help())),
        }
    }
    let read = if with_sessions { sessions::read_runs_with_sessions(home, &query, sessions::hours(since)) } else { store::read_runs(home, &query) };
    match read {
        Ok(runs) => {
            println!("{}", json!({ "runs": runs }));
            0
        }
        Err(error) => fail(&error.to_string()),
    }
}
