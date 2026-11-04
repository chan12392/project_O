//! Verified, bounded access to completed predecessor results in the same run.
use std::{fs, io::{ErrorKind, Read, Write}, path::{Path, PathBuf}};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use super::store::{is_run_id, locked_event_text, read_plan};
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

fn lead_dir(home: &Path, run: &str, create: bool) -> Result<(PathBuf, Vec<fs::File>), String> {
    if !is_run_id(run) { return Err("실행 ID가 올바르지 않습니다".into()); }
    let home = fs::canonicalize(home).map_err(|e| e.to_string())?;
    let runs = home.join("runs");
    let run_dir = runs.join(run);
    let dir = run_dir.join("lead-results");
    #[cfg(windows)]
    let mut held = Vec::new();
    #[cfg(not(windows))]
    let held = Vec::new();
    for part in [&home, &runs, &run_dir, &dir] {
        if part == &dir && create {
            match fs::create_dir(part) {
                Ok(()) => {},
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {},
                Err(e) => return Err(e.to_string()),
            }
        }
        let meta = fs::symlink_metadata(part).map_err(|e| e.to_string())?;
        if !meta.is_dir() || meta.file_type().is_symlink() { return Err("결과 폴더가 올바르지 않습니다".into()); }
        #[cfg(windows)]
        {
            use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
            use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE};
            let file = fs::OpenOptions::new().read(true).access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(part).map_err(|e| e.to_string())?;
            let opened = file.metadata().map_err(|e| e.to_string())?;
            if !opened.is_dir() || opened.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 { return Err("결과 폴더가 연결 지점입니다".into()); }
            held.push(file);
        }
        #[cfg(not(windows))]
        if fs::canonicalize(part).map_err(|e| e.to_string())? != *part { return Err("결과 폴더 경로가 변경되었습니다".into()); }
    }
    Ok((dir, held))
}

/// Store the complete text before publishing its reference; existing data is never overwritten.
pub fn store_lead(home: &Path, run: &str, text: &str) -> Result<String, String> {
    let hash = digest(text);
    let (dir, _held) = lead_dir(home, run, true)?;
    let path = dir.join(&hash);
    let temporary = dir.join(format!(".pending-{}", super::store::new_run_id(chrono::Utc::now())));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    { use std::os::windows::fs::OpenOptionsExt; options.share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ); }
    let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
    let saved = file.write_all(text.as_bytes()).and_then(|()| file.sync_all());
    let published = saved.and_then(|()| fs::hard_link(&temporary, &path));
    drop(file);
    fs::remove_file(&temporary).map_err(|e| format!("임시 총괄 결과를 정리하지 못했습니다: {e}"))?;
    match published {
        Ok(()) => {},
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {},
        Err(e) => return Err(e.to_string()),
    }
    let mut file = super::workspaces::open_plain(&path).map_err(|e| e.to_string())?;
    let mut existing = Vec::new();
    file.read_to_end(&mut existing).map_err(|e| e.to_string())?;
    if existing != text.as_bytes() { return Err("저장된 총괄 결과가 일치하지 않습니다".into()); }
    Ok(hash)
}

/// Keep a complete CLI prompt stable until the child has read it.
pub(crate) struct PromptFile {
    path: PathBuf,
    file: Option<fs::File>,
    _directories: Vec<fs::File>,
}

impl PromptFile {
    pub(crate) fn path(&self) -> &Path { &self.path }
}

impl Drop for PromptFile {
    fn drop(&mut self) {
        self.file.take();
        let _ = fs::remove_file(&self.path);
    }
}

pub(crate) fn prompt_file(home: &Path, run: &str, text: &str) -> Result<PromptFile, String> {
    let (dir, directories) = lead_dir(home, run, true)?;
    let path = dir.join(format!(".prompt-{}", super::store::new_run_id(chrono::Utc::now())));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let file = options.open(&path).map_err(|e| e.to_string())?;
    let mut prompt = PromptFile { path, file: Some(file), _directories: directories };
    let file = prompt.file.as_mut().expect("newly created prompt file");
    file.write_all(text.as_bytes()).and_then(|()| file.sync_all()).map_err(|e| e.to_string())?;
    prompt.file.take();
    let mut file = super::workspaces::open_plain(&prompt.path).map_err(|e| e.to_string())?;
    let mut actual = Vec::new();
    file.read_to_end(&mut actual).map_err(|e| e.to_string())?;
    if actual != text.as_bytes() { return Err("저장된 프롬프트가 일치하지 않습니다".into()); }
    prompt.file = Some(file);
    Ok(prompt)
}

#[cfg(all(test, unix))]
mod unix_prompt_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn prompt_file_is_private_to_its_owner() {
        let base = std::env::temp_dir().join(format!("o-prompt-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let home = base.join("state");
        fs::create_dir_all(home.join("runs/run")).unwrap();
        let prompt = prompt_file(&home, "run", "private").unwrap();
        assert_eq!(fs::metadata(prompt.path()).unwrap().permissions().mode() & 0o077, 0);
    }
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
    let (dir, _held) = lead_dir(home, run, false)?;
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
    let path = dir.join(hash);
    let mut file = super::workspaces::open_plain(&path).map_err(|e| e.to_string())?;
    let mut text = String::new();
    file.read_to_string(&mut text).map_err(|e| e.to_string())?;
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
    let text = verify_event_result(&events, source, expected)?;
    chunk(run_id, source, expected, offset, &text)
}

/// Validate a locked event snapshot without acquiring the event lock again.
pub(crate) fn verify_event_result(events: &str, source: &str, expected: &str) -> Result<String, String> {
    framed(events)?;
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
    Ok(text.to_string())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn prompt_file_stays_readable_and_immutable_until_dropped() {
        let base = std::env::temp_dir().join(format!("o-prompt-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let home = base.join("state");
        fs::create_dir_all(home.join("runs/run")).unwrap();
        let text = format!("start{}끝", "data".repeat(12_000));
        let prompt = prompt_file(&home, "run", &text).unwrap();
        let path = prompt.path().to_path_buf();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert!(fs::write(&path, "changed").is_err());
        assert!(fs::remove_file(&path).is_err());
        drop(prompt);
        assert!(!path.exists());
    }

    #[test]
    fn lead_result_storage_refuses_a_junction_and_an_existing_hard_link() {
        let base = std::env::temp_dir().join(format!("o-result-path-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        fs::create_dir(&base).unwrap();
        let canonical = base.canonicalize().unwrap();
        assert!(canonical.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        let home = base.join("state");
        let run = home.join("runs").join("run");
        let outside = base.join("outside");
        fs::create_dir_all(&run).unwrap();
        fs::create_dir(&outside).unwrap();
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/C", "mklink", "/J"]).arg(run.join("lead-results")).arg(&outside).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        assert!(crate::sys::proc::hide_window(&mut cmd).status().unwrap().success());
        assert!(store_lead(&home, "run", "secret").is_err());
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir(run.join("lead-results")).unwrap();
        fs::create_dir(run.join("lead-results")).unwrap();
        let original = base.join("original.txt");
        fs::write(&original, "secret").unwrap();
        fs::hard_link(&original, run.join("lead-results").join(digest("secret"))).unwrap();
        assert!(store_lead(&home, "run", "secret").is_err());
        assert_eq!(fs::read_to_string(original).unwrap(), "secret");
    }
}
