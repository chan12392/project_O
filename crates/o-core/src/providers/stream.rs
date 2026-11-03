//! Pieces every stream reader shares: token counts, short tool summaries and
//! limit wording.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::run::events::StreamSignal;
use crate::sys::js::{as_number, coalesce, js_head, js_len, js_string, squash_spaces};

pub(super) fn num(value: Option<&Value>) -> f64 {
    as_number(value).filter(|n| n.is_finite()).unwrap_or(0.0)
}

pub(super) fn reported(values: &[Option<&Value>]) -> bool {
    values.iter().all(|value| value.and_then(Value::as_f64).is_some_and(|n| n.is_finite() && n >= 0.0))
}

/// How the CLIs and APIs say a plan or rate limit stopped the turn. A bare "429"
/// is not enough: file names, line numbers and sizes contain it too.
static LIMIT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)rate[ _-]?limit|usage[ _-]?limit|quota|resource[_ ]exhausted|too many requests|",
        r"(?:http|status|error|code)[ :]*429\b|credit balance|out of credits|",
        r"exceeded your (?:current )?(?:quota|usage|plan|limit)|hit your (?:usage )?limit|weekly limit|5-hour limit|session limit|",
        r"사용량 한도|사용 한도|요청 한도|한도(?:를)? ?초과|한도에 (?:도달|걸)|用量上限|配额|使用上限|利用上限|レート制限",
    ))
    .unwrap()
});

/// Size limits say "limit" too (context window, output tokens, file size), but the
/// same request would fail again later, so they must not rest the provider.
static SIZE_LIMIT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)context (?:window|length)|max[_ ]?tokens|maximum context|output tokens?|too long|file size|upload|",
        r"크기 한도|길이 한도|파일 크기|출력 토큰|(?:^|[^사])용량 한도|업로드|컨텍스트|上下文|コンテキスト",
    ))
    .unwrap()
});

/// A per-minute limit ("8,000 output tokens per minute") is a rate limit even
/// though it names output tokens.
static PER_MINUTE_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)per[- ](?:1 |one )?min(?:ute)?\b|(?:tokens?|requests?|reqs?)\s*/\s*min(?:ute)?\b|\b[io]?tpm\b|분당|每分钟|毎分|1分あたり").unwrap()
});

/// A plan or rate limit, not a size limit.
pub fn is_limit_text(text: &str) -> bool {
    LIMIT_TEXT.is_match(text) && (PER_MINUTE_TEXT.is_match(text) || !SIZE_LIMIT_TEXT.is_match(text))
}

pub(super) fn limit_from_text(text: &str, ok: bool) -> Vec<StreamSignal> {
    if ok || !is_limit_text(text) {
        return Vec::new();
    }
    vec![StreamSignal::Limit { until: String::new() }]
}

pub(super) fn tool_summary(input: Option<&Value>) -> String {
    let Some(input) = input.and_then(Value::as_object) else { return String::new() };
    let path = coalesce([input.get("file_path"), input.get("Path"), input.get("TargetFile")]);
    // Paths keep their last two segments; the project folder is already known.
    if let Some(Value::String(path)) = path {
        let normalized = path.replace('\\', "/");
        let parts: Vec<&str> = normalized.split('/').collect();
        return short(&parts[parts.len().saturating_sub(2)..].join("/"));
    }
    let detail = coalesce([input.get("command"), input.get("CommandLine"), input.get("pattern"), input.get("url"), input.get("description")]);
    short_value(detail)
}

pub(super) fn short_value(value: Option<&Value>) -> String {
    match value {
        None => String::new(),
        Some(v) => short(&js_string(v)),
    }
}

/// One line of at most 120 characters.
pub(super) fn short(text: &str) -> String {
    let line = squash_spaces(text);
    if js_len(&line) > 120 { format!("{}...", js_head(&line, 117)) } else { line }
}
