//! The rules that decide whether a plan may run. The supervising agent only
//! proposes; this module decides, and a plan with any violation runs nothing.
//!
//! Limits here are ceilings, not defaults: there is deliberately no setting,
//! flag or environment variable that raises them. Changing one is an ADR.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use o_guard::rules::{code_rules, find_hits};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use crate::js::{js_len, parse_time};
use crate::plan::{Card, Plan, ProviderId, CARD_FIELDS, PLAN_FIELDS};

pub struct Limits {
    pub max_cards_per_plan: usize,
    pub max_task_chars: usize,
    pub max_message_chars: usize,
    pub max_active_total: usize,
    pub max_cards_per_day: usize,
    /// When a provider reports a limit without a reset time.
    pub default_cooldown_ms: i64,
}

impl Limits {
    pub const fn max_active_per_provider(&self, provider: ProviderId) -> usize {
        match provider {
            ProviderId::Claude => 3,
            ProviderId::Codex => 3,
            ProviderId::Agy => 2,
        }
    }
}

pub const LIMITS: Limits = Limits {
    max_cards_per_plan: 8,
    max_task_chars: 20_000,
    max_message_chars: 4_000,
    max_active_total: 6,
    max_cards_per_day: 60,
    default_cooldown_ms: 60 * 60 * 1000,
};

const EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
static MODEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._:\-/\[\]]{0,79}$").unwrap());
static CARD_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,31}$").unwrap());
static ABSOLUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]:[\\/]").unwrap());

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub rule: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
}

/// What the machine is doing right now, read from O's state directory.
pub struct PolicyContext {
    pub now: DateTime<Utc>,
    pub active_by_provider: HashMap<ProviderId, usize>,
    pub started_today: usize,
    /// Provider → ISO time until which it may not be used.
    pub cooldowns: HashMap<ProviderId, String>,
    pub project_exists: Box<dyn Fn(&str) -> bool>,
    /// Whether the folder is a project_O protected-branch (test_1/test_2/main) mirror checkout.
    pub is_mirror: Box<dyn Fn(&str) -> bool>,
    /// O's state folder: a project may neither be inside it nor contain it.
    pub o_home: String,
    /// The user's home: a project may be inside it, never be it or above it.
    pub user_home: String,
}

pub struct Checked {
    pub plan: Option<Plan>,
    pub violations: Vec<Violation>,
}

/// Whether `path` is `dir` or inside it, compared as Windows does (any slash, any case).
fn within(path: &str, dir: &str) -> bool {
    let norm = |p: &str| p.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let (p, d) = (norm(path), norm(dir));
    p == d || p.starts_with(&format!("{d}/"))
}

fn is_absolute(path: &str) -> bool {
    ABSOLUTE.is_match(path) || path.starts_with('/')
}

#[derive(Default)]
struct Found(Vec<Violation>);

impl Found {
    fn add(&mut self, rule: impl Into<String>, message: impl Into<String>, card_id: Option<&str>) {
        self.0.push(Violation { rule: rule.into(), message: message.into(), card_id: card_id.map(str::to_string) });
    }
}

pub fn check_plan(input: &Value, ctx: &PolicyContext) -> Checked {
    let mut found = Found::default();
    let Some(object) = input.as_object() else {
        found.add("plan-shape", "계획은 JSON 객체여야 합니다", None);
        return Checked { plan: None, violations: found.0 };
    };
    for key in object.keys() {
        if !PLAN_FIELDS.contains(&key.as_str()) {
            found.add("unsupported-field", format!("계획에 허용되지 않는 필드: {key}"), None);
        }
    }
    let title = object.get("title").and_then(Value::as_str);
    if title.is_none_or(|t| t.trim().is_empty()) {
        found.add("plan-shape", "title이 필요합니다", None);
    }
    let project = object.get("project").and_then(Value::as_str);
    match project {
        Some(project) if is_absolute(project) => {
            if !(ctx.project_exists)(project) {
                found.add("plan-shape", format!("project 폴더가 없습니다: {project}"), None);
            } else if (ctx.is_mirror)(project) {
                found.add(
                    "read-only-project",
                    format!("{project}는 project_O의 보호 브랜치(test_1·test_2·main) 거울 폴더라 작업을 보낼 수 없습니다. 개인 브랜치 폴더를 지정하세요"),
                    None,
                );
            } else if within(project, &ctx.o_home) || within(&ctx.o_home, project) || within(&ctx.user_home, project) {
                // A card may write inside its project: O's own state and the home folder (CLI settings) must stay out of reach.
                found.add(
                    "protected-project",
                    format!("{project}는 O의 상태 폴더나 홈 폴더를 포함해 작업 폴더로 쓸 수 없습니다. 프로젝트 폴더를 지정하세요"),
                    None,
                );
            }
        }
        _ => found.add("plan-shape", "project는 절대 경로여야 합니다", None),
    }
    let cards = match object.get("cards").and_then(Value::as_array) {
        Some(cards) if !cards.is_empty() => cards,
        _ => {
            found.add("plan-shape", "cards가 1개 이상 필요합니다", None);
            return Checked { plan: None, violations: found.0 };
        }
    };
    if cards.len() > LIMITS.max_cards_per_plan {
        found.add("plan-size", format!("카드는 계획당 최대 {}개입니다", LIMITS.max_cards_per_plan), None);
    }

    let mut ids = HashSet::new();
    let mut valid: Vec<Card> = Vec::new();
    for raw in cards {
        let Some(card) = check_card(raw, &mut found) else { continue };
        if ids.contains(&card.id) {
            found.add("plan-shape", format!("카드 id가 중복됩니다: {}", card.id), Some(&card.id));
        }
        ids.insert(card.id.clone());
        valid.push(card);
    }

    for card in &valid {
        if let Some(until) = ctx.cooldowns.get(&card.provider) {
            if parse_time(until).is_some_and(|t| t > ctx.now) {
                found.add(
                    "provider-cooldown",
                    format!(
                        "{}는 사용량 한도에 걸려 {until}까지 쓸 수 없습니다. 같은 서비스의 다른 계정으로 돌려 쓰는 것도 막혀 있습니다",
                        card.provider
                    ),
                    Some(&card.id),
                );
            }
        }
    }

    let active = |p: ProviderId| ctx.active_by_provider.get(&p).copied().unwrap_or(0);
    let active_total: usize = ctx.active_by_provider.values().sum();
    if active_total >= LIMITS.max_active_total {
        found.add(
            "active-cap",
            format!("이미 {active_total}개 작업이 실행 중입니다(최대 {}). 끝난 뒤 다시 보내세요", LIMITS.max_active_total),
            None,
        );
    }
    for provider in ProviderId::ALL {
        let wanted = valid.iter().any(|c| c.provider == provider);
        let max = LIMITS.max_active_per_provider(provider);
        if wanted && active(provider) >= max {
            found.add("active-cap", format!("{provider} 작업이 이미 최대치({max})로 실행 중입니다"), None);
        }
    }
    if ctx.started_today + valid.len() > LIMITS.max_cards_per_day {
        found.add(
            "daily-cap",
            format!("오늘 실행한 카드 {}개 — 하루 최대 {}개를 넘습니다", ctx.started_today, LIMITS.max_cards_per_day),
            None,
        );
    }

    if !found.0.is_empty() {
        return Checked { plan: None, violations: found.0 };
    }
    let plan = Plan { title: title.unwrap_or_default().trim().to_string(), project: project.unwrap_or_default().to_string(), cards: valid };
    Checked { plan: Some(plan), violations: Vec::new() }
}

fn check_card(raw: &Value, found: &mut Found) -> Option<Card> {
    let Some(raw) = raw.as_object() else {
        found.add("plan-shape", "카드는 JSON 객체여야 합니다", None);
        return None;
    };
    let id = raw.get("id").and_then(Value::as_str).unwrap_or("?").to_string();
    let mut ok = true;
    let mut fail = |found: &mut Found, rule: String, message: String| {
        ok = false;
        found.add(rule, message, Some(&id));
    };

    for key in raw.keys() {
        if !CARD_FIELDS.contains(&key.as_str()) {
            fail(found, "unsupported-field".into(), format!("카드에 허용되지 않는 필드: {key} — 실행 파일·인자·환경변수·계정은 카드로 지정할 수 없습니다"));
        }
    }
    if !CARD_ID.is_match(&id) {
        fail(found, "plan-shape".into(), "카드 id는 영문 소문자·숫자·하이픈 32자 이내입니다".into());
    }
    let provider = raw.get("provider").and_then(Value::as_str).and_then(ProviderId::parse);
    if provider.is_none() {
        let names = ProviderId::ALL.map(ProviderId::as_str).join(", ");
        fail(found, "unknown-provider".into(), format!("provider는 {names} 중 하나여야 합니다"));
    }
    if let Some(role) = raw.get("role") {
        if role.as_str().is_none_or(|r| js_len(r) > 40) {
            fail(found, "plan-shape".into(), "role은 40자 이내의 글자여야 합니다(선택 항목)".into());
        }
    }
    let task = raw.get("task").and_then(Value::as_str);
    match task {
        Some(t) if !t.trim().is_empty() => {
            if js_len(t) > LIMITS.max_task_chars {
                fail(found, "plan-size".into(), format!("task는 {}자 이내입니다", LIMITS.max_task_chars));
            }
        }
        _ => fail(found, "plan-shape".into(), "task가 필요합니다".into()),
    }
    if let Some(model) = raw.get("model") {
        if model.as_str().is_none_or(|m| !MODEL.is_match(m)) {
            fail(found, "plan-shape".into(), "model 이름 형식이 올바르지 않습니다".into());
        }
    }
    if let Some(effort) = raw.get("effort") {
        if effort.as_str().is_none_or(|e| !EFFORTS.contains(&e)) {
            fail(found, "plan-shape".into(), format!("effort는 {} 중 하나입니다", EFFORTS.join(", ")));
        }
    }
    if let Some(task) = task {
        for hit in find_hits(task, code_rules()) {
            fail(found, format!("task-{}", hit.rule), hit.reason.to_string());
        }
    }
    if !ok {
        return None;
    }
    let text = |key: &str| raw.get(key).and_then(Value::as_str).map(str::to_string);
    Some(Card {
        id,
        provider: provider?,
        role: raw.get("role").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty()).map(str::to_string),
        task: task?.to_string(),
        model: text("model"),
        effort: text("effort"),
    })
}

/// The parties of a team message: sender, receiver and the run's card ids.
pub struct Parties<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub known: &'a [String],
}

/// A team message passes the same credential, backend and bypass rules as a task.
pub fn check_message(text: &str, parties: &Parties) -> Vec<Violation> {
    let mut found = Found::default();
    if text.trim().is_empty() {
        found.add("msg-shape", "빈 메시지는 보낼 수 없습니다", None);
    }
    if js_len(text) > LIMITS.max_message_chars {
        found.add("msg-size", format!("메시지는 {}자 이내입니다", LIMITS.max_message_chars), None);
    }
    for party in [parties.from, parties.to] {
        if !(party == "lead" || party == "all" || parties.known.iter().any(|k| k == party)) {
            found.add("msg-party", format!("이 실행에 없는 상대입니다: {party}"), None);
        }
    }
    for hit in find_hits(text, code_rules()) {
        found.add(format!("msg-{}", hit.rule), hit.reason, None);
    }
    found.0
}

/// No provider has anything running.
pub fn empty_counts() -> HashMap<ProviderId, usize> {
    ProviderId::ALL.into_iter().map(|p| (p, 0)).collect()
}
