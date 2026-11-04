//! `o status [runId]`: each card's state, for one run or the last five.

use std::path::Path;

use o_core::run::snapshot::snapshot;
use o_core::run::events::OEvent;
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

pub const CHANGES: Command = Command {
    name: "changes",
    usage: "  o changes <runId> <cardId>  분리된 카드의 변경 목록 (읽기 전용 JSON)",
    run: |home, args| match args {
        [run, card] if run_id_arg(Some(run)).is_some() => match o_core::run::review_changes(home, run, card) {
            Ok(changes) => { println!("{changes}"); 0 }
            Err(error) => super::fail(&error.to_string()),
        },
        _ => super::fail("사용법: o changes <runId> <cardId>"),
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
        let events = store::read_events(home, &id);
        let Some(run) = snapshot(&events) else { continue };
        println!("{id}  {}  [{}]", run.title, run.status.as_str());
        if let Ok(bytes) = std::fs::read(store::run_dir(home, &id).join("workspace-baseline.json")) {
            if let Ok(baseline) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(commit) = baseline.get("gitCommit").and_then(|v| v.as_str()) {
                    println!("  작업 전 Git 기준: {commit}");
                    println!("  분리된 결과는 원본에 자동 반영되지 않습니다. 기준과 작업 폴더를 비교해 검토하세요.");
                    println!("  변경 목록: o changes {id} <cardId>");
                } else if let Some(path) = baseline.get("copyPath").and_then(|v| v.as_str()) {
                    println!("  작업 전 복사본 (변경 가능): {path}");
                    println!("  분리된 결과는 원본에 자동 반영되지 않습니다. 기준과 작업 폴더를 비교해 검토하세요.");
                    println!("  변경 목록: o changes {id} <cardId>");
                }
            }
        }
        if run.failed_before_cards() {
            println!("  막힘     카드를 시작하지 않음: {}", collapse_spaces(&run.blocked_reason()));
        }
        for c in run.cards {
            let activity = collapse_spaces(&c.activity);
            println!("  {:<8} {} ({} · {}) {}", c.state.as_str(), c.id, c.provider, c.label, js_head(&activity, 100));
            if let Some(path) = events.iter().rev().find_map(|event| match event {
                OEvent::CardTool { card_id, name, summary, .. } if card_id == &c.id && name == "O 작업 폴더" => Some(summary),
                _ => None,
            }) { println!("           작업 폴더: {path}"); }
        }
    }
    0
}
