use std::path::Path;

use o_core::run::store;
use super::{fail, Command};

pub const RESOURCES: Command = Command {
    name: "resources",
    usage: "  o resources [--json]       남아 있는 자원 예약과 프로세스 기록 확인\n  o resources recover <id> [--confirm-exited]   예약 복구 (직접 확인은 사람 전용)",
    run: list,
};

fn list(home: &Path, args: &[String]) -> i32 {
    if args.first().is_some_and(|arg| arg == "recover") {
        let Some(id) = args.get(1).and_then(|arg| arg.parse::<i64>().ok()).filter(|id| *id > 0) else { return fail("사용법: o resources recover <id>") };
        let confirmed = args.get(2).is_some_and(|arg| arg == "--confirm-exited");
        if args.len() != if confirmed { 3 } else { 2 } { return fail("사용법: o resources recover <id> [--confirm-exited]"); }
        let result = if confirmed {
            eprintln!("경고: O는 카드의 자식 프로세스가 모두 끝났는지 증명하지 못했습니다. 작업 관리자 등에서 이 카드가 실행한 프로그램이 모두 끝났는지 확인한 경우에만 복구하세요.");
            store::release_confirmed(home, id)
        } else { store::recover_resource(home, id) };
        return match result {
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
