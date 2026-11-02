//! `o budget [--json] [--cached] [--percent N] [--on|--off]`: today's usage budget per provider (ADR-0004, ADR-0013).

use std::path::Path;

use chrono::Utc;
use o_core::usage::budget;
use serde_json::{json, Value};

use super::{fail, help, Command};

pub const BUDGET: Command = Command {
    name: "budget",
    usage: "  o budget [--json] [--cached] [--percent 12|13|14] [--on|--off]
                             에이전트별 오늘 사용 예산(주간 한도의 12~14%)·속도·effort 조절
                             (기록이 10분보다 오래되면 먼저 조회, --cached: 조회 안 함,
                             --percent: settings.json의 하루 예산 비율 저장,
                             --on/--off: 하루 사용 예산 켜기·끄기. 끄면 예산으로 막거나 effort를
                             낮추지 않음. 구독 한도·쿨다운·LIMITS는 그대로)",
    run: report,
};

fn report(home: &Path, args: &[String]) -> i32 {
    let (mut as_json, mut cached) = (false, false);
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--cached" => cached = true,
            "--on" | "--off" => {
                if let Err(error) = budget::write_enabled(home, arg == "--on") {
                    return fail(&error);
                }
            }
            "--percent" => {
                let value = items.next().and_then(|v| v.parse::<f64>().ok()).map_or(Value::Null, |n| json!(n));
                if let Err(error) = budget::write_percent(home, &value) {
                    return fail(&error);
                }
            }
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{}", help())),
        }
    }
    if !cached {
        budget::refresh_if_stale(home, Utc::now());
    }
    let report = match budget::report(home, Utc::now()) {
        Ok(report) => report,
        Err(error) => return fail(&error.to_string()),
    };
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return 0;
    }
    if report.enabled {
        println!("하루 사용 예산: 주간 한도의 {}% (settings.json {})", report.percent, budget::PERCENT_KEY);
    } else {
        println!("하루 사용 예산: 꺼짐 — 예산으로 막거나 effort를 낮추지 않음 (settings.json {}, 켜기: o budget --on)", budget::ENABLED_KEY);
    }
    for b in &report.providers {
        let Some(spent) = b.spent else {
            println!("{:<6} 주간 기록 없음 — 조절하지 않음", b.provider.as_str());
            continue;
        };
        let group = b.group.as_deref().map(|g| format!(" ({g})")).unwrap_or_default();
        let pace = b.pace.map_or("-".to_string(), |p| format!("{p:.2}"));
        let efforts: Vec<String> = b.efforts.iter().map(|c| format!("{}→{}", c.from.as_deref().unwrap_or("미지정"), c.to)).collect();
        let adjust = match b.state {
            budget::Pace::Over => format!("새 카드 거부 ({})", b.message),
            budget::Pace::Off => "예산 꺼짐".to_string(),
            _ if efforts.is_empty() => "effort 그대로".to_string(),
            _ => format!("effort {}단계 낮춤: {}", b.steps_down, efforts.join(", ")),
        };
        println!(
            "{:<6} 주간 {:.1}% · 오늘 {spent:.1}/{:.0}%p · {:.1}시간 남음 · 속도 {pace}{group} — {adjust}",
            b.provider.as_str(),
            b.weekly_used.unwrap_or_default(),
            b.allowance,
            b.remaining_hours.unwrap_or_default(),
        );
    }
    0
}
