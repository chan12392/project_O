//! `o ram [--json] [--on|--off] [--percent N]`: the RAM cap on card starts (ADR-0037).

use std::path::Path;

use o_core::run::ram;
use serde_json::{json, Value};

use super::{fail, help, Command};

pub const RAM: Command = Command {
    name: "ram",
    usage: "  o ram [--json] [--on|--off] [--percent 50~95]
                             RAM 상한: 사용률이 상한 이상이면 새 카드를 차례 기다림으로 둠
                             (실행 중인 카드가 하나도 없으면 시작, 실행 중인 카드는 끝내지 않음.
                             --percent: settings.json에 상한 저장, 기본 켜짐·70%)",
    run: report,
};

fn report(home: &Path, args: &[String]) -> i32 {
    let mut as_json = false;
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--on" | "--off" => {
                if let Err(error) = ram::write_enabled(home, arg == "--on") {
                    return fail(&error);
                }
            }
            "--percent" => {
                let value = items.next().and_then(|v| v.parse::<f64>().ok()).map_or(Value::Null, |n| json!(n));
                if let Err(error) = ram::write_percent(home, &value) {
                    return fail(&error);
                }
            }
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{}", help())),
        }
    }
    let report = ram::report(home);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return 0;
    }
    let used = report.used.map_or("알 수 없음 (상한이 적용되지 않음)".to_string(), |p| format!("{p}%"));
    if report.enabled {
        println!("RAM 상한: {}% (settings.json {}) · 지금 사용률 {used}", report.percent, ram::PERCENT_KEY);
    } else {
        println!("RAM 상한: 꺼짐 (settings.json {}, 켜기: o ram --on) · 지금 사용률 {used}", ram::ENABLED_KEY);
    }
    0
}
