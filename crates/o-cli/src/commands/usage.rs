//! `o usage [--cached] [--json]`: subscription windows from each CLI's usage probe.

use std::path::Path;

use o_core::usage::{probe, UsageReading};
use serde_json::{json, Value};

use super::{fail, Command};

pub const USAGE: Command = Command {
    name: "usage",
    usage: "  o usage [--json]           Codex·Antigravity 구독 한도를 각 CLI로 조회해 usage.json에 저장
  o usage --cached [--json]  조회하지 않고 저장된 usage.json만 출력",
    run: |home, args| {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        usage(home, has("--json"), has("--cached"))
    },
};

fn usage(home: &Path, as_json: bool, cached: bool) -> i32 {
    let reading: Value = if cached {
        match probe::load_usage(home) {
            Ok(saved) => saved.unwrap_or_else(|| json!(UsageReading::empty())),
            Err(error) => return fail(&error.to_string()),
        }
    } else {
        let reading = probe::read_usage();
        if let Err(error) = probe::save_usage(home, &reading) {
            eprintln!("{error}");
        }
        json!(reading)
    };
    if as_json {
        println!("{}", serde_json::to_string_pretty(&reading).unwrap_or_default());
        return 0;
    }
    let reading: UsageReading = match serde_json::from_value(reading) {
        Ok(reading) => reading,
        Err(error) => return fail(&format!("usage.json: {error}")),
    };
    for w in &reading.windows {
        let name = match w.window.as_str() {
            "five_hour" => "5시간",
            "seven_day" => "주간",
            other => other,
        };
        let percent = (w.used * 100.0 + 0.5).floor();
        println!("{:<6} {:<22} {:<4} {:>3}% 사용  {} 초기화", w.provider.as_str(), w.group.as_deref().unwrap_or(""), name, percent as i64, w.resets_at);
    }
    for u in &reading.unavailable {
        println!("{:<6} 조회 실패: {}", u.provider, u.reason);
    }
    0
}
