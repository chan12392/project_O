use std::path::Path;
use o_core::run::plan::ProviderId;
use o_core::usage::fallback::{self, Target};
use super::{fail, Command};

pub const FALLBACK: Command = Command { name: "fallback", usage: "  o fallback [--on|--off] [--provider 에이전트 --model 모델] [--json]\n                             확인된 구독 한도의 자동 전환(기본 OFF), 사용자가 지정한 대상으로 한 번만", run };

fn run(home: &Path, args: &[String]) -> i32 {
    if args.first().is_some_and(|v| v == "--settings") {
        if super::inside_agent() { return fail("자동 전환 설정은 사용자가 직접 바꿔야 합니다"); }
        if args.len() < 2 || args.iter().skip(2).any(|v| v != "--json") { return fail("--settings 뒤에 설정 JSON이 필요합니다"); }
        let settings = match serde_json::from_str(&args[1]) { Ok(value) => value, Err(_) => return fail("자동 전환 설정 형식이 올바르지 않습니다") };
        return match fallback::write(home, settings) { Ok(value) => { println!("{}", serde_json::json!(value)); 0 }, Err(error) => fail(&error) };
    }
    let mut settings = fallback::read(home);
    let (mut enabled, mut provider, mut model) = (None, None, None);
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--on" | "--off" => {
                let value = arg == "--on";
                if enabled.is_some_and(|old| old != value) { return fail("--on과 --off는 함께 쓸 수 없습니다"); }
                enabled = Some(value);
            }
            "--provider" => { provider = items.next().and_then(|p| ProviderId::parse(p)); if provider.is_none() { return fail("대체 에이전트가 필요합니다"); } }
            "--model" => { model = items.next().cloned(); if model.is_none() { return fail("대체 모델이 필요합니다"); } }
            "--json" => {}
            _ => return fail("알 수 없는 자동 전환 인자입니다"),
        }
    }
    let changed = enabled.is_some() || provider.is_some() || model.is_some();
    match (provider, model) {
        (Some(provider), Some(model)) => settings.target = Some(Target { provider, model }),
        (None, None) => {}
        _ => return fail("대체 에이전트와 모델을 함께 지정하세요"),
    }
    if let Some(value) = enabled { settings.enabled = value; }
    if changed {
        if super::inside_agent() { return fail("자동 전환 설정은 사용자가 직접 바꿔야 합니다"); }
        settings = match fallback::write(home, settings) { Ok(value) => value, Err(error) => return fail(&error) };
    }
    println!("{}", serde_json::json!(settings));
    0
}
