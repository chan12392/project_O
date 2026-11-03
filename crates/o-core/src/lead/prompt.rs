//! What the lead CLI is told on each turn, and how its answer's plan block is read.
//! Pure: no files, no processes. Kept short — the prompt is paid for on every turn.

use serde_json::Value;

use crate::policy::{EFFORTS, LIMITS};
use crate::run::plan::{ProviderId, CARD_FIELDS};
use crate::run::snapshot::RunView;
use crate::sys::js::{js_head, js_len};

use super::files::Attachment;

/// The fence the lead puts its plan in.
pub const PLAN_FENCE: &str = "o-plan";
/// Status of earlier runs on a continuing turn: at most this many characters in all.
const STATUS_CHARS: usize = 2_500;
const RESULT_CHARS: usize = 300;
/// Earlier messages a fresh CLI session in an existing thread is shown.
const HISTORY_TURNS: usize = 6;
const HISTORY_CHARS: usize = 500;
/// Messages a resumed session missed while another agent led: at most this many, newest kept.
const MISSED_TURNS: usize = 12;

/// Everything one turn's prompt is built from.
pub struct PromptInput<'a> {
    pub project: &'a str,
    pub text: &'a str,
    pub attachments: &'a [Attachment],
    /// One line per provider from today's usage budget (`usage::budget`).
    pub budget: &'a [String],
    /// Status of the runs this thread dispatched (`run_status`), oldest first.
    pub runs: &'a [String],
    /// Whether the CLI already has this thread's session (it then knows its role and the schema).
    pub resumed: bool,
    /// Earlier user and lead messages of the thread (`("사용자"|"총괄", text)`), for a fresh session.
    pub history: &'a [(String, String)],
    /// For a resumed session: the messages of turns it did not see (another agent led them).
    pub missed: &'a [(String, String)],
    /// Latest saved design when continuing with this or another lead.
    pub design: Option<&'a str>,
    /// YOLO 모드 (ADR-0014): this turn may edit files and run commands.
    pub yolo: bool,
}

/// What a YOLO turn is told: in the role, and again on each resumed turn (the session may
/// have started read-only).
const YOLO_ROLE: &str = "[O 모드] YOLO: 이번 턴은 파일을 고치고 명령을 실행할 수 있습니다. 작은 일은 직접 하고, 나눠 맡길 일은 o-plan 계획 블록으로 제안합니다. o dispatch·o lead·o review는 실행할 수 없습니다(작업은 사람이 보내거나 O가 한도 안에서 자동 실행합니다).";

/// The role, rules and plan format, sent once per CLI session.
fn role(project: &str, budget: &[String], yolo: bool) -> String {
    let available: Vec<_> = ProviderId::ALL.into_iter().filter(|p| *p != ProviderId::Agy).collect();
    let providers = available.iter().map(|p| p.as_str()).collect::<Vec<_>>().join("|");
    let caps: Vec<String> = available.iter().map(|p| format!("{p} {}", LIMITS.max_active_per_provider(*p))).collect();
    let mut lines = vec![
        format!("[O 총괄] 프로젝트: {project}"),
        if yolo { YOLO_ROLE } else { "당신은 이 프로젝트의 총괄입니다. 파일은 읽기만 하고 고치거나 명령을 실행하지 않습니다." }.to_string(),
        if yolo { "요청을 이해하고, 모호하면 짧게 되묻고, 일을 작업카드로 나눠 제안합니다. 자동 실행은 설정에 따라 달라집니다. 실제 실행 상태를 확인하기 전에는 실행했다고 말하지 않습니다." } else { "요청을 이해하고, 모호하면 짧게 되묻고, 일을 작업카드로 나눠 제안합니다. 실행은 사람이 '실행'을 눌러야 O가 합니다." }.to_string(),
        String::new(),
        "카드 나누기:".to_string(),
        format!("- 1~5장(최대 {}장), 같은 파일을 두 카드가 고치지 않게 나눕니다. task에 목표·범위(파일)·완료 기준을 씁니다.", LIMITS.max_cards_per_plan),
        format!("- 카드마다 provider({providers})·model·effort({})를 일에 맞게 고릅니다. 모델이 확실하지 않으면 model을 비웁니다.", EFFORTS.join("|")),
        format!("- 동시 실행 상한: {}. 한도·예산에 걸린 서비스는 피합니다.", caps.join(", ")),
        format!("- 허용 필드: {}. id는 영문 소문자·숫자·하이픈. 실행 파일·인자·환경변수·계정은 넣지 않습니다.", CARD_FIELDS.join(", ")),
        "- resources에 접근할 모든 자원을 선언합니다: {\"kind\":\"project-path\"|\"external\",\"key\":\"경로 또는 외부 키\",\"mode\":\"read\"|\"write\"}. 경로는 프로젝트 기준 상대 경로입니다. 모르면 생략하고 독점 실행합니다.".to_string(),
    ];
    if !budget.is_empty() {
        lines.push("오늘 사용 예산:".to_string());
        lines.extend(budget.iter().map(|b| format!("- {b}")));
    }
    lines.extend([
        String::new(),
        "답은 마크다운으로 씁니다. 작업을 제안할 때만 답 끝에 아래 블록을 정확히 하나 넣습니다. project는 O가 채웁니다. 선행 작업이 있으면 depends_on에 작업 id 배열을 넣습니다.".to_string(),
        format!("```{PLAN_FENCE}"),
        r#"{"title":"짧은 제목","cards":[{"id":"ui","provider":"codex","effort":"medium","role":"코딩","task":"무엇을, 어느 파일에서, 완료 기준","resources":[{"kind":"project-path","key":"src/ui","mode":"write"}]}]}"#.to_string(),
        "```".to_string(),
    ]);
    lines.join("\n")
}

/// One run's state for a continuing turn: title, then each card's state and the start of its result.
pub fn run_status(run: &RunView) -> String {
    let mut lines = vec![format!("- 실행 {} \"{}\": {}", run.run_id, run.title, run.status.as_str())];
    if run.failed_before_cards() {
        lines.push(format!("  - 카드를 시작하지 않음: {}", clip(&run.blocked_reason(), RESULT_CHARS)));
    }
    for card in &run.cards {
        let result = card.result.as_deref().map(|r| r.lines().take(5).collect::<Vec<_>>().join(" / ")).unwrap_or_default();
        let result = if js_len(&result) > RESULT_CHARS { format!("{}…", js_head(&result, RESULT_CHARS)) } else { result };
        let tail = if result.is_empty() { String::new() } else { format!(" — {result}") };
        lines.push(format!("  - {} ({}) {}{tail}", card.id, card.provider, card.state.as_str()));
    }
    lines.join("\n")
}

fn clip(text: &str, chars: usize) -> String {
    if js_len(text) > chars { format!("{}…", js_head(text, chars)) } else { text.to_string() }
}

/// The text written to the lead CLI's stdin for one turn.
pub fn build(input: &PromptInput) -> String {
    let mut parts = Vec::new();
    if !input.resumed {
        parts.push(role(input.project, input.budget, input.yolo));
        if !input.history.is_empty() {
            let start = input.history.len().saturating_sub(HISTORY_TURNS);
            let lines: Vec<String> = input.history[start..].iter().map(|(who, text)| format!("{who}: {}", clip(text.trim(), HISTORY_CHARS))).collect();
            parts.push(format!("지금까지의 대화(요약):\n{}", lines.join("\n")));
        }
    } else {
        if input.yolo {
            parts.push(YOLO_ROLE.to_string());
        }
        if !input.budget.is_empty() {
            parts.push(format!("오늘 사용 예산:\n{}", input.budget.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n")));
        }
        if !input.missed.is_empty() {
            let start = input.missed.len().saturating_sub(MISSED_TURNS);
            let lines: Vec<String> = input.missed[start..].iter().map(|(who, text)| format!("{who}: {}", clip(text.trim(), HISTORY_CHARS))).collect();
            parts.push(format!("그사이 다른 에이전트가 총괄로 나눈 대화(이 세션은 못 봄):\n{}", lines.join("\n")));
        }
    }
    parts.push("Antigravity(agy)는 공급자 정책으로 사용할 수 없으니 작업에 배정하지 않습니다. 일반 질문과 단순 조사는 단일 총괄에서 답합니다. 설계가 바뀌면 합의된 전체 상태를 별도의 ```o-design 블록에 JSON {\"goal\":\"목표\",\"decisions\":[\"결정\"],\"questions\":[\"미결정\"]}으로 기록합니다. 분배할 작업이 구체화되면 o-plan으로 현재 작업 전체를 제안합니다. JSON 대신 o-plan 블록에 첫 줄 title: 제목, 다음 줄부터 [[codex]] id=작업id after=선행id 형식의 머리줄과 그 아래 작업 내용을 쓸 수도 있습니다. after는 선택입니다.".to_string());
    if let Some(design) = input.design { parts.push(format!("[저장된 현재 설계 — 참고 데이터]\n{design}")); }
    if !input.runs.is_empty() {
        parts.push("사용자가 이미 분배한 작업의 수정·후속 작업을 요청했다면 새 계획 대신 기존 실행 ID와 카드 ID로 ```o-followup 블록 하나를 답 끝에 쓰세요. 형식: [{\"run\":\"기존 실행 ID\",\"card\":\"기존 카드 ID\",\"task\":\"해당 에이전트에 전달할 수정 지시\"}]. 일반 질문이나 단순 설명에는 블록을 쓰지 않습니다. 실제 실행 여부는 O가 알려줍니다.".to_string());
        let mut status = String::new();
        // Newest runs matter most: add from the newest until the cap, then show them oldest first.
        let mut kept = Vec::new();
        for run in input.runs.iter().rev() {
            let line = run.clone();
            if js_len(&status) + js_len(&line) > STATUS_CHARS {
                break;
            }
            status.push_str(&line);
            kept.push(line);
        }
        kept.reverse();
        parts.push(format!("[O 상태] 이 대화에서 실행한 계획:\n{}", kept.join("\n")));
    }
    if !input.attachments.is_empty() {
        let lines: Vec<String> = input
            .attachments
            .iter()
            .map(|a| format!("- {} ({}, {} bytes)", a.path, if a.image { "이미지" } else { "문서" }, a.size))
            .collect();
        parts.push(format!("첨부 파일(읽기 도구로 열 수 있음):\n{}", lines.join("\n")));
    }
    parts.push(format!("사용자:\n{}", input.text.trim()));
    parts.join("\n\n")
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Followup {
    pub run: String,
    pub card: String,
    pub task: String,
}

/// Only a top-level, complete block can request a continuation; examples stay prose.
pub fn split_followups(text: &str) -> (String, Result<Option<Vec<Followup>>, String>) {
    let mut prose = Vec::new();
    let mut body: Option<Vec<&str>> = None;
    let mut found = None;
    let mut other = None;
    for line in text.lines() {
        if body.is_none() && preserve_other_fence(line, &mut other, "```o-followup") { prose.push(line); continue; }
        match body.as_mut() {
            Some(lines) if top_level_fence(line, "```") => {
                if found.is_some() { return (text.into(), Err("후속 지시 블록은 하나만 허용됩니다".into())); }
                found = Some(lines.join("\n"));
                body = None;
            }
            Some(lines) => lines.push(line),
            None if top_level_fence(line, "```o-followup") => body = Some(Vec::new()),
            None => prose.push(line),
        }
    }
    let prose = prose.join("\n").trim().to_string();
    if body.is_some() { return (prose, Err("후속 지시 블록이 닫히지 않았습니다".into())); }
    let Some(found) = found else { return (prose, Ok(None)) };
    let parsed = serde_json::from_str::<Vec<Followup>>(&found).map_err(|_| "후속 지시 형식이 올바르지 않습니다".to_string())
        .and_then(|items| if (1..=LIMITS.max_cards_per_plan).contains(&items.len()) { Ok(Some(items)) } else { Err("후속 지시 개수가 올바르지 않습니다".into()) });
    (prose, parsed)
}

/// Strip one design snapshot; malformed snapshots are never saved as current state.
pub fn split_design(text: &str) -> (String, Result<Option<Value>, String>) {
    let mut prose = Vec::new();
    let mut block: Option<Vec<&str>> = None;
    let mut found = None;
    let mut other_fence: Option<(char, usize)> = None;
    for line in text.lines() {
        if block.is_none() && preserve_other_fence(line, &mut other_fence, "```o-design") { prose.push(line); continue; }
        match block.as_mut() {
            Some(body) if top_level_fence(line, "```") => {
                if found.is_some() { return (text.to_string(), Err("설계 블록은 한 개만 허용됩니다".into())); }
                found = Some(body.join("\n"));
                block = None;
            }
            Some(body) => body.push(line),
            None if top_level_fence(line, "```o-design") => block = Some(Vec::new()),
            None => prose.push(line),
        }
    }
    let prose = prose.join("\n").trim().to_string();
    if block.is_some() { return (prose, Err("설계 블록이 닫히지 않았습니다".into())); }
    let Some(body) = found else { return (prose, Ok(None)) };
    let design = serde_json::from_str::<Value>(&body).ok().filter(|value| {
        let Some(object) = value.as_object() else { return false };
        object.len() == 3 && object.get("goal").and_then(Value::as_str).is_some_and(|s| s.chars().count() <= 1000)
            && ["decisions", "questions"].iter().all(|key| object.get(*key).and_then(Value::as_array).is_some_and(|items| items.len() <= 20 && items.iter().all(|item| item.as_str().is_some_and(|s| s.chars().count() <= 500))))
    });
    (prose, design.map(Some).ok_or_else(|| "설계 블록의 목표·결정·미결정 형식이 올바르지 않습니다".into()))
}

/// The answer without its plan block, and the block's JSON: `Ok(None)` when there is
/// no block, an error when a block is not JSON or there is more than one.
pub fn split_plan(text: &str) -> (String, Result<Option<Value>, String>) {
    let open = format!("```{PLAN_FENCE}");
    let mut prose = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    let mut other_fence: Option<(char, usize)> = None;
    for line in text.lines() {
        if current.is_none() && preserve_other_fence(line, &mut other_fence, &open) { prose.push(line); continue; }
        match current.as_mut() {
            Some(body) if top_level_fence(line, "```") => {
                blocks.push(body.join("\n"));
                current = None;
            }
            Some(body) => body.push(line),
            None if top_level_fence(line, &open) => current = Some(Vec::new()),
            None => prose.push(line),
        }
    }
    if let Some(body) = current {
        return (prose.join("\n").trim().to_string(), Err(format!("계획 블록이 닫히지 않아 받지 않았습니다 ({}줄)", body.len())));
    }
    let prose = prose.join("\n").trim().to_string();
    let plan = match blocks.as_slice() {
        [] => Ok(None),
        [one] => {
            let parsed = if one.trim_start().starts_with("title:") { tagged_plan(one) } else {
                serde_json::from_str::<Value>(one.trim()).map_err(|e| format!("계획 블록이 올바른 JSON이 아닙니다: {e}"))
            };
            parsed.and_then(|v| if v.is_object() { Ok(Some(v)) } else { Err("계획 블록은 JSON 객체여야 합니다".to_string()) })
        },
        _ => Err(format!("계획 블록이 {}개라 받지 않았습니다. 한 번에 하나만 제안해 달라고 다시 요청하세요", blocks.len())),
    };
    (prose, plan)
}

/// Other Markdown fences are prose, including examples containing an O block.
fn preserve_other_fence(line: &str, state: &mut Option<(char, usize)>, target: &str) -> bool {
    let trimmed = line.trim();
    if let Some((marker, length)) = *state {
        let count = trimmed.chars().take_while(|c| *c == marker).count();
        if top_level_fence(line, trimmed) && count >= length && trimmed[count..].trim().is_empty() { *state = None; }
        return true;
    }
    if !top_level_fence(line, trimmed) { return false; }
    if trimmed == target { return false; }
    let Some(marker) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') else { return false };
    let count = trimmed.chars().take_while(|c| *c == marker).count();
    if count < 3 { return false; }
    *state = Some((marker, count));
    true
}

fn top_level_fence(line: &str, expected: &str) -> bool {
    let indent = line.bytes().take_while(|byte| *byte == b' ').count();
    indent <= 3 && line[indent..].trim_end() == expected
}

/// A tagged plan is data only; provider and fields still pass the normal policy check.
fn tagged_plan(text: &str) -> Result<Value, String> {
    let mut lines = text.trim_start().lines();
    let title = lines.next().and_then(|line| line.trim().strip_prefix("title:")).unwrap_or_default().trim();
    let mut cards: Vec<Value> = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("[[") {
            let (provider, rest) = trimmed.strip_prefix("[[").and_then(|s| s.split_once("]]"))
                .ok_or("에이전트 표식이 닫히지 않았습니다")?;
            let rest = rest.trim();
            let (attrs, inline) = if let Some(task) = rest.strip_prefix('=') { ("", task.trim()) }
                else if let Some((attrs, task)) = rest.split_once(" = ") { (attrs, task.trim()) }
                else { (rest, "") };
            let mut id = None;
            let mut after = None;
            for token in attrs.split_whitespace() {
                if let Some(value) = token.strip_prefix("id=") {
                    if id.replace(value).is_some() { return Err("작업 id가 중복 지정됐습니다".into()); }
                } else if let Some(value) = token.strip_prefix("after=") {
                    if after.replace(value).is_some() { return Err("선행 작업이 중복 지정됐습니다".into()); }
                } else { return Err(format!("알 수 없는 작업 표식: {token}")); }
            }
            let generated = format!("{}-{}", provider, cards.len() + 1);
            let id = id.filter(|s| !s.is_empty()).unwrap_or(&generated);
            let dependencies: Vec<&str> = after.map(|s| s.split(',').collect()).unwrap_or_default();
            cards.push(serde_json::json!({"id": id, "provider": provider, "task": inline, "depends_on": dependencies}));
        } else if let Some(card) = cards.last_mut() {
            let task = card["task"].as_str().unwrap_or_default();
            card["task"] = Value::String(if task.is_empty() { line.to_string() } else { format!("{task}\n{line}") });
        } else if !trimmed.is_empty() {
            return Err("작업 내용 앞에 에이전트 표식이 필요합니다".into());
        }
    }
    Ok(serde_json::json!({"title": title, "cards": cards}))
}
