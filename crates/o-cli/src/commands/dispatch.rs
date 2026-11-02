//! `o check`, `o dispatch` and its background half `o _run`.

use std::io::Read;
use std::path::Path;
use std::process::Command as Process;

use chrono::Utc;
use o_core::policy::check_plan;
use o_core::run::runner;
use o_core::run::store::{self, run_dir};
use o_core::sys::proc;
use serde_json::{json, Value};

use super::{fail, print_violations, run_id_arg, Command};

pub const CHECK: Command = Command { name: "check", usage: "  o check [plan.json|-]      정책 검사만 (실행 안 함)", run: |home, args| check(home, plan_arg(args), false, false, false) };

pub const DISPATCH: Command = Command {
    name: "dispatch",
    usage: "  o dispatch [--clean] [--no-context] [plan.json|-]
                             정책 검사 후 백그라운드 실행, runId 출력
                             --clean: 카드를 깨끗한 에이전트 프로필로 실행 (o agents)
                             --no-context: 이전 작업 컨텍스트를 카드에 넣지 않음 (토큰 A/B 기준선)",
    run: |home, args| {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        check(home, plan_arg(args), true, has("--clean"), has("--no-context"))
    },
};

/// `o _run <runId>`: the runner `o dispatch` starts in the background.
pub const RUN: Command = Command {
    name: "_run",
    usage: "",
    run: |home, args| {
        run_id_arg(args.first().map(String::as_str)).map_or(1, |id| match runner::run_detached(home, id) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                1
            }
        })
    },
};

fn read_input(path: Option<&str>) -> Result<Value, String> {
    let bytes = match path {
        None | Some("-") => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).map_err(|e| e.to_string())?;
            buf
        }
        Some(path) => std::fs::read(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// The plan file among `check`/`dispatch` arguments (`--clean`, `--no-context` are flags).
fn plan_arg(args: &[String]) -> Option<&str> {
    args.iter().map(String::as_str).find(|a| *a != "--clean" && *a != "--no-context")
}

fn check(home: &Path, path: Option<&str>, dispatch: bool, clean: bool, no_context: bool) -> i32 {
    let input = match read_input(path) {
        Ok(input) => input,
        Err(error) => return fail(&error),
    };
    let now = Utc::now();
    let ctx = match store::policy_context(home, now) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&error.to_string()),
    };
    let checked = check_plan(&input, &ctx);
    let Some(plan) = checked.plan else {
        if dispatch {
            let title = input.as_object().and_then(|o| o.get("title"));
            if let Err(error) = store::record_blocked(home, now, title, &checked.violations) {
                eprintln!("{error}");
            }
        }
        print_violations(&checked.violations);
        return 2;
    };
    if !dispatch {
        let cards: Vec<String> = plan.cards.iter().map(|c| format!("{}→{}", c.id, c.provider)).collect();
        println!("정책 통과: 카드 {}개 ({})", plan.cards.len(), cards.join(", "));
        return 0;
    }
    let run_id = store::new_run_id(now);
    let created = store::create_run(home, &run_id, &plan)
        .and_then(|()| if clean { store::mark_clean(home, &run_id) } else { Ok(()) })
        .and_then(|()| if no_context { o_core::context::mark_off(home, &run_id) } else { Ok(()) });
    if let Err(error) = created {
        return fail(&error.to_string());
    }
    let started = std::env::current_exe().and_then(|exe| proc::spawn_detached(Process::new(exe).args(["_run", &run_id])));
    if let Err(error) = started {
        return fail(&format!("실행기를 시작하지 못했습니다: {error}"));
    }
    let events = run_dir(home, &run_id).join("events.jsonl");
    let cards: Vec<&str> = plan.cards.iter().map(|c| c.id.as_str()).collect();
    println!("{}", json!({ "runId": run_id, "events": events.to_string_lossy(), "cards": cards, "clean": clean }));
    0
}
