//! Folds a run's event stream into what a screen shows. Pure: `o status` and
//! the app both read runs through this, so they never disagree.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

use crate::policy::Violation;
use crate::run::events::{CardStatus, OEvent};
use crate::sys::js::ser_num;
use crate::run::plan::{card_label, ProviderId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CardState {
    Waiting,
    Queued,
    Running,
    Done,
    Failed,
    Stopped,
}

impl CardState {
    pub fn as_str(self) -> &'static str {
        match self {
            CardState::Waiting => "waiting",
            CardState::Queued => "queued",
            CardState::Running => "running",
            CardState::Done => "done",
            CardState::Failed => "failed",
            CardState::Stopped => "stopped",
        }
    }
}

impl From<CardStatus> for CardState {
    fn from(status: CardStatus) -> Self {
        match status {
            CardStatus::Done => CardState::Done,
            CardStatus::Failed => CardState::Failed,
            CardStatus::Stopped => CardState::Stopped,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogKind {
    Queued,
    Tool,
    Message,
    Input,
    Limit,
    End,
    Team,
    /// The runner lowered the card's effort for the daily usage budget.
    Budget,
    /// The project skills and O harness blocks the card's prompt got.
    Guides,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ToolWords {
    pub word: String,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TeamParties {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CardLogEntry {
    pub at: String,
    pub kind: LogKind,
    pub text: String,
    /// For tool entries: the act in words and the raw detail the CLI reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<ToolWords>,
    /// For team entries: who sent it and to whom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamParties>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Usage {
    #[serde(serialize_with = "ser_num")]
    pub input: f64,
    #[serde(serialize_with = "ser_num")]
    pub cached: f64,
    #[serde(serialize_with = "ser_num")]
    pub output: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardView {
    pub id: String,
    pub provider: ProviderId,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    pub state: CardState,
    /// The latest thing the card did, one line.
    pub activity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub log: Vec<CardLogEntry>,
    /// Tokens the card spent, as its CLI reported them.
    pub usage: Usage,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TeamMessage {
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(rename = "replyTo", skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<String>,
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub recipients: std::collections::BTreeMap<String, String>,
    pub from: String,
    pub to: String,
    pub text: String,
}

/// A provider's subscription window as last reported.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quota {
    pub provider: ProviderId,
    pub window: String,
    #[serde(serialize_with = "ser_num")]
    pub used: f64,
    pub resets_at: String,
    pub at: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Stopped,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Done => "done",
            RunStatus::Failed => "failed",
            RunStatus::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run_id: String,
    pub title: String,
    pub project: String,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    pub status: RunStatus,
    pub cards: Vec<CardView>,
    /// The run's team channel, in order.
    pub messages: Vec<TeamMessage>,
    /// Latest report per provider and window.
    pub quotas: Vec<Quota>,
    /// Why the runner refused to start the run (`run.blocked`); empty when it did not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<Violation>,
}

impl RunView {
    /// A run that ended failed before any card was made (refused by the runner's last check,
    /// or its runner failed): no card shows it, so it is counted as a run of its own.
    pub fn failed_before_cards(&self) -> bool {
        self.status == RunStatus::Failed && self.cards.is_empty()
    }

    /// Why it never started, one line: the policy's reasons, or where to look when there are none.
    pub fn blocked_reason(&self) -> String {
        if self.blocked.is_empty() {
            return BLOCKED_UNKNOWN.to_string();
        }
        self.blocked.iter().map(|v| v.message.as_str()).collect::<Vec<_>>().join(" / ")
    }
}

/// What a run that failed before its cards says when the runner left no reason (as src/core/snapshot.ts).
pub const BLOCKED_UNKNOWN: &str = "카드를 만들기 전에 실행기가 멈췄습니다. 실행 폴더의 runner.err.log를 확인하세요";

pub fn snapshot(events: &[OEvent]) -> Option<RunView> {
    let mut run: Option<RunView> = None;
    for e in events {
        if let OEvent::RunStart { at, run_id, title, project, cards } = e {
            run = Some(RunView {
                run_id: run_id.clone(),
                title: title.clone(),
                project: project.clone(),
                started_at: at.clone(),
                ended_at: None,
                status: RunStatus::Running,
                // Runs recorded before labels existed carry only a role.
                // A repeated card id is one card: the first keeps it (as src/core/snapshot.ts does).
                cards: cards
                    .iter()
                    .enumerate()
                    .filter(|(i, c)| !cards[..*i].iter().any(|earlier| earlier.id == c.id))
                    .map(|(_, c)| CardView {
                        id: c.id.clone(),
                        provider: c.provider,
                        label: c.label.clone().unwrap_or_else(|| card_label(&c.id, c.role.as_deref())),
                        role: c.role.clone(),
                        model: c.model.clone(),
                        effort: c.effort.clone(),
                        state: CardState::Waiting,
                        activity: String::new(),
                        result: None,
                        started_at: None,
                        ended_at: None,
                        log: Vec::new(),
                        usage: Usage::default(),
                    })
                    .collect(),
                messages: Vec::new(),
                quotas: Vec::new(),
                blocked: Vec::new(),
            });
            continue;
        }
        let Some(run) = run.as_mut() else { continue };
        match e {
            OEvent::Msg { at, from, to, text, id, kind, reply_to } => {
                run.messages.push(TeamMessage { at: at.clone(), from: from.clone(), to: to.clone(), text: text.clone(), id: id.clone(), kind: kind.clone(), reply_to: reply_to.clone(), recipients: Default::default() });
                for card in &mut run.cards {
                    if card.id == *from || card.id == *to || (to == "all" && card.id != *from) {
                        card.log.push(CardLogEntry {
                            at: at.clone(),
                            kind: LogKind::Team,
                            text: text.clone(),
                            tool: None,
                            team: Some(TeamParties { from: from.clone(), to: to.clone() }),
                        });
                    }
                }
                continue;
            }
            OEvent::MsgState { id, recipient, state, .. } => {
                if let Some(message) = run.messages.iter_mut().find(|m| m.id.as_deref() == Some(id)) {
                    message.recipients.insert(recipient.clone(), state.clone());
                }
                continue;
            }
            OEvent::RunEnd { at, status, .. } => {
                run.status = match status {
                    CardStatus::Done => RunStatus::Done,
                    CardStatus::Failed => RunStatus::Failed,
                    CardStatus::Stopped => RunStatus::Stopped,
                };
                run.ended_at = Some(at.clone());
                continue;
            }
            OEvent::Quota { at, provider, window, used, resets_at } => {
                run.quotas.retain(|q| q.provider != *provider || q.window != *window);
                run.quotas.push(Quota { provider: *provider, window: window.clone(), used: *used, resets_at: resets_at.clone(), at: at.clone() });
                continue;
            }
            OEvent::RunBlocked { violations, .. } => {
                run.blocked = violations.clone();
                continue;
            }
            _ => {}
        }
        let card_id = match e {
            OEvent::CardQueued { card_id, .. }
            | OEvent::CardStart { card_id, .. }
            | OEvent::CardSession { card_id, .. }
            | OEvent::CardMessage { card_id, .. }
            | OEvent::CardInput { card_id, .. }
            | OEvent::CardTool { card_id, .. }
            | OEvent::CardLimit { card_id, .. }
            | OEvent::CardEnd { card_id, .. }
            | OEvent::CardUsage { card_id, .. }
            | OEvent::CardBudget { card_id, .. }
            | OEvent::CardGuides { card_id, .. } => card_id,
            _ => continue,
        };
        let Some(card) = run.cards.iter_mut().find(|c| c.id == *card_id) else { continue };
        let entry = |at: &str, kind: LogKind, text: String| CardLogEntry { at: at.to_string(), kind, text, tool: None, team: None };
        match e {
            OEvent::CardQueued { at, reason, .. } => {
                card.state = CardState::Queued;
                card.activity = reason.clone();
                card.log.push(entry(at, LogKind::Queued, reason.clone()));
            }
            OEvent::CardStart { at, .. } => {
                card.state = CardState::Running;
                card.started_at = Some(at.clone());
            }
            OEvent::CardTool { at, name, summary, .. } => {
                card.activity = describe_tool(name, summary);
                let mut log = entry(at, LogKind::Tool, format!("{name} {summary}").trim().to_string());
                log.tool = Some(ToolWords { word: tool_word(name), detail: tool_detail(summary) });
                card.log.push(log);
            }
            OEvent::CardMessage { at, text, .. } => {
                card.activity = first_line(text);
                card.log.push(entry(at, LogKind::Message, text.clone()));
            }
            OEvent::CardInput { at, text, .. } => card.log.push(entry(at, LogKind::Input, text.clone())),
            OEvent::CardLimit { at, until, .. } => {
                card.activity = format!("사용량 한도: {until}까지 차단");
                card.log.push(entry(at, LogKind::Limit, card.activity.clone()));
            }
            OEvent::CardBudget { at, effort_from, effort_to, spent, allowance, .. } => {
                card.activity = budget_text(effort_from.as_deref(), effort_to, *spent, *allowance);
                card.effort = Some(effort_to.clone());
                card.log.push(entry(at, LogKind::Budget, card.activity.clone()));
            }
            OEvent::CardGuides { at, skills, harness, .. } => card.log.push(entry(at, LogKind::Guides, guides_text(skills, harness))),
            OEvent::CardUsage { input, cached, output, .. } => {
                card.usage.input += input;
                card.usage.cached += cached;
                card.usage.output += output;
            }
            OEvent::CardEnd { at, status, result, .. } => {
                card.state = (*status).into();
                card.ended_at = Some(at.clone());
                card.result = result.clone();
                if let Some(r) = result.as_deref().filter(|r| !r.is_empty()) {
                    card.activity = first_line(r);
                }
                card.log.push(entry(at, LogKind::End, result.clone().unwrap_or_else(|| end_word(*status).to_string())));
            }
            _ => {}
        }
    }
    run
}

/// "사용 예산: effort high → medium (오늘 9.5/13%p)", as `budgetText` in src/core/snapshot.ts.
pub fn budget_text(from: Option<&str>, to: &str, spent: f64, allowance: f64) -> String {
    format!("사용 예산: effort {} → {to} (오늘 {}/{}%p)", from.unwrap_or("기본"), crate::sys::js::number_text(spent), crate::sys::js::number_text(allowance))
}

/// "넣은 지침: 하네스 game · 스킬 ui, test", as `guidesText` in src/core/snapshot.ts.
pub fn guides_text(skills: &[String], harness: &[String]) -> String {
    let parts: Vec<String> = [("하네스", harness), ("스킬", skills)].into_iter()
        .filter(|(_, names)| !names.is_empty()).map(|(word, names)| format!("{word} {}", names.join(", "))).collect();
    format!("넣은 지침: {}", parts.join(" · "))
}

/// Tool names as each CLI reports them, grouped into what a person would call the act.
static TOOL_WORD: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)^(shell|bash|run_command|powershell)$", "명령 실행"),
        (r"(?i)^(read|read_file|view_file|grep|glob|list_dir|find_by_name)$", "파일 읽기"),
        (r"(?i)^(edit|write|multiedit|write_file|replace_file_content|apply_patch|notebookedit)$", "파일 수정"),
        (r"(?i)^(web_search|websearch|webfetch|search_web|read_url_content)$", "웹 검색"),
    ]
    .into_iter()
    .map(|(p, w)| (Regex::new(p).unwrap(), w))
    .collect()
});

fn tool_word(name: &str) -> String {
    TOOL_WORD.iter().find(|(p, _)| p.is_match(name)).map(|(_, w)| w.to_string()).unwrap_or_else(|| name.to_string())
}

fn change_word(kind: &str) -> Option<&'static str> {
    match kind {
        "add" => Some("추가"),
        "update" => Some("수정"),
        "delete" => Some("삭제"),
        _ => None,
    }
}

fn unquote(text: &str) -> String {
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') { text[1..text.len() - 1].to_string() } else { text.to_string() }
}

fn tool_detail(summary: &str) -> String {
    if let Some(files) = file_changes(summary) {
        return files;
    }
    unquote(summary.replace("\\\"", "\"").trim())
}

static CHANGES_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*\[\s*\{\s*\\?"path\\?""#).unwrap());
static CHANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""path":"((?:[^"\\]|\\.)*)"(?:,"kind":"([0-9A-Za-z_]+)")?"#).unwrap());
static PATH_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\\/]+").unwrap());

/// Codex reports file edits as a JSON list of {path, kind}; a person reads "app.js 수정".
/// Read with a pattern, not a JSON parser: long summaries arrive cut short.
fn file_changes(summary: &str) -> Option<String> {
    if !CHANGES_START.is_match(summary) {
        return None;
    }
    let text = summary.replace("\\\"", "\"");
    let files: Vec<String> = CHANGE
        .captures_iter(&text)
        .map(|c| {
            let name = PATH_SPLIT.split(&c[1]).last().unwrap_or_default().to_string();
            match c.get(2).and_then(|k| change_word(k.as_str())) {
                Some(word) => format!("{name} {word}"),
                None => name,
            }
        })
        .collect();
    // A path cut off mid-way names no file; say there were more rather than guess.
    let cut = text.matches("\"path\":").count() > files.len();
    Some(if files.is_empty() { String::new() } else { format!("{}{}", files.join(", "), if cut { " 외" } else { "" }) })
}

/// "명령 실행: git status" rather than `shell "git status\"`; the log keeps the original.
pub fn describe_tool(name: &str, summary: &str) -> String {
    let detail = tool_detail(summary);
    if detail.is_empty() { tool_word(name) } else { format!("{}: {detail}", tool_word(name)) }
}

/// The first line, without the Markdown agents write in (`code`, **bold**): an activity is read at a glance.
pub fn first_line(text: &str) -> String {
    let line = text.split('\n').map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    line.replace("**", "").replace('`', "")
}

pub fn end_word(status: CardStatus) -> &'static str {
    match status {
        CardStatus::Done => "완료",
        CardStatus::Stopped => "중지됨",
        CardStatus::Failed => "실패",
    }
}
