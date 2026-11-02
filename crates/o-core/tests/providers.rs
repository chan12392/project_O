//! Port of tests/providers.test.ts.

use std::ffi::OsString;

use o_core::run::events::StreamSignal;
use o_core::sys::js::iso_from_secs;
use o_core::run::plan::{Card, Plan, ProviderId};
use o_core::providers::agy::parse_agy_models;
use o_core::providers::codex::parse_codex_models;
use o_core::providers::{card_prompt, is_limit_text, launch_spec, names_to_remove, unwrap_shell, LaunchSpec, LeadTurn, ModelChoice, StreamReader, TeamChannel};
use o_guard::rules::{code_rules, find_hits};
use regex::Regex;
use serde_json::{json, Value};

fn card(id: &str, provider: ProviderId, role: Option<&str>, task: &str, model: Option<&str>, effort: Option<&str>) -> Card {
    Card { id: id.into(), provider, role: role.map(Into::into), task: task.into(), model: model.map(Into::into), effort: effort.map(Into::into) }
}

fn plan() -> Plan {
    Plan {
        title: "랜딩 개편".into(),
        project: "C:/work/demo".into(),
        cards: vec![
            card("code", ProviderId::Codex, Some("코딩"), "구현", Some("gpt-6.1-sol"), Some("xhigh")),
            card("plan", ProviderId::Claude, Some("설계"), "설계", None, Some("high")),
            card("design", ProviderId::Agy, Some("디자인"), "시안", Some("gemini-3.8-flash-high"), None),
        ],
    }
}

const TEAM: TeamChannel = TeamChannel { run_id: "r1", run_dir: "C:/o/runs/r1" };

fn specs(plan: &Plan) -> Vec<LaunchSpec> {
    plan.cards.iter().map(|c| launch_spec(c, plan, &TEAM)).collect()
}

fn after<'a>(args: &'a [String], flag: &str) -> &'a str {
    &args[args.iter().position(|a| a == flag).unwrap() + 1]
}

fn has(args: &[String], value: &str) -> bool {
    args.iter().any(|a| a == value)
}

#[test]
fn launch_specs_pin_model_and_effort_and_never_add_a_bypass_flag() {
    let p = plan();
    let [codex, claude, agy] = <[LaunchSpec; 3]>::try_from(specs(&p)).unwrap();
    assert_eq!(codex.args[..5], ["exec", "--json", "-s", "workspace-write", "-C"]);
    assert!(has(&codex.args, "model_reasoning_effort=xhigh"));
    assert!(codex.stdin.as_deref().unwrap().contains("구현"));
    assert!(has(&claude.args, "--effort") && !has(&claude.args, "--model"));
    assert_eq!(agy.stdin, None);
    assert!(agy.args.last().unwrap().contains("시안"));
    for spec in [&codex, &claude, &agy] {
        assert!(find_hits(&spec.args.join(" "), code_rules().filter(|r| r.id == "permission-bypass")).is_empty());
    }
}

#[test]
fn the_only_opening_each_cli_gets_is_the_runs_team_channel() {
    let p = plan();
    let [codex, claude, agy] = <[LaunchSpec; 3]>::try_from(specs(&p)).unwrap();
    assert_eq!(after(&claude.args, "--allowedTools"), "Bash(o msg *)");
    assert_eq!(after(&codex.args, "--add-dir"), "C:/o/runs/r1");
    // agy matches whole commands only: two fixed ones, and its outbox lives in the run folder.
    let settings: Value = serde_json::from_str(after(&agy.args, "--settings")).unwrap();
    assert_eq!(settings, json!({ "permissions": { "allow": ["command(o msg read)", "command(o msg post)"] } }));
    assert_eq!(after(&agy.args, "--add-dir"), "C:/o/runs/r1");
    assert!(agy.args.last().unwrap().contains("C:/o/runs/r1/outbox/design.txt"));
    let env: Vec<(&str, &str)> = codex.env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(env, [("O_CARD_ID", "code"), ("O_RUN_ID", "r1")]);
}

#[test]
fn a_card_needs_no_role_and_a_role_never_picks_a_model() {
    let mut free = plan();
    free.cards = vec![card("a", ProviderId::Codex, None, "README 오타 고치기", None, None), plan().cards[1].clone()];
    let prompt = card_prompt(&free.cards[0], &free, &TEAM);
    assert!(prompt.starts_with("[O 작업카드] 랜딩 개편\n"));
    assert!(prompt.contains("- plan: 설계 (claude)"));
    assert!(prompt.contains("o msg send --to"));
    assert!(!has(&launch_spec(&free.cards[0], &free, &TEAM).args, "-m"));
}

fn quota(window: &str, used: f64, resets: f64) -> StreamSignal {
    StreamSignal::Quota { window: window.into(), used, resets_at: iso_from_secs(resets) }
}

// Lines below are trimmed from real headless runs (2026-11-01).
#[test]
fn claude_stream_session_text_tools_limit_and_result() {
    let mut r = StreamReader::new(ProviderId::Claude);
    assert_eq!(r.read(r#"{"type":"system","subtype":"init","session_id":"s1"}"#), [StreamSignal::Session { session_id: "s1".into() }]);
    assert_eq!(r.read(r#"{"type":"system","subtype":"hook_started"}"#), []);
    assert_eq!(
        r.read(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"ok"},{"type":"tool_use","name":"Edit","input":{"file_path":"src/a.ts"}}]}}"#),
        [StreamSignal::Message { text: "ok".into() }, StreamSignal::Tool { name: "Edit".into(), summary: "src/a.ts".into() }]
    );
    assert_eq!(r.read(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1793526000}}"#), []);
    // A warning near the limit must not block the provider for the rest of the window.
    assert_eq!(r.read(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","resetsAt":1793526000}}"#), []);
    assert_eq!(
        r.read(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1793526000}}"#),
        [StreamSignal::Limit { until: "2026-11-01T09:40:00.000Z".into() }]
    );
    assert_eq!(
        r.read(r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#),
        [StreamSignal::Final { ok: true, text: "ok".into() }]
    );
}

#[test]
fn codex_stream_config_warnings_are_not_failures() {
    let mut r = StreamReader::new(ProviderId::Codex);
    assert_eq!(r.read(r#"{"type":"thread.started","thread_id":"t1"}"#), [StreamSignal::Session { session_id: "t1".into() }]);
    assert_eq!(
        r.read(r#"{"type":"item.completed","item":{"id":"item_0","type":"error","message":"Codex is ignoring 1 unrecognized configuration setting."}}"#),
        []
    );
    assert_eq!(
        r.read(r#"{"type":"item.completed","item":{"id":"item_4","type":"agent_message","text":"ok"}}"#),
        [StreamSignal::Message { text: "ok".into() }]
    );
    // File edits keep file names only, so two long Windows paths still fit the summary.
    assert_eq!(
        r.read(r#"{"type":"item.completed","item":{"type":"file_change","changes":[{"path":"C:\\Users\\someone\\projects\\todo-app\\index.html","kind":"update"},{"path":"C:/Users/someone/projects/todo-app/app.js","kind":"add"}]}}"#),
        [StreamSignal::Tool { name: "edit".into(), summary: r#"[{"path":"index.html","kind":"update"},{"path":"app.js","kind":"add"}]"#.into() }]
    );
    assert_eq!(
        r.read(r#"{"type":"turn.completed","usage":{"input_tokens":127105,"cached_input_tokens":112384,"output_tokens":519}}"#),
        [StreamSignal::Usage { input: 14721.0, cached: 112384.0, output: 519.0 }, StreamSignal::Final { ok: true, text: String::new() }]
    );
    let failed = r.read(r#"{"type":"turn.failed","error":{"message":"You have hit your usage limit."}}"#);
    assert!(matches!(failed.as_slice(), [StreamSignal::Limit { .. }, StreamSignal::Final { ok: false, .. }]));
}

#[test]
fn agy_stream_deltas_become_one_message() {
    let mut r = StreamReader::new(ProviderId::Agy);
    assert_eq!(r.read(r#"{"event":"init","conversation_id":"c1","init":{}}"#), [StreamSignal::Session { session_id: "c1".into() }]);
    assert_eq!(r.read(r#"{"event":"step_update","step_update":{"state":"DONE","step_type":"user_input"}}"#), []);
    assert_eq!(r.read(r#"{"event":"step_update","step_update":{"state":"ACTIVE","step_type":"agent_response","text_delta":"o"}}"#), []);
    assert_eq!(
        r.read(r#"{"event":"step_update","step_update":{"state":"DONE","step_type":"agent_response","text_delta":"k\n"}}"#),
        [StreamSignal::Message { text: "ok".into() }]
    );
    assert_eq!(
        r.read(r#"{"event":"result","result":{"status":"SUCCESS","response":"ok\n"}}"#),
        [StreamSignal::Final { ok: true, text: "ok".into() }]
    );
    assert_eq!(r.read("not json"), []);
}

// From a real run where headless agy could not get approval for `dir` (2026-11-01).
#[test]
fn agy_a_permission_denial_that_ended_the_turn_is_a_failure() {
    let mut r = StreamReader::new(ProviderId::Agy);
    assert_eq!(
        r.read(r#"{"event":"step_update","step_update":{"state":"ACTIVE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"dir"}}}}"#),
        [StreamSignal::Tool { name: "run_command".into(), summary: "dir".into() }]
    );
    let signals = r.read(r#"{"event":"result","result":{"status":"SUCCESS","response":"","num_turns":1,"denied_actions":[{"action":"command","display_name":"RunCommand"}]}}"#);
    match signals.as_slice() {
        [StreamSignal::Final { ok: false, text }] => assert!(text.contains("RunCommand")),
        other => panic!("{other:?}"),
    }
}

// Real lines from 2026-11-01 runs: Claude reports subscription windows; every CLI reports tokens.
#[test]
fn usage_and_subscription_windows_come_from_what_each_cli_reports() {
    let mut claude = StreamReader::new(ProviderId::Claude);
    assert_eq!(
        claude.read(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1793562000,"rateLimitType":"five_hour","unifiedWindows":{"five_hour":{"utilization":0.02,"resetsAt":1793562000},"seven_day":{"utilization":0.46,"resetsAt":1793750400}}}}"#),
        [quota("five_hour", 0.02, 1793562000.0), quota("seven_day", 0.46, 1793750400.0)]
    );
    let usage = claude.read(r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":12,"cache_creation_input_tokens":19104,"cache_read_input_tokens":189889,"output_tokens":659}}"#);
    assert_eq!(usage[0], StreamSignal::Usage { input: 19116.0, cached: 189889.0, output: 659.0 });
    let agy = StreamReader::new(ProviderId::Agy).read(r#"{"event":"result","result":{"status":"SUCCESS","response":"ok","usage":{"input_tokens":6981,"output_tokens":13,"thinking_tokens":8,"cache_read_tokens":0}}}"#);
    assert_eq!(agy[0], StreamSignal::Usage { input: 6981.0, cached: 0.0, output: 21.0 });
}

#[test]
fn codex_shell_commands_are_shown_without_their_shell_wrapper() {
    assert_eq!(
        unwrap_shell(r#""C:\Program Files\PowerShell\7.6.4\pwsh.exe" -Command "Get-Content -LiteralPath 'index.html'""#),
        "Get-Content -LiteralPath 'index.html'"
    );
    assert_eq!(unwrap_shell("/bin/bash -lc 'npm test'"), "npm test");
    assert_eq!(unwrap_shell("git status"), "git status");
    let tool = StreamReader::new(ProviderId::Codex)
        .read(r#"{"type":"item.completed","item":{"type":"command_execution","command":"\"C:\\pwsh.exe\" -Command \"git diff\""}}"#);
    assert_eq!(tool, [StreamSignal::Tool { name: "shell".into(), summary: "git diff".into() }]);
}

#[test]
fn card_prompts_warn_about_parallel_siblings_and_headless_shells() {
    let p = plan();
    let prompts: Vec<String> = p.cards.iter().map(|c| card_prompt(c, &p, &TEAM)).collect();
    let (claude, agy) = (&prompts[1], &prompts[2]);
    assert!(agy.contains("- code: 코딩 (codex gpt-6.1-sol)"));
    assert!(agy.contains("\"o msg read\"와 \"o msg post\" 두 개만"));
    assert!(claude.contains("셸 명령은 승인할 사람이 없어"));
    let solo = Plan { cards: vec![p.cards[0].clone()], ..p.clone() };
    assert!(!Regex::new("동시에 일합니다").unwrap().is_match(&card_prompt(&solo.cards[0], &solo, &TEAM)));
}

/// The whole prompt, byte for byte as the TS port wrote it for the codex card.
#[test]
fn card_prompt_text_is_unchanged() {
    let p = plan();
    let expected = [
        "[O 작업카드] 랜딩 개편 · 코딩",
        "프로젝트: C:/work/demo",
        "같은 폴더에서 다른 카드가 동시에 일합니다. 맡은 파일 밖의 파일은 지우거나 고치지 마세요.",
        "",
        "구현",
        "",
        "팀 대화 (실행 r1, 내 카드 code):",
        "- plan: 설계 (claude)",
        "- design: 디자인 (agy gemini-3.8-flash-high)",
        "- lead: 총괄",
        "보내기: o msg send --to <카드 id|lead|all> \"내용\"   받기: o msg read",
        "시작할 때 한 번 받아 보고, 다른 카드가 알아야 할 결과나 막힌 점이 생기면 짧게 보내세요.",
        "",
        "끝나면 결과를 다섯 줄 이내로 요약하세요: 한 일, 바꾼 파일, 확인한 것, 남은 일.",
    ]
    .join("\n");
    assert_eq!(card_prompt(&p.cards[0], &p, &TEAM), expected);
    let codex = launch_spec(&p.cards[0], &p, &TEAM);
    let expected_args = ["exec", "--json", "-s", "workspace-write", "-C", "C:/work/demo", "--add-dir", "C:/o/runs/r1", "-m", "gpt-6.1-sol", "-c", "model_reasoning_effort=xhigh", "-"];
    assert_eq!(codex.args, expected_args);
    let claude = launch_spec(&p.cards[1], &p, &TEAM);
    let expected_args = ["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--allowedTools", "Bash(o msg *)", "Bash(o context *)", "--effort", "high"];
    assert_eq!(claude.args, expected_args);
}

/// A key left on the PC must not move a card from the subscription to the API bill.
#[test]
fn cards_drop_api_key_variables_so_each_cli_uses_its_own_sign_in() {
    let p = plan();
    let [codex, claude, agy] = <[LaunchSpec; 3]>::try_from(specs(&p)).unwrap();
    assert_eq!(claude.env_remove, ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
    assert_eq!(codex.env_remove, ["CODEX_API_KEY"]);
    assert!(agy.env_remove.is_empty());
    for spec in [&codex, &claude, &agy] {
        assert!(spec.env.keys().all(|k| k == "O_RUN_ID" || k == "O_CARD_ID"), "{:?}", spec.env);
    }
}

fn lead_spec(provider: ProviderId, session: Option<&str>, effort: Option<&str>, images: &[String]) -> Option<LaunchSpec> {
    o_core::providers::get(provider).lead(&LeadTurn {
        project: "C:/work/demo",
        prompt: "계획해 줘".into(),
        session,
        model: None,
        effort,
        files_dir: Some("C:/o-lead/t1"),
        images,
    })
}

#[test]
fn the_claude_lead_reads_only_and_resumes_its_session() {
    let first = lead_spec(ProviderId::Claude, None, Some("low"), &[]).unwrap();
    assert_eq!(first.bin, "claude");
    let expected = [
        "-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "default", "--tools", "Read,Grep,Glob", "--allowedTools", "Read,Grep,Glob",
        "--strict-mcp-config", "--setting-sources", "user", "--settings", r#"{"disableAllHooks":true}"#, "--effort", "low", "--add-dir", "C:/o-lead/t1",
    ];
    assert_eq!(first.args, expected);
    assert_eq!(first.stdin.as_deref(), Some("계획해 줘"));
    let next = lead_spec(ProviderId::Claude, Some("s-1"), None, &[]).unwrap();
    assert_eq!(after(&next.args, "--resume"), "s-1");
    assert!(!has(&next.args, "--effort"));
}

#[test]
fn the_codex_lead_runs_in_a_read_only_sandbox_and_resumes_with_exec_resume() {
    let images = vec!["C:/o-lead/t1/01-a.png".to_string()];
    let first = lead_spec(ProviderId::Codex, None, Some("high"), &images).unwrap();
    let expected = [
        "exec", "-s", "read-only", "-C", "C:/work/demo", "--skip-git-repo-check", "--disable", "hooks", "-i", "C:/o-lead/t1/01-a.png", "--json", "-c",
        "model_reasoning_effort=high", "-",
    ];
    assert_eq!(first.args, expected);
    let next = lead_spec(ProviderId::Codex, Some("th-9"), None, &images).unwrap();
    let expected = [
        "exec", "-s", "read-only", "-C", "C:/work/demo", "--skip-git-repo-check", "--disable", "hooks", "resume", "-i", "C:/o-lead/t1/01-a.png", "--json", "th-9", "-",
    ];
    assert_eq!(next.args, expected);
    assert_eq!(next.stdin.as_deref(), Some("계획해 줘"));
}

#[test]
fn the_agy_lead_plans_with_every_change_denied_and_resumes_its_conversation() {
    let first = lead_spec(ProviderId::Agy, None, Some("low"), &["C:/o-lead/t1/01-a.png".to_string()]).unwrap();
    assert_eq!(first.bin, "agy");
    let deny = r#"{"permissions":{"deny":["write_file(*)","command(*)","mcp(*)","read_url(*)","execute_url(*)"]}}"#;
    let expected = [
        "--output-format", "stream-json", "--mode", "plan", "--sandbox", "--settings", deny, "--add-dir", "C:/o-lead/t1", "--effort", "low", "-p", "계획해 줘",
    ];
    assert_eq!(first.args, expected);
    // agy takes no image flag: the attachment is named in the prompt and its folder added.
    assert!(first.stdin.is_none());
    assert_eq!(first.env.get("AGY_CLI_DISABLE_AUTO_UPDATE").map(String::as_str), Some("true"));
    let next = o_core::providers::get(ProviderId::Agy)
        .lead(&LeadTurn { project: "C:/work/demo", prompt: "또".into(), session: Some("c-1"), model: Some("gemini-3.8-flash-high"), effort: None, files_dir: None, images: &[] })
        .unwrap();
    assert_eq!(after(&next.args, "--conversation"), "c-1");
    assert_eq!(after(&next.args, "--model"), "gemini-3.8-flash-high");
    assert!(!has(&next.args, "--effort") && !has(&next.args, "--add-dir"));
    assert_eq!(next.args[next.args.len() - 2..], ["-p", "또"]);
}

#[test]
fn every_lead_takes_the_model_it_is_given() {
    for (provider, flag) in [(ProviderId::Claude, "--model"), (ProviderId::Codex, "-m"), (ProviderId::Agy, "--model")] {
        for session in [None, Some("s")] {
            let turn = LeadTurn { project: "C:/work/demo", prompt: "p".into(), session, model: Some("m-1"), effort: None, files_dir: None, images: &[] };
            let spec = o_core::providers::get(provider).lead(&turn).unwrap();
            assert_eq!(after(&spec.args, flag), "m-1", "{provider} {session:?}");
        }
    }
}

#[test]
fn lead_launches_never_write_and_never_turn_approvals_off() {
    for provider in ProviderId::ALL {
        for session in [None, Some("s")] {
            let spec = lead_spec(provider, session, Some("high"), &[]).unwrap_or_else(|| panic!("{provider} cannot lead"));
            let line = spec.args.join(" ");
            assert!(find_hits(&line, code_rules()).is_empty(), "{provider}: {line}");
            for word in ["acceptEdits", "accept-edits", "auto", "workspace-write", "danger-full-access", "Bash", "Edit", "Write", "NotebookEdit"] {
                assert!(!spec.args.iter().any(|a| a.split(',').any(|part| part == word)), "{provider}: {word}");
            }
        }
    }
    // agy's mode is plan, and its session settings deny every change rather than allow any.
    let agy = lead_spec(ProviderId::Agy, None, None, &[]).unwrap();
    assert_eq!(after(&agy.args, "--mode"), "plan");
    let settings: Value = serde_json::from_str(after(&agy.args, "--settings")).unwrap();
    assert!(settings["permissions"].get("allow").is_none());
    let deny: Vec<&str> = settings["permissions"]["deny"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for rule in ["write_file(*)", "command(*)", "mcp(*)"] {
        assert!(deny.contains(&rule), "{rule}");
    }
}

#[test]
fn lead_model_lists_come_from_each_cli() {
    let claude: Vec<String> = o_core::providers::lead_models(ProviderId::Claude).unwrap().into_iter().map(|m| m.id).collect();
    assert_eq!(claude, ["fable", "opus", "sonnet", "haiku"]);
    let agy = "Fetching available models...\ngemini-4-argon-high\tGemini 4 Argon (High)\nclaude-opus-5-5-low\tClaude Opus 5.5 (Low)\n\n";
    assert_eq!(
        parse_agy_models(agy),
        [
            ModelChoice { id: "gemini-4-argon-high".into(), label: "Gemini 4 Argon (High)".into() },
            ModelChoice { id: "claude-opus-5-5-low".into(), label: "Claude Opus 5.5 (Low)".into() },
        ]
    );
    let codex = json!({ "models": [
        { "slug": "gpt-6-luna", "display_name": "GPT-6-Luna", "visibility": "list", "priority": 5 },
        { "slug": "gpt-reserve", "display_name": "GPT-Reserve", "visibility": "hide", "priority": 4 },
        { "slug": "gpt-6.1-sol", "display_name": "GPT-6.1-Sol", "visibility": "list", "priority": 1 },
    ] });
    let ids: Vec<String> = parse_codex_models(&codex.to_string()).into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["gpt-6.1-sol", "gpt-6-luna"]);
    assert!(parse_codex_models("not json").is_empty());
}

#[test]
fn inherited_spellings_in_another_case_are_dropped_too() {
    let inherited = ["Path", "anthropic_api_key", "ANTHROPIC_API_KEY_HELPER", "Anthropic_Auth_Token"].map(OsString::from);
    let names = names_to_remove(&["ANTHROPIC_API_KEY".into(), "ANTHROPIC_AUTH_TOKEN".into()], inherited);
    assert_eq!(names, ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "anthropic_api_key", "Anthropic_Auth_Token"].map(OsString::from));
    assert!(names_to_remove(&[], [OsString::from("ANTHROPIC_API_KEY")]).is_empty());
}

/// A wrong match rests the provider for an hour and fails the cards waiting for it.
#[test]
fn only_plan_and_rate_limits_count_as_limits() {
    for text in [
        "Claude AI usage limit reached|1793523600",
        "You've hit your usage limit. Upgrade to Pro or try again later.",
        "stream error: exceeded retry limit, last status: 429 Too Many Requests",
        "RESOURCE_EXHAUSTED: Quota exceeded for quota metric 'Generate Content API requests'",
        "This request would exceed the rate limit for your organization of 8,000 output tokens per minute.",
        "사용량 한도에 도달했어",
    ] {
        assert!(is_limit_text(text), "{text}");
    }
    for text in [
        "Prompt is too long: 210000 tokens > 200000 maximum",
        "max_tokens: 64000 > 32000, which is the maximum allowed number of output tokens",
        "upload quota exceeded for files",
        "context window exceeded: rate limit",
        "src/app.ts:429: unexpected token",
        "Error: ENOENT, line 1429",
    ] {
        assert!(!is_limit_text(text), "{text}");
    }
}

#[test]
fn a_claude_limit_shows_in_its_error_text_too_but_one_limit_per_card_keeps_the_event_reset_time() {
    let failed = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Claude AI usage limit reached"}"#;
    let alone = StreamReader::new(ProviderId::Claude).read(failed);
    assert!(matches!(alone.as_slice(), [StreamSignal::Limit { until }, StreamSignal::Final { ok: false, .. }] if until.is_empty()));
    let mut r = StreamReader::new(ProviderId::Claude);
    assert_eq!(
        r.read(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1793526000}}"#),
        [StreamSignal::Limit { until: "2026-11-01T09:40:00.000Z".into() }]
    );
    assert!(matches!(r.read(failed).as_slice(), [StreamSignal::Final { ok: false, .. }]));
}

#[test]
fn every_cli_with_a_read_only_resumable_mode_can_lead() {
    for provider in ProviderId::ALL {
        assert!(o_core::providers::can_lead(provider), "{provider}");
    }
}
