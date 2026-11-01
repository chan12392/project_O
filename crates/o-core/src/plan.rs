//! A plan is what the supervising agent hands to `o dispatch`: one project and
//! a few work cards, each pinned to a provider CLI.
//!
//! Cards carry only data. They cannot name a binary, arguments, environment or
//! credentials — `providers` alone decides how a provider is launched, and
//! `policy` rejects any field outside [`CARD_FIELDS`].

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Claude,
    Codex,
    Agy,
}

impl ProviderId {
    pub const ALL: [ProviderId; 3] = [ProviderId::Claude, ProviderId::Codex, ProviderId::Agy];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderId::Claude => "claude",
            ProviderId::Codex => "codex",
            ProviderId::Agy => "agy",
        }
    }

    pub fn parse(value: &str) -> Option<ProviderId> {
        ProviderId::ALL.into_iter().find(|p| p.as_str() == value)
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const CARD_FIELDS: [&str; 6] = ["id", "provider", "role", "task", "model", "effort"];
pub const PLAN_FIELDS: [&str; 3] = ["title", "project", "cards"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub provider: ProviderId,
    /// A free label for people ("코딩", "검토"). It never selects or limits a model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub task: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

impl Card {
    pub fn label(&self) -> String {
        card_label(&self.id, self.role.as_deref())
    }
}

/// What screens and prompts call a card: its role label if it has one, else its id.
pub fn card_label(id: &str, role: Option<&str>) -> String {
    match role.map(str::trim) {
        Some(r) if !r.is_empty() => r.to_string(),
        _ => id.to_string(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    pub title: String,
    pub project: String,
    pub cards: Vec<Card>,
}
