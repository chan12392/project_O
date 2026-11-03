//! Verified, bounded access to completed predecessor results in the same run.
use std::{fs, path::Path};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use super::store::{is_run_id, read_plan, run_dir};

pub const CHUNK_BYTES: usize = 4096;

pub fn digest(text: &str) -> String { format!("{:x}", Sha256::digest(text.as_bytes())) }

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chunk {
    pub run_id: String,
    pub card_id: String,
    pub sha256: String,
    pub total_bytes: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub text: String,
}

/// Verify the entire result for every chunk: a changed record cannot be silently mixed in.
pub fn read(home: &Path, run_id: &str, recipient: &str, source: &str, expected: &str, offset: usize) -> Result<Chunk, String> {
    if !is_run_id(run_id) || !is_run_id(recipient) || !is_run_id(source)
        || expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err("실행·카드 ID 또는 결과 해시가 올바르지 않습니다".into());
    }
    let plan = read_plan(home, run_id).map_err(|e| format!("실행 계획을 읽지 못했습니다: {e}"))?;
    let allowed = plan["cards"].as_array().and_then(|cards| cards.iter().find(|c| c["id"] == recipient))
        .and_then(|card| card["depends_on"].as_array())
        .is_some_and(|dependencies| dependencies.iter().any(|id| id == source));
    if !allowed { return Err("이 카드가 의존하는 선행 결과만 읽을 수 있습니다".into()); }
    let events = fs::read_to_string(run_dir(home, run_id).join("events.jsonl")).map_err(|e| format!("실행 기록을 읽지 못했습니다: {e}"))?;
    let mut found: Option<Value> = None;
    for line in events.lines() {
        let event: Value = serde_json::from_str(line).map_err(|_| "실행 기록이 불완전하거나 손상되었습니다")?;
        if event["t"] == "card.end" && event["cardId"] == source { found = Some(event); }
    }
    let event = found.ok_or("선행 결과를 찾지 못했습니다")?;
    if event["status"] != "done" { return Err("선행 작업이 성공하지 않았습니다".into()); }
    let text = match &event["result"] { Value::Null => "", Value::String(value) => value, _ => return Err("선행 결과 형식이 올바르지 않습니다".into()) };
    if digest(text) != expected { return Err("선행 결과가 변경되어 읽기를 중단했습니다".into()); }
    if offset > text.len() || !text.is_char_boundary(offset) { return Err("결과 위치가 올바르지 않습니다".into()); }
    let mut end = offset.saturating_add(CHUNK_BYTES).min(text.len());
    while !text.is_char_boundary(end) { end -= 1; }
    Ok(Chunk { run_id: run_id.into(), card_id: source.into(), sha256: expected.into(), total_bytes: text.len(), offset, next_offset: (end < text.len()).then_some(end), text: text[offset..end].into() })
}
