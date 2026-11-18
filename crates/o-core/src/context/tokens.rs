//! `o tokens`: what cards spent, from the `card.usage` events in the run index,
//! grouped by how the run was set up, so the default harness can be compared
//! with clean agents that get only the shared context.
//!
//! A card's group is its run's mode (`o dispatch` or `o dispatch --clean`, the
//! run folder's `clean` marker) and whether the card got a context section
//! (`run.context` names it). Missing or partial usage remains visible separately.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use rusqlite::params;
use serde::Serialize;
use serde_json::Value;

use crate::run::index::{self, sql};
use crate::run::store::is_clean;

use super::normalize_project;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct Tokens {
    /// Fresh input (cache writes included, as the CLIs report them).
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub total: u64,
}

impl Tokens {
    fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.cached += other.cached;
        self.output += other.output;
        self.total += other.total;
    }

    fn per(&self, n: usize) -> Tokens {
        let avg = |v: u64| if n == 0 { 0 } else { (v as f64 / n as f64).round() as u64 };
        Tokens { input: avg(self.input), cached: avg(self.cached), output: avg(self.output), total: avg(self.total) }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardTokens {
    pub card_id: String,
    pub provider: String,
    pub context: bool,
    pub tokens: Tokens,
    pub by_provider: BTreeMap<String, Tokens>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunTokens {
    pub run_id: String,
    pub title: String,
    pub project: String,
    pub started_at: String,
    pub clean: bool,
    /// Some card of the run got a context section.
    pub context: bool,
    pub cards: Vec<CardTokens>,
    pub tokens: Tokens,
    /// Planned cards for which no usage event has arrived (including waiting cards).
    pub unreported_cards: Vec<String>,
    /// Cards with missing usage fields, unreported attempts, or ambiguous restarts.
    pub incomplete_cards: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTokens {
    pub provider: String,
    pub cards: usize,
    pub per_card: Tokens,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    /// `normal`, `normal+context`, `clean`, `clean+context`.
    pub key: String,
    pub label: String,
    pub clean: bool,
    pub context: bool,
    pub runs: usize,
    pub cards: usize,
    pub tokens: Tokens,
    pub per_card: Tokens,
    pub by_provider: Vec<ProviderTokens>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Compare {
    /// The default harness without shared context (`o dispatch --no-context`, or runs before it existed).
    pub baseline: Option<Group>,
    /// Clean agents with the shared context (`o dispatch --clean`).
    pub shared: Option<Group>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub groups: Vec<Group>,
    pub compare: Compare,
    /// Newest first.
    pub runs: Vec<RunTokens>,
    /// What lead chat turns spent planning (ADR-0008): overhead, not in any card or group.
    pub lead: crate::lead::LeadTokens,
    /// Only values the providers reported; absent lead usage is not inferred as zero.
    pub summary: Summary,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub reported: Tokens,
    pub reported_cards: usize,
    pub unreported_cards: usize,
    pub incomplete_cards: usize,
    /// Cached input as a percentage of reported fresh + cached input.
    pub cache_percent: Option<f64>,
}

#[derive(Default)]
pub struct TokenQuery<'a> {
    pub project: Option<&'a str>,
    /// Runs started at or after this ISO time or date (`2026-11-02`).
    pub since: Option<&'a str>,
}

const GROUPS: [(bool, bool, &str, &str); 4] = [
    (false, false, "normal", "기본 하네스"),
    (false, true, "normal+context", "기본+세션 공유"),
    (true, false, "clean", "깨끗한 에이전트"),
    (true, true, "clean+context", "깨끗한+세션 공유"),
];

fn number(value: Option<&Value>) -> u64 {
    value.and_then(Value::as_f64).filter(|n| n.is_finite() && *n > 0.0).map_or(0, |n| n.round() as u64)
}

fn valid_number(value: Option<&Value>) -> bool {
    value.and_then(Value::as_f64).is_some_and(|n| n.is_finite() && n >= 0.0)
}

/// Reads the index (brought up to date first) and adds it up.
pub fn report(home: &Path, query: &TokenQuery) -> io::Result<Report> {
    let mut conn = index::open(home)?;
    index::ingest(&mut conn, home)?;
    let project = query.project.map(normalize_project);
    let collate = if cfg!(windows) { "COLLATE NOCASE" } else { "" };
    let mut stmt = conn
        .prepare(&format!(
            r"SELECT r.run_id, r.title, r.project, r.started_at, e.json FROM runs r JOIN events e ON e.run_id = r.run_id
              WHERE r.started_at <> ''
                AND (?1 IS NULL OR replace(rtrim(r.project, '/\'), '\', '/') = ?1 {collate})
                AND (?2 IS NULL OR r.started_at >= ?2)
              ORDER BY r.started_at DESC, r.run_id DESC, e.seq"
        ))
        .map_err(sql)?;
    let rows = stmt
        .query_map(params![project, query.since], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })
        .map_err(sql)?;
    let mut grouped = Vec::<Vec<(String, Option<String>, Option<String>, String, String)>>::new();
    for row in rows {
        let row = row.map_err(sql)?;
        if grouped.last().is_none_or(|group| group[0].0 != row.0) { grouped.push(Vec::new()); }
        grouped.last_mut().unwrap().push(row);
    }
    let rows = grouped.into_iter().flat_map(|group| {
        let values: Vec<Value> = group.iter().filter_map(|r| serde_json::from_str(&r.4).ok()).collect();
        let first = &group[0];
        crate::usage::accounting::project(&values).into_iter().map(|value| (first.0.clone(), first.1.clone(), first.2.clone(), first.3.clone(), value.to_string())).collect::<Vec<_>>()
    });
    let mut runs: Vec<RunTokens> = Vec::new();
    struct CardUsage {
        card_id: String,
        provider: String,
        context: bool,
        tokens: Tokens,
        by_provider: BTreeMap<String, Tokens>,
        reported: bool,
        complete: bool,
        started: bool,
        attempt_reported: bool,
    }
    let mut cards: Vec<CardUsage> = Vec::new();
    let finish = |run: &mut RunTokens, cards: &mut Vec<CardUsage>| {
        for CardUsage { card_id, provider, context, tokens, by_provider, reported, complete, started, attempt_reported } in cards.drain(..) {
            run.context |= context;
            if !reported {
                run.unreported_cards.push(card_id);
                continue;
            }
            if !complete || (started && !attempt_reported) {
                run.incomplete_cards.push(card_id.clone());
            }
            run.tokens.add(&tokens);
            run.cards.push(CardTokens { card_id, provider, context, tokens, by_provider });
        }
    };
    for row in rows {
        let (run_id, title, project, started_at, json) = row;
        let Ok(event) = serde_json::from_str::<Value>(&json) else { continue };
        if runs.last().is_none_or(|r| r.run_id != run_id) {
            if let Some(last) = runs.last_mut() {
                finish(last, &mut cards);
            }
            runs.push(RunTokens {
                clean: is_clean(home, &run_id),
                run_id,
                title: title.unwrap_or_default(),
                project: normalize_project(project.as_deref().unwrap_or_default()),
                started_at,
                context: false,
                cards: Vec::new(),
                tokens: Tokens::default(),
                unreported_cards: Vec::new(),
                incomplete_cards: Vec::new(),
            });
        }
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        match event.get("t").and_then(Value::as_str) {
            Some("run.start") => {
                // A reused run ID makes attribution ambiguous, but earlier usage still occurred.
                for card in &mut cards {
                    if card.started || card.reported { card.complete = false; }
                }
                for c in event.get("cards").and_then(Value::as_array).into_iter().flatten() {
                    let field = |key: &str| c.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
                    let (card_id, provider) = (field("id"), field("provider"));
                    if let Some(card) = cards.iter_mut().find(|c| c.card_id == card_id) {
                        if card.provider != provider {
                            card.provider = "multiple".into();
                            card.complete = false;
                        }
                    } else {
                        cards.push(CardUsage { card_id, provider, context: false, tokens: Tokens::default(), by_provider: BTreeMap::new(), reported: false, complete: true, started: false, attempt_reported: false });
                    }
                }
            }
            Some("run.context") => {
                for id in event.get("cardIds").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    if let Some(card) = cards.iter_mut().find(|c| c.card_id == id) {
                        card.context = true;
                    }
                }
            }
            Some("card.start") => {
                let id = text("cardId");
                if let Some(card) = cards.iter_mut().find(|c| c.card_id == id) {
                    if card.started && !card.attempt_reported { card.complete = false; }
                    card.started = true;
                    card.attempt_reported = false;
                }
            }
            Some("card.usage") => {
                let id = text("cardId");
                if let Some(card) = cards.iter_mut().find(|c| c.card_id == id) {
                    let (input, cached, output) = (number(event.get("input")), number(event.get("cached")), number(event.get("output")));
                    card.tokens.add(&Tokens { input, cached, output, total: input + cached + output });
                    let provider = event["provider"].as_str().unwrap_or(&card.provider).to_string();
                    card.by_provider.entry(provider).or_default().add(&Tokens { input, cached, output, total: input + cached + output });
                    card.reported = true;
                    card.attempt_reported = true;
                    card.complete &= event.get("complete").and_then(Value::as_bool) == Some(true)
                        && ["input", "cached", "output"].iter().all(|key| valid_number(event.get(*key)));
                }
            }
            _ => {}
        }
    }
    if let Some(last) = runs.last_mut() {
        finish(last, &mut cards);
    }
    runs.retain(|r| !r.cards.is_empty() || !r.unreported_cards.is_empty());
    let groups: Vec<Group> = GROUPS.iter().filter_map(|&(clean, context, key, label)| group(&runs, clean, context, key, label)).collect();
    let pick = |key: &str| groups.iter().find(|g| g.key == key).cloned();
    let compare = Compare { baseline: pick("normal"), shared: pick("clean+context") };
    let lead = crate::lead::tokens(home, query.project, query.since);
    let mut summary = Summary::default();
    for run in &runs {
        for card in &run.cards {
            if !run.incomplete_cards.contains(&card.card_id) { summary.reported.add(&card.tokens); }
        }
        summary.reported_cards += run.cards.len();
        summary.unreported_cards += run.unreported_cards.len();
        summary.incomplete_cards += run.incomplete_cards.len();
    }
    summary.reported.add(&lead.complete_tokens);
    let input = summary.reported.input.saturating_add(summary.reported.cached);
    if input > 0 && summary.unreported_cards == 0 && summary.incomplete_cards == 0 && lead.unreported == 0 && lead.incomplete == 0 {
        summary.cache_percent = Some(summary.reported.cached as f64 * 100.0 / input as f64);
    }
    Ok(Report { groups, compare, runs, lead, summary })
}

fn group(runs: &[RunTokens], clean: bool, context: bool, key: &str, label: &str) -> Option<Group> {
    let mut tokens = Tokens::default();
    let mut cards = 0usize;
    let mut run_count = 0usize;
    let mut providers: BTreeMap<String, (usize, Tokens)> = BTreeMap::new();
    for run in runs.iter().filter(|r| r.clean == clean) {
        let mine: Vec<&CardTokens> = run.cards.iter().filter(|c| c.context == context && !run.incomplete_cards.contains(&c.card_id)).collect();
        if mine.is_empty() {
            continue;
        }
        run_count += 1;
        for card in mine {
            cards += 1;
            tokens.add(&card.tokens);
            for (provider, tokens) in &card.by_provider {
                let entry = providers.entry(provider.clone()).or_default();
                entry.0 += 1;
                entry.1.add(tokens);
            }
        }
    }
    (cards > 0).then(|| Group {
        key: key.into(),
        label: label.into(),
        clean,
        context,
        runs: run_count,
        cards,
        tokens,
        per_card: tokens.per(cards),
        by_provider: providers.into_iter().map(|(provider, (n, t))| ProviderTokens { provider, cards: n, per_card: t.per(n) }).collect(),
    })
}
