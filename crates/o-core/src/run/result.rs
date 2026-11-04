//! Verified, bounded access to completed predecessor results in the same run.
use std::{fs, io::{ErrorKind, Write}, path::{Path, PathBuf}};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use super::store::{is_run_id, locked_event_text, read_plan, run_dir};
use super::events::{CardStatus, OEvent};

pub const CHUNK_BYTES: usize = 4096;

pub fn digest(text: &str) -> String { format!("{:x}", Sha256::digest(text.as_bytes())) }

fn valid_hash(hash: &str) -> bool { hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }

fn valid_message_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 160 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn framed(text: &str) -> Result<(), String> {
    if !text.is_empty() && !text.ends_with('\n') { return Err("실행 기록이 불완전합니다".into()); }
    Ok(())
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
    let events = locked_event_text(home, run).map_err(|e| e.to_string())?;
    framed(&events)?;
    let mut asked = false;
    let mut delivered = false;
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).map_err(|_| "실행 기록이 손상되었습니다")?;
        let kind = value.get("t").and_then(Value::as_str).ok_or("실행 기록이 손상되었습니다")?;
        if kind == "msg" {
            let OEvent::Msg { from, to, text, id, kind, reply_to, .. } = serde_json::from_value(value).map_err(|_| "실행 기록이 손상되었습니다")? else { unreachable!() };
            if id.as_deref() == Some(question) && from == card && (to == "lead" || to == "all") && kind.as_deref() == Some("question") { asked = true; }
            if asked && from == "lead" && to == card && reply_to.as_deref() == Some(question) && text == lead_reference(question, hash) { delivered = true; }
        }
    }
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
    let events = locked_event_text(home, run_id).map_err(|e| format!("실행 기록을 읽지 못했습니다: {e}"))?;
    framed(&events)?;
    let mut found: Option<(CardStatus, Option<String>)> = None;
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line).map_err(|_| "실행 기록이 불완전하거나 손상되었습니다")?;
        let kind = event.get("t").and_then(Value::as_str).ok_or("실행 기록이 손상되었습니다")?;
        if matches!(kind, "card.start" | "card.queued" | "card.end") {
            match serde_json::from_value::<OEvent>(event).map_err(|_| "실행 기록이 손상되었습니다")? {
                OEvent::CardEnd { card_id, status, result, .. } if card_id == source => found = Some((status, result)),
                OEvent::CardStart { card_id, .. } | OEvent::CardQueued { card_id, .. } if card_id == source => found = None,
                _ => (),
            }
        }
    }
    let (status, result) = found.ok_or("완료된 선행 결과를 찾지 못했습니다")?;
    if status != CardStatus::Done { return Err("선행 작업이 성공하지 않았습니다".into()); }
    let text = result.as_deref().unwrap_or("");
    if digest(text) != expected { return Err("선행 결과가 변경되어 읽기를 중단했습니다".into()); }
    chunk(run_id, source, expected, offset, text)
}
