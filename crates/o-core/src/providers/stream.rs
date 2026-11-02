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

static LIMIT_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)usage limit|rate limit|quota|resource[_ ]exhausted|too many requests|429").unwrap());

pub(super) fn limit_from_text(text: &str, ok: bool) -> Vec<StreamSignal> {
    if ok || !LIMIT_TEXT.is_match(text) {
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
