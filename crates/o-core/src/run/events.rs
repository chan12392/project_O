//! The normalized event stream every run writes to `events.jsonl`.
//!
//! This is the contract between O CLI and any screen (chat UI, character pane,
//! Control board). Provider-specific stream formats never leave `providers`;
//! screens only ever see these shapes.

use serde::{Deserialize, Serialize};

use crate::policy::Violation;
use crate::sys::js::ser_num;
use crate::run::plan::ProviderId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CardStatus {
    Done,
    Failed,
    Stopped,
}

impl CardStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CardStatus::Done => "done",
            CardStatus::Failed => "failed",
            CardStatus::Stopped => "stopped",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CardInfo {
    pub id: String,
    pub provider: ProviderId,
    /// What people call the card. Runs recorded before labels existed carry only a role.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all_fields = "camelCase")]
pub enum OEvent {
    #[serde(rename = "run.start")]
    RunStart { at: String, run_id: String, title: String, project: String, cards: Vec<CardInfo> },
    #[serde(rename = "card.queued")]
    CardQueued { at: String, card_id: String, reason: String },
    #[serde(rename = "card.start")]
    CardStart {
        at: String,
        card_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
    },
    #[serde(rename = "card.session")]
    CardSession { at: String, card_id: String, session_id: String },
    #[serde(rename = "card.message")]
    CardMessage { at: String, card_id: String, text: String },
    #[serde(rename = "card.tool")]
    CardTool { at: String, card_id: String, name: String, summary: String },
    #[serde(rename = "card.limit")]
    CardLimit { at: String, card_id: String, until: String },
    #[serde(rename = "card.end")]
    CardEnd {
        at: String,
        card_id: String,
        status: CardStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<String>,
        #[serde(default)]
        exit_code: Option<i64>,
    },
    /// A team message between agents of one run (`o msg send`), "lead" for the supervisor, "all" for everyone.
    #[serde(rename = "msg")]
    Msg { at: String, from: String, to: String, text: String },
    /// Tokens a card spent since its last usage event (fresh input, cached input, output).
    #[serde(rename = "card.usage")]
    CardUsage {
        at: String,
        card_id: String,
        #[serde(serialize_with = "ser_num")]
        input: f64,
        #[serde(serialize_with = "ser_num")]
        cached: f64,
        #[serde(serialize_with = "ser_num")]
        output: f64,
    },
    /// A subscription window the provider reported: share used (0 to 1) and when it resets.
    #[serde(rename = "quota")]
    Quota {
        at: String,
        provider: ProviderId,
        window: String,
        #[serde(serialize_with = "ser_num")]
        used: f64,
        resets_at: String,
    },
    #[serde(rename = "run.end")]
    RunEnd { at: String, run_id: String, status: CardStatus },
    /// `o review --post` commented the review card's answer on the PR (ADR-0006).
    #[serde(rename = "review.posted")]
    ReviewPosted {
        at: String,
        pr: u64,
        url: String,
        /// The PR head commit that was reviewed (and was still the head when posted).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        head: Option<String>,
    },
    /// The runner lowered a card's effort before launching it to keep its provider within
    /// today's usage budget (ADR-0004). `spent` and `allowance` are percent points of the weekly window.
    #[serde(rename = "card.budget")]
    CardBudget {
        at: String,
        card_id: String,
        provider: ProviderId,
        /// The plan's effort; absent when the card named none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        effort_from: Option<String>,
        effort_to: String,
        #[serde(serialize_with = "ser_num")]
        spent: f64,
        #[serde(serialize_with = "ser_num")]
        allowance: f64,
    },
    /// Cards whose prompt got a shared context section (`context`), and its characters in all.
    #[serde(rename = "run.context")]
    RunContext { at: String, run_id: String, cards: usize, chars: usize, card_ids: Vec<String> },
    /// The runner's own check right before the start (`o _run`) refused the plan: no card
    /// started. The violations are the ones `o check` would print (a cap, a cooldown, today's
    /// budget read again since dispatch). `run.end failed` follows.
    #[serde(rename = "run.blocked")]
    RunBlocked { at: String, run_id: String, violations: Vec<Violation> },
}

impl OEvent {
    /// The `t` tag.
    pub fn kind(&self) -> &'static str {
        match self {
            OEvent::RunStart { .. } => "run.start",
            OEvent::CardQueued { .. } => "card.queued",
            OEvent::CardStart { .. } => "card.start",
            OEvent::CardSession { .. } => "card.session",
            OEvent::CardMessage { .. } => "card.message",
            OEvent::CardTool { .. } => "card.tool",
            OEvent::CardLimit { .. } => "card.limit",
            OEvent::CardEnd { .. } => "card.end",
            OEvent::Msg { .. } => "msg",
            OEvent::CardUsage { .. } => "card.usage",
            OEvent::Quota { .. } => "quota",
            OEvent::RunEnd { .. } => "run.end",
            OEvent::ReviewPosted { .. } => "review.posted",
            OEvent::CardBudget { .. } => "card.budget",
            OEvent::RunContext { .. } => "run.context",
            OEvent::RunBlocked { .. } => "run.blocked",
        }
    }
}

/// What a provider stream line means, before it gets a timestamp and card id.
#[derive(Clone, Debug, PartialEq)]
pub enum StreamSignal {
    Session { session_id: String },
    Message { text: String },
    Tool { name: String, summary: String },
    Limit { until: String },
    Usage { input: f64, cached: f64, output: f64 },
    Quota { window: String, used: f64, resets_at: String },
    Final { ok: bool, text: String },
}
