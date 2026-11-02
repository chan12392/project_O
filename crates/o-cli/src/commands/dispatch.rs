//! `o check`, `o dispatch` and its background half `o _run`.

use std::io::Read;
use std::path::Path;
use std::process::Command as Process;

use chrono::Utc;
use o_core::policy::check_plan;
use o_core::run::mode::{take_flags, RunMode};
use o_core::run::{runner, yolo};
use o_core::run::store::{self, run_dir};
use o_core::sys::proc;
use serde_json::{json, Value};

use super::{fail, print_violations, run_id_arg, Command};

pub const CHECK: Command = Command {
    name: "check",
    usage: "  o check [plan.json|-]      정책 검사만 (실행 안 함)",
    run: |home, args| match plan_flags(home, args) {
        Ok((mode, plan)) => check(home, plan.as_deref(), false, mode),
        Err(error) => fail(&error),
    },
};

pub const DISPATCH: Command = Command {
    name: "dispatch",
    usage: "  o dispatch [--profile project-o|own] [--no-context] [--mode yolo|normal] [plan.json|-]
                             정책 검사 후 백그라운드 실행, runId 출력
                             --profile (project_O 프로필 사용, 기본 project-o — ADR-0013):
                               project-o: 카드를 O의 깨끗한 에이전트 프로필로 실행 (o agents)
                               own      : 이 PC의 평소 CLI 설정(스킬·훅·메모리) 그대로
                             --clean: --profile project-o와 같음
                             --no-context: 이전 작업 컨텍스트를 카드에 넣지 않음 (토큰 A/B 기준선)
                             --mode yolo|normal: 이 실행만 YOLO 모드 켜기·끄기 (주지 않으면 o yolo 설정, ADR-0014)
                             하루 사용 예산(차단·effort 낮춤)은 o budget --on|--off로 따로 정함",
    run: |home, args| match plan_flags(home, args) {
        Ok((mode, plan)) => check(home, plan.as_deref(), true, mode),
        Err(error) => fail(&error),
    },
};

/// The run mode and the plan file among `check`/`dispatch` arguments; any other flag is refused
/// (a removed or misspelled one included, so it never reads as a plan file name).
fn plan_flags(home: &Path, args: &[String]) -> Result<(RunMode, Option<String>), String> {
    let (mut mode, rest) = take_flags(args)?;
    let (yolo, rest) = yolo::take_flag(rest)?;
    mode.yolo = yolo::resolve(home, yolo);
    if let Some(flag) = rest.iter().find(|a| a.starts_with("--")) {
        return Err(format!("알 수 없는 인자: {flag}"));
    }
    Ok((mode, rest.into_iter().next()))
}

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
        json!({ "runId": run_id, "events": events.to_string_lossy(), "cards": cards, "profile": mode.profile, "clean": mode.clean(), "context": mode.context, "yolo": mode.yolo })
    );
    0
}
