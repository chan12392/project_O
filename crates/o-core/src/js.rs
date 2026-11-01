//! The few JavaScript semantics O's on-disk formats and messages depend on:
//! ISO timestamps as `Date#toISOString` writes them, `String(value)`, string
//! lengths in UTF-16 units and numbers written without a trailing `.0`.

use chrono::{DateTime, Local, SecondsFormat, TimeZone, Utc};
use serde::Serializer;
use serde_json::Value;

/// `2026-11-01T10:00:00.000Z`, as `Date#toISOString`.
pub fn iso(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn now_iso() -> String {
    iso(Utc::now())
}

/// `new Date(seconds * 1000).toISOString()`; empty when out of range.
pub fn iso_from_secs(seconds: f64) -> String {
    let ms = (seconds * 1000.0).trunc();
    if !ms.is_finite() {
        return String::new();
    }
    Utc.timestamp_millis_opt(ms as i64).single().map(iso).unwrap_or_default()
}

/// `Date.parse` for the ISO times O writes and the CLIs report.
pub fn parse_time(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text.trim()).ok().map(|t| t.with_timezone(&Utc))
}

/// The local calendar day, as `getFullYear()-getMonth()-getDate()` compares days.
pub fn local_day(time: DateTime<Utc>) -> chrono::NaiveDate {
    time.with_timezone(&Local).date_naive()
}

/// A number the way JavaScript prints it: integers without a fraction.
pub fn number_text(n: f64) -> String {
    if n.is_nan() {
        "NaN".into()
    } else if n.is_infinite() {
        if n > 0.0 { "Infinity".into() } else { "-Infinity".into() }
    } else if n.fract() == 0.0 && n.abs() < 1e21 {
        format!("{}", n as i128)
    } else {
        format!("{n}")
    }
}

/// `String(value)` for a JSON value.
pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.as_f64().map(number_text).unwrap_or_else(|| n.to_string()),
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(|v| if v.is_null() { String::new() } else { js_string(v) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// `a ?? b ?? …`: the first value that is present and not null.
pub fn coalesce<'a>(values: impl IntoIterator<Item = Option<&'a Value>>) -> Option<&'a Value> {
    values.into_iter().flatten().find(|v| !v.is_null())
}

/// `typeof value === "number"` (finite or not, JSON only has finite ones).
pub fn as_number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

/// `string.length`.
pub fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `string.slice(0, units)`, never splitting a character.
pub fn js_head(text: &str, units: usize) -> &str {
    let mut used = 0;
    for (i, c) in text.char_indices() {
        used += c.len_utf16();
        if used > units {
            return &text[..i];
        }
    }
    text
}

/// `string.slice(-units)`, never splitting a character.
pub fn js_tail(text: &str, units: usize) -> &str {
    let mut used = 0;
    for (i, c) in text.char_indices().rev() {
        used += c.len_utf16();
        if used > units {
            return &text[i + c.len_utf8()..];
        }
    }
    text
}

/// `text.replace(/\s+/g, " ")`, ends kept.
pub fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// `text.replace(/\s+/g, " ").trim()`.
pub fn squash_spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Serializes a JSON number as JavaScript writes it (`1`, not `1.0`).
pub fn ser_num<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
    if value.fract() == 0.0 && value.abs() < 9_007_199_254_740_992.0 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_matches_to_iso_string() {
        assert_eq!(iso_from_secs(1793526000.0), "2026-11-01T09:40:00.000Z");
        assert_eq!(number_text(14721.0), "14721");
        assert_eq!(number_text(0.57), "0.57");
        assert_eq!(js_head("가나다", 2), "가나");
        assert_eq!(js_tail("가나다", 2), "나다");
        assert_eq!(js_string(&serde_json::json!([1, null, "a"])), "1,,a");
    }
}
