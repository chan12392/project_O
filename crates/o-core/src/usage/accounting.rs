//! Durable, replaceable usage reports. The journal is evidence; o.db is only an index.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::run::plan::ProviderId;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settlement {
    pub usage_id: String,
    pub revision: u64,
    pub provider: ProviderId,
    pub profile: String,
    pub session_id: Option<String>,
    pub occurred_at: String,
    pub ended_at: String,
    pub source_start: u64,
    pub source_end: u64,
    pub resumed: bool,
    pub app_server: bool,
    pub input: f64,
    pub cached: f64,
    pub output: f64,
    pub reported: bool,
    pub complete: bool,
    pub scope: String,
    pub reason: Option<String>,
    pub models: Vec<String>,
    pub cumulative: Option<[f64; 3]>,
    pub replaces: Vec<String>,
    pub source: String,
    pub version: u32,
}

pub fn profile(provider: ProviderId, clean: Option<&Path>) -> PathBuf {
    if let Some(root) = clean {
        let home = crate::agents::clean::profile_home(root, provider);
        return PathBuf::from(crate::providers::get(provider).clean_env(&home).1);
    }
    let (env, default) = match provider {
        ProviderId::Codex => ("CODEX_HOME", ".codex"),
        ProviderId::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
        _ => return crate::run::store::user_home(),
    };
    std::env::var_os(env).filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| crate::run::store::user_home().join(default))
}

fn counts(value: &Value, keys: [&str; 3]) -> Option<([f64; 3], bool)> {
    let mut result = [0.0; 3];
    let (mut reported, mut complete) = (false, true);
    for (i, key) in keys.iter().enumerate() {
        if let Some(n) = value.get(*key).and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0) { result[i] = n; reported = true; }
        else { complete = false; }
    }
    reported.then_some((result, complete))
}

pub(crate) fn add(to: &mut [f64; 3], value: [f64; 3]) {
    for i in 0..3 { to[i] += value[i]; }
}

/// Collects only token/model metadata. Conversation bodies remain in the original CLI log.
pub struct Collector {
    pub provider: ProviderId,
    pub session: Option<String>,
    pub resumed: bool,
    pub app_server: bool,
    pub started: String,
    pub source_start: u64,
    pub source_end: u64,
    total: Option<[f64; 3]>,
    cumulative: bool,
    complete: bool,
    tree: bool,
    models: BTreeSet<String>,
    background: BTreeSet<String>,
    messages: BTreeMap<String, [f64; 3]>,
}

impl Collector {
    pub fn new(provider: ProviderId, session: Option<String>, app_server: bool, started: String) -> Self {
        Self { provider, resumed: session.is_some(), session, app_server, started, source_start: 0, source_end: 0, total: None, cumulative: false, complete: false, tree: false, models: BTreeSet::new(), background: BTreeSet::new(), messages: BTreeMap::new() }
    }

    pub fn observe(&mut self, line: &str) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return };
        match self.provider {
            ProviderId::Claude => {
                if let Some(id) = v["session_id"].as_str() { self.session = Some(id.into()); }
                for model in [v["model"].as_str(), v["message"]["model"].as_str()].into_iter().flatten() { self.models.insert(model.into()); }
                if v["type"] == "assistant" {
                    if let (Some(id), Some((mut usage, _))) = (v["message"]["id"].as_str(), counts(&v["message"]["usage"], ["input_tokens", "cache_read_input_tokens", "output_tokens"])) {
                        usage[0] += v["message"]["usage"]["cache_creation_input_tokens"].as_f64().filter(|n| n.is_finite() && *n >= 0.0).unwrap_or(0.0);
                        self.messages.insert(id.into(), usage);
                        let mut total = [0.0; 3];
                        for usage in self.messages.values() { add(&mut total, *usage); }
                        self.total = Some(total);
                    }
                }
                if let Some(id) = v["task_id"].as_str() {
                    match v["subtype"].as_str() {
                        Some("task_started") => { self.background.insert(id.into()); }
                        Some("task_notification") => { self.background.remove(id); }
                        _ => {}
                    }
                }
                if v["type"] != "result" { return; }
                if let Some(models) = v["modelUsage"].as_object().filter(|models| !models.is_empty()) {
                    let mut total = [0.0; 3];
                    let mut reported = false;
                    self.complete = true;
                    for (model, usage) in models {
                        self.models.insert(model.clone());
                        match (counts(usage, ["inputTokens", "cacheReadInputTokens", "outputTokens"]), usage["cacheCreationInputTokens"].as_f64().filter(|n| n.is_finite() && *n >= 0.0)) {
                            (Some((mut tokens, complete)), write) => { tokens[0] += write.unwrap_or(0.0); add(&mut total, tokens); self.complete &= complete && write.is_some(); reported = true; }
                            _ => self.complete = false,
                        }
                    }
                    self.total = reported.then_some(total);
                    self.cumulative = true;
                    self.tree = true;
                    self.complete &= self.background.is_empty();
                } else if let Some((mut total, _)) = counts(&v["usage"], ["input_tokens", "cache_read_input_tokens", "output_tokens"]) {
                    total[0] += v["usage"]["cache_creation_input_tokens"].as_f64().filter(|n| n.is_finite() && *n >= 0.0).unwrap_or(0.0);
                    self.total = Some(total);
                    self.complete = false;
                }
            }
            ProviderId::Codex => {
                if v["type"] == "thread.started" { self.session = v["thread_id"].as_str().map(str::to_owned); }
                for model in [v["model"].as_str(), v["thread"]["model"].as_str()].into_iter().flatten() { self.models.insert(model.into()); }
                if matches!(v["type"].as_str(), Some("turn.completed" | "turn.usage")) {
                    if let Some((mut total, _)) = counts(&v["usage"], ["input_tokens", "cached_input_tokens", "output_tokens"]) {
                        if total[1] > total[0] { return; }
                        total[0] -= total[1];
                        self.total = Some(total);
                        self.cumulative = !self.app_server;
                    }
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn settle(&self, home: &Path, profile: &Path, usage_id: &str, source: &str, ended: &str, finished: bool, replaces: Vec<String>) -> io::Result<Option<Settlement>> {
        fs::create_dir_all(home)?;
        let mut journal = OpenOptions::new().create(true).read(true).append(true).open(home.join("usage-journal.jsonl"))?;
        journal.lock()?;
        let mut text = String::new();
        journal.read_to_string(&mut text)?;
        let previous = parse_journal(&text)?;
        let Some(settled) = self.calculate(&previous, profile, usage_id, source, ended, finished, replaces)? else { return Ok(None) };
        if !previous.iter().any(|v| v == &settled) {
            serde_json::to_writer(&mut journal, &settled)?;
            journal.write_all(b"\n")?;
            journal.sync_data()?;
        }
        Ok(Some(settled))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn calculate(&self, previous: &[Settlement], profile: &Path, usage_id: &str, source: &str, ended: &str, finished: bool, replaces: Vec<String>) -> io::Result<Option<Settlement>> {
        let profile_text = crate::run::store::resolved_path(&profile.to_string_lossy());
        let mut total = self.total;
        let mut complete = self.complete && finished;
        let mut scope = if self.tree { "tree" } else { "parent" };
        let mut reason = (!complete).then(|| "하위 사용량 또는 종료 상태를 모두 확인하지 못했습니다".to_string());
        let mut models = self.models.clone();
        let cumulative = self.cumulative.then_some(self.total).flatten();
        if self.cumulative {
            let started = chrono::DateTime::parse_from_rfc3339(&self.started).ok();
            let baseline = previous.iter().filter(|v| v.provider == self.provider && v.profile == profile_text && v.session_id == self.session && v.usage_id != usage_id)
                .filter_map(|v| Some((chrono::DateTime::parse_from_rfc3339(&v.occurred_at).ok()?, v.revision, v.cumulative?)))
                .filter(|(at, _, _)| started.is_some_and(|start| *at < start)).max_by_key(|(at, revision, _)| (*at, *revision)).map(|(_, _, total)| total);
            match (total, baseline) {
                (Some(value), Some(base)) if (0..3).all(|i| value[i] >= base[i]) => total = Some([value[0] - base[0], value[1] - base[1], value[2] - base[2]]),
                (Some(_), Some(_)) | (Some(_), None) if self.resumed => {
                    total = None;
                    complete = false;
                    reason = Some("재개 세션의 누적 기준값이 없거나 감소해 사용량을 확정할 수 없습니다".into());
                }
                (Some(_), Some(_)) => { total = None; complete = false; reason = Some("세션 누적값이 감소했습니다".into()); }
                _ => {}
            }
        }
        if self.provider == ProviderId::Codex {
            if let Some(session) = &self.session {
                match super::codex_tree::collect(profile, session, &self.started, ended) {
                    Ok(found) => {
                        if found.root_reported { total = Some(found.tokens); scope = "tree"; }
                        else if let Some(tokens) = total.as_mut() { add(tokens, found.children); }
                        models.extend(found.models);
                        // The rollout format has no authoritative inventory of all descendants.
                        complete = false;
                        reason = Some(found.reason.unwrap_or_else(|| "확인된 세션 트리 집계입니다. CLI가 전체 하위 목록의 완전성을 보장하지 않습니다".into()));
                    }
                    Err(error) => { complete = false; reason = Some(format!("공식 세션 집계 실패: {error}")); }
                }
            }
        }
        if total.is_none() && self.session.is_none() { return Ok(None); }
        let [input, cached, output] = total.unwrap_or([0.0; 3]);
        if self.session.is_none() { complete = false; reason = Some("세션 ID가 없어 재개·중복 보고를 확인하지 못했습니다".into()); }
        let last = previous.iter().filter(|v| v.usage_id == usage_id).max_by_key(|v| v.revision);
        let mut settled = Settlement { usage_id: usage_id.into(), revision: last.map_or(1, |s| s.revision), provider: self.provider, profile: profile_text, session_id: self.session.clone(), occurred_at: self.started.clone(), ended_at: ended.into(), source_start: self.source_start, source_end: self.source_end, resumed: self.resumed, app_server: self.app_server, input, cached, output, reported: total.is_some(), complete, scope: scope.into(), reason, models: models.into_iter().collect(), cumulative, replaces, source: source.into(), version: 1 };
        if last == Some(&settled) { return Ok(Some(settled)); }
        if last.is_some() { settled.revision += 1; }
        Ok(Some(settled))
    }
}

pub fn read_journal(home: &Path) -> io::Result<Vec<Settlement>> {
    let mut file = match fs::File::open(home.join("usage-journal.jsonl")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    file.lock_shared()?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    parse_journal(&text)
}

fn parse_journal(text: &str) -> io::Result<Vec<Settlement>> {
    text.lines().filter(|l| !l.is_empty()).map(|line| serde_json::from_str(line).map_err(io::Error::other)).collect()
}

/// References survive consumers dropping unknown events or serializing typed events again.
pub fn legacy_references(events: &[Value]) -> Vec<Option<String>> {
    let mut seen = BTreeMap::new();
    events.iter().map(|event| {
        if !matches!(event["t"].as_str(), Some("card.usage" | "lead.usage")) { return None; }
        let key = json!([event["t"], event["at"], event["cardId"], event["provider"]]).to_string();
        let ordinal = seen.entry(key.clone()).or_insert(0);
        let reference = format!("legacy:{key}:{ordinal}");
        *ordinal += 1;
        Some(reference)
    }).collect()
}

/// Latest revisions replace, never add to, their explicitly referenced legacy events.
pub fn project(events: &[Value]) -> Vec<Value> {
    let references: BTreeMap<_, _> = legacy_references(events).into_iter().enumerate().filter_map(|(index, key)| key.map(|key| (key, index))).collect();
    let mut latest: BTreeMap<&str, (usize, u64)> = BTreeMap::new();
    let mut conflicts = BTreeSet::new();
    for (index, e) in events.iter().enumerate().filter(|(_, e)| matches!(e["t"].as_str(), Some("card.usage.settled" | "lead.usage.settled"))) {
        let (Some(id), Some(revision)) = (e["usageId"].as_str(), e["revision"].as_u64()) else { continue };
        if revision == 0 || e["version"] != 1 || ["input", "cached", "output"].iter().any(|k| !e[*k].as_f64().is_some_and(|n| n.is_finite() && n >= 0.0)) { continue; }
        match latest.get(id) {
            Some((old, r)) if *r == revision => {
                let mut prior = events[*old].clone();
                prior["at"] = e["at"].clone();
                if prior != *e { conflicts.insert(id); }
            }
            Some((_, r)) if *r >= revision => {}
            _ => { latest.insert(id, (index, revision)); conflicts.remove(id); }
        }
    }
    let (mut replaced, mut partial) = (BTreeSet::new(), BTreeSet::new());
    for (index, _) in latest.values() {
        let e = &events[*index];
        for reference in e["replaces"].as_array().into_iter().flatten().filter_map(|v| v.as_str().and_then(|key| references.get(key).copied()).or_else(|| v.as_u64().and_then(|i| usize::try_from(i).ok()))).filter(|i| *i < *index) {
            let old = &events[reference];
            let same = if e["t"] == "card.usage.settled" { old["t"] == "card.usage" && old["cardId"] == e["cardId"] } else { old["t"] == "lead.usage" && old["provider"] == e["provider"] };
            if same { if e["reported"] == false { partial.insert(reference); } else { replaced.insert(reference); } }
        }
    }
    events.iter().enumerate().filter_map(|(index, event)| {
        if replaced.contains(&index) && matches!(event["t"].as_str(), Some("card.usage" | "lead.usage")) { return None; }
        if !matches!(event["t"].as_str(), Some("card.usage.settled" | "lead.usage.settled")) {
            let mut result = event.clone();
            if partial.contains(&index) { result["complete"] = json!(false); }
            return Some(result);
        }
        let id = event["usageId"].as_str()?;
        if latest.get(id)?.0 != index || event["reported"] == false { return None; }
        let mut projected = event.clone();
        projected["t"] = json!(if event["t"] == "card.usage.settled" { "card.usage" } else { "lead.usage" });
        projected["at"] = event["occurredAt"].clone();
        if conflicts.contains(id) { projected["complete"] = json!(false); }
        Some(projected)
    }).collect()
}
