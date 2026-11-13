//! Human CLI access to a completed run's original event log, in verified pages.

use std::path::Path;
use serde::Serialize;

#[cfg(windows)]
const PAGE_BYTES: usize = 4096;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub run_id: String,
    pub sha256: String,
    pub total_bytes: u64,
    pub offset: u64,
    pub next_offset: Option<u64>,
    pub text: String,
}

/// The raw log may contain tool and team events; it is deliberately not offered to agents.
#[cfg(windows)]
pub fn read(home: &Path, project: &str, run_id: &str, expected: &str, offset: u64) -> Result<Page, String> {
    use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
    use sha2::{Digest, Sha256};
    use super::{project_key, thread_dir, LeadEvent};
    use crate::run::{events::OEvent, store::is_run_id};

    if !is_run_id(run_id) || (expected != "-" && (expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))) || (expected == "-" && offset != 0) {
        return Err("실행 ID, 해시 또는 위치가 올바르지 않습니다".into());
    }
    const MAX_LEAD_RECORD: u64 = 1024 * 1024;
    let threads = match super::tabs::read_open(home, &Default::default()).map_err(|e| e.to_string())?.remove(&project_key(project)) {
        Some(tabs) => tabs.open,
        None => {
            let current_file = std::fs::File::open(home.join("leads/current.json")).map_err(|e| e.to_string())?;
            let mut current = Vec::new();
            current_file.take(MAX_LEAD_RECORD + 1).read_to_end(&mut current).map_err(|e| e.to_string())?;
            if current.len() as u64 > MAX_LEAD_RECORD { return Err("총괄 대화 목록이 안전 상한을 초과했습니다".into()); }
            let mut current: std::collections::BTreeMap<String, String> = serde_json::from_slice(&current).map_err(|e| e.to_string())?;
            current.remove(&project_key(project)).into_iter().collect()
        }
    };
    if threads.is_empty() { return Err("열린 총괄 대화가 없습니다".into()); }
    let mut allowed = false;
    for thread in threads.into_iter().filter(|id| is_run_id(id)) {
        let file = std::fs::File::open(thread_dir(home, &thread).join("events.jsonl")).map_err(|e| e.to_string())?;
        let mut events = BufReader::new(file);
        let mut owner = None;
        let mut dispatched = false;
        loop {
            let mut line = Vec::new();
            let n = events.by_ref().take(MAX_LEAD_RECORD + 1).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
            if n == 0 { break; }
            if line.len() as u64 > MAX_LEAD_RECORD || line.last() != Some(&b'\n') { return Err("총괄 대화 기록이 너무 크거나 불완전합니다".into()); }
            match serde_json::from_slice::<LeadEvent>(&line) {
                Ok(LeadEvent::Start { project: source, .. }) if owner.is_none() => owner = Some(project_key(&source) == project_key(project)),
                Ok(LeadEvent::Dispatch { run_id: id, .. } | LeadEvent::Followup { run_id: id, .. }) if id == run_id => dispatched = true,
                _ => {}
            }
            if owner == Some(false) { break; }
            if owner == Some(true) && dispatched {
                allowed = true;
                break;
            }
        }
        if allowed { break; }
    }
    if !allowed {
        return Err("열린 총괄 대화에서 분배한 실행만 읽을 수 있습니다".into());
    }
    let (mut file, _parents) = super::status_file::open_events(home, run_id).map_err(|e| e.to_string())?.ok_or("실행 기록이 없습니다")?;
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    if len == 0 || offset > len { return Err("기록 또는 위치가 올바르지 않습니다".into()); }
    // The terminal event is small and must be complete; no inference from a PID is made.
    let tail_len = len.min(8192);
    file.seek(SeekFrom::Start(len - tail_len)).map_err(|e| e.to_string())?;
    let mut tail = Vec::with_capacity(tail_len as usize);
    file.read_to_end(&mut tail).map_err(|e| e.to_string())?;
    if tail.last() != Some(&b'\n') { return Err("기록 끝이 잘려 완료를 확인할 수 없습니다".into()); }
    let end = &tail[..tail.len() - 1];
    let start = end.iter().rposition(|b| *b == b'\n').map(|i| i + 1).unwrap_or(0);
    if start == 0 && len > tail_len { return Err("마지막 기록이 너무 커 완료를 확인할 수 없습니다".into()); }
    let end_event: OEvent = serde_json::from_slice(&end[start..]).map_err(|_| "마지막 실행 기록을 해석할 수 없습니다")?;
    if !matches!(end_event, OEvent::RunEnd { run_id: ref id, .. } if id == run_id) { return Err("실행 완료 기록을 확인할 수 없습니다".into()); }

    file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 { break; }
        digest.update(&buffer[..count]);
    }
    let hash = format!("{:x}", digest.finalize());
    if expected != "-" && expected != hash { return Err("기록이 변경되어 이어 읽을 수 없습니다".into()); }
    file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
    let mut page = vec![0; (len - offset).min(PAGE_BYTES as u64) as usize];
    file.read_exact(&mut page).map_err(|e| e.to_string())?;
    let usable = match std::str::from_utf8(&page) {
        Ok(_) => page.len(),
        Err(error) if error.error_len().is_none() && offset + (page.len() as u64) < len => error.valid_up_to(),
        Err(_) => return Err("기록이 UTF-8이 아니거나 위치가 문자 경계가 아닙니다".into()),
    };
    if usable == 0 && !page.is_empty() { return Err("위치가 문자 경계가 아닙니다".into()); }
    let next = offset + usable as u64;
    let text = std::str::from_utf8(&page[..usable]).map_err(|e| e.to_string())?.to_owned();
    Ok(Page { run_id: run_id.into(), sha256: hash, total_bytes: len, offset, next_offset: (next < len).then_some(next), text })
}

#[cfg(not(windows))]
pub fn read(_home: &Path, _project: &str, _run_id: &str, _expected: &str, _offset: u64) -> Result<Page, String> {
    Err("이 운영체제에서는 실행 기록의 안전한 읽기를 지원하지 않습니다".into())
}
