use std::path::Path;

use o_core::run::store;
use super::{fail, Command};

pub const RESOURCES: Command = Command {
    name: "resources",
    usage: "  o resources [--json]       남아 있는 자원 예약과 프로세스 기록 확인\n  o resources recover <id>   시작 전 중단 또는 종료 확인된 예약 복구 (실행기가 죽은 뒤 작업 프로세스와 자손이 모두 끝난 예약 포함, Windows)",
    run: list,
};

fn list(home: &Path, args: &[String]) -> i32 {
    if args.first().is_some_and(|arg| arg == "recover") {
        let Some(id) = args.get(1).and_then(|arg| arg.parse::<i64>().ok()).filter(|id| *id > 0) else { return fail("사용법: o resources recover <id>") };
        if args.len() != 2 { return fail("사용법: o resources recover <id>"); }
        return match store::recover_resource(home, id) {
            Ok(()) => { println!("예약 {id} 복구 완료"); 0 }
            Err(error) => fail(&error.to_string()),
        };
    }
    if args.iter().any(|arg| arg != "--json") || args.len() > 1 { return fail("사용법: o resources [--json] | o resources recover <id>"); }
    let leases = match store::resource_leases(home) { Ok(value) => value, Err(error) => return fail(&error.to_string()) };
    if !args.is_empty() {
        println!("{}", serde_json::json!({"leases": leases}));
    } else if leases.is_empty() {
        println!("남아 있는 자원 예약이 없습니다");
    } else {
        for lease in leases {
            let status = if lease.state == "recoverable" { "복구 가능" } else { "종료 불명" };
            println!("{} / {} · 예약 {} · {} · 실행기 PID {:?} · 작업 PID {:?} · 자원 {}", lease.run_id, lease.card_id, lease.id, status, lease.owner_pid, lease.worker_pid, lease.resources);
        }
        println!("시작 전이거나 종료가 기록된 예약도 소유자 종료가 확인될 때만 복구할 수 있습니다");
    }
    0
}
