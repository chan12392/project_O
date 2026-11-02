//! `o check`, `o dispatch` and its background half `o _run`.

use std::io::Read;
use std::path::Path;
use std::process::Command as Process;

use chrono::Utc;
use o_core::policy::check_plan;
use o_core::run::mode::{take_flags, RunMode};
use o_core::run::runner;
use o_core::run::store::{self, run_dir};
use o_core::sys::proc;
use serde_json::{json, Value};

use super::{fail, print_violations, run_id_arg, Command};

pub const CHECK: Command = Command {
    name: "check",
    usage: "  o check [plan.json|-]      정책 검사만 (실행 안 함)",
    run: |home, args| match take_flags(args) {
        Ok((mode, rest)) => check(home, rest.first().map(String::as_str), false, mode),
        Err(error) => fail(&error),
    },
};

pub const DISPATCH: Command = Command {
    name: "dispatch",
    usage: "  o dispatch [--efficiency on|off] [--clean] [--no-context] [plan.json|-]
                             정책 검사 후 백그라운드 실행, runId 출력
                             --efficiency (토큰 효율, 기본 on — ADR-0010):
                               on : 깨끗한 프로필 + 세션 공유 컨텍스트 + 하루 예산 effort 자동 낮춤
                               off: 평소 하네스 + 컨텍스트 없음 + 계획의 effort 그대로
                               어느 쪽이든 예산 소진 차단·LIMITS·가드 규칙은 같음
                             --clean: 깨끗한 프로필만 켬 (off와 함께 쓰는 세부 조정)
                             --no-context: 컨텍스트만 끔 (on과 함께 쓰는 세부 조정)",
    run: |home, args| match take_flags(args) {
        Ok((mode, rest)) => check(home, rest.first().map(String::as_str), true, mode),
        Err(error) => fail(&error),
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

fn check(home: &Path, path: Option<&str>, dispatch: bool, mode: RunMode) -> i32 {
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
    let created = store::create_run(home, &run_id, &plan).and_then(|()| store::mark_mode(home, &run_id, &mode));
    if let Err(error) = created {
        return fail(&error.to_string());
    }
    let started = std::env::current_exe().and_then(|exe| proc::spawn_detached(Process::new(exe).args(["_run", &run_id])));
    if let Err(error) = started {
        return fail(&format!("실행기를 시작하지 못했습니다: {error}"));
    }
    let events = run_dir(home, &run_id).join("events.jsonl");
    let cards: Vec<&str> = plan.cards.iter().map(|c| c.id.as_str()).collect();
    println!(
        "{}",
        json!({ "runId": run_id, "events": events.to_string_lossy(), "cards": cards, "efficiency": mode.efficiency, "clean": mode.clean, "context": mode.context, "lowerEffort": mode.lower_effort })
    );
    0
}
