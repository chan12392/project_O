//! `o`: argument parsing and printing. Everything that decides or touches
//! state lives in `o_core`; the guard commands live in `o_guard`.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use o_core::js::{collapse_spaces, js_head};
use o_core::policy::{check_plan, Violation};
use o_core::index::RunQuery;
use o_core::snapshot::snapshot;
use o_core::store::{self, default_home, is_run_id, run_dir};
use o_core::team::{self, Caller, Refused};
use o_core::usage::UsageReading;
use o_core::plan::ProviderId;
use o_core::{clean, mirror, probe, proc, runner};
use serde_json::{json, Value};

mod context_cmd;

const USAGE: &str = "O CLI — 작업카드를 공식 에이전트 CLI에 병렬로 전달합니다.

  o check [plan.json|-]      정책 검사만 (실행 안 함)
  o dispatch [--clean] [--no-context] [plan.json|-]
                             정책 검사 후 백그라운드 실행, runId 출력
                             --clean: 카드를 깨끗한 에이전트 프로필로 실행 (o agents)
                             --no-context: 이전 작업 컨텍스트를 카드에 넣지 않음 (토큰 A/B 기준선)
  o status [runId]           카드별 상태 (runId 없으면 최근 5개 실행)
  o events <runId> [--follow]  정규화된 이벤트(JSONL) — UI·총괄 에이전트용
  o stop <runId>             실행 중지
  o msg send [--to <카드|lead|all>] \"내용\"   팀 대화에 보내기 (카드 안에서는 실행·카드가 자동)
  o msg read [--all]         나에게 온 새 메시지 (--all: 전체 대화)
  o msg post                 카드의 outbox 파일(첫 줄 \"to: <상대>\")을 팀 대화로 보내기
                             카드 밖에서는 --run <runId> 와 --as <카드|lead> 로 지정
  o context <질문> [--project <폴더>] [--limit N] [--json]
                             이전 작업 결과·팀 대화·메모에서 관련 내용 찾기 (카드도 읽기 전용으로 사용)
  o context add [--project <폴더>] \"내용\"   프로젝트 컨텍스트에 메모 추가
  o tokens [--project <폴더>] [--since 날짜] [--compare] [--json]
                             카드별 토큰(새 입력·캐시·출력)과 모드별 평균
                             --compare: 기본 하네스 vs 깨끗한 에이전트+세션 공유
  o usage [--json]           Codex·Antigravity 구독 한도를 각 CLI로 조회해 usage.json에 저장
  o usage --cached [--json]  조회하지 않고 저장된 usage.json만 출력
  o runs --json [--limit N] [--project <폴더>]
                             실행별 이벤트(최근 시작 순, 기본 50개) — 앱 화면용
  o hook                     에이전트 PreToolUse 훅 (도구 호출 JSON을 stdin으로 받음)
  o scan [--all|--staged|--pre-push]   저장소 정책 검사 (git 훅·CI용)
  o sync-mirror <거울 폴더>   거울 폴더(test_2, test_1, main)를 fast-forward (예약 작업용)
  o agents path              깨끗한 에이전트 프로필(개인 훅·스킬·MCP·전역 지시 없음) 위치
  o agents login <claude|codex|agy>   그 프로필로 공식 로그인 (CLI마다 한 번)
  o agents shell <claude|codex|agy> [인자…]   지금 폴더에서 깨끗한 CLI를 대화형으로 실행
  o agents doctor [--json]   로그인 상태와 프로필 오염 검사 (깨끗하면 종료 코드 0)
  o agents status [--clean] [--json]   CLI별 설치·버전·로그인 상태 (--clean: 깨끗한 프로필 기준)
  o agents install <claude|codex|agy> [--json]   공식 npm 패키지로 설치 (없으면 공식 안내)
  o agents open-login <claude|codex|agy> [--clean] [--json]   새 콘솔 창에서 공식 로그인
  o about [--json]           버전과 상태·프로필 폴더 위치

정책 위반이 하나라도 있으면 계획 전체를 실행하지 않고 종료 코드 2로 끝납니다.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

fn run(args: &[String]) -> i32 {
    let home = default_home();
    let (command, rest) = match args.split_first() {
        Some((command, rest)) => (Some(command.as_str()), rest),
        None => (None, &[][..]),
    };
    let first = rest.first().map(String::as_str);
    let has = |flag: &str| rest.iter().any(|a| a == flag);
    match command {
        Some("check") => check(&home, plan_arg(rest), false, false, false),
        Some("dispatch") => check(&home, plan_arg(rest), true, has("--clean"), has("--no-context")),
        Some("context") => context_cmd::context_main(&home, rest),
        Some("tokens") => context_cmd::tokens_main(&home, rest),
        Some("agents") => agents(rest),
        Some("about") => about(&home, has("--json")),
        Some("status") => match first {
            None => status(&home, None),
            Some(id) => run_id_arg(Some(id)).map_or(1, |id| status(&home, Some(id))),
        },
        Some("events") => run_id_arg(first).map_or(1, |id| events(&home, id, has("--follow"))),
        Some("stop") => run_id_arg(first).map_or(1, |id| stop(&home, id)),
        Some("msg") => message(&home, rest),
        Some("usage") => usage(&home, has("--json"), has("--cached")),
        Some("runs") => runs(&home, rest),
        Some("hook") => o_guard::hook::hook_main(),
        Some("scan") => o_guard::scan::scan_main(rest),
        Some("sync-mirror") => {
            if first.is_none() {
                eprintln!("{USAGE}");
            }
            mirror::sync_mirror(&home, first)
        }
        Some("_run") => run_id_arg(first).map_or(1, |id| match runner::run_detached(&home, id) {
            Ok(()) => 0,
            Err(message) => {
                eprintln!("{message}");
                1
            }
        }),
        other => {
            println!("{USAGE}");
            i32::from(other.is_some_and(|c| c != "help" && c != "--help"))
        }
    }
}

fn read_input(path: Option<&str>) -> Result<Value, String> {
    let bytes = match path {
        None | Some("-") => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf).map_err(|e| e.to_string())?;
            buf
        }
        Some(path) => std::fs::read(path).map_err(|e| format!("{path}: {e}"))?,
    };
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// The plan file among `check`/`dispatch` arguments (`--clean`, `--no-context` are flags).
fn plan_arg(args: &[String]) -> Option<&str> {
    args.iter().map(String::as_str).find(|a| *a != "--clean" && *a != "--no-context")
}

fn check(home: &Path, path: Option<&str>, dispatch: bool, clean: bool, no_context: bool) -> i32 {
    let input = match read_input(path) {
        Ok(input) => input,
        Err(error) => return fail(&error),
    };
    let now = Utc::now();
    let ctx = match store::policy_context(home, now) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&error.to_string()),
    };
    let checked = check_plan(&input, &ctx);
    let Some(plan) = checked.plan else {
        if dispatch {
            let title = input.as_object().and_then(|o| o.get("title"));
            if let Err(error) = store::record_blocked(home, now, title, &checked.violations) {
                eprintln!("{error}");
            }
        }
        print_violations(&checked.violations);
        return 2;
    };
    if !dispatch {
        let cards: Vec<String> = plan.cards.iter().map(|c| format!("{}→{}", c.id, c.provider)).collect();
        println!("정책 통과: 카드 {}개 ({})", plan.cards.len(), cards.join(", "));
        return 0;
    }
    let run_id = store::new_run_id(now);
    let created = store::create_run(home, &run_id, &plan)
        .and_then(|()| if clean { store::mark_clean(home, &run_id) } else { Ok(()) })
        .and_then(|()| if no_context { o_core::context::mark_off(home, &run_id) } else { Ok(()) });
    if let Err(error) = created {
        return fail(&error.to_string());
    }
    let started = std::env::current_exe().and_then(|exe| proc::spawn_detached(Command::new(exe).args(["_run", &run_id])));
    if let Err(error) = started {
        return fail(&format!("실행기를 시작하지 못했습니다: {error}"));
    }
    let events = run_dir(home, &run_id).join("events.jsonl");
    let cards: Vec<&str> = plan.cards.iter().map(|c| c.id.as_str()).collect();
    println!("{}", json!({ "runId": run_id, "events": events.to_string_lossy(), "cards": cards, "clean": clean }));
    0
}

fn provider_arg(value: Option<&String>) -> Option<ProviderId> {
    let found = value.and_then(|v| ProviderId::parse(v));
    if found.is_none() {
        eprintln!("에이전트를 claude, codex, agy 중에서 지정하세요\n\n{USAGE}");
    }
    found
}

/// Runs a CLI attached to this console and passes on its exit code.
fn attached(command: std::io::Result<Command>) -> i32 {
    match command.and_then(|mut c| c.status()) {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => fail(&error.to_string()),
    }
}

/// `o agents path|login|shell|doctor`: clean agent profiles (see `o_core::clean`).
fn agents(args: &[String]) -> i32 {
    let root = clean::default_root();
    let (verb, rest) = match args.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("", &[][..]),
    };
    match verb {
        "path" => {
            for provider in ProviderId::ALL {
                println!("{:<6} {}", provider.as_str(), clean::profile_home(&root, provider).to_string_lossy());
            }
            0
        }
        "login" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            match clean::login_args(provider) {
                Some(args) => attached(clean::command(&root, provider, args)),
                None => {
                    println!("Antigravity는 이 PC의 시스템 로그인(OS 자격 증명 저장소)을 그대로 씁니다. 따로 로그인하지 않습니다.");
                    let state = clean::login_state(&root, provider);
                    println!("agy    {}", state.detail);
                    i32::from(state.logged_in != Some(true))
                }
            }
        }
        "shell" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            let args: Vec<&str> = rest[1..].iter().map(String::as_str).collect();
            attached(clean::command(&root, provider, &args))
        }
        "doctor" => doctor(&root, rest.iter().any(|a| a == "--json")),
        "status" => {
            let list = o_core::agents::status(&root, rest.iter().any(|a| a == "--clean"));
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(list));
            } else {
                for a in &list {
                    let state = match (a.installed, a.logged_in) {
                        (false, _) => format!("설치 안 됨 — {}", a.install_hint),
                        (true, Some(true)) => "로그인됨".into(),
                        (true, Some(false)) => "로그인 안 됨".into(),
                        (true, None) => "로그인 상태 모름".into(),
                    };
                    println!("{:<6} {:<12} {}  {}", a.id.as_str(), a.version.as_deref().unwrap_or("-"), state, a.path.as_deref().unwrap_or(""));
                }
            }
            0
        }
        "install" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            action(o_core::agents::install(provider), rest)
        }
        "open-login" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            action(o_core::agents::open_login(&root, provider, rest.iter().any(|a| a == "--clean")), rest)
        }
        _ => fail(USAGE),
    }
}

/// Prints an install or sign-in result: JSON for the app (exit 0 once printed), or words.
fn action(result: o_core::agents::ActionResult, args: &[String]) -> i32 {
    if args.iter().any(|a| a == "--json") {
        println!("{}", json!(result));
        return 0;
    }
    println!("{}", result.message);
    if !result.ok {
        if let Some(log) = &result.log {
            eprintln!("{log}");
        }
    }
    i32::from(!result.ok)
}

/// `o about [--json]`: this build's version and where O keeps its state.
fn about(home: &Path, as_json: bool) -> i32 {
    let version = env!("CARGO_PKG_VERSION");
    let agents_home = clean::default_root();
    if as_json {
        println!("{}", json!({ "version": version, "home": home.to_string_lossy(), "agentsHome": agents_home.to_string_lossy() }));
    } else {
        println!("o {version}\n상태 폴더   {}\n에이전트 프로필 {}", home.to_string_lossy(), agents_home.to_string_lossy());
    }
    0
}

fn doctor(root: &Path, as_json: bool) -> i32 {
    let profiles = clean::doctor(root);
    let cwd = std::env::current_dir().unwrap_or_default();
    let real_home = store::user_home();
    let ancestors: Vec<String> = clean::ancestor_instructions(&cwd, &real_home).iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let inside_home = clean::inside_home(&cwd, &real_home);
    let is_clean = profiles.iter().all(|p| p.issues.is_empty());
    if as_json {
        println!("{}", json!({ "clean": is_clean, "profiles": profiles, "ancestorInstructions": ancestors, "insideHome": inside_home }));
    } else {
        for p in &profiles {
            let state = if p.issues.is_empty() { "깨끗함" } else { "문제 있음" };
            println!("{:<6} {state:<6} {}  {}", p.provider.as_str(), p.login.detail, p.home);
            for issue in &p.issues {
                println!("         - {issue}");
            }
        }
        if !ancestors.is_empty() {
            println!("\n주의: 이 폴더 위의 지시 파일을 깨끗한 Claude도 읽습니다(프로필이 제외하는 것은 홈 폴더의 파일뿐):");
            for file in &ancestors {
                println!("  - {file}");
            }
        }
        if inside_home {
            println!("\n주의: 지금 폴더가 홈 폴더({}) 안입니다. 테스트 프로젝트는 홈 폴더 밖에 두세요.", real_home.to_string_lossy());
        }
    }
    if is_clean {
        0
    } else {
        1
    }
}

fn fail(message: &str) -> i32 {
    eprintln!("{message}");
    1
}

fn print_violations(violations: &[Violation]) {
    eprintln!("차단됨 — 정책 위반으로 이 계획은 실행되지 않았습니다:");
    for v in violations {
        let card = v.card_id.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
        eprintln!("  - [{}]{card} {}", v.rule, v.message);
    }
    eprintln!("위반을 우회하지 말고 사용자에게 이 내용을 그대로 보고하세요.");
}

fn status(home: &Path, run_id: Option<&str>) -> i32 {
    let ids: Vec<String> = match run_id {
        Some(id) => vec![id.to_string()],
        None => {
            let all = store::list_runs(home);
            all[all.len().saturating_sub(5)..].to_vec()
        }
    };
    if ids.is_empty() {
        println!("실행 기록이 없습니다.");
    }
    for id in ids {
        let Some(run) = snapshot(&store::read_events(home, &id)) else { continue };
        println!("{id}  {}  [{}]", run.title, run.status.as_str());
        for c in run.cards {
            let activity = collapse_spaces(&c.activity);
            println!("  {:<8} {} ({} · {}) {}", c.state.as_str(), c.id, c.provider, c.label, js_head(&activity, 100));
        }
    }
    0
}

fn events(home: &Path, run_id: &str, follow: bool) -> i32 {
    let mut printed = 0;
    loop {
        let list = store::read_event_values(home, run_id);
        for e in list.iter().skip(printed) {
            println!("{e}");
        }
        printed = list.len();
        if !follow || list.iter().any(|e| e.get("t").and_then(Value::as_str) == Some("run.end")) {
            return 0;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// `o runs --json [--limit N] [--project <folder>]`: always JSON, the app's shape.
fn runs(home: &Path, args: &[String]) -> i32 {
    let mut query = RunQuery { limit: 50, project: None };
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => {}
            "--limit" => match items.next().and_then(|n| n.parse().ok()) {
                Some(limit) => query.limit = limit,
                None => return fail("--limit 뒤에 0 이상의 정수가 필요합니다"),
            },
            "--project" => match items.next() {
                Some(project) => query.project = Some(project),
                None => return fail("--project 뒤에 폴더 경로가 필요합니다"),
            },
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{USAGE}")),
        }
    }
    match store::read_runs(home, &query) {
        Ok(runs) => {
            println!("{}", json!({ "runs": runs }));
            0
        }
        Err(error) => fail(&error.to_string()),
    }
}

fn stop(home: &Path, run_id: &str) -> i32 {
    match runner::stop_run(home, run_id) {
        Ok(true) => {
            println!("중지 요청: {run_id}");
            0
        }
        Ok(false) => fail(&format!("실행을 찾지 못했습니다: {run_id}")),
        Err(error) => fail(&error.to_string()),
    }
}

fn message(home: &Path, args: &[String]) -> i32 {
    let (verb, rest) = match args.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("", &[][..]),
    };
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut words: Vec<&str> = Vec::new();
    let mut items = rest.iter();
    while let Some(arg) = items.next() {
        if arg == "--all" {
            flags.insert("all".into(), "1".into());
        } else if let Some(name) = arg.strip_prefix("--") {
            flags.insert(name.into(), items.next().cloned().unwrap_or_default());
        } else {
            words.push(arg);
        }
    }
    let speaker = match team::speaker(home, Caller::from_env(flags.get("run").cloned(), flags.get("as").cloned())) {
        Ok(speaker) => speaker,
        Err(message) => return fail(&message),
    };
    let send = |to: &str, text: &str| match team::send(home, &speaker, to, text) {
        Ok(()) => {
            println!("보냄: {} → {to}", speaker.me);
            0
        }
        Err(Refused::Policy(violations)) => {
            print_violations(&violations);
            2
        }
        Err(Refused::Io(error)) => fail(&error.to_string()),
    };
    match verb {
        "send" => send(flags.get("to").map_or("all", String::as_str), &words.join(" ")),
        "post" => {
            // For CLIs that may only run fixed commands: the message waits in the card's outbox file.
            let path = team::outbox(home, &speaker);
            let Ok(content) = std::fs::read(&path) else {
                return fail(&format!("보낼 메시지 파일이 없습니다: {}", path.to_string_lossy()));
            };
            let (to, text) = team::parse_outbox(&String::from_utf8_lossy(&content));
            let code = send(&to, &text);
            if code == 0 {
                let _ = std::fs::remove_file(&path);
            }
            code
        }
        "read" => match team::read(home, &speaker, flags.contains_key("all")) {
            Ok(fresh) => {
                if fresh.is_empty() {
                    println!("새 메시지가 없습니다.");
                }
                for m in fresh {
                    println!("{}", m.line());
                }
                0
            }
            Err(error) => fail(&error.to_string()),
        },
        _ => fail(USAGE),
    }
}

fn usage(home: &Path, as_json: bool, cached: bool) -> i32 {
    let reading: Value = if cached {
        match probe::load_usage(home) {
            Ok(saved) => saved.unwrap_or_else(|| json!(UsageReading::empty())),
            Err(error) => return fail(&error.to_string()),
        }
    } else {
        let reading = probe::read_usage();
        if let Err(error) = probe::save_usage(home, &reading) {
            eprintln!("{error}");
        }
        json!(reading)
    };
    if as_json {
        println!("{}", serde_json::to_string_pretty(&reading).unwrap_or_default());
        return 0;
    }
    let reading: UsageReading = match serde_json::from_value(reading) {
        Ok(reading) => reading,
        Err(error) => return fail(&format!("usage.json: {error}")),
    };
    for w in &reading.windows {
        let name = match w.window.as_str() {
            "five_hour" => "5시간",
            "seven_day" => "주간",
            other => other,
        };
        let percent = (w.used * 100.0 + 0.5).floor();
        println!("{:<6} {:<22} {:<4} {:>3}% 사용  {} 초기화", w.provider.as_str(), w.group.as_deref().unwrap_or(""), name, percent as i64, w.resets_at);
    }
    for u in &reading.unavailable {
        println!("{:<6} 조회 실패: {}", u.provider, u.reason);
    }
    0
}

/// Run ids name folders, so a malformed one stops here.
fn run_id_arg(value: Option<&str>) -> Option<&str> {
    match value {
        Some(id) if is_run_id(id) => Some(id),
        Some(id) => {
            eprintln!("실행 id 형식이 아닙니다: {id}");
            None
        }
        None => {
            eprintln!("{USAGE}");
            None
        }
    }
}
