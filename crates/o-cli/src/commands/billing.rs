//! `o billing [--json] [--warn|--subscription]`: the API billing mode (ADR-0035 개정 1).

use std::path::Path;

use o_core::agents::billing::{self, Mode};

use super::{fail, help, Command};

pub const BILLING: Command = Command {
    name: "billing",
    usage: "  o billing [--json] [--warn|--subscription]
                             API 과금 모드: --warn(경고만, 기본)은 CLI 인증 환경변수를 그대로 넘기고 경고,
                             --subscription(구독만)은 그 CLI가 쓰는 변수가 있으면 카드·총괄·검토를 시작하지 않음
                             (변수 이름만 확인, 값은 읽거나 지우지 않음. 상태·사용량·모델 조회는 두 모드 모두 실행)",
    run: report,
};

/// A card or a lead turn may read the mode but not change it: the person picks how they pay.
fn inside_agent() -> bool {
    ["O_AGENT_ROLE", "O_CARD_ID"].iter().any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
}

fn report(home: &Path, args: &[String]) -> i32 {
    let mut as_json = false;
    let mut mode: Option<Mode> = None;
    for arg in args {
        match arg.as_str() {
            "--json" => as_json = true,
            "--warn" | "--subscription" => {
                let value = if arg == "--warn" { Mode::Warn } else { Mode::Subscription };
                if mode.is_some_and(|previous| previous != value) {
                    return fail("--warn과 --subscription을 함께 쓸 수 없습니다");
                }
                mode = Some(value);
            }
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{}", help())),
        }
    }
    if let Some(mode) = mode {
        if inside_agent() {
            return fail("에이전트 안에서는 API 과금 모드를 바꿀 수 없습니다. 사용자가 실행해 주세요.");
        }
        if let Err(error) = billing::write_mode(home, mode.as_str()) {
            return fail(&error);
        }
    }
    let report = billing::report(home);
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return 0;
    }
    match report.mode {
        Mode::Warn => println!("API 과금: 경고만 (settings.json {}=warn) · 인증 환경변수를 넘기고 경고합니다. 구독만: o billing --subscription", billing::KEY),
        Mode::Subscription => println!("API 과금: 구독만 (settings.json {}=subscription) · CLI가 쓰는 인증 환경변수가 있으면 작업을 시작하지 않습니다. 경고만: o billing --warn", billing::KEY),
    }
    0
}
