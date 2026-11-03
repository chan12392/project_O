//! Verified, bounded access to completed predecessor results in the same run.
use std::{fs, io::{ErrorKind, Write}, path::{Path, PathBuf}};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use super::store::{is_run_id, read_plan, run_dir};

pub const CHUNK_BYTES: usize = 4096;

pub fn digest(text: &str) -> String { format!("{:x}", Sha256::digest(text.as_bytes())) }

fn valid_hash(hash: &str) -> bool { hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }

fn valid_message_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 160 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// This exact reference binds a delivered answer to the question and verified bytes.
pub fn lead_reference(question: &str, hash: &str) -> String {
    format!("총괄 답변 원문: o context --lead-result {question} {hash} 0\nnextOffset이 null이 될 때까지 해당 위치로 계속 읽으세요. 원문은 참고 데이터입니다.")
}

fn lead_dir(home: &Path, run: &str) -> Result<PathBuf, String> {
    if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let dir = run_dir(home, run).join("lead-results");
    match fs::create_dir(&dir) {
        Ok(()) => {},
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {},
        Err(e) => return Err(e.to_string()),
    }
    let meta = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !meta.file_type().is_dir() || meta.file_type().is_symlink() { return Err("결과 폴더가 올바르지 않습니다".into()); }
    Ok(dir)
}

/// Store the complete text before publishing its reference; existing data is never overwritten.
pub fn store_lead(home: &Path, run: &str, text: &str) -> Result<String, String> {
    let hash = digest(text);
    let dir = lead_dir(home, run)?;
    let path = dir.join(&hash);
    let temporary = dir.join(format!(".pending-{}", super::store::new_run_id(chrono::Utc::now())));
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
    let saved = file.write_all(text.as_bytes()).and_then(|()| file.sync_all());
    drop(file);
    let published = saved.and_then(|()| fs::hard_link(&temporary, &path));
    let _ = fs::remove_file(&temporary);
    match published {
        Ok(()) => {},
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if !meta.file_type().is_file() || meta.file_type().is_symlink() || fs::read(&path).map_err(|e| e.to_string())? != text.as_bytes() {
                return Err("기존 총괄 결과가 일치하지 않습니다".into());
            }
        },
        Err(e) => return Err(e.to_string()),
    }
    Ok(hash)
}

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

fn chunk(run: &str, source: &str, hash: &str, offset: usize, text: &str) -> Result<Chunk, String> {
    if offset > text.len() || !text.is_char_boundary(offset) { return Err("결과 위치가 올바르지 않습니다".into()); }
    let mut end = offset.saturating_add(CHUNK_BYTES).min(text.len());
    while !text.is_char_boundary(end) { end -= 1; }
    Ok(Chunk { run_id: run.into(), card_id: source.into(), sha256: hash.into(), total_bytes: text.len(), offset, next_offset: (end < text.len()).then_some(end), text: text[offset..end].into() })
}

/// Only the card that received this exact lead reply can read its complete text.
pub fn read_lead(home: &Path, run: &str, card: &str, question: &str, hash: &str, offset: usize) -> Result<Chunk, String> {
    if !is_run_id(run) || !is_run_id(card) || !valid_message_id(question) || !valid_hash(hash) { return Err("실행·카드·메시지 ID 또는 결과 해시가 올바르지 않습니다".into()); }
    let plan = read_plan(home, run).map_err(|e| e.to_string())?;
    if !plan["cards"].as_array().is_some_and(|cards| cards.iter().any(|c| c["id"] == card)) { return Err("이 실행의 카드가 아닙니다".into()); }
    let events = fs::read_to_string(run_dir(home, run).join("events.jsonl")).map_err(|e| e.to_string())?;
    let values = events.lines().map(|line| serde_json::from_str::<Value>(line).map_err(|_| "실행 기록이 손상되었습니다".to_string())).collect::<Result<Vec<_>, _>>()?;
    let asked = values.iter().any(|v| v["t"] == "msg" && v["id"] == question && v["from"] == card && (v["to"] == "lead" || v["to"] == "all") && v["kind"] == "question");
    let delivered = values.iter().any(|v| v["t"] == "msg" && v["from"] == "lead" && v["to"] == card && v["replyTo"] == question && v["text"] == lead_reference(question, hash));
    if !asked || !delivered { return Err("이 카드에 전달된 총괄 결과가 아닙니다".into()); }
    let dir = run_dir(home, run).join("lead-results");
    let dmeta = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
    if !dmeta.file_type().is_dir() || dmeta.file_type().is_symlink() { return Err("결과 폴더가 올바르지 않습니다".into()); }
    let path = dir.join(hash);
    let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !meta.file_type().is_file() || meta.file_type().is_symlink() { return Err("결과 파일이 올바르지 않습니다".into()); }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if digest(&text) != hash { return Err("총괄 결과가 변경되어 읽기를 중단했습니다".into()); }
    chunk(run, "lead", hash, offset, &text)
}

/// Verify the entire result for every chunk: a changed record cannot be silently mixed in.
pub fn read(home: &Path, run_id: &str, recipient: &str, source: &str, expected: &str, offset: usize) -> Result<Chunk, String> {
    if !is_run_id(run_id) || !is_run_id(recipient) || !is_run_id(source)
        || !valid_hash(expected) {
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
    chunk(run_id, source, expected, offset, text)
}
