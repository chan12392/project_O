//! Reads only official rollout metadata/token records under the executing CODEX_HOME.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use serde_json::Value;

#[derive(Default)]
struct Session {
    id: String,
    parent: Option<String>,
    file_created: Option<std::time::SystemTime>,
    offset: u64,
    previous: Option<[f64; 3]>,
    requests: Vec<(String, [f64; 3])>,
    models: Vec<(String, String)>,
    partial: bool,
}

static CACHE: LazyLock<Mutex<BTreeMap<PathBuf, Session>>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn regular(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    { use std::os::windows::fs::MetadataExt; meta.file_attributes() & 0x400 == 0 }
    #[cfg(not(windows))]
    { !meta.file_type().is_symlink() }
}

fn files(dir: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    if !regular(&fs::symlink_metadata(dir)?) { return Err(io::Error::other("세션 폴더의 링크는 읽지 않습니다")); }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let meta = fs::symlink_metadata(&path)?;
        if !regular(&meta) { continue; }
        if meta.is_dir() { files(&path, out)?; }
        else if meta.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") { out.push(path); }
    }
    Ok(())
}

fn metadata(path: &Path) -> io::Result<Option<Session>> {
    let mut line = String::new();
    BufReader::new(File::open(path)?).read_line(&mut line)?;
    let Ok(v) = serde_json::from_str::<Value>(&line) else { return Ok(None) };
    if v["type"] != "session_meta" { return Ok(None); }
    let payload = &v["payload"];
    let Some(id) = payload["id"].as_str().filter(|id| crate::run::store::is_run_id(id)) else { return Ok(None) };
    let parent = payload.pointer("/source/subagent/thread_spawn/parent_thread_id").and_then(Value::as_str)
        .or_else(|| payload["parent_thread_id"].as_str()).map(str::to_owned);
    Ok(Some(Session { id: id.into(), parent, file_created: fs::metadata(path)?.created().ok(), ..Session::default() }))
}

fn tokens(value: &Value) -> Option<[f64; 3]> {
    let input = value["input_tokens"].as_f64()?;
    let cached = value["cached_input_tokens"].as_f64()?;
    let output = value["output_tokens"].as_f64()?;
    ([input, cached, output].iter().all(|n| n.is_finite() && *n >= 0.0) && cached <= input).then_some([input - cached, cached, output])
}

fn read(path: &Path, session: &mut Session) -> io::Result<()> {
    let mut file = File::open(path)?;
    if file.metadata()?.len() < session.offset || file.metadata()?.created().ok() != session.file_created {
        let fresh = metadata(path)?.ok_or_else(|| io::Error::other("세션 메타데이터가 바뀌었습니다"))?;
        *session = fresh;
    }
    file.seek(SeekFrom::Start(session.offset))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 || !line.ends_with('\n') { break; }
        session.offset += n as u64;
        let Ok(v) = serde_json::from_str::<Value>(&line) else { session.partial = true; continue };
        if v["type"] == "turn_context" {
            if let (Some(at), Some(model)) = (v["timestamp"].as_str(), v["payload"]["model"].as_str()) { session.models.push((at.into(), model.into())); }
        }
        if v["type"] != "event_msg" || v["payload"]["type"] != "token_count" { continue; }
        let info = &v["payload"]["info"];
        let Some(total) = tokens(&info["total_token_usage"]) else { continue };
        if session.previous == Some(total) { continue; }
        let used = match session.previous {
            Some(previous) if (0..3).all(|i| total[i] >= previous[i]) => Some([total[0] - previous[0], total[1] - previous[1], total[2] - previous[2]]),
            None => tokens(&info["last_token_usage"]),
            Some(_) => { session.partial = true; None }
        };
        session.previous = Some(total);
        match (v["timestamp"].as_str(), used) {
            (Some(at), Some(used)) => session.requests.push((at.into(), used)),
            _ => session.partial = true,
        }
    }
    Ok(())
}

pub struct Tree {
    pub tokens: [f64; 3],
    pub children: [f64; 3],
    pub root_reported: bool,
    pub models: BTreeSet<String>,
    pub reason: Option<String>,
}

pub fn collect(profile: &Path, root: &str, started: &str, ended: &str) -> io::Result<Tree> {
    let (Some(started), Some(ended)) = (crate::sys::js::parse_time(started), crate::sys::js::parse_time(ended)) else { return Err(io::Error::other("사용량 구간의 시각을 확인하지 못했습니다")); };
    let within = |at: &str| crate::sys::js::parse_time(at).is_some_and(|at| at >= started && at <= ended);
    let mut paths = Vec::new();
    files(&profile.join("sessions"), &mut paths)?;
    let mut cache = CACHE.lock().map_err(|_| io::Error::other("세션 색인 잠금 실패"))?;
    for path in &paths {
        if cache.get(path).is_none_or(|session| fs::metadata(path).ok().and_then(|m| m.created().ok()) != session.file_created) {
            if let Some(session) = metadata(path)? { cache.insert(path.clone(), session); }
        }
    }
    let mut selected = BTreeSet::from([root.to_string()]);
    loop {
        let before = selected.len();
        for path in &paths {
            if let Some(session) = cache.get(path) {
                if session.parent.as_ref().is_some_and(|id| selected.contains(id)) { selected.insert(session.id.clone()); }
            }
        }
        if selected.len() == before { break; }
    }
    let mut tree = Tree { tokens: [0.0; 3], children: [0.0; 3], root_reported: false, models: BTreeSet::new(), reason: None };
    let mut seen = BTreeSet::new();
    for path in &paths {
        let Some(session) = cache.get_mut(path).filter(|s| selected.contains(&s.id)) else { continue };
        if !seen.insert(session.id.clone()) { tree.reason = Some("같은 세션 ID의 기록이 여러 개라 한 번만 집계했습니다".into()); continue; }
        read(path, session)?;
        if !selected.contains(&session.id) { tree.reason = Some("집계 중 세션 파일이 바뀌었습니다".into()); continue; }
        tree.models.extend(session.models.iter().filter(|(at, _)| within(at)).map(|(_, model)| model.clone()));
        if session.partial { tree.reason = Some("누적값 감소 또는 해석할 수 없는 세션 기록이 있습니다".into()); }
        // Late file arrival is included by its original event time; a later resumed child is another turn.
        for (at, used) in &session.requests {
            if within(at) {
                super::accounting::add(&mut tree.tokens, *used);
                if session.id == root { tree.root_reported = true; }
                else { super::accounting::add(&mut tree.children, *used); }
            }
        }
    }
    if !seen.contains(root) { tree.reason = Some("부모의 공식 세션 기록이 없습니다".into()); }
    Ok(tree)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn descendants_duplicates_inheritance_and_other_roots() {
        let dir = std::env::temp_dir().join(format!("o-tree-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        fs::create_dir_all(dir.join("sessions/old" )).unwrap();
        let write = |id: &str, parent: Option<&str>, input: u64, inherited: u64| {
            let meta = json!({"type":"session_meta","timestamp":"2026-11-12T01:00:01Z","payload":{"id":id,"source":{"subagent":{"thread_spawn":{"parent_thread_id":parent}}}}});
            let report = json!({"type":"event_msg","timestamp":"2026-11-12T01:00:02Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":input+inherited,"cached_input_tokens":0,"output_tokens":0},"last_token_usage":{"input_tokens":input,"cached_input_tokens":0,"output_tokens":0}}}});
            fs::write(dir.join(format!("sessions/old/{id}.jsonl")), format!("{meta}\n{report}\n{report}\n")).unwrap();
        };
        write("parent", None, 100, 0);
        write("child", Some("parent"), 60, 100);
        write("grandchild", Some("child"), 30, 160);
        write("unrelated", None, 999, 0);
        let tree = collect(&dir, "parent", "2026-11-12T01:00:00Z", "2026-11-12T01:00:03Z").unwrap();
        assert_eq!(tree.tokens, [190.0, 0.0, 0.0]);
        assert_eq!(collect(&dir, "parent", "2026-11-12T01:00:00Z", "2026-11-12T01:00:03Z").unwrap().tokens, tree.tokens);
        assert_eq!(collect(&dir, "parent", "2026-11-12T02:00:00Z", "2026-11-12T02:00:03Z").unwrap().tokens, [0.0; 3]);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn smoke_totals_and_late_child_revise_the_same_attempt_once() {
        use crate::{run::plan::ProviderId, usage::accounting::Collector};
        let dir = std::env::temp_dir().join(format!("o-tree-smoke-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        fs::create_dir_all(dir.join("sessions/older-date")).unwrap();
        let write = |id: &str, parent: Option<&str>, usage: [u64; 3], inherited: [u64; 3]| {
            let meta = json!({"type":"session_meta","payload":{"id":id,"source":{"subagent":{"thread_spawn":{"parent_thread_id":parent}}}}});
            let report = json!({"type":"event_msg","timestamp":"2026-11-12T01:00:02Z","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":usage[0]+inherited[0],"cached_input_tokens":usage[1]+inherited[1],"output_tokens":usage[2]+inherited[2]},"last_token_usage":{"input_tokens":usage[0],"cached_input_tokens":usage[1],"output_tokens":usage[2],"reasoning_output_tokens":40}}}});
            fs::write(dir.join(format!("sessions/older-date/{id}.jsonl")), format!("{meta}\n{report}\n{report}\n")).unwrap();
        };
        write("explicit", None, [63_666, 54_528, 114], [0; 3]);
        let mut collector = Collector::new(ProviderId::Codex, None, false, "2026-11-12T01:00:00Z".into());
        collector.observe(r#"{"type":"thread.started","thread_id":"explicit"}"#);
        collector.observe(r#"{"type":"turn.completed","usage":{"input_tokens":63666,"cached_input_tokens":54528,"output_tokens":114}}"#);
        let first = collector.calculate(&[], &dir, "attempt", "raw", "2026-11-12T01:00:03Z", true, vec![]).unwrap().unwrap();
        assert_eq!(first.input + first.cached + first.output, 63_780.0);
        write("child", Some("explicit"), [64_323, 52_992, 46], [63_666, 54_528, 114]);
        let next = collector.calculate(std::slice::from_ref(&first), &dir, "attempt", "raw", "2026-11-12T01:00:03Z", true, vec![]).unwrap().unwrap();
        assert_eq!((next.revision, next.input + next.cached + next.output), (2, 128_149.0));
        assert_eq!(next.input + next.cached + next.output + 42_553.0 + 22_289.0, 192_991.0);
        assert!(!next.complete);
        assert_eq!(collector.calculate(&[first, next.clone()], &dir, "attempt", "raw", "2026-11-12T01:00:03Z", true, vec![]).unwrap().unwrap(), next);
    }
}
