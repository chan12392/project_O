//! `o sessions [--json] [--since H]`: this PC's recent Claude Code sessions, read-only.

use std::path::Path;

use chrono::Utc;
use o_core::run::snapshot::snapshot;
use o_core::sessions;
use o_core::sys::js::{collapse_spaces, js_head};
use serde_json::json;

use super::{fail, help, since_arg, Command};

pub const SESSIONS: Command = Command {
    name: "sessions",
    usage: "  o sessions [--json] [--since H]   최근 H시간(기본 24) Claude Code 세션과 서브에이전트 (읽기 전용)",
    run: list,
};

fn list(home: &Path, args: &[String]) -> i32 {
    let mut since = sessions::DEFAULT_SINCE_HOURS;
    let mut as_json = false;
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--since" => match since_arg(items.next()) {
                Ok(hours) => since = hours,
                Err(code) => return code,
            },
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{}", help())),
        }
    }
    let list = sessions::read_sessions(&sessions::default_roots(), Some(&sessions::cache_path(home)), Utc::now(), sessions::hours(since));
    if as_json {
        println!("{}", json!({ "sessions": list }));
        return 0;
    }
    if list.is_empty() {
        println!("최근 {since}시간 동안 Claude Code 세션이 없습니다.");
    }
    for record in &list {
        let events: Vec<_> = record.events.iter().filter_map(|e| serde_json::from_value(e.clone()).ok()).collect();
        let Some(run) = snapshot(&events) else { continue };
        let count = |state: &str| run.cards.iter().filter(|c| c.state.as_str() == state).count();
        println!(
            "{}  [{}]  {}  서브에이전트 {} (진행 {} · 완료 {} · 실패 {} · 중지 {})  {}",
            run.run_id,
            run.status.as_str(),
            run.project,
            run.cards.len(),
            count("running"),
            count("done"),
            count("failed"),
            count("stopped"),
            js_head(&collapse_spaces(&run.title), 60)
        );
    }
    0
}
