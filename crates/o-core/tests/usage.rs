//! Port of tests/usage.test.ts.

use o_core::sys::js::iso_from_secs;
use o_core::run::plan::ProviderId;
use o_core::usage::{parse_agy_usage, parse_codex_rate_limits, UsageWindow};
use serde_json::json;

// Trimmed from real answers on 2026-11-02 (codex-cli 0.166.0, agy 1.4.8); account ids removed.
#[test]
fn codex_app_server_rate_limits_become_windows() {
    let result = json!({
        "ordinaryUsageAllowed": true,
        "rateLimits": { "limitId": "codex", "primary": { "usedPercent": 57, "windowDurationMins": 10080, "resetsAt": 1793889279 }, "secondary": null, "planType": "prolite" },
    });
    assert_eq!(
        parse_codex_rate_limits(&result),
        [UsageWindow { provider: ProviderId::Codex, window: "seven_day".into(), group: None, used: 0.57, resets_at: iso_from_secs(1793889279.0) }]
    );
    assert_eq!(parse_codex_rate_limits(&json!({})), []);
}

#[test]
fn agy_usage_groups_become_windows_per_model_group() {
    let stdout = json!({
        "status": "SUCCESS",
        "num_turns": 0,
        "command": {
            "name": "usage",
            "data": {
                "groups": [
                    {
                        "name": "Gemini Models",
                        "buckets": [
                            { "id": "gemini-weekly", "window": "weekly", "remaining_fraction": 0.9982682466506958, "reset_time": "2026-11-08T09:17:28Z" },
                            { "id": "gemini-5h", "window": "5h", "remaining_fraction": 1, "reset_time": "2026-11-01T19:17:28Z" },
                        ],
                    },
                    { "name": "Claude and GPT models", "buckets": [{ "id": "3p-weekly", "window": "weekly", "remaining_fraction": 0.49, "reset_time": "2026-11-04T04:25:49Z" }] },
                ],
            },
        },
    })
    .to_string();
    let windows = parse_agy_usage(&stdout);
    assert_eq!(windows.len(), 3);
    assert_eq!(
        windows[2],
        UsageWindow {
            provider: ProviderId::Agy,
            window: "seven_day".into(),
            group: Some("Claude and GPT models".into()),
            used: 0.51,
            resets_at: "2026-11-04T04:25:49Z".into()
        }
    );
    assert!((windows[0].used - 0.0017).abs() < 0.0001);
    assert_eq!(parse_agy_usage("not json"), []);
}

#[test]
fn a_reading_is_written_as_the_app_reads_it() {
    let window = UsageWindow { provider: ProviderId::Agy, window: "five_hour".into(), group: None, used: 0.0, resets_at: "x".into() };
    assert_eq!(serde_json::to_string(&window).unwrap(), r#"{"provider":"agy","window":"five_hour","used":0,"resetsAt":"x"}"#);
}
