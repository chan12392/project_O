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
}

/// The role, rules and plan format, sent once per CLI session.
fn role(project: &str, budget: &[String]) -> String {
    let providers = ProviderId::ALL.map(ProviderId::as_str).join("|");
    let caps: Vec<String> = ProviderId::ALL.iter().map(|p| format!("{p} {}", LIMITS.max_active_per_provider(*p))).collect();
    let mut lines = vec![
        format!("[O 총괄] 프로젝트: {project}"),
        "당신은 이 프로젝트의 총괄입니다. 파일은 읽기만 하고 고치거나 명령을 실행하지 않습니다.".to_string(),
        "요청을 이해하고, 모호하면 짧게 되묻고, 일을 작업카드로 나눠 제안합니다. 실행은 사람이 '실행'을 눌러야 O가 합니다.".to_string(),
        String::new(),
        "카드 나누기:".to_string(),
        format!("- 1~5장(최대 {}장), 같은 파일을 두 카드가 고치지 않게 나눕니다. task에 목표·범위(파일)·완료 기준을 씁니다.", LIMITS.max_cards_per_plan),
        format!("- 카드마다 provider({providers})·model·effort({})를 일에 맞게 고릅니다. 모델이 확실하지 않으면 model을 비웁니다.", EFFORTS.join("|")),
        format!("- 동시 실행 상한: {}. 한도·예산에 걸린 서비스는 피합니다.", caps.join(", ")),
        format!("- 허용 필드: {}. id는 영문 소문자·숫자·하이픈. 실행 파일·인자·환경변수·계정은 넣지 않습니다.", CARD_FIELDS.join(", ")),
    ];
    if !budget.is_empty() {
        lines.push("오늘 사용 예산:".to_string());
        lines.extend(budget.iter().map(|b| format!("- {b}")));
    }
    lines.extend([
        String::new(),
        "답은 마크다운으로 씁니다. 작업을 제안할 때만 답 끝에 아래 블록을 정확히 하나 넣고, 블록 앞에 카드마다 그 에이전트·모델을 고른 이유를 한 줄씩 적습니다. project는 O가 채웁니다.".to_string(),
        format!("```{PLAN_FENCE}"),
        r#"{"title":"짧은 제목","cards":[{"id":"ui","provider":"codex","effort":"medium","role":"코딩","task":"무엇을, 어느 파일에서, 완료 기준"}]}"#.to_string(),
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
        parts.push(role(input.project, input.budget));
        if !input.history.is_empty() {
            let start = input.history.len().saturating_sub(HISTORY_TURNS);
            let lines: Vec<String> = input.history[start..].iter().map(|(who, text)| format!("{who}: {}", clip(text.trim(), HISTORY_CHARS))).collect();
            parts.push(format!("지금까지의 대화(요약):\n{}", lines.join("\n")));
        }
    } else {
        if !input.budget.is_empty() {
            parts.push(format!("오늘 사용 예산:\n{}", input.budget.iter().map(|b| format!("- {b}")).collect::<Vec<_>>().join("\n")));
        }
        if !input.missed.is_empty() {
            let start = input.missed.len().saturating_sub(MISSED_TURNS);
            let lines: Vec<String> = input.missed[start..].iter().map(|(who, text)| format!("{who}: {}", clip(text.trim(), HISTORY_CHARS))).collect();
            parts.push(format!("그사이 다른 에이전트가 총괄로 나눈 대화(이 세션은 못 봄):\n{}", lines.join("\n")));
        }
    }
    if !input.runs.is_empty() {
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

/// The answer without its plan block, and the block's JSON: `Ok(None)` when there is
/// no block, an error when a block is not JSON or there is more than one.
pub fn split_plan(text: &str) -> (String, Result<Option<Value>, String>) {
    let open = format!("```{PLAN_FENCE}");
    let mut prose = Vec::new();
    let mut blocks: Vec<String> = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in text.lines() {
        match current.as_mut() {
            Some(body) if line.trim() == "```" => {
                blocks.push(body.join("\n"));
                current = None;
            }
            Some(body) => body.push(line),
            None if line.trim() == open => current = Some(Vec::new()),
            None => prose.push(line),
        }
    }
    // An unclosed block still counts as the lead's plan, cut off.
    if let Some(body) = current {
        blocks.push(body.join("\n"));
    }
    let prose = prose.join("\n").trim().to_string();
    let plan = match blocks.as_slice() {
        [] => Ok(None),
        [one] => serde_json::from_str::<Value>(one.trim())
            .map_err(|e| format!("계획 블록이 올바른 JSON이 아니라 받지 않았습니다: {e}"))
            .and_then(|v| if v.is_object() { Ok(Some(v)) } else { Err("계획 블록은 JSON 객체여야 합니다".to_string()) }),
        _ => Err(format!("계획 블록이 {}개라 받지 않았습니다. 한 번에 하나만 제안해 달라고 다시 요청하세요", blocks.len())),
    };
    (prose, plan)
}
