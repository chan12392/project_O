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
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sys::js::{js_len, parse_time};
use crate::run::mode::{Profile, RunMode};
use crate::run::plan::{Card, Plan, ProviderId, CARD_FIELDS, PLAN_FIELDS};

pub struct Limits {
    pub max_cards_per_plan: usize,
    pub max_task_chars: usize,
    pub max_message_chars: usize,
    pub max_active_total: usize,
    /// When a provider reports a limit without a reset time.
    pub default_cooldown_ms: i64,
}

impl Limits {
    pub const fn max_active_per_provider(&self, provider: ProviderId) -> usize {
        match provider {
            ProviderId::Claude => 3,
            ProviderId::Codex => 3,
            ProviderId::Agy => 2,
            // ADR-0015: no usage probe or daily budget, so no more than Antigravity.
            ProviderId::OpenCode => 2,
            // A new CLI whose steps O sees only after the card ends (ADR-0016).
            ProviderId::Devin => 2,
        }
    }
}

pub const LIMITS: Limits = Limits {
    max_cards_per_plan: 8,
    max_task_chars: 20_000,
    max_message_chars: 4_000,
    max_active_total: 6,
    default_cooldown_ms: 60 * 60 * 1000,
};

/// The effort ladder, lowest first (the daily budget lowers along it, see `budget`).
pub const EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];
pub const AGY_UNAVAILABLE: &str = "Antigravity는 제3자 앱을 통한 사용에 관한 Google 정책 때문에 Project O에서 실행할 수 없습니다. Claude 또는 Codex를 선택하세요";
pub const AGY_WORKERS_DISABLED: &str = "Antigravity 작업 에이전트가 꺼져 있습니다. 설정 → 에이전트 → Antigravity 또는 o agy --workers on --ack에서 켜세요";
static MODEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9._:\-/\[\]]{0,79}$").unwrap());
static CARD_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,31}$").unwrap());
/// Team-channel names a card must not take: its messages would read as the lead's or as a broadcast.
const RESERVED_CARD_IDS: [&str; 3] = ["lead", "all", WIKI_PARTY];

/// The team-message recipient that is nobody's inbox: what a card sends there is a Wiki note (ADR-0041).
pub const WIKI_PARTY: &str = "wiki";
static ABSOLUTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z]:[\\/]").unwrap());

/// Whether a model id has the shape a card's `model` field takes (the lead's `--model` uses it too).
pub fn is_model_id(model: &str) -> bool {
    MODEL.is_match(model)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    pub rule: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card_id: Option<String>,
}

/// What the machine is doing right now, read from O's state directory.
pub struct PolicyContext {
    pub now: DateTime<Utc>,
    pub active_by_provider: HashMap<ProviderId, usize>,
    /// Provider → ISO time until which it may not be used.
    pub cooldowns: HashMap<ProviderId, String>,
    /// Provider → why it spent today's usage budget (ADR-0004, see `budget`).
    pub over_budget: HashMap<ProviderId, String>,
    pub project_exists: Box<dyn Fn(&str) -> bool>,
    /// Whether the folder is a project_O protected-branch (test_1/test_1_hotfix/test_2/main_hotfix/main) mirror checkout.
    pub is_mirror: Box<dyn Fn(&str) -> bool>,
    /// O's state folder: a project may neither be inside it nor contain it.
    pub o_home: String,
    /// The user's home: a project may be inside it, never be it or above it.
    pub user_home: String,
    /// The folder a path names on disk (links, junctions and short names
    /// resolved), or the path itself when it cannot be resolved.
    pub resolve: Box<dyn Fn(&str) -> String>,
    /// Why a provider's project_O profile cannot do a sandboxed card's work yet
    /// (`agents::clean::profile_not_ready`, ADR-0040); `None` when it can.
    pub profile_not_ready: Box<dyn Fn(ProviderId) -> Option<String>>,
    pub agy_workers_allowed: bool,
}

pub struct Checked {
    pub plan: Option<Plan>,
    pub violations: Vec<Violation>,
    pub notices: Vec<String>,
}

/// Whether `path` is `dir` or inside it, compared as Windows does (any slash, any
/// case) after `.` and `..` are resolved, so `C:/Users/me/x/..` is `C:/Users/me`.
fn within(path: &str, dir: &str) -> bool {
    let (p, d) = (lexical(path), lexical(dir));
    p == d || p.starts_with(&format!("{d}/"))
}

/// A path with every slash as `/`, in lower case, with `.` and `..` resolved by
/// text alone: no disk access, and `..` never climbs above the drive or the root.
fn lexical(path: &str) -> String {
    let lowered = path.replace('\\', "/").to_lowercase();
    let mut parts: Vec<&str> = Vec::new();
    for (i, part) in lowered.split('/').enumerate() {
        match part {
            "" if i > 0 => {}
            "." => {}
            ".." => {
                // The first part is the root: "" for "/…" or a drive such as "c:".
                if parts.len() > 1 || parts.first().is_some_and(|p| !p.is_empty() && !p.ends_with(':')) {
                    parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// A card may write inside its project: O's own state and the home folder (CLI
/// settings) must stay out of reach, however either path is spelled.
fn reaches_protected(project: &str, ctx: &PolicyContext) -> bool {
    let mut o_homes = vec![ctx.o_home.clone(), (ctx.resolve)(&ctx.o_home)];
    for sibling in o_siblings(&ctx.o_home) {
        o_homes.push((ctx.resolve)(&sibling));
        o_homes.push(sibling);
    }
    let user_homes = [ctx.user_home.clone(), (ctx.resolve)(&ctx.user_home)];
    [project.to_string(), (ctx.resolve)(project)]
        .iter()
        .any(|p| o_homes.iter().any(|h| within(p, h) || within(h, p)) || user_homes.iter().any(|u| within(u, p)))
}

/// The folders O keeps beside its state folder, as off-limits as the state folder itself: the
/// agent profiles (each CLI's sign-in), lead attachments and plans, terminal card tasks and
/// card worktrees. A card whose project is one of them would be handed a copy of it.
fn o_siblings(o_home: &str) -> Vec<String> {
    let home = o_home.replace('\\', "/");
    let Some((parent, name)) = home.trim_end_matches('/').rsplit_once('/') else { return Vec::new() };
    let mut names = vec!["project_O-agents".to_string()];
    names.extend(["agents", "lead", "term", "worktrees"].map(|kind| format!("{name}-{kind}")));
    names.dedup();
    names.into_iter().map(|name| format!("{parent}/{name}")).collect()
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
        return Checked { plan: None, violations: found.0, notices: Vec::new() };
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
    found.0.extend(check_project(project, ctx));
    let cards = match object.get("cards").and_then(Value::as_array) {
        Some(cards) if !cards.is_empty() => cards,
        _ => {
            found.add("plan-shape", "cards가 1개 이상 필요합니다", None);
            return Checked { plan: None, violations: found.0, notices: Vec::new() };
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
        let mut seen = HashSet::new();
        for dependency in &card.depends_on {
            if dependency == &card.id || !ids.contains(dependency) || !seen.insert(dependency) {
                found.add("plan-dependency", format!("{}의 선행 작업이 올바르지 않습니다: {dependency}", card.id), Some(&card.id));
            }
        }
    }
    let mut resolved = HashSet::new();
    loop {
        let before = resolved.len();
        for card in &valid {
            if card.depends_on.iter().all(|id| resolved.contains(id)) {
                resolved.insert(card.id.clone());
            }
        }
        if before == resolved.len() { break; }
    }
    if resolved.len() != valid.len() {
        found.add("plan-dependency", "선행 작업을 해결할 수 없습니다. 순환이나 없는 작업을 확인하세요", None);
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
        if let Some(reason) = ctx.over_budget.get(&card.provider) {
            found.add("daily-budget", reason.clone(), Some(&card.id));
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
    if !found.0.is_empty() {
        return Checked { plan: None, violations: found.0, notices: Vec::new() };
    }
    let plan = Plan { title: title.unwrap_or_default().trim().to_string(), project: project.unwrap_or_default().to_string(), cards: valid };
    Checked { plan: Some(plan), violations: Vec::new(), notices: Vec::new() }
}

/// `check_plan` for a sandboxed run on `profile`.
pub fn check_plan_for(input: &Value, ctx: &PolicyContext, profile: Profile) -> Checked {
    check_plan_in(input, ctx, RunMode::of(profile))
}

/// `check_plan_for` in the run's mode: on the project_O profile with YOLO off, a card whose
/// CLI's profile still needs its one-time setup (`ctx.profile_not_ready`, Codex's Windows
/// sandbox) is refused too (rule `profile-not-ready`, ADR-0040): it would end "done" having
/// changed nothing. YOLO cards run without the CLI's sandbox and are not refused for it.
pub fn check_plan_in(input: &Value, ctx: &PolicyContext, mode: RunMode) -> Checked {
    let profile = mode.profile;
    let mut checked = check_plan(input, ctx);
    let cards = input.get("cards").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    for card in cards {
        if !ctx.agy_workers_allowed && card.get("provider").and_then(Value::as_str) == Some("agy") {
            checked.plan = None;
            checked.violations.push(Violation { rule: "provider-terms".into(), message: AGY_WORKERS_DISABLED.into(), card_id: card.get("id").and_then(Value::as_str).map(str::to_string) });
        }
    }
    let named: Vec<_> = cards.iter().filter_map(|c| Some((c.get("id").and_then(Value::as_str).unwrap_or("?"), ProviderId::parse(c.get("provider")?.as_str()?)?))).collect();
    if cards.iter().any(|card| card.get("provider").and_then(Value::as_str).and_then(ProviderId::parse).is_some_and(|provider| crate::providers::get(provider).terminal_card()) && (card.get("model").is_some_and(|v| !v.is_null()) || card.get("effort").is_some_and(|v| !v.is_null()))) {
        checked.notices.push("터미널 카드는 model·effort를 적용하지 않습니다(CLI의 현재 설정으로 실행)".into());
    }
    if profile.is_clean() && named.iter().any(|(_, provider)| *provider == ProviderId::Devin) {
        checked.notices.push(crate::providers::devin::OWN_PROFILE_NOTICE.into());
    }
    let mut refused = profile_violations(named.iter().copied(), profile);
    if profile.is_clean() && !mode.yolo {
        let mut asked: HashMap<ProviderId, Option<String>> = HashMap::new();
        for (id, provider) in named {
            if let Some(why) = asked.entry(provider).or_insert_with(|| (ctx.profile_not_ready)(provider)) {
                refused.push(Violation { rule: "profile-not-ready".into(), message: why.clone(), card_id: Some(id.to_string()) });
            }
        }
    }
    if !refused.is_empty() {
        checked.plan = None;
        checked.violations.extend(refused);
    }
    checked
}

/// The cards (id, provider) `profile` cannot run, each with its CLI's reason (rule `profile-unsupported`).
pub fn profile_violations<'a>(cards: impl IntoIterator<Item = (&'a str, ProviderId)>, profile: Profile) -> Vec<Violation> {
    if !profile.is_clean() {
        return Vec::new();
    }
    let mut found = Found::default();
    for (id, provider) in cards {
        if provider == ProviderId::Devin { continue; }
        if let Some(why) = crate::providers::get(provider).profile_unsupported() {
            found.add("profile-unsupported", why, Some(id));
        }
    }
    found.0
}

/// Whether a folder may be a project: absolute, existing, not a protected mirror, and
/// neither inside nor around O's state folder or the home folder. A plan's check and the
/// lead chat (which reads the project) both use it.
pub fn check_project(project: Option<&str>, ctx: &PolicyContext) -> Vec<Violation> {
    let mut found = Found::default();
    match project {
        Some(project) if is_absolute(project) => {
            if !(ctx.project_exists)(project) {
                found.add("plan-shape", format!("project 폴더가 없습니다: {project}"), None);
            } else if (ctx.is_mirror)(project) {
                found.add(
                    "read-only-project",
                    format!("{project}는 project_O의 보호 브랜치(test_1·test_1_hotfix·test_2·main_hotfix·main) 거울 폴더라 작업을 보낼 수 없습니다. 개인 브랜치 폴더를 지정하세요"),
                    None,
                );
            } else if reaches_protected(project, ctx) {
                found.add(
                    "protected-project",
                    format!("{project}는 O의 상태 폴더(와 그 옆의 프로필·총괄·터미널·작업 사본 폴더)나 홈 폴더를 포함해 작업 폴더로 쓸 수 없습니다. 프로젝트 폴더를 지정하세요"),
                    None,
                );
            }
        }
        _ => found.add("plan-shape", "project는 절대 경로여야 합니다", None),
    }
    found.0
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
    } else if RESERVED_CARD_IDS.contains(&id.as_str()) {
        fail(found, "plan-shape".into(), format!("카드 id {id}는 팀 대화의 총괄(lead)·전체 방송(all)·Wiki 메모(wiki)용으로 예약돼 있어 쓸 수 없습니다"));
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
    if let Some(dependencies) = raw.get("depends_on") {
        if dependencies.as_array().is_none_or(|items| items.iter().any(|id| id.as_str().is_none_or(|s| !CARD_ID.is_match(s)))) {
            fail(found, "plan-dependency".into(), "depends_on은 선행 작업 id의 배열이어야 합니다".into());
        }
    }
    let resources = match raw.get("resources") {
        None => Vec::new(),
        Some(value) => match serde_json::from_value::<Vec<crate::run::plan::Resource>>(value.clone()) {
            Ok(items) if items.len() <= 64 && items.iter().all(|r| {
                if r.key.trim().is_empty() || r.key.len() > 1024 || r.key.chars().any(char::is_control) { return false; }
                if r.kind != crate::run::plan::ResourceKind::ProjectPath { return true; }
                let key = r.key.replace('\\', "/");
                std::path::Path::new(&key).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir))
            }) => items,
            _ => {
                fail(found, "plan-resource".into(), "resources는 유효한 kind·key·mode 객체의 배열이어야 합니다".into());
                Vec::new()
            }
        },
    };
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
        depends_on: raw.get("depends_on").and_then(Value::as_array).map(|ids| ids.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
        resources,
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
    for (party, receives) in [(parties.from, false), (parties.to, true)] {
        // A run planned before "wiki" was reserved may have a card of that name; it keeps its inbox.
        if !(party == "lead" || party == "all" || (receives && party == WIKI_PARTY) || parties.known.iter().any(|k| k == party)) {
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
