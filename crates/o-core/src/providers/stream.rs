//! Pieces every stream reader shares: token counts, short tool summaries and
//! limit wording.

use std::sync::LazyLock;

use chrono::{DateTime, Duration, Utc};
use regex::Regex;
use serde_json::Value;

use crate::run::events::StreamSignal;
use crate::sys::js::{as_number, coalesce, iso, js_head, js_len, js_string, parse_time, squash_spaces};

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

/// How long the wording says to wait: "try again in 1h 30m", "retry after 120 seconds",
/// "resets in 20 minutes", or a `Retry-After: 120` header (seconds).
static WAIT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:try again|retry|resets?|available again)\s+(?:in|after)\s+(?P<wait>\d.*)|retry-after\s*:\s*(?P<secs>\d+)\b").unwrap()
});

/// One "<number><unit>" piece at the start of a wait, after an optional comma or "and".
static WAIT_PART: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[\s,]*(?:and\s+)?(\d+(?:\.\d+)?)\s*([a-z]+)").unwrap());

/// The time the wording names itself: an RFC 3339 time with its zone, or epoch seconds,
/// right after "resets at", "try again at", "until" and the like. A time with no zone
/// ("3pm") would be a guess, so it is not read.
static RESET_AT_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r#"(?i)(?:resets?(?:[_ ]?at)?|try again (?:at|after)|retry (?:at|after)|available (?:again )?(?:at|after)|until)["'\s:=]*"#,
        r"(?:(?P<iso>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2}))|(?P<epoch>\d{10})\b)",
    ))
    .unwrap()
});

/// Longer than the weekly window plus a day of slack is not a plan limit.
const LONGEST_WAIT_MS: f64 = 8.0 * 86_400_000.0;

fn unit_millis(unit: &str) -> Option<f64> {
    Some(match unit.to_ascii_lowercase().as_str() {
        "ms" | "millisecond" | "milliseconds" => 1.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000.0,
        "d" | "day" | "days" => 86_400_000.0,
        _ => return None,
    })
}

/// "1h 30m", "3 days 2 hours", "1m30.5s", "120 seconds" in milliseconds. Reading stops at the
/// first piece that is not a time ("and 3 more tries"); a first piece with no known unit
/// ("5 months") reads nothing.
fn wait_millis(mut rest: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut parts = 0;
    while let Some(found) = WAIT_PART.captures(rest) {
        let Some(unit) = unit_millis(&found[2]) else { break };
        total += found[1].parse::<f64>().ok()? * unit;
        parts += 1;
        rest = &rest[found.get(0)?.end()..];
    }
    (parts > 0 && total.is_finite()).then_some(total)
}

/// When the limit text says the provider is usable again, or `None` when it names no time
/// it can be sure of. A time already past, or further off than a weekly
/// window, is not believed.
pub(super) fn reset_from_text(text: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let waited = WAIT_TEXT.captures_iter(text).find_map(|found| {
        let ms = match (found.name("wait"), found.name("secs")) {
            (Some(wait), _) => wait_millis(wait.as_str())?,
            (None, Some(secs)) => secs.as_str().parse::<f64>().ok()? * 1_000.0,
            (None, None) => return None,
        };
        (ms > 0.0 && ms <= LONGEST_WAIT_MS).then(|| now + Duration::milliseconds(ms.ceil() as i64))
    });
    let named = || {
        RESET_AT_TEXT.captures_iter(text).find_map(|found| match (found.name("iso"), found.name("epoch")) {
            (Some(at), _) => parse_time(at.as_str()),
            (None, Some(secs)) => DateTime::from_timestamp(secs.as_str().parse().ok()?, 0),
            (None, None) => None,
        })
    };
    waited.or_else(named).filter(|at| *at > now && *at <= now + Duration::milliseconds(LONGEST_WAIT_MS as i64))
}

pub(super) fn limit_from_text(text: &str, ok: bool) -> Vec<StreamSignal> {
    limit_from_text_at(text, ok, Utc::now())
}

fn limit_from_text_at(text: &str, ok: bool, now: DateTime<Utc>) -> Vec<StreamSignal> {
    if ok || !is_limit_text(text) {
        return Vec::new();
    }
    // No reset time in the wording: empty, and the default cooldown applies.
    let until = reset_from_text(text, now).map(iso).unwrap_or_default();
    vec![StreamSignal::Limit { until }]
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

/// Why a tool call failed, as one line of at most 300 characters.
pub(super) fn error_text(text: &str) -> String {
    let line = squash_spaces(text);
    if js_len(&line) > 300 { format!("{}...", js_head(&line, 297)) } else { line }
}

/// One line of at most 120 characters.
pub(super) fn short(text: &str) -> String {
    let line = squash_spaces(text);
    if js_len(&line) > 120 { format!("{}...", js_head(&line, 117)) } else { line }
}

#[cfg(test)]
mod reset_from_text_tests {
    use super::{limit_from_text_at, reset_from_text};
    use crate::run::events::StreamSignal;
    use chrono::{DateTime, Duration, Utc};

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-11-01T09:00:00Z").unwrap().with_timezone(&Utc)
    }

    fn wait(text: &str) -> Option<Duration> {
        reset_from_text(text, now()).map(|at| at - now())
    }

    #[test]
    fn waits_the_wording_gives_are_added_to_now() {
        assert_eq!(wait("Rate limit hit. Please try again in 20 minutes."), Some(Duration::minutes(20)));
        assert_eq!(wait("You've hit your usage limit. Try again in 1h 30m."), Some(Duration::minutes(90)));
        assert_eq!(wait("rate limit reached, retry after 1m30s"), Some(Duration::seconds(90)));
        assert_eq!(wait("usage limit; try again in 3 days 2 hours 5 minutes"), Some(Duration::minutes(3 * 1440 + 125)));
        assert_eq!(wait("429 Too Many Requests\nRetry-After: 120"), Some(Duration::seconds(120)));
        assert_eq!(wait("rate limit: resets in 1.5 hours and 3 more tries"), Some(Duration::minutes(90)));
    }

    #[test]
    fn times_the_wording_names_with_their_zone_are_read() {
        assert_eq!(
            reset_from_text("usage limit reached, resets at 2026-11-01T12:30:00Z", now()),
            Some(DateTime::parse_from_rfc3339("2026-11-01T12:30:00Z").unwrap().with_timezone(&Utc))
        );
        // +09:00 is 09:30 UTC.
        assert_eq!(wait("rate limit. Try again at 2026-11-01T18:30:00+09:00"), Some(Duration::minutes(30)));
        assert_eq!(wait(r#"{"error":"rate limit","resets_at":"2026-11-01T10:00:00.000Z"}"#), Some(Duration::hours(1)));
        // 1793523600 is 2026-11-01T09:00:00Z.
        assert_eq!(wait("usage limit, resets at 1793527200"), Some(Duration::hours(1)));
    }

    #[test]
    fn a_limit_text_with_a_wait_carries_its_reset_time() {
        assert_eq!(
            limit_from_text_at("Claude AI usage limit reached. Try again in 1 hour.", false, now()),
            [StreamSignal::Limit { until: "2026-11-01T10:00:00.000Z".into() }]
        );
        assert_eq!(limit_from_text_at("Claude AI usage limit reached", false, now()), [StreamSignal::Limit { until: String::new() }]);
        assert_eq!(limit_from_text_at("Claude AI usage limit reached. Try again in 1 hour.", true, now()), []);
    }

    #[test]
    fn a_time_it_cannot_be_sure_of_is_left_empty() {
        for text in [
            "usage limit reached, try again at 3pm",
            "usage limit reached, resets at 2026-11-01T12:30:00",
            "rate limit, try again in 5 months",
            "rate limit, try again in a while",
            "rate limit exceeded",
            // Past, now, and further off than a weekly window.
            "rate limit, resets at 2026-10-31T12:00:00Z",
            "rate limit, resets at 2026-11-01T09:00:00Z",
            "rate limit, resets at 1793437200",
            "rate limit, try again in 0 seconds",
            "rate limit, try again in 30 days",
            "rate limit, retry-after: 99999999999",
            // A request time is not a reset time.
            "rate limit at 2026-11-01T09:00:00Z",
        ] {
            assert_eq!(reset_from_text(text, now()), None, "{text}");
        }
    }
}
