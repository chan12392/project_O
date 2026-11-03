use std::io::Read;
use std::path::Path;
use std::process::Command as Process;
use o_core::agents::clean;
use o_core::lead::{self, LeadEvent, SendRequest};
use o_core::run::plan::ProviderId;
use o_core::run::runner::official_command;
use o_core::run::store;
use o_core::schedule;
use super::{fail, Command};

pub const SCHEDULE: Command = Command { name: "schedule", usage: "  o schedule <list|create|enable|disable|delete|run>  예약 요청 관리", run: main };

fn main(home: &Path, args: &[String]) -> i32 {
    let result = match args.first().map(String::as_str) {
        Some("list") if args.len() == 1 => schedule::list(home).and_then(|v| serde_json::to_string(&v).map_err(|e| e.to_string())),
        Some("create") if args.len() == 2 && args[1] == "-" => {
            let mut bytes = Vec::new();
            match std::io::stdin().take(64 * 1024).read_to_end(&mut bytes) {
                Ok(_) => serde_json::from_slice::<schedule::Request>(&bytes).map_err(|e| e.to_string()).and_then(|request| {
                    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                    schedule::create(home, &store::user_home(), &exe, request).and_then(|v| serde_json::to_string(&v).map_err(|e| e.to_string()))
                }),
                Err(e) => Err(e.to_string()),
            }
        },
        Some("delete") if args.len() == 2 => schedule::delete(home, &args[1]).map(|_| "{}".into()),
        Some("enable" | "disable") if args.len() == 2 => schedule::set_enabled(home, &args[1], args[0] == "enable").and_then(|v| serde_json::to_string(&v).map_err(|e| e.to_string())),
        Some("run") if args.len() == 2 => {
            let result = run(home, &args[1]);
            let recorded = schedule::record(home, &args[1], &result);
            result.and(recorded).map(|_| "{}".into())
        },
        _ => Err(SCHEDULE.usage.into()),
    };
    match result { Ok(text) => { println!("{text}"); 0 }, Err(e) => fail(&e) }
}

fn run(home: &Path, id: &str) -> Result<(), String> {
    let job = schedule::get(home, id)?;
    if !job.enabled { return Err("일시 중지된 예약입니다".into()); }
    let req = job.request;
    let provider = match req.provider.as_str() { "claude" => ProviderId::Claude, "codex" => ProviderId::Codex, _ => return Err("예약의 총괄 에이전트가 올바르지 않습니다".into()) };
    let root = req.clean.then(clean::default_root);
    let request = SendRequest { project: &req.project, provider, model: None, effort: None, text: &req.text, attach: &[], fresh: false, clean: root.as_deref(), yolo: false };
    let start = |run_id: &str| -> std::io::Result<()> { o_core::sys::proc::spawn_detached(Process::new(std::env::current_exe()?).args(["_run", run_id])).map(|_| ()) };
    let events = if req.auto_dispatch { lead::send_with_scheduled_autorun(home, &request, &official_command, &start)? } else { lead::send(home, &request, &official_command)? };
    if let Some(error) = events.iter().find_map(|event| match event { LeadEvent::Error { text, .. } => Some(text), _ => None }) { return Err(error.clone()); }
    if req.auto_dispatch && !events.iter().any(|event| matches!(event, LeadEvent::Dispatch { .. })) {
        return Err(events.iter().rev().find_map(|event| match event { LeadEvent::AutoRun { reason, .. } => reason.clone(), _ => None }).unwrap_or_else(|| "분배할 설계가 생성되지 않았습니다".into()));
    }
    Ok(())
}
