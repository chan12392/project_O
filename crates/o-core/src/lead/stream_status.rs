//! Bounded streaming fallback for long run logs. Files are held until the lead exits.
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::LazyLock;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use super::{status_file, StatusRecord, MAX_STATUS_RECORDS};

// shortcut: limit per-turn disk and scan time; upgrade to a bounded, on-demand index.
const MAX_INPUT: u64 = 128 * 1024 * 1024;
const MAX_OUTPUT: u64 = 256 * 1024 * 1024;
const SMALL_LINE: u64 = 64 * 1024;
static LARGE_CARD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\{"t":"card\.end","at":"[^"\\\r\n]{1,128}","cardId":"([A-Za-z0-9][A-Za-z0-9_-]{0,63})","status":"(?:done|failed|stopped)","result":""#).unwrap());
static LARGE_END: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"","exitCode":(?:null|-?[0-9]{1,19})\}\r?\n$"#).unwrap());

fn emit_pages(mut input: impl Read, output: &mut std::fs::File, first: &mut u64, meta: &mut Value) -> io::Result<()> {
    let mut hash = Sha256::new();
    let start = *first;
    let mut buf = [0u8; 256];
    let mut len = 0usize;
    let mut eof = false;
    loop {
        while len < buf.len() && !eof {
            let read = input.read(&mut buf[len..])?;
            if read == 0 { eof = true; } else { len += read; }
        }
        if len == 0 { break; }
        let n = match std::str::from_utf8(&buf[..len]) {
            Ok(_) => len,
            Err(e) if e.error_len().is_none() && !eof => e.valid_up_to(),
            Err(_) => return Err(io::Error::other("실행 기록이 UTF-8이 아닙니다")),
        };
        if n == 0 { return Err(io::Error::other("실행 기록의 UTF-8이 잘렸습니다")); }
        let text = std::str::from_utf8(&buf[..n]).map_err(io::Error::other)?;
        hash.update(&buf[..n]);
        let page = serde_json::to_string(text).map_err(io::Error::other)?;
        if output.stream_position()?.saturating_add(page.len() as u64 + 1) > MAX_OUTPUT { return Err(io::Error::other("결과 페이지의 저장 상한을 초과했습니다")); }
        output.write_all(page.as_bytes())?;
        output.write_all(b"\n")?;
        *first += 1;
        buf.copy_within(n..len, 0);
        len -= n;
    }
    meta["format"] = json!("json-string-lines");
    meta["pageStart"] = json!(start);
    meta["pageCount"] = json!(*first - start);
    meta["sha256"] = json!(format!("{:x}", hash.finalize()));
    Ok(())
}

fn scan_run(home: &Path, run: &str, pages: &mut std::fs::File, page: &mut u64, rows: &mut Vec<Value>, scanned: &mut u64) -> io::Result<()> {
    let Some((file, _parents)) = status_file::open_events(home, run)? else {
        rows.push(json!({"runId":run,"type":"unavailable","error":"실행 기록이 없음"}));
        return Ok(());
    };
    let size = file.metadata()?.len();
    *scanned = scanned.checked_add(size).ok_or_else(|| io::Error::other("실행 기록 크기 초과"))?;
    if *scanned > MAX_INPUT { return Err(io::Error::other("실행 기록의 스트리밍 안전 상한 128 MiB 초과")); }
    let mut source = BufReader::new(file);
    let mut line_no = 0u64;
    let mut unknown = 0u64;
    loop {
        let start = source.stream_position()?;
        let mut prefix = Vec::new();
        let mut len = 0u64;
        let mut depth = 0i32;
        let mut string = false;
        let mut escape = false;
        loop {
            let buf = source.fill_buf()?;
            if buf.is_empty() {
                if len != 0 { return Err(io::Error::other("실행 기록의 마지막 줄이 잘렸습니다")); }
                break;
            }
            let n = buf.iter().position(|b| *b == b'\n').map_or(buf.len(), |i| i + 1);
            prefix.extend_from_slice(&buf[..n.min(1024usize.saturating_sub(prefix.len()))]);
            for &b in &buf[..n] {
                if string { if escape { escape = false; } else if b == b'\\' { escape = true; } else if b == b'"' { string = false; } }
                else if b == b'"' { string = true; }
                else if b == b'{' || b == b'[' { depth += 1; if depth > 128 { return Err(io::Error::other("실행 기록 중첩 한도 초과")); } }
                else if b == b'}' || b == b']' { depth -= 1; }
            }
            len = len.checked_add(n as u64).ok_or_else(|| io::Error::other("실행 기록 크기 초과"))?;
            if start.saturating_add(len) > size { return Err(io::Error::other("실행 기록의 크기가 바뀌었습니다")); }
            let complete = buf[n - 1] == b'\n';
            source.consume(n);
            if complete { break; }
        }
        if len == 0 { break; }
        line_no += 1;
        let mut meta = None;
        if len <= SMALL_LINE {
            source.seek(SeekFrom::Start(start))?;
            let value: Option<Value> = serde_json::from_reader(source.by_ref().take(len)).ok();
            let typed = value.clone().and_then(|v| serde_json::from_value::<crate::run::events::OEvent>(v).ok());
            let kind = value.as_ref().and_then(|v| v.get("t")).and_then(Value::as_str).unwrap_or("unknown");
            let private = matches!(kind, "msg" | "msg.state" | "card.tool" | "card.message" | "card.input");
            let keep = !private && (typed.is_none() || matches!(kind, "run.start" | "run.blocked" | "run.end" | "card.queued" | "card.start" | "card.end"));
            if typed.is_none() { unknown += 1; }
            if keep {
                let card = value.as_ref().and_then(|v| v.get("cardId")).and_then(Value::as_str).filter(|s| crate::run::store::is_run_id(s));
                meta = Some(json!({"runId":run,"cardId":card,"type":if kind.len() <= 80 {kind} else {"unknown"},"line":line_no,"unknown":typed.is_none()}));
            }
        } else if let Some(captures) = LARGE_CARD.captures(std::str::from_utf8(&prefix[..std::str::from_utf8(&prefix).err().map_or(prefix.len(), |e| e.valid_up_to())]).unwrap_or("")) {
            source.seek(SeekFrom::Start(start + len - len.min(256)))?;
            let mut tail = Vec::new();
            source.by_ref().take(len.min(256)).read_to_end(&mut tail)?;
            let marker = b"\",\"exitCode\":";
            let suffix = tail.windows(marker.len()).rposition(|bytes| bytes == marker).ok_or_else(|| io::Error::other("대용량 카드 결과의 형식을 확인할 수 없습니다"))?;
            if !std::str::from_utf8(&tail[suffix..]).ok().is_some_and(|s| LARGE_END.find(s).is_some_and(|m| m.start() == 0)) { return Err(io::Error::other("대용량 카드 결과의 형식을 확인할 수 없습니다")); }
            let body_start = captures.get(0).unwrap().end() as u64;
            let body_end = len - len.min(256) + suffix as u64;
            if body_end < body_start { return Err(io::Error::other("대용량 카드 결과의 형식을 확인할 수 없습니다")); }
            source.seek(SeekFrom::Start(start + body_start))?;
            let mut body = source.by_ref().take(body_end - body_start);
            let mut chunk = [0u8; 8192];
            let mut escaped = false;
            loop {
                let count = body.read(&mut chunk)?;
                if count == 0 { break; }
                for &byte in &chunk[..count] {
                    if escaped { escaped = false; } else if byte == b'\\' { escaped = true; } else if byte == b'"' { return Err(io::Error::other("대용량 카드 결과에 예상하지 못한 필드가 있습니다")); }
                }
            }
            if escaped { return Err(io::Error::other("대용량 카드 결과가 잘렸습니다")); }
            source.seek(SeekFrom::Start(start))?;
            let mut parser = serde_json::Deserializer::from_reader(source.by_ref().take(len));
            serde::de::IgnoredAny::deserialize(&mut parser).map_err(io::Error::other)?;
            parser.end().map_err(io::Error::other)?;
            unknown += 1;
            meta = Some(json!({"runId":run,"cardId":&captures[1],"type":"card.end","line":line_no,"unknown":true}));
        } else {
            unknown += 1;
        }
        if let Some(mut item) = meta {
            if rows.len() >= MAX_STATUS_RECORDS { return Err(io::Error::other("실행 결과 색인 항목 상한 초과")); }
            source.seek(SeekFrom::Start(start))?;
            emit_pages(source.by_ref().take(len), pages, page, &mut item)?;
            rows.push(item);
        }
        source.seek(SeekFrom::Start(start + len))?;
    }
    if source.get_ref().metadata()?.len() != size || source.stream_position()? != size { return Err(io::Error::other("실행 기록의 크기가 바뀌었습니다")); }
    rows.push(json!({"runId":run,"type":"run-summary","unrecognized":unknown}));
    if rows.len() > MAX_STATUS_RECORDS { return Err(io::Error::other("실행 결과 색인 항목 상한 초과")); }
    Ok(())
}

fn verify_pages(reader: impl Read, rows: &[Value]) -> io::Result<()> {
    let mut reader = BufReader::new(reader);
    for row in rows {
        if let Some(count) = row["pageCount"].as_u64() {
            let mut hash = Sha256::new();
            for _ in 0..count {
                let mut line = Vec::new();
                reader.by_ref().take(1540).read_until(b'\n', &mut line)?;
                if line.is_empty() || line.len() > 1539 || line.last() != Some(&b'\n') { return Err(io::Error::other("결과 페이지가 잘렸습니다")); }
                let decoded: String = serde_json::from_slice(&line).map_err(io::Error::other)?;
                hash.update(decoded.as_bytes());
            }
            if row["sha256"].as_str() != Some(&format!("{:x}", hash.finalize())) { return Err(io::Error::other("결과 페이지 해시가 일치하지 않습니다")); }
        }
    }
    if !reader.fill_buf()?.is_empty() { return Err(io::Error::other("예상하지 못한 페이지가 있습니다")); }
    Ok(())
}

pub(super) fn write(home: &Path, thread: &str, records: Vec<StatusRecord>) -> io::Result<status_file::StatusFile> {
    let mut guard = status_file::create(home, thread)?;
    let (path, mut pages) = guard.temporary()?;
    let mut rows = Vec::new();
    let mut page = 1u64;
    let mut scanned = 0u64;
    for mut record in records {
        if record.meta["type"] == "stream-run" {
            let id = record.meta["runId"].as_str().ok_or_else(|| io::Error::other("실행 ID 누락"))?;
            scan_run(home, id, &mut pages, &mut page, &mut rows, &mut scanned)?;
        } else {
            if let Some(raw) = record.raw { emit_pages(Cursor::new(raw), &mut pages, &mut page, &mut record.meta)?; }
            rows.push(record.meta);
        }
        if rows.len() > MAX_STATUS_RECORDS { return Err(io::Error::other("실행 결과 색인 항목 상한 초과")); }
    }
    pages.flush()?;
    pages.seek(SeekFrom::Start(0))?;
    verify_pages(&mut pages, &rows)?;
    let (index_path, mut index) = guard.temporary()?;
    for mut row in rows {
        if row.get("pageStart").is_some() { row["file"] = json!(path.to_string_lossy()); }
        serde_json::to_writer(&mut index, &row).map_err(io::Error::other)?;
        index.write_all(b"\n")?;
    }
    index.flush()?;
    guard.path = index_path;
    guard.retain(pages);
    guard.retain(index);
    Ok(guard)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_changed_missing_and_oversized_pages() {
        let rows = [json!({"pageCount":1,"sha256":format!("{:x}", Sha256::digest(b"hello"))})];
        assert!(verify_pages(Cursor::new(b"\"hello\"\n"), &rows).is_ok());
        assert!(verify_pages(Cursor::new(b"\"other\"\n"), &rows).is_err());
        assert!(verify_pages(Cursor::new(b"\"hello\""), &rows).is_err());
        assert!(verify_pages(Cursor::new(vec![b'x'; 5000]), &rows).is_err());
    }

    #[test]
    fn input_budget_is_checked_before_copying() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("o-stream-budget-{}-{stamp}", std::process::id()));
        let home = base.join("state");
        let run = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&run).unwrap();
        std::fs::write(run.join("events.jsonl"), b"{}\n").unwrap();
        let mut output = std::fs::File::create(base.join("pages.txt")).unwrap();
        let mut rows = Vec::new();
        let mut first = 1;
        let mut scanned = MAX_INPUT;
        assert!(scan_run(&home, "run", &mut output, &mut first, &mut rows, &mut scanned).is_err());
        assert_eq!(output.metadata().unwrap().len(), 0);
    }

    #[test]
    fn large_card_event_with_extra_fields_is_not_exported() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("o-stream-fields-{}-{stamp}", std::process::id()));
        let home = base.join("state");
        let run = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&run).unwrap();
        let event = format!("{{\"t\":\"card.end\",\"at\":\"now\",\"cardId\":\"card\",\"status\":\"done\",\"result\":\"{}\",\"extra\":\"PRIVATE\",\"exitCode\":0}}\n", "a".repeat(70 * 1024));
        std::fs::write(run.join("events.jsonl"), event).unwrap();
        let mut output = std::fs::File::create(base.join("pages.txt")).unwrap();
        let mut rows = Vec::new();
        let mut first = 1;
        let mut scanned = 0;
        assert!(scan_run(&home, "run", &mut output, &mut first, &mut rows, &mut scanned).is_err());
        assert_eq!(output.metadata().unwrap().len(), 0);
    }
}
