//! Antigravity worker consent.
use std::path::Path;
use o_core::agents::agy::{self, Change};
use super::{fail, Command};

pub const AGY: Command = Command {
    name: "agy",
    usage: "  o agy [--json] [--workers on|off] [--ack] [--confirm-paste on|off]  Antigravity 작업 설정",
    run: report,
};

fn report(home: &Path, args: &[String]) -> i32 {
    let mut change = Change::default();
    let mut as_json = false;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--ack" => change.ack = Some(true),
            "--workers" | "--confirm-paste" => {
                let value = match args.next().map(String::as_str) {
                    Some("on") => true,
                    Some("off") => false,
                    _ => return fail(&format!("{arg}에는 on 또는 off가 필요합니다")),
                };
                if arg == "--workers" { change.workers = Some(value); } else { change.confirm_paste = Some(value); }
            }
            other => return fail(&format!("알 수 없는 인자: {other}")),
        }
    }
    let result = if change.workers.is_some() || change.confirm_paste.is_some() {
        match agy::set(home, change) { Ok(report) => report, Err(error) => return fail(&error) }
    } else { agy::report(home) };
    if as_json { println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default()); }
    else {
        println!("Antigravity 작업: {} · 붙여넣기 확인: {}", if result.workers { "켜짐" } else { "꺼짐" }, if result.confirm_paste { "켜짐" } else { "꺼짐" });
        if !result.workers {
            for quote in result.notice["quotes"].as_array().into_iter().flatten() { println!("{}", quote.as_str().unwrap_or_default()); }
            for link in result.notice["links"].as_array().into_iter().flatten() { println!("{}: {}", link["label"].as_str().unwrap_or_default(), link["url"].as_str().unwrap_or_default()); }
            println!("{}\n켜기: o agy --workers on --ack", result.notice["warning"].as_str().unwrap_or_default());
        }
    }
    0
}
