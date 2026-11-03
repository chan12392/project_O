//! Port of tests/providers.test.ts.

use std::ffi::OsString;

use o_core::run::events::StreamSignal;
use o_core::sys::js::iso_from_secs;
use o_core::run::plan::{Card, Plan, ProviderId};
use o_core::providers::agy::parse_agy_models;
use o_core::providers::codex::parse_codex_models;
use o_core::providers::devin::read_run;
use o_core::providers::{
    card_prompt, is_limit_text, launch_spec, launch_spec_for, launch_spec_with, names_to_remove, unwrap_shell, LaunchSpec, LeadTurn, ModelChoice, StreamReader, TeamChannel,
};
use o_guard::rules::{code_rules, find_hits};
use regex::Regex;
use serde_json::{json, Value};

fn card(id: &str, provider: ProviderId, role: Option<&str>, task: &str, model: Option<&str>, effort: Option<&str>) -> Card {
    Card { id: id.into(), provider, role: role.map(Into::into), task: task.into(), model: model.map(Into::into), effort: effort.map(Into::into), depends_on: vec![] }
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
    // app-server (v2) names the kind in an object; the summary shows the same word.
    assert_eq!(
        r.read(r#"{"type":"item.completed","item":{"type":"file_change","changes":[{"path":"C:/p/a.txt","kind":{"type":"add"}},{"path":"b.txt","kind":{"type":"update","move_path":null}}]}}"#),
        [StreamSignal::Tool { name: "edit".into(), summary: r#"[{"path":"a.txt","kind":"add"},{"path":"b.txt","kind":"update"}]"#.into() }]
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
    let expected_args = ["exec", "--json", "-s", "workspace-write", "-C", "C:/work/demo", "--add-dir", "C:/o/runs/r1", "--skip-git-repo-check", "-m", "gpt-6.1-sol", "-c", "model_reasoning_effort=xhigh", "-"];
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
    lead_spec_on(provider, session, effort, images, true)
}

/// `app_server`: whether the Codex CLI has app-server (`codex::supports_app_server`).
fn lead_spec_on(provider: ProviderId, session: Option<&str>, effort: Option<&str>, images: &[String], app_server: bool) -> Option<LaunchSpec> {
    o_core::providers::get(provider).lead(&LeadTurn {
        project: "C:/work/demo",
        prompt: "계획해 줘".into(),
        session,
        model: None,
        effort,
        files_dir: Some("C:/o-lead/t1"),
        images,
        app_server,
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
fn the_codex_lead_starts_app_server_with_hooks_disabled() {
    let images = vec!["C:/o-lead/t1/01-a.png".to_string()];
    let first = lead_spec(ProviderId::Codex, None, Some("high"), &images).unwrap();
    assert_eq!(first.args, ["app-server", "--disable", "hooks"]);
    let next = lead_spec(ProviderId::Codex, Some("th-9"), None, &images).unwrap();
    assert_eq!(next.args, first.args);
    assert!(next.stdin.is_none());
}

/// A CLI without app-server gets the earlier read-only exec lead, hooks off, resumed by id.
#[test]
fn the_codex_lead_falls_back_to_read_only_exec_without_app_server() {
    let images = vec!["C:/o-lead/t1/01-a.png".to_string()];
    let first = lead_spec_on(ProviderId::Codex, None, Some("high"), &images, false).unwrap();
    assert_eq!(first.bin, "codex");
    assert_eq!(
        first.args,
        ["exec", "-s", "read-only", "-C", "C:/work/demo", "--skip-git-repo-check", "--disable", "hooks", "-i", "C:/o-lead/t1/01-a.png", "--json", "-c", "model_reasoning_effort=high", "-"]
    );
    assert_eq!(first.stdin.as_deref(), Some("계획해 줘"));
    assert_eq!(first.env_remove, ["CODEX_API_KEY"]);
    let next = lead_spec_on(ProviderId::Codex, Some("th-9"), None, &[], false).unwrap();
    assert_eq!(
        next.args,
        ["exec", "-s", "read-only", "-C", "C:/work/demo", "--skip-git-repo-check", "--disable", "hooks", "resume", "--json", "th-9", "-"]
    );
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
        .lead(&LeadTurn { project: "C:/work/demo", prompt: "또".into(), session: Some("c-1"), model: Some("gemini-3.8-flash-high"), effort: None, files_dir: None, images: &[], app_server: false })
        .unwrap();
    assert_eq!(after(&next.args, "--conversation"), "c-1");
    assert_eq!(after(&next.args, "--model"), "gemini-3.8-flash-high");
    assert!(!has(&next.args, "--effort") && !has(&next.args, "--add-dir"));
    assert_eq!(next.args[next.args.len() - 2..], ["-p", "또"]);
}

#[test]
fn every_lead_takes_the_model_it_is_given() {
    for (provider, flag) in [(ProviderId::Claude, "--model"), (ProviderId::Agy, "--model")] {
        for session in [None, Some("s")] {
            let turn = LeadTurn { project: "C:/work/demo", prompt: "p".into(), session, model: Some("m-1"), effort: None, files_dir: None, images: &[], app_server: false };
            let spec = o_core::providers::get(provider).lead(&turn).unwrap();
            assert_eq!(after(&spec.args, flag), "m-1", "{provider} {session:?}");
        }
    }
}

/// With YOLO off (the default, ADR-0014) the lead reads only; the YOLO-on launches are fixed
/// separately below (`yolo_leads_*`).
#[test]
fn lead_launches_never_write_and_never_turn_approvals_off() {
    for provider in ProviderId::ALL.into_iter().filter(|p| o_core::providers::can_lead(*p)) {
        for (session, app_server) in [(None, false), (Some("s"), false), (None, true), (Some("s"), true)] {
            let spec = lead_spec_on(provider, session, Some("high"), &[], app_server).unwrap_or_else(|| panic!("{provider} cannot lead"));
            let turn = LeadTurn { project: "C:/work/demo", prompt: "계획해 줘".into(), session, model: None, effort: Some("high"), files_dir: Some("C:/o-lead/t1"), images: &[], app_server };
            assert_eq!(o_core::providers::lead_spec(provider, &turn, false).as_ref(), Some(&spec), "{provider}: YOLO off is the read-only lead");
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
            ModelChoice { id: "gemini-4-argon-high".into(), label: "Gemini 4 Argon (High)".into(), hint: None },
            ModelChoice { id: "claude-opus-5-5-low".into(), label: "Claude Opus 5.5 (Low)".into(), hint: None },
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

// YOLO mode (ADR-0014). This file and the provider files are the only code that may spell
// the CLIs' "never ask" flags (o_guard::rules BYPASS_PATHS).

const GUARD_HOOK: &str = r#"{"hooks":{"PreToolUse":[{"matcher":"Bash|PowerShell|Write|Edit|MultiEdit|NotebookEdit|Read|Grep|Glob","hooks":[{"type":"command","command":"o hook","timeout":10,"onFailure":"block"}]}]}}"#;
const CLAUDE_DENY: [&str; 7] = ["Bash(o dispatch*)", "Bash(o followup*)", "Bash(o lead*)", "Bash(o review*)", "Bash(o _run*)", "Bash(o _followup*)", "Bash(o _review*)"];
const AGY_DENY: &str = r#"{"permissions":{"deny":["command(o dispatch*)","command(o followup*)","command(o lead*)","command(o review*)","command(o _run*)","command(o _followup*)","command(o _review*)"]}}"#;

fn yolo_specs(plan: &Plan, yolo: bool) -> Vec<LaunchSpec> {
    plan.cards.iter().map(|c| launch_spec_for(c, plan, &TEAM, None, yolo)).collect()
}

#[test]
fn yolo_off_launches_are_the_launches_cards_always_had() {
    let p = plan();
    assert_eq!(yolo_specs(&p, false), specs(&p));
    assert_eq!(launch_spec_for(&p.cards[0], &p, &TEAM, Some("맥락"), false), launch_spec_with(&p.cards[0], &p, &TEAM, Some("맥락")));
}

#[test]
fn yolo_cards_use_each_clis_official_flag_and_keep_the_deny_rules() {
    let p = plan();
    let [codex, claude, agy] = <[LaunchSpec; 3]>::try_from(yolo_specs(&p, true)).unwrap();
    let mut expected: Vec<&str> = vec!["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "bypassPermissions", "--disallowedTools"];
    expected.extend(CLAUDE_DENY);
    expected.extend(["--settings", GUARD_HOOK, "--effort", "high"]);
    assert_eq!(claude.args, expected);
    assert_eq!(
        codex.args,
        ["exec", "--json", "--dangerously-bypass-approvals-and-sandbox", "-C", "C:/work/demo", "--skip-git-repo-check", "-m", "gpt-6.1-sol", "-c", "model_reasoning_effort=xhigh", "-"]
    );
    let prompt = agy.args.last().unwrap().clone();
    assert_eq!(
        agy.args,
        ["--output-format", "stream-json", "--dangerously-skip-permissions", "--add-dir", "C:/o/runs/r1", "--settings", AGY_DENY, "--model", "gemini-3.8-flash-high", "-p", prompt.as_str()]
    );
    for spec in [&codex, &claude, &agy] {
        // Same identity for `o msg`, same API keys taken out, the prompt says YOLO and what stays closed.
        assert!(spec.env.keys().all(|k| k == "O_RUN_ID" || k == "O_CARD_ID"));
        let text = spec.stdin.clone().unwrap_or_else(|| prompt.clone());
        assert!(text.contains("YOLO 모드") && text.contains("o dispatch"), "{text}");
        for flag in ["acceptEdits", "accept-edits", "workspace-write", "--allowedTools"] {
            assert!(!has(&spec.args, flag), "{flag}");
        }
    }
    assert_eq!(claude.env_remove, ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
    // The headless-shell warning of YOLO off is not in the YOLO prompt.
    assert!(!claude.stdin.unwrap().contains("셸 명령은 승인할 사람이 없어"));
}

fn yolo_lead(provider: ProviderId, session: Option<&str>) -> LaunchSpec {
    let turn = LeadTurn { project: "C:/work/demo", prompt: "고쳐 줘".into(), session, model: None, effort: Some("low"), files_dir: Some("C:/o-lead/t1"), images: &[], app_server: false };
    o_core::providers::lead_spec(provider, &turn, true).unwrap()
}

#[test]
fn yolo_leads_may_edit_and_run_commands_but_never_send_work() {
    let mut expected: Vec<&str> = vec!["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "bypassPermissions", "--disallowedTools"];
    expected.extend(CLAUDE_DENY);
    expected.extend(["--strict-mcp-config", "--setting-sources", "user", "--settings", GUARD_HOOK, "--resume", "s-1", "--effort", "low", "--add-dir", "C:/o-lead/t1"]);
    assert_eq!(yolo_lead(ProviderId::Claude, Some("s-1")).args, expected);

    assert_eq!(
        yolo_lead(ProviderId::Codex, None).args,
        ["exec", "--dangerously-bypass-approvals-and-sandbox", "-C", "C:/work/demo", "--skip-git-repo-check", "--json", "-c", "model_reasoning_effort=low", "-"]
    );
    assert_eq!(
        yolo_lead(ProviderId::Codex, Some("th-9")).args,
        ["exec", "--dangerously-bypass-approvals-and-sandbox", "-C", "C:/work/demo", "--skip-git-repo-check", "resume", "--json", "-c", "model_reasoning_effort=low", "th-9", "-"]
    );

    let agy = yolo_lead(ProviderId::Agy, Some("c-1"));
    assert_eq!(
        agy.args,
        ["--output-format", "stream-json", "--dangerously-skip-permissions", "--settings", AGY_DENY, "--conversation", "c-1", "--add-dir", "C:/o-lead/t1", "--effort", "low", "-p", "고쳐 줘"]
    );
    assert_eq!(agy.env.get("AGY_CLI_DISABLE_AUTO_UPDATE").map(String::as_str), Some("true"));
    // Of the read-only lead's rules only the send-work block is left; no plan mode, no sandbox.
    for gone in ["--mode", "--sandbox", "--tools", "disableAllHooks", "--disable"] {
        for spec in [yolo_lead(ProviderId::Claude, None), yolo_lead(ProviderId::Codex, None), agy.clone()] {
            assert!(!spec.args.iter().any(|a| a == gone || a.contains("disableAllHooks")), "{gone}: {:?}", spec.args);
        }
    }
}

#[test]
fn devin_cannot_lead() {
    // Devin leads later, through its ACP server (ADR-0016).
    assert!(!o_core::providers::can_lead(ProviderId::Devin));
    assert!(o_core::providers::get(ProviderId::Devin).lead(&LeadTurn::default()).is_none());
}

fn devin_card(model: Option<&str>) -> Card {
    card("fix", ProviderId::Devin, Some("수정"), "README 오타 고치기", model, Some("high"))
}

#[test]
fn a_devin_card_prints_headless_with_edits_approved_and_its_transcript_exported() {
    let p = Plan { cards: vec![devin_card(Some("swe-2-medium")), plan().cards[0].clone()], ..plan() };
    let spec = launch_spec(&p.cards[0], &p, &TEAM);
    assert_eq!(spec.bin, "devin");
    let prompt = card_prompt(&p.cards[0], &p, &TEAM);
    let expected = [
        "-p", "--permission-mode", "accept-edits", "--export", "C:/o/runs/r1/cards/fix.atif.json", "--model", "swe-2-medium", "--",
        prompt.as_str(),
    ];
    assert_eq!(spec.args, expected);
    // No effort flag exists (a Devin model id names its level), and the prompt is an argument.
    assert!(!has(&spec.args, "high") && spec.stdin.is_none());
    assert_eq!(spec.env_remove, ["WINDSURF_API_KEY"]);
    assert!(find_hits(&spec.args.join(" "), code_rules()).is_empty());
    // Nothing is allowed without a prompt, so the card is told to keep to file tools, never to read the team channel.
    assert!(prompt.contains("셸은 쓰지 말고") && prompt.contains("팀 대화 명령을 실행할 수 없습니다"));
    assert!(!prompt.contains("o msg") && !prompt.contains("시작할 때 한 번 받아 보고"));
    let default_model = launch_spec(&devin_card(None), &p, &TEAM);
    assert!(!has(&default_model.args, "--model"));
}

#[test]
fn devin_yolo_approves_every_tool_and_changes_nothing_else() {
    let p = Plan { cards: vec![devin_card(None)], ..plan() };
    let normal = launch_spec(&p.cards[0], &p, &TEAM);
    let yolo = launch_spec_for(&p.cards[0], &p, &TEAM, None, true);
    assert_eq!(yolo.args[..3], ["-p", "--permission-mode", "dangerous"]);
    assert!(!find_hits("devin --permission-mode dangerous", code_rules()).is_empty());
    assert_eq!(yolo.args[3..yolo.args.len()-1], normal.args[3..normal.args.len()-1]);
}

/// Trimmed from a real `devin -p --export` transcript (3000.11.205, swe-2-medium, 2026-11-03).
const DEVIN_ANSWERED: &str = r#"{"schema_version":"ATIF-v1.7","session_id":"merry-otter","agent":{"name":"devin","version":"3000.11.205"},
 "steps":[
  {"step_id":1,"source":"system","message":"You are Devin"},
  {"step_id":8,"source":"user","message":"Run o msg read"},
  {"step_id":10,"source":"agent","message":"","tool_calls":[{"tool_call_id":"exec:0#a1","function_name":"exec","arguments":{"command":"o msg read"}}],
   "observation":{"results":[{"source_call_id":"exec:0#a1","content":"Output from command in shell 009f5b:\n...\nExit code: 1"}]}},
  {"step_id":11,"source":"agent","message":"It printed: no run found.","tool_calls":[]}],
 "final_metrics":{"total_prompt_tokens":35032,"total_completion_tokens":304,"total_cached_tokens":24704,"total_steps":11}}"#;

/// The same kind of run where headless Devin refused the shell command (exit 0, empty stdout).
const DEVIN_REFUSED: &str = r#"{"schema_version":"ATIF-v1.7","session_id":"fragrant-lamb","steps":[
  {"step_id":13,"source":"agent","message":"","tool_calls":[
    {"tool_call_id":"mcp_list_tools:0#b1","function_name":"mcp_list_tools","arguments":{"server_name":"m"}},
    {"tool_call_id":"edit:1#b2","function_name":"edit","arguments":{"file_path":"C:\\work\\demo\\docs\\README.md","old_string":"teh","new_string":"the"}}],
   "observation":{"results":[{"source_call_id":"mcp_list_tools:0#b1","content":"[]"},{"source_call_id":"edit:1#b2","content":"ok"}]}},
  {"step_id":14,"source":"agent","message":"","tool_calls":[
    {"tool_call_id":"mcp_call_tool:0#c1","function_name":"mcp_call_tool","arguments":{"server_name":"m","tool_name":"search","arguments":{"query":"q"}}},
    {"tool_call_id":"exec:1#c2","function_name":"exec","arguments":{"command":"o msg read"}}],
   "observation":{"results":[
    {"source_call_id":"mcp_call_tool:0#c1","content":"Tool call canceled because another tool call (id=exec:1#c2) was rejected"},
    {"source_call_id":"exec:1#c2","content":"Tool execution was rejected by the user"}]}}],
 "final_metrics":{"total_prompt_tokens":35032,"total_completion_tokens":304,"total_cached_tokens":24704,"total_steps":14}}"#;

#[test]
fn devin_answers_come_from_stdout_and_its_steps_from_the_transcript() {
    let signals = read_run("It printed: no run found.\n", "", Some(DEVIN_ANSWERED));
    assert_eq!(
        signals,
        [
            StreamSignal::Session { session_id: "merry-otter".into() },
            StreamSignal::Tool { name: "exec".into(), summary: "o msg read".into() },
            StreamSignal::Message { text: "It printed: no run found.".into() },
            StreamSignal::Usage { input: 10328.0, cached: 24704.0, output: 304.0 },
            StreamSignal::Final { ok: true, text: "It printed: no run found.".into() },
        ]
    );
    // Without a transcript the answer still counts.
    assert_eq!(read_run("ok", "", None), [StreamSignal::Final { ok: true, text: "ok".into() }]);
    assert!(matches!(read_run("", "", Some(DEVIN_ANSWERED)).last(), Some(StreamSignal::Final { ok: true, text }) if text == "It printed: no run found."));
    // A plain-text stdout is never read line by line.
    assert_eq!(StreamReader::new(ProviderId::Devin).read(r#"{"type":"result","result":"x"}"#), []);
}

#[test]
fn a_devin_turn_ended_by_a_refused_tool_is_a_failure_that_names_it() {
    let warning = "warning: rejected a tool call that requires confirmation. Running in non-interactive mode. Use --permission-mode dangerous to auto-approve all tools.";
    let signals = read_run("", warning, Some(DEVIN_REFUSED));
    let tools: Vec<(String, String)> = signals.iter().filter_map(|s| match s {
        StreamSignal::Tool { name, summary } => Some((name.clone(), summary.clone())),
        _ => None,
    }).collect();
    assert_eq!(tools, [("mcp_list_tools".into(), String::new()), ("edit".into(), "docs/README.md".into()), ("mcp_call_tool".into(), "m/search".into()), ("exec".into(), "o msg read".into())]);
    match signals.last() {
        Some(StreamSignal::Final { ok: false, text }) => assert!(text.contains("권한 거부") && text.contains("exec o msg read"), "{text}"),
        other => panic!("{other:?}"),
    }
    // Only the stderr warning (no transcript): still a refusal, not a silent success.
    assert!(matches!(read_run("", warning, None).as_slice(), [StreamSignal::Final { ok: false, text }] if text.contains("권한 거부")));
    // An empty answer for another reason says the last stderr line, and a limit there rests the provider.
    let limited = read_run("", "Error: You have hit your usage limit.", None);
    assert!(matches!(limited.as_slice(), [StreamSignal::Limit { .. }, StreamSignal::Final { ok: false, text }] if text.contains("usage limit")));
}

#[test]
fn devin_sign_in_is_read_from_the_first_status_line_only() {
    let p = o_core::providers::get(ProviderId::Devin);
    assert_eq!(p.status_args(), ["auth", "status"]);
    assert_eq!(p.login_args(), Some(&["auth", "login"][..]));
    let yes = p.read_login(true, "Logged in (via Devin).\n\nUser:\n  Email: someone@example.com\n", "").unwrap();
    assert_eq!((yes.logged_in, yes.detail.as_str()), (Some(true), "로그인됨"));
    assert_eq!(p.read_login(false, "Not logged in.\n", "").unwrap().logged_in, Some(false));
    assert!(p.read_login(false, "", "boom").is_none());
    let official = p.official();
    assert_eq!((official.name, official.npm_package, official.url), ("Devin", None, "https://docs.devin.ai/cli"));
}

#[test]
fn every_cli_with_a_read_only_resumable_mode_can_lead() {
    for provider in [ProviderId::Claude, ProviderId::Codex, ProviderId::Agy] {
        assert!(o_core::providers::can_lead(provider), "{provider}");
    }
    // OpenCode's `run` takes no session settings and its plan agent does not deny the shell, so
    // nothing the CLI enforces keeps a lead read-only (ADR-0015): it takes cards only.
    assert!(!o_core::providers::can_lead(ProviderId::OpenCode));
    assert!(o_core::providers::get(ProviderId::OpenCode).lead(&LeadTurn::default()).is_none());
}

fn opencode_card(model: Option<&str>, effort: Option<&str>) -> LaunchSpec {
    let p = Plan { cards: vec![card("oc", ProviderId::OpenCode, None, "구현", model, effort)], ..plan() };
    launch_spec(&p.cards[0], &p, &TEAM)
}

#[test]
fn the_opencode_card_runs_standalone_with_json_events_and_its_prompt_on_stdin() {
    let spec = opencode_card(Some("opencode/deepseek-v4.1-flash-free"), Some("high"));
    assert_eq!(spec.bin, "opencode");
    assert_eq!(spec.args, ["run", "--format", "json", "--standalone", "-m", "opencode/deepseek-v4.1-flash-free#high"]);
    assert!(spec.stdin.as_deref().unwrap().contains("구현"));
    // No model, no variant to name: the effort is left to OpenCode's default.
    assert_eq!(opencode_card(None, Some("high")).args, ["run", "--format", "json", "--standalone"]);
    assert_eq!(opencode_card(Some("openrouter/x/y:free"), None).args[4..], ["-m", "openrouter/x/y:free"]);
    // The default launch keeps approvals and removes the provider API key variables.
    assert!(!spec.args.iter().any(|a| a == "--auto" || a.contains("dangerously")));
    for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"] {
        assert!(spec.env_remove.iter().any(|k| k == key), "{key}");
    }
    assert!(spec.env.keys().all(|k| k == "O_RUN_ID" || k == "O_CARD_ID"));
    let p = Plan { cards: vec![card("oc", ProviderId::OpenCode, None, "구현", None, None)], ..plan() };
    assert_eq!(launch_spec_for(&p.cards[0], &p, &TEAM, None, true).args, ["run", "--format", "json", "--standalone", "--auto"]);
    assert!(!find_hits("opencode run --auto", code_rules()).is_empty());
}

/// `opencode run --format json --standalone -m opencode/deepseek-v4.1-flash-free` (v2.1.4), recorded
/// 2026-11-03 in a throwaway project: a read tool step, then the answer. Ids shortened.
const OPENCODE_READ: [&str; 4] = [
    r#"{"type":"step_start","timestamp":1793633019926,"sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","part":{"id":"prt_1","sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","messageID":"msg_1","type":"step-start"}}"#,
    r#"{"type":"tool_use","timestamp":1793633020122,"sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","part":{"partID":"prt_2","sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","messageID":"msg_1","type":"tool","id":"call_1","tool":"read","state":{"status":"completed","input":{"path":"a.txt"},"output":"Read file a.txt, lines 1-1\n1: banana42","title":"read","metadata":{"metadata":{"truncated":false}},"time":{"start":1793633019932,"end":1793633020122}}}}"#,
    r#"{"type":"step_finish","timestamp":1793633020127,"sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","part":{"id":"prt_3","sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","messageID":"msg_1","type":"step-finish","reason":"tool-calls","cost":0,"tokens":{"input":12311,"output":38,"reasoning":0,"cache":{"read":1536,"write":0}}}}"#,
    r#"{"type":"text","timestamp":1793633022734,"sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","part":{"id":"prt_4_text-0","sessionID":"ses_e6324313cffeEhZ5ayKEDFOU1B","messageID":"msg_2","type":"text","text":"banana42","time":{"start":1793633020190,"end":1793633022734}}}"#,
];

#[test]
fn opencode_stream_session_tool_tokens_and_answer() {
    let mut reader = StreamReader::new(ProviderId::OpenCode);
    let signals: Vec<StreamSignal> = OPENCODE_READ.iter().flat_map(|line| reader.read(line)).collect();
    assert_eq!(signals, [
        StreamSignal::Session { session_id: "ses_e6324313cffeEhZ5ayKEDFOU1B".into() },
        StreamSignal::Tool { name: "read".into(), summary: "a.txt".into() },
        StreamSignal::Usage { input: 12311.0, cached: 1536.0, output: 38.0 },
        StreamSignal::Message { text: "banana42".into() },
        StreamSignal::Final { ok: true, text: "banana42".into() },
    ]);
    assert_eq!(reader.read(r#"{"type":"text","part":{"text":"끝"}}"#), [
        StreamSignal::Message { text: "끝".into() },
        StreamSignal::Final { ok: true, text: "banana42\n끝".into() },
    ]);
}

#[test]
fn opencode_denied_tools_and_errors() {
    // Recorded with the project_O profile's rules: the shell outside `o msg`/`o context` is denied.
    let denied = r#"{"type":"tool_use","sessionID":"ses_a","part":{"type":"tool","tool":"shell","state":{"status":"error","input":{"command":"echo hi > made.txt"},"error":"Permission denied: shell"}}}"#;
    let mut reader = StreamReader::new(ProviderId::OpenCode);
    assert_eq!(reader.read(denied)[1], StreamSignal::Tool { name: "shell".into(), summary: "거부·실패 echo hi > made.txt".into() });
    // A model it cannot route to (recorded): the card fails with OpenCode's own words.
    let error = r#"{"type":"error","timestamp":1793633043194,"sessionID":"ses_b","error":{"type":"provider.no-route","message":"Model unavailable: opencode/no-such-model-xyz"}}"#;
    let mut reader = StreamReader::new(ProviderId::OpenCode);
    assert_eq!(reader.read(error), [
        StreamSignal::Session { session_id: "ses_b".into() },
        StreamSignal::Final { ok: false, text: "Model unavailable: opencode/no-such-model-xyz".into() },
    ]);
    // A rate limit (shape from the error event above; the wording is the provider's).
    let limited = r#"{"type":"error","sessionID":"ses_c","error":{"type":"provider.rate-limit","message":"Rate limit exceeded, try again later"}}"#;
    let mut reader = StreamReader::new(ProviderId::OpenCode);
    assert!(reader.read(limited).contains(&StreamSignal::Limit { until: String::new() }));
}

#[test]
fn opencode_model_list_keeps_provider_model_lines() {
    let out = "opencode/deepseek-v4.1-flash-free\nopenrouter/openrouter/free\n\nsome banner line\nollama/qwen3:0.6b\n";
    let ids: Vec<String> = o_core::providers::opencode::parse_models(out).into_iter().map(|m| m.id).collect();
    assert_eq!(ids, ["opencode/deepseek-v4.1-flash-free", "openrouter/openrouter/free", "ollama/qwen3:0.6b"]);
}

/// #37: OpenCode project config is found in the folder and every parent, nearest first.
#[test]
fn opencode_project_config_is_found_from_the_folder_up() {
    use o_core::providers::opencode::project_config;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-opencode-config-{}-{nanos}", std::process::id()));
    let deep = base.join("a").join("b");
    std::fs::create_dir_all(&deep).unwrap();
    assert!(project_config(&deep).is_none_or(|found| !found.starts_with(&base)));
    // The install script's `~/.opencode/bin` alone is not project config.
    std::fs::create_dir_all(base.join(".opencode").join("bin")).unwrap();
    assert!(project_config(&deep).is_none_or(|found| !found.starts_with(&base)));
    std::fs::create_dir_all(base.join(".opencode").join("plugins")).unwrap();
    assert_eq!(project_config(&deep), Some(base.join(".opencode")));
    std::fs::write(base.join("a").join("opencode.json"), "{}").unwrap();
    assert_eq!(project_config(&deep), Some(base.join("a").join("opencode.json")));
    std::fs::write(deep.join("opencode.jsonc"), "{}").unwrap();
    assert_eq!(project_config(&deep), Some(deep.join("opencode.jsonc")));
    std::fs::remove_dir_all(&base).unwrap();
}
