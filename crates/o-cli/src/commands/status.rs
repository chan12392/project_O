//! `o status [runId]`: each card's state, for one run or the last five.

use std::path::Path;

use o_core::run::snapshot::snapshot;
use o_core::run::store;
use o_core::sys::js::{collapse_spaces, js_head};

use super::{run_id_arg, Command};

pub const STATUS: Command = Command {
    name: "status",
    usage: "  o status [runId]           카드별 상태 (runId 없으면 최근 5개 실행)",
    run: |home, args| match args.first().map(String::as_str) {
        None => status(home, None),
        Some(id) => run_id_arg(Some(id)).map_or(1, |id| status(home, Some(id))),
    },
};

fn status(home: &Path, run_id: Option<&str>) -> i32 {
    let ids: Vec<String> = match run_id {
        Some(id) => vec![id.to_string()],
        None => {
            let all = store::list_runs(home);
            all[all.len().saturating_sub(5)..].to_vec()
        }
    };
    if ids.is_empty() {
        println!("실행 기록이 없습니다.");
    }
    for id in ids {
        let Some(run) = snapshot(&store::read_events(home, &id)) else { continue };
        println!("{id}  {}  [{}]", run.title, run.status.as_str());
        if run.failed_before_cards() {
            println!("  막힘     카드를 시작하지 않음: {}", collapse_spaces(&run.blocked_reason()));
        }
        for c in run.cards {
            let activity = collapse_spaces(&c.activity);
            println!("  {:<8} {} ({} · {}) {}", c.state.as_str(), c.id, c.provider, c.label, js_head(&activity, 100));
        }
    }
    0
}
