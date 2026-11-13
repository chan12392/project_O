//! Pick up plan files without blocking the terminal's PTY forwarding.
use super::*;
use sha2::{Digest, Sha256};

const MAX_PLAN: u64 = 1024 * 1024;

fn plain_dir(path: &Path) -> io::Result<()> {
    match fs::create_dir(path) {
        Ok(()) => {},
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {},
        Err(e) => return Err(e),
    }
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir() || meta.file_type().is_symlink() { return Err(io::Error::other("계획 폴더가 링크이거나 폴더가 아닙니다")); }
    Ok(())
}

/// Each session remembers handled names; a busy conversation is retried later.
pub(crate) fn pickup(home: &Path, project: &str, thread_id: &str, session_dir: &Path, stability: &mut std::collections::HashMap<PathBuf, crate::term::Stability>, cancelled: &std::sync::atomic::AtomicBool) -> io::Result<()> {
    let root = files::files_root(home);
    let thread = root.join(thread_id);
    let plans = thread.join("plans");
    for path in [&root, &thread, &plans] { plain_dir(path)?; }
    let handled = session_dir.join("handled-plans");
    plain_dir(&handled)?;
    let mut entries = Vec::new();
    for entry in fs::read_dir(&plans)?.flatten() {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) { return Ok(()); }
        entries.push(entry);
    }
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if cancelled.load(std::sync::atomic::Ordering::Acquire) { return Ok(()); }
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") { continue; }
        let file = match crate::run::workspaces::open_plain(&path) { Ok(file) => file, Err(_) => continue };
        let before = file.metadata()?;
        if before.len() == 0 || !stability.entry(path.clone()).or_default().ready(&before, std::time::Duration::from_millis(1500)) { continue; }
        let mut digest = Sha256::new();
        digest.update(entry.file_name().as_encoded_bytes());
        digest.update([0]);
        let mut bytes = Vec::new();
        if before.len() > MAX_PLAN {
            digest.update(b"oversized\0");
            digest.update(before.len().to_le_bytes());
            digest.update(format!("{:?}", before.modified()?).as_bytes());
        } else {
            (&file).take(MAX_PLAN + 1).read_to_end(&mut bytes)?;
            digest.update(&bytes);
        }
        let after = file.metadata()?;
        if before.len() != after.len() || before.modified()? != after.modified()? { stability.remove(&path); continue; }
        let marker = handled.join(format!("{:x}", digest.finalize()));
        if marker.exists() { continue; }
        let parsed = if before.len() > MAX_PLAN || bytes.len() as u64 > MAX_PLAN { Err("계획 파일이 너무 큽니다".to_string()) }
            else { serde_json::from_slice::<Value>(&bytes).map_err(|_| "계획 파일이 올바른 JSON이 아닙니다".to_string()).and_then(|plan| {
                if plan.get("title").and_then(Value::as_str).is_some() && plan.get("cards").and_then(Value::as_array).is_some() { Ok(plan) }
                else { Err("계획 파일의 형식이 올바르지 않습니다".to_string()) }
            }) };
        let current = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock"))?;
        if current.try_lock().is_err() { return Ok(()); }
        let turn = turn_lock(home, thread_id)?;
        if turn.try_lock().is_err() || running_path(home, thread_id).exists() { return Ok(()); }
        if !is_open(home, project, thread_id) { return Ok(()); }
        let source = format!("plan-file/{}", marker.file_name().unwrap_or_default().to_string_lossy());
        if event_values(home, thread_id).iter().any(|event| event.get("terminalSource").and_then(Value::as_str) == Some(source.as_str())) {
            let _ = OpenOptions::new().write(true).create_new(true).open(&marker);
            continue;
        }
        let events = read_thread(home, thread_id);
        let event = match parsed {
            Ok(plan) => {
                let count = events.iter().filter(|event| matches!(event, LeadEvent::Plan { .. })).count();
                let (plan, violations, mut notices) = check_lead_plan(home, project, plan, RunMode::default());
                // ADR-0043: any process of this user can leave a file here. The person who presses
                // "실행" is told where the plan came from and that its author is not known.
                let name: String = entry.file_name().to_string_lossy().chars().filter(|c| !c.is_control()).take(80).collect();
                notices.insert(0, format!("계획 파일 {name}에서 읽은 계획입니다. 총괄 터미널이 쓴 것인지 O는 확인하지 못합니다. 카드와 작업 내용을 확인한 뒤 실행하세요."));
                LeadEvent::Plan { at: now_iso(), plan_id: format!("{thread_id}-p{}", count + 1), plan, ok: violations.is_empty(), violations, notices }
            }
            Err(text) => LeadEvent::Error { at: now_iso(), text },
        };
        let mut value = serde_json::to_value(event).map_err(io::Error::other)?;
        value["terminalSource"] = Value::String(source);
        let mut file = OpenOptions::new().create(true).append(true).open(thread_dir(home, thread_id).join("events.jsonl"))?;
        store::append_value(&mut file, &value)?;
        match OpenOptions::new().write(true).create_new(true).open(&marker) {
            Ok(_) => {},
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_each_plain_plan_once_and_ignores_links() {
        let base = std::env::temp_dir().join(format!("o-terminal-plans-{}", new_run_id(Utc::now())));
        let home = base.join("state");
        let project = base.join("project");
        let session = base.join("session");
        for path in [&home, &project, &session] { fs::create_dir_all(path).unwrap(); }
        let project = project.to_string_lossy().into_owned();
        let id = new_thread(&home, &project).unwrap();
        let mut stability = std::collections::HashMap::new();
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        let plans = files::thread_files(&home, &id).join("plans");
        fs::write(plans.join("good.json"), br#"{"title":"test","project":"wrong","cards":["#).unwrap();
        fs::write(plans.join("bad.json"), b"{").unwrap();
        let outside = base.join("outside.json");
        fs::write(&outside, br#"{"title":"linked","cards":[]}"#).unwrap();
        fs::hard_link(&outside, plans.join("linked.json")).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(500));
        OpenOptions::new().append(true).open(plans.join("good.json")).unwrap().write_all(br#"{"id":"a","provider":"claude","task":"work"}]}"#).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        assert!(!read_thread(&home, &id).iter().any(|event| matches!(event, LeadEvent::Plan { .. } | LeadEvent::Error { .. })));
        std::thread::sleep(std::time::Duration::from_millis(1600));
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        for entry in fs::read_dir(session.join("handled-plans")).unwrap() { fs::remove_file(entry.unwrap().path()).unwrap(); }
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        let reopened = base.join("reopened");
        fs::create_dir(&reopened).unwrap();
        let mut restarted = std::collections::HashMap::new();
        pickup(&home, &project, &id, &reopened, &mut restarted, &cancelled).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1600));
        pickup(&home, &project, &id, &reopened, &mut restarted, &cancelled).unwrap();
        let events = read_thread(&home, &id);
        let found: Vec<_> = events.iter().filter_map(|event| match event { LeadEvent::Plan { plan, ok, notices, .. } => Some((plan, ok, notices)), _ => None }).collect();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0["project"], project);
        assert!(*found[0].1);
        assert!(found[0].2[0].starts_with("계획 파일 good.json에서 읽은 계획입니다"), "{:?}", found[0].2);
        assert_eq!(events.iter().filter(|event| matches!(event, LeadEvent::Error { .. })).count(), 1);
        fs::write(plans.join("bad.json"), br#"{"title":"fixed","cards":[]}"#).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1600));
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        assert_eq!(read_thread(&home, &id).iter().filter(|event| matches!(event, LeadEvent::Plan { .. })).count(), 2);
        let mut oversized = vec![b'x'; MAX_PLAN as usize + 2];
        fs::write(plans.join("large.json"), &oversized).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1600));
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        assert_eq!(read_thread(&home, &id).iter().filter(|event| matches!(event, LeadEvent::Error { .. })).count(), 2);
        *oversized.last_mut().unwrap() = b'y';
        fs::write(plans.join("large.json"), &oversized).unwrap();
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1600));
        pickup(&home, &project, &id, &session, &mut stability, &cancelled).unwrap();
        assert_eq!(read_thread(&home, &id).iter().filter(|event| matches!(event, LeadEvent::Error { .. })).count(), 3);
    }
}
