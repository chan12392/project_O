//! `o stop <runId>`: asks a run's runner to stop.

use std::path::Path;

use o_core::run::runner;

use super::{fail, run_id_arg, Command};

pub const STOP: Command = Command {
    name: "stop",
    usage: "  o stop <runId>             실행 중지",
    run: |home, args| run_id_arg(args.first().map(String::as_str)).map_or(1, |id| stop(home, id)),
};

fn stop(home: &Path, run_id: &str) -> i32 {
    match runner::stop_run(home, run_id) {
        Ok(true) => {
            println!("중지 요청: {run_id}");
            0
        }
        Ok(false) => fail(&format!("실행을 찾지 못했습니다: {run_id}")),
        Err(error) => fail(&error.to_string()),
    }
}
