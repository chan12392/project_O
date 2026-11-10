//! `o usage [--cached] [--json] [--profile project-o|own]`: subscription windows from each CLI's usage probe.

use std::path::Path;

use o_core::run::mode::Profile;
use o_core::usage::{probe, UsageReading};
use serde_json::{json, Value};

use super::{fail, Command};

pub const USAGE: Command = Command {
    name: "usage",
    usage: "  o usage [--json] [--profile project-o|own]
                             Codex·Antigravity 구독 한도를 각 CLI로 조회해 usage.json에 저장
                             (--profile: 어느 로그인의 한도인지. 없으면 지난 조회와 같은 프로필,
                             기록이 없으면 project_O 프로필, 거기 로그인이 없으면 내 설정)
  o usage --cached [--json]  조회하지 않고 저장된 usage.json만 출력",
    run: |home, args| {
        let (mut as_json, mut cached, mut profile) = (false, false, None);
        let mut items = args.iter();
        while let Some(arg) = items.next() {
            match arg.as_str() {
                "--json" => as_json = true,
                "--cached" => cached = true,
                "--profile" => match items.next().and_then(|value| Profile::parse(value)) {
                    Some(value) => profile = Some(value),
                    None => return fail("--profile은 project-o 또는 own이어야 합니다"),
                },
                other => return fail(&format!("알 수 없는 인자: {other}")),
            }
        }
        usage(home, as_json, cached, profile)
    },
};

fn usage(home: &Path, as_json: bool, cached: bool, profile: Option<Profile>) -> i32 {
    let reading: Value = if cached {
        match probe::load_usage(home) {
            Ok(saved) => saved.unwrap_or_else(|| json!(UsageReading::empty())),
            Err(error) => return fail(&error.to_string()),
        }
    } else {
        let reading = probe::read_usage(home, profile);
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
    if let Some(profile) = reading.profile {
        println!("{} 로그인의 한도", profile.label());
    }
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
