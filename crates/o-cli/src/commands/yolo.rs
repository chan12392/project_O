//! `o yolo [--json] [--on|--off] [--max-cards N] [--per-hour N] [--dedupe-minutes N]`:
//! YOLO 모드 and its auto-run limits (ADR-0014).

use std::path::Path;

use o_core::run::yolo;
use serde_json::{json, Value};

use super::{fail, help, Command};

pub const YOLO: Command = Command {
    name: "yolo",
    usage: "  o yolo [--json] [--on|--off] [--max-cards 1-6] [--per-hour 1-6] [--dedupe-minutes 10-120]
                             YOLO 모드: 켜면 총괄과 카드가 묻지 않고 파일을 고치고 명령을 실행 (기본 꺼짐)
                             에이전트의 직접 전달을 제한하지만 같은 사용자 권한에서는 우회될 수 있음
                             하루 사용 예산도 켜져 있으면 정책을 통과한 계획을 한도 안에서 자동 실행
                             (카드 수, 진행 중 실행 없음, 시간당 횟수, 같은 계획 반복 금지 — ADR-0014)",
    run: yolo_main,
};

fn yolo_main(home: &Path, args: &[String]) -> i32 {
    let mut as_json = false;
    let mut enabled = None;
    let (mut max_cards, mut per_hour, mut dedupe) = (None, None, None);
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        let mut number = || items.next().and_then(|v| v.parse::<f64>().ok()).map_or(Value::Null, |n| json!(n));
        match arg.as_str() {
            "--json" => as_json = true,
            "--on" | "--off" => {
                let value = arg == "--on";
                if enabled.is_some_and(|previous| previous != value) { return fail("--on과 --off를 함께 쓸 수 없습니다"); }
                enabled = Some(value);
            }
            "--max-cards" => max_cards = Some(number()),
            "--per-hour" => per_hour = Some(number()),
            "--dedupe-minutes" => dedupe = Some(number()),
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{}", help())),
        }
    }
    if max_cards.is_some() || per_hour.is_some() || dedupe.is_some() {
        if let Err(error) = yolo::write_limits(home, max_cards.as_ref(), per_hour.as_ref(), dedupe.as_ref()) {
            return fail(&error);
        }
    }
    if let Some(enabled) = enabled {
        if let Err(error) = yolo::write_enabled(home, enabled) { return fail(&error); }
    }
    let report = yolo::report(home);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return 0;
    }
    let limits = report.auto_run;
    if report.enabled {
        println!("YOLO 모드: 켜짐 — 총괄과 카드가 묻지 않고 파일을 고치고 명령을 실행합니다 (끄기: o yolo --off)");
        if report.budget_enabled {
            println!(
                "자동 실행: 카드 {}장 이하, 이 대화의 실행이 끝난 뒤, 한 시간에 {}회까지, 같은 계획은 {}분 안에 다시 하지 않음",
                limits.max_cards, limits.per_hour, limits.dedupe_minutes
            );
        } else {
            println!("자동 실행: 하루 사용 예산이 꺼져 있어 하지 않음 (계획은 실행을 눌러야 시작)");
        }
    } else {
        println!("YOLO 모드: 꺼짐 — CLI의 권한 확인을 그대로 둡니다 (켜기: o yolo --on, settings.json {})", yolo::ENABLED_KEY);
    }
    0
}
