//! `o dream` (ADR-0041): the Dreaming documents — materials a person put in and one record per finished card.

use std::io::Read;
use std::path::Path;

use o_core::{dream, tuning};
use serde_json::{json, Value};

use super::{fail, Command};

pub const DREAM: Command = Command {
    name: "dream",
    usage: "  o dream list|graph [--json]   Dreaming 문서 목록·연결 그래프 (배운 것, 외부 자료, 작업 성공·실패 기록, 도구 오류)
  o dream read <문서> [--json]   문서 본문과 연결
  o dream add -                 외부 자료 넣기 (stdin JSON: { kind: text|url, title?, text?, source?, url?, project? })
  o dream remove <문서>          외부 자료 빼기
  o dream changes [--json]      반복 실패에서 카드 프롬프트에 더한 줄 (자동 적용)
  o dream revert|restore <ID>   그 줄을 되돌리기 / 다시 적용하기",
    run: dream_main,
};

/// The most a material's JSON may be on stdin: its text (1 MiB) with room for escapes.
const MAX_INPUT_BYTES: u64 = 8 * 1024 * 1024;

fn dream_main(home: &Path, args: &[String]) -> i32 {
    match run(home, args) {
        Ok(value) => { println!("{value}"); 0 }
        Err(error) => fail(&error),
    }
}

/// Inside a card (`O_CARD_ID` set by the runner) nothing is added or removed: a card
/// must not plant what later reads as something a person filed.
fn in_card() -> bool {
    std::env::var("O_CARD_ID").is_ok_and(|v| !v.is_empty())
}

fn run(home: &Path, args: &[String]) -> Result<Value, String> {
    let Some((command, rest)) = args.split_first() else { return Err("dream 하위 명령이 필요합니다".into()) };
    let rest: Vec<&str> = rest.iter().map(String::as_str).filter(|arg| *arg != "--json").collect();
    match (command.as_str(), rest.as_slice()) {
        ("list", []) => dream::list(home).map(|v| json!(v)),
        ("graph", []) => dream::graph(home).map(|v| json!(v)),
        ("read", [document]) if !document.starts_with("--") => dream::read(home, document).map(|v| json!(v)),
        ("add" | "remove", _) if in_card() => Err("카드 안에서는 Dreaming 자료를 넣거나 뺄 수 없습니다".into()),
        ("add", ["-"]) => {
            let mut bytes = Vec::new();
            std::io::stdin().lock().take(MAX_INPUT_BYTES + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            if bytes.len() as u64 > MAX_INPUT_BYTES { return Err("입력이 너무 큽니다".into()); }
            let input: Value = serde_json::from_slice(&bytes).map_err(|e| format!("입력을 읽지 못했습니다: {e}"))?;
            dream::add(home, &input).map(|v| json!(v))
        }
        ("remove", [document]) if !document.starts_with("--") => dream::remove(home, document).map(|()| json!({ "ok": true })),
        ("changes", []) => tuning::refresh(home).map(|v| json!(v)),
        ("revert" | "restore", _) if in_card() || std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) => Err("Dreaming이 바꾼 것은 사람만 되돌리거나 다시 적용할 수 있습니다".into()),
        ("revert" | "restore", [id]) if !id.starts_with("--") => tuning::set_reverted(home, id, command == "revert").map(|v| json!(v)),
        ("list" | "graph" | "read" | "add" | "remove" | "changes" | "revert" | "restore", _) => Err(format!("dream {command}의 인자가 올바르지 않습니다")),
        _ => Err(format!("알 수 없는 dream 명령: {command}")),
    }
}
