//! `o tokens`: what cards spent, from the `card.usage` events in the run index,
//! grouped by how the run was set up, so the default harness can be compared
//! with clean agents that get only the shared context.
//!
//! A card's group is its run's mode (`o dispatch` or `o dispatch --clean`, the
//! run folder's `clean` marker) and whether the card got a context section
//! (`run.context` names it). Cards that never reported usage are left out.

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
                AND json_extract(e.json, '$.t') IN ('run.start', 'card.usage', 'run.context')
              ORDER BY r.started_at DESC, r.run_id DESC, e.seq"
        ))
        .map_err(sql)?;
    let rows = stmt
        .query_map(params![project, query.since], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?))
        })
        .map_err(sql)?;
    let mut runs: Vec<RunTokens> = Vec::new();
    // Per run, in card order: provider and tokens so far.
    let mut cards: Vec<(String, String, bool, Tokens, bool)> = Vec::new();
    let finish = |run: &mut RunTokens, cards: &mut Vec<(String, String, bool, Tokens, bool)>| {
        run.cards = cards.drain(..).filter(|c| c.4).map(|(card_id, provider, context, tokens, _)| CardTokens { card_id, provider, context, tokens }).collect();
        run.context = run.cards.iter().any(|c| c.context);
        for c in &run.cards {
            run.tokens.add(&c.tokens);
        }
    };
    for row in rows {
        let (run_id, title, project, started_at, json) = row.map_err(sql)?;
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
            });
        }
        let text = |key: &str| event.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        match event.get("t").and_then(Value::as_str) {
            // Like the index: a start begins the run afresh.
            Some("run.start") => {
                cards = event
                    .get("cards")
                    .and_then(Value::as_array)
                    .map(|list| {
                        list.iter()
                            .map(|c| {
                                let field = |key: &str| c.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
                                (field("id"), field("provider"), false, Tokens::default(), false)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            Some("run.context") => {
                for id in event.get("cardIds").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
                    if let Some(card) = cards.iter_mut().find(|c| c.0 == id) {
                        card.2 = true;
                    }
                }
            }
            Some("card.usage") => {
                let id = text("cardId");
                if let Some(card) = cards.iter_mut().find(|c| c.0 == id) {
                    let (input, cached, output) = (number(event.get("input")), number(event.get("cached")), number(event.get("output")));
                    card.3.add(&Tokens { input, cached, output, total: input + cached + output });
                    card.4 = true;
                }
            }
            _ => {}
        }
    }
    if let Some(last) = runs.last_mut() {
        finish(last, &mut cards);
    }
    runs.retain(|r| !r.cards.is_empty());
    let groups: Vec<Group> = GROUPS.iter().filter_map(|&(clean, context, key, label)| group(&runs, clean, context, key, label)).collect();
    let pick = |key: &str| groups.iter().find(|g| g.key == key).cloned();
    let compare = Compare { baseline: pick("normal"), shared: pick("clean+context") };
    Ok(Report { groups, compare, runs })
}

fn group(runs: &[RunTokens], clean: bool, context: bool, key: &str, label: &str) -> Option<Group> {
    let mut tokens = Tokens::default();
    let mut cards = 0usize;
    let mut run_count = 0usize;
    let mut providers: BTreeMap<String, (usize, Tokens)> = BTreeMap::new();
    for run in runs.iter().filter(|r| r.clean == clean) {
        let mine: Vec<&CardTokens> = run.cards.iter().filter(|c| c.context == context).collect();
        if mine.is_empty() {
            continue;
        }
        run_count += 1;
        for card in mine {
            cards += 1;
            tokens.add(&card.tokens);
            let entry = providers.entry(card.provider.clone()).or_default();
            entry.0 += 1;
            entry.1.add(&card.tokens);
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
