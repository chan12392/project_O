use std::io::Read;
use std::path::Path;
use std::process::Command as Process;

use o_core::run::followup;
use o_core::run::input::{self, RunInput};
use serde_json::json;

use super::{fail, Command};

pub const FOLLOWUP: Command = Command {
    name: "followup",
    usage: "  o followup <runId> <cardId>   기존 작업에 표준 입력으로 후속 지시",
    run: send,
};

pub const RUN_FOLLOWUP: Command = Command {
    name: "_followup",
    usage: "",
    run: |home, args| {
        let Some(id) = super::run_id_arg(args.first().map(String::as_str)) else { return 1 };
        match input::receive(home, id, true).and_then(|input| o_core::run::runner::run_detached(&input)) { Ok(()) => 0, Err(error) => fail(&error) }
    },
};
pub const RUN_FOLLOWUP_APPROVED: Command = Command { name: "_followup-approved", ..RUN_FOLLOWUP };

fn send(home: &Path, args: &[String]) -> i32 {
    if args.len() != 2 { return fail("실행 ID와 작업 ID가 필요합니다"); }
    let mut text = String::new();
    if let Err(error) = std::io::stdin().take(65_537).read_to_string(&mut text) { return fail(&error.to_string()); }
    if text.len() > 65_536 { return fail("후속 지시가 너무 깁니다"); }
    let start = |id: &str, input: &RunInput| std::env::current_exe().and_then(|exe| input::spawn(Process::new(exe).args(["_followup-approved", id]), input));
    match followup::start(home, &args[0], &args[1], &text, &start) {
        Ok(run) => { println!("{}", json!({"runId": run})); 0 }
        Err(error) => fail(&error),
    }
}
