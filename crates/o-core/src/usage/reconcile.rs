//! Explicit maintenance only. A preview never writes usage, events or the database.
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::accounting::{self, Collector, Settlement};
use crate::run::events::OEvent;
use crate::run::plan::ProviderId;
use crate::run::store;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change { pub usage_id: String, pub before: f64, pub after: f64, pub complete: bool, pub reason: Option<String>, pub applied: bool }

pub fn run(home: &Path, selected_run: Option<&str>, project: Option<&str>, apply: bool) -> Result<Vec<Change>, String> {
    if selected_run.is_none() && project.is_none() { return Err("--run 또는 --project로 보정 범위를 명시하세요".into()); }
    if selected_run.is_some_and(|id| !store::is_run_id(id)) { return Err("실행 ID 형식이 올바르지 않습니다".into()); }
    if apply && crate::agents::agy::inside_agent() { return Err("과거 사용량 적용은 사용자가 직접 실행해야 합니다".into()); }
    reconcile(home, selected_run, project, apply).map_err(|e| e.to_string())
}

fn values(path: &Path) -> io::Result<Vec<Value>> {
    Ok(fs::read_to_string(path)?.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
}

fn reconcile(home: &Path, selected_run: Option<&str>, project: Option<&str>, apply: bool) -> io::Result<Vec<Change>> {
    let mut journal = accounting::read_journal(home)?;
    let reservations = home.join("o.db").is_file().then(|| rusqlite::Connection::open_with_flags(home.join("o.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)).transpose().map_err(io::Error::other)?;
    let mut changes = Vec::new();
    let mut dirs = match fs::read_dir(home.join("runs")) { Ok(entries) => entries.collect::<Result<Vec<_>, _>>()?, Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(), Err(e) => return Err(e) };
    dirs.sort_by_key(|e| e.file_name());
    for dir in dirs {
        let id = dir.file_name().to_string_lossy().into_owned();
        if !store::is_run_id(&id) || selected_run.is_some_and(|wanted| wanted != id) { continue; }
        let events = values(&dir.path().join("events.jsonl"))?;
        let Some(start) = events.iter().find(|e| e["t"] == "run.start") else { continue };
        if project.is_some_and(|p| !crate::projects::same(p, start["project"].as_str().unwrap_or_default())) { continue; }
        if !events.iter().any(|e| e["t"] == "run.end") { continue; }
        let projected = accounting::project(&events);
        for card in start["cards"].as_array().into_iter().flatten() {
            let Some(card_id) = card["id"].as_str().filter(|id| store::is_run_id(id)) else { continue };
            let Some(provider) = card["provider"].as_str().and_then(ProviderId::parse).filter(|p| matches!(p, ProviderId::Claude | ProviderId::Codex)) else { continue };
            if reservations.as_ref().map(|db| db.query_row("SELECT EXISTS(SELECT 1 FROM resource_leases WHERE run_id=?1 AND card_id=?2)", rusqlite::params![id, card_id], |row| row.get::<_, bool>(0))).transpose().map_err(io::Error::other)?.unwrap_or(false) { continue; }
            let attempts: Vec<_> = events.iter().filter(|e| e["t"] == "card.start" && e["cardId"] == card_id).collect();
            let published: Vec<Settlement> = events.iter().filter(|e| e["t"] == "card.usage.settled" && e["cardId"] == card_id).filter_map(|e| serde_json::from_value(e.clone()).ok()).collect();
            let mut existing = published.clone();
            existing.extend(journal.iter().filter(|v| v.usage_id.starts_with(&format!("run:{id}:{card_id}:"))).cloned());
            if !existing.is_empty() {
                for old in latest(&existing) {
                    let next = match replay(home, &journal, &old, apply) { Ok(next) => next, Err(error) => { changes.push(skipped(&old.usage_id, old.input + old.cached + old.output, &error.to_string())); continue; } };
                    if let Some(next) = next {
                        let before = old.input + old.cached + old.output;
                        let changed = !published.contains(&next);
                        if apply && changed { store::append_event(home, &id, &OEvent::CardUsageSettled { at: crate::sys::js::now_iso(), card_id: card_id.into(), usage: Box::new(next.clone()) })?; }
                        changes.push(change(&next, before, apply && changed));
                        journal.push(next);
                    }
                }
                continue;
            }
            let before = projected.iter().filter(|e| e["t"] == "card.usage" && e["cardId"] == card_id).map(|e| ["input", "cached", "output"].iter().filter_map(|k| e[*k].as_f64()).sum::<f64>()).sum();
            if attempts.len() != 1 { changes.push(skipped(&format!("run:{id}:{card_id}:legacy"), before, "시도별 원본 경계가 없어 기존 사용량을 유지했습니다")); continue; }
            let Some(end) = events.iter().rev().find(|e| e["t"] == "card.end" && e["cardId"] == card_id).and_then(|e| e["at"].as_str()) else { continue };
            let source = format!("runs/{id}/cards/{card_id}.raw.jsonl");
            let raw = match fs::read_to_string(home.join(&source)) { Ok(raw) => raw, Err(error) => { changes.push(skipped(&format!("run:{id}:{card_id}:legacy"), before, &format!("원본을 읽지 못해 기존 사용량을 유지했습니다: {error}"))); continue; } };
            let mut collector = Collector::new(provider, None, false, attempts[0]["at"].as_str().unwrap_or_default().into());
            collector.resumed = dir.path().join("followup.json").exists();
            collector.source_end = raw.len() as u64;
            for line in raw.lines() { collector.observe(line); }
            let root = crate::agents::clean::default_root();
            let profile = accounting::profile(provider, store::is_clean(home, &id).then_some(root.as_path()));
            let usage_id = format!("run:{id}:{card_id}:{provider}:1");
            let references = accounting::legacy_references(&events);
            let replaces = events.iter().enumerate().filter(|(_, e)| e["t"] == "card.usage" && e["cardId"] == card_id).filter_map(|(i, _)| references[i].clone()).collect();
            let next = if apply { collector.settle(home, &profile, &usage_id, &source, end, true, replaces)? }
                else { collector.calculate(&journal, &profile, &usage_id, &source, end, true, replaces)? };
            if let Some(next) = next {
                if apply { store::append_event(home, &id, &OEvent::CardUsageSettled { at: crate::sys::js::now_iso(), card_id: card_id.into(), usage: Box::new(next.clone()) })?; }
                changes.push(change(&next, before, apply));
                journal.push(next);
            }
        }
    }
    if selected_run.is_none() {
        for old in latest(&journal.clone()).into_iter().filter(|v| v.source.starts_with("leads/")) {
            let parts: Vec<_> = old.source.split('/').collect();
            if parts.len() != 3 || !store::is_run_id(parts[1]) || parts[2] != "turn.raw.jsonl" { continue; }
            let thread = parts[1];
            if home.join("leads").join(thread).join("running.json").exists() { continue; }
            let events = values(&home.join("leads").join(thread).join("events.jsonl"))?;
            if project.is_some_and(|p| !events.iter().any(|e| e["t"] == "lead.start" && e["project"].as_str().is_some_and(|v| crate::projects::same(p, v)))) { continue; }
            let next = match replay(home, &journal, &old, apply) { Ok(next) => next, Err(error) => { changes.push(skipped(&old.usage_id, old.input + old.cached + old.output, &error.to_string())); continue; } };
            if let Some(next) = next {
                let changed = !events.iter().filter(|e| e["t"] == "lead.usage.settled").filter_map(|e| serde_json::from_value::<Settlement>(e.clone()).ok()).any(|published| published == next);
                if apply && changed { crate::lead::append(home, thread, &crate::lead::LeadEvent::UsageSettled { at: crate::sys::js::now_iso(), usage: next.clone() })?; }
                changes.push(change(&next, old.input + old.cached + old.output, apply && changed));
                journal.push(next);
            }
        }
        if let Ok(dirs) = fs::read_dir(home.join("leads")) {
            for dir in dirs.flatten().filter(|d| d.path().is_dir()) {
                let Ok(events) = values(&dir.path().join("events.jsonl")) else { continue };
                if !events.iter().any(|e| e["t"] == "lead.start" && project.is_some_and(|p| e["project"].as_str().is_some_and(|v| crate::projects::same(p, v)))) { continue; }
                let legacy: Vec<_> = accounting::project(&events).into_iter().filter(|e| e["t"] == "lead.usage" && e.get("usageId").is_none()).collect();
                if !legacy.is_empty() {
                    let before = legacy.iter().map(|e| ["input", "cached", "output"].iter().filter_map(|k| e[*k].as_f64()).sum::<f64>()).sum();
                    changes.push(skipped(&format!("lead:{}:legacy", dir.file_name().to_string_lossy()), before, "옛 총괄 기록은 턴별 원본 경계가 없어 자동으로 재분배하지 않았습니다"));
                }
            }
        }
    }
    Ok(changes)
}

fn latest(reports: &[Settlement]) -> Vec<Settlement> {
    let mut latest = std::collections::BTreeMap::new();
    for report in reports {
        let kept = latest.entry(&report.usage_id).or_insert(report);
        if kept.revision < report.revision { *kept = report; }
    }
    latest.values().map(|s| (*s).clone()).collect()
}

fn replay(home: &Path, journal: &[Settlement], old: &Settlement, apply: bool) -> io::Result<Option<Settlement>> {
    let path = Path::new(&old.source);
    if path.is_absolute() || path.components().any(|c| !matches!(c, std::path::Component::Normal(_))) { return Err(io::Error::other("원본 참조 경로가 올바르지 않습니다")); }
    let raw = fs::read(home.join(path))?;
    let Some(bytes) = raw.get(old.source_start as usize..old.source_end as usize) else { return Err(io::Error::other("원본 보고의 범위가 없어 보정하지 않았습니다")) };
    let mut collector = Collector::new(old.provider, old.resumed.then(|| old.session_id.clone()).flatten(), old.app_server, old.occurred_at.clone());
    collector.source_start = old.source_start;
    collector.source_end = old.source_end;
    for line in String::from_utf8_lossy(bytes).lines() { collector.observe(line); }
    if apply { collector.settle(home, Path::new(&old.profile), &old.usage_id, &old.source, &old.ended_at, old.complete || old.scope == "tree", old.replaces.clone()) }
    else { collector.calculate(journal, Path::new(&old.profile), &old.usage_id, &old.source, &old.ended_at, old.complete || old.scope == "tree", old.replaces.clone()) }
}

fn change(report: &Settlement, before: f64, applied: bool) -> Change {
    Change { usage_id: report.usage_id.clone(), before, after: if report.reported { report.input + report.cached + report.output } else { before }, complete: report.complete, reason: report.reason.clone(), applied }
}

fn skipped(usage_id: &str, before: f64, reason: &str) -> Change {
    Change { usage_id: usage_id.into(), before, after: before, complete: false, reason: Some(reason.into()), applied: false }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn journal_only_report_is_recovered_and_reapplying_does_not_duplicate() {
        let home = std::env::temp_dir().join(format!("o-reconcile-journal-{}", store::new_run_id(chrono::Utc::now())));
        let cards = home.join("runs/run/cards");
        fs::create_dir_all(&cards).unwrap();
        let events = [json!({"t":"run.start","at":"2026-11-12T01:00:00Z","runId":"run","title":"fixture","project":"fixture-project","cards":[{"id":"a","provider":"claude","task":"fixture"}]}), json!({"t":"card.start","at":"2026-11-12T01:00:00Z","cardId":"a"}), json!({"t":"card.end","at":"2026-11-12T01:01:00Z","cardId":"a","status":"done"}), json!({"t":"run.end","at":"2026-11-12T01:01:00Z","status":"done"})];
        fs::write(home.join("runs/run/events.jsonl"), events.iter().map(|e| format!("{e}\n")).collect::<String>()).unwrap();
        let raw = json!({"type":"result","session_id":"fixture-session","modelUsage":{"fixture-model":{"inputTokens":10,"cacheCreationInputTokens":2,"cacheReadInputTokens":5,"outputTokens":3}}}).to_string();
        fs::write(cards.join("a.raw.jsonl"), &raw).unwrap();
        let mut collector = Collector::new(ProviderId::Claude, None, false, "2026-11-12T01:00:00Z".into());
        collector.source_end = raw.len() as u64; collector.observe(&raw);
        collector.settle(&home, &home.join("profile"), "run:run:a:claude:1", "runs/run/cards/a.raw.jsonl", "2026-11-12T01:01:00Z", true, vec![]).unwrap();
        // Exercise the maintenance implementation on fixture data, below the human-only CLI boundary.
        assert!(reconcile(&home, Some("run"), None, true).unwrap()[0].applied);
        assert!(!reconcile(&home, Some("run"), None, true).unwrap()[0].applied);
        assert_eq!(values(&home.join("runs/run/events.jsonl")).unwrap().iter().filter(|e| e["t"] == "card.usage.settled").count(), 1);
    }
}
