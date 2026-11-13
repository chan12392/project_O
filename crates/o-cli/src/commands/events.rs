//! `o events <runId> [--follow]`: a run's normalized events as JSON lines.

use std::path::Path;
use std::time::Duration;

use o_core::run::store;
use serde_json::Value;

use super::{run_id_arg, Command};

pub const EVENTS: Command = Command {
    name: "events",
    usage: "  o events <runId> [--follow]  정규화된 이벤트(JSONL) — UI·총괄 에이전트용",
    run: |home, args| run_id_arg(args.first().map(String::as_str)).map_or(1, |id| events(home, id, args.iter().any(|a| a == "--follow"))),
};

fn events(home: &Path, run_id: &str, follow: bool) -> i32 {
    let mut printed = 0;
    loop {
        let list = store::read_event_values(home, run_id);
        for e in list.iter().skip(printed) {
            println!("{e}");
        }
        printed = list.len();
        if !follow || list.iter().any(|e| e.get("t").and_then(Value::as_str) == Some("run.end")) {
            return 0;
        }
        // A runner that died never writes run.end: close the run on record (or find it closed by
        // another reader) and print the closing events on the next pass (ADR-0039).
        if store::close_dead_run(home, run_id).is_ok_and(|current| current.is_some_and(|events| events.iter().any(|e| e.get("t").and_then(Value::as_str) == Some("run.end")))) {
            continue;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
