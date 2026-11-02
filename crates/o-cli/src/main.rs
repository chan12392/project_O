//! `o`: argument parsing and printing. Everything that decides or touches
//! state lives in `o_core`; the guard commands live in `o_guard`.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use o_core::sys::js::{collapse_spaces, js_head};
use o_core::policy::{check_plan, Violation};
use o_core::run::index::RunQuery;
use o_core::run::snapshot::snapshot;
use o_core::run::store::{self, default_home, is_run_id, run_dir};
use o_core::run::team::{self, Caller, Refused};
use o_core::usage::UsageReading;
use o_core::run::plan::ProviderId;
use o_core::agents::clean;
use o_core::run::runner;
use o_core::sys::{mirror, proc};
use o_core::usage::probe;
use o_core::{review, sessions};
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
  o review <PR 번호|주소> [--provider claude|codex|agy] [--model M] [--effort E] [--clean] [--post] [--dry-run] [--detach]
                             지금 폴더 저장소의 PR을 gh로 읽어 에이전트 카드 하나로 검토 (파일 변경 없음)
                             --post: 끝나면 검토문을 PR 댓글로 게시 (승인·병합은 하지 않음)
                             --dry-run: 실행 없이 프롬프트 크기·고른 에이전트·정책 결과만 출력
                             --detach: 백그라운드로 시작하고 JSON 출력 (앱용)
  o context <질문> [--project <폴더>] [--limit N] [--json]
                             이전 작업 결과·팀 대화·메모에서 관련 내용 찾기 (카드도 읽기 전용으로 사용)
  o context add [--project <폴더>] \"내용\"   프로젝트 컨텍스트에 메모 추가
  o tokens [--project <폴더>] [--since 날짜] [--compare] [--json]
                             카드별 토큰(새 입력·캐시·출력)과 모드별 평균
                             --compare: 기본 하네스 vs 깨끗한 에이전트+세션 공유
  o usage [--json]           Codex·Antigravity 구독 한도를 각 CLI로 조회해 usage.json에 저장
  o usage --cached [--json]  조회하지 않고 저장된 usage.json만 출력
  o budget [--json] [--cached] [--percent 12|13|14]
                             에이전트별 오늘 사용 예산(주간 한도의 12~14%)·속도·effort 조절
                             (기록이 10분보다 오래되면 먼저 조회, --cached: 조회 안 함,
                             --percent: settings.json의 하루 예산 비율 저장)
  o runs --json [--limit N] [--project <폴더>] [--sessions [--since H]]
                             실행별 이벤트(최근 시작 순, 기본 50개) — 앱 화면용
                             --sessions: 최근 H시간(기본 24) Claude Code 세션도 실행으로 함께
  o sessions [--json] [--since H]   최근 H시간(기본 24) Claude Code 세션과 서브에이전트 (읽기 전용)
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
        Some("review") => review(&home, rest),
        Some("_review") => review_background(&home, rest),
        Some("usage") => usage(&home, has("--json"), has("--cached")),
        Some("budget") => budget(&home, rest),
        Some("runs") => runs(&home, rest),
        Some("sessions") => sessions(&home, rest),
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

/// `o agents path|login|shell|doctor`: clean agent profiles (see `o_core::agents::clean`).
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
            match o_core::providers::get(provider).login_args() {
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

/// `--since <hours>`: a positive number of hours.
fn since_arg(value: Option<&String>) -> Result<f64, i32> {
    match value.and_then(|h| h.parse::<f64>().ok()).filter(|h| h.is_finite() && *h > 0.0) {
        Some(hours) => Ok(hours),
        None => Err(fail("--since 뒤에 0보다 큰 시간(시간 단위)이 필요합니다")),
    }
}

/// `o sessions [--json] [--since H]`: this PC's recent Claude Code sessions, read-only.
fn sessions(home: &Path, args: &[String]) -> i32 {
    let mut since = sessions::DEFAULT_SINCE_HOURS;
    let mut as_json = false;
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--since" => match since_arg(items.next()) {
                Ok(hours) => since = hours,
                Err(code) => return code,
            },
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{USAGE}")),
        }
    }
    let list = sessions::read_sessions(&sessions::default_roots(), Some(&sessions::cache_path(home)), Utc::now(), sessions::hours(since));
    if as_json {
        println!("{}", json!({ "sessions": list }));
        return 0;
    }
    if list.is_empty() {
        println!("최근 {since}시간 동안 Claude Code 세션이 없습니다.");
    }
    for record in &list {
        let events: Vec<_> = record.events.iter().filter_map(|e| serde_json::from_value(e.clone()).ok()).collect();
        let Some(run) = snapshot(&events) else { continue };
        let count = |state: &str| run.cards.iter().filter(|c| c.state.as_str() == state).count();
        println!(
            "{}  [{}]  {}  서브에이전트 {} (진행 {} · 완료 {} · 실패 {} · 중지 {})  {}",
            run.run_id,
            run.status.as_str(),
            run.project,
            run.cards.len(),
            count("running"),
            count("done"),
            count("failed"),
            count("stopped"),
            js_head(&collapse_spaces(&run.title), 60)
        );
    }
    0
}

/// `o runs --json [--limit N] [--project <folder>] [--sessions [--since H]]`: always JSON, the app's shape.
fn runs(home: &Path, args: &[String]) -> i32 {
    let mut query = RunQuery { limit: 50, project: None };
    let mut with_sessions = false;
    let mut since = sessions::DEFAULT_SINCE_HOURS;
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => {}
            "--sessions" => with_sessions = true,
            "--since" => match since_arg(items.next()) {
                Ok(hours) => since = hours,
                Err(code) => return code,
            },
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
    let read = if with_sessions { sessions::read_runs_with_sessions(home, &query, sessions::hours(since)) } else { store::read_runs(home, &query) };
    match read {
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

/// `o review <pr> …`: reads the PR with gh in this folder, checks a one-card
/// review plan like `o dispatch`, then runs it here (or in the background with
/// `--detach`) and, with `--post`, comments the review on the PR.
fn review(home: &Path, args: &[String]) -> i32 {
    let args = match review::parse_args(args) {
        Ok(args) => args,
        Err(error) => return fail(&format!("{error}\n\n{USAGE}")),
    };
    let project = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(error) => return fail(&error.to_string()),
    };
    let gh = review::OfficialGh;
    let (pr, diff) = match review::fetch(&gh, &project, &args.selector) {
        Ok(found) => found,
        Err(error) => return fail(&format!("PR을 읽지 못했습니다: {error}")),
    };
    let author = review::authorship(home, &pr);
    let provider = args.provider.unwrap_or_else(|| review::choose_provider(author.provider));
    // Local files may fill in a cut diff only when this folder is the PR's head commit.
    let local_is_head = review::local_head(&project).is_some_and(|local| local == pr.head_ref_oid);
    let task = review::build_task(&pr, &diff, &review::new_nonce(), local_is_head);
    let project_text = project.to_string_lossy().into_owned();
    let input = review::build_plan(&project_text, pr.number, &task.text, provider, args.model.as_deref(), args.effort.as_deref());

    let now = Utc::now();
    let ctx = match store::policy_context(home, now) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&error.to_string()),
    };
    let checked = check_plan(&input, &ctx);
    let summary = || {
        let cut = if task.cut_files.is_empty() { String::new() } else { format!(", 잘린 파일 {}개", task.cut_files.len()) };
        let redacted = if task.redacted_lines > 0 { format!(", 정책 규칙으로 뺀 줄 {}개", task.redacted_lines) } else { String::new() };
        eprintln!("PR #{} {} — {}", pr.number, pr.title, pr.url);
        eprintln!(
            "검토할 커밋: head {} (base {}){}",
            review::short_sha(&pr.head_ref_oid),
            review::short_sha(&pr.base_ref_oid),
            if local_is_head { " · 이 폴더와 같음" } else { " · 이 폴더는 다른 커밋이라 로컬 파일로 보충하지 않음" }
        );
        eprintln!("작성: {} → 검토: {provider}{}", author.describe(), if args.provider.is_some() { " (지정)" } else { "" });
        eprintln!("프롬프트 {}자 (상한 {}) · diff {}자 중 {}자 포함{cut}{redacted}", o_core::sys::js::js_len(&task.text), o_core::policy::LIMITS.max_task_chars, task.diff_chars, task.diff_included);
    };
    if !args.detach || args.dry_run {
        summary();
    }
    let Some(plan) = checked.plan else {
        if !args.dry_run {
            if let Err(error) = store::record_blocked(home, now, input.get("title"), &checked.violations) {
                eprintln!("{error}");
            }
        }
        print_violations(&checked.violations);
        return 2;
    };
    if args.dry_run {
        println!("정책 통과 — 실행하지 않음 (--dry-run). 검토: {provider}, 프롬프트 {}자", o_core::sys::js::js_len(&task.text));
        return 0;
    }

    let run_id = store::new_run_id(now);
    if let Err(error) = review::create_run(home, &run_id, &plan, args.clean) {
        return fail(&error.to_string());
    }
    let events = run_dir(home, &run_id).join("events.jsonl");
    if args.detach {
        // The PR, its head commit and the post flag go to the background runner as arguments, out of the cards' reach.
        let number = pr.number.to_string();
        let mut background = vec!["_review", &run_id, &args.selector, &number, "--head", &pr.head_ref_oid];
        if args.post {
            background.push("--post");
        }
        let started = std::env::current_exe().and_then(|exe| proc::spawn_detached(Command::new(exe).args(&background).current_dir(&project)));
        if let Err(error) = started {
            return fail(&format!("실행기를 시작하지 못했습니다: {error}"));
        }
        let message = format!(
            "PR #{} 검토를 시작했습니다 · {provider}{} · 커밋 {}{}",
            pr.number,
            if args.clean { " (깨끗한 에이전트)" } else { "" },
            review::short_sha(&pr.head_ref_oid),
            if args.post { " · 끝나면 PR 댓글로 게시 (그사이 새 커밋이 오면 게시하지 않음)" } else { "" }
        );
        println!(
            "{}",
            json!({ "ok": true, "message": message, "runId": run_id, "pr": pr.number, "head": pr.head_ref_oid, "provider": provider, "post": args.post, "clean": args.clean, "events": events.to_string_lossy() })
        );
        return 0;
    }
    eprintln!("실행 {run_id} — 검토 중… (다른 창에서 o events {run_id} --follow)");
    review_run(home, &run_id, &args.selector, pr.number, Some(&pr.head_ref_oid), args.post, &project)
}

/// Runs the review card (policy checked again, like `o _run`) and finishes: saves, and posts when asked.
fn review_run(home: &Path, run_id: &str, selector: &str, number: u64, head: Option<&str>, post: bool, project: &Path) -> i32 {
    if let Err(message) = runner::run_detached(home, run_id) {
        return fail(&message);
    }
    match review::finish(home, run_id, selector, number, head, post, project, &review::OfficialGh) {
        Ok(done) => {
            if let Some(head) = &done.head {
                println!("검토한 커밋: {head}");
            }
            match (&done.review_file, &done.comment_url) {
                (_, Some(url)) => println!("검토 댓글을 게시했습니다: {url}"),
                (Some(file), None) => println!("검토문: {}\n게시하려면 --post를 붙여 다시 실행하거나 o status {run_id}로 확인하세요", file.to_string_lossy()),
                (None, None) => println!("검토 카드가 {}로 끝나 검토문이 없습니다. o status {run_id}", done.status.as_str()),
            }
            i32::from(done.review_file.is_none())
        }
        Err(error) => fail(&error),
    }
}

/// `o _review <runId> <pr> <number> --head <sha> [--post]`: the background half of `o review --detach`.
fn review_background(home: &Path, args: &[String]) -> i32 {
    let (Some(run_id), Some(selector), Some(number)) = (run_id_arg(args.first().map(String::as_str)), args.get(1), args.get(2).and_then(|n| n.parse().ok())) else {
        return fail(USAGE);
    };
    if !review::is_selector(selector) {
        return fail(&format!("PR 형식이 아닙니다: {selector}"));
    }
    let head = args.iter().position(|a| a == "--head").and_then(|i| args.get(i + 1)).filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()));
    let project = std::env::current_dir().unwrap_or_default();
    review_run(home, run_id, selector, number, head.map(String::as_str), args.iter().any(|a| a == "--post"), &project)
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

/// `o budget [--json] [--cached] [--percent N]`: today's usage budget per provider (ADR-0004).
fn budget(home: &Path, args: &[String]) -> i32 {
    let (mut as_json, mut cached) = (false, false);
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        match arg.as_str() {
            "--json" => as_json = true,
            "--cached" => cached = true,
            "--percent" => {
                let value = items.next().and_then(|v| v.parse::<f64>().ok()).map_or(Value::Null, |n| json!(n));
                if let Err(error) = o_core::usage::budget::write_percent(home, &value) {
                    return fail(&error);
                }
            }
            other => return fail(&format!("알 수 없는 인자: {other}\n\n{USAGE}")),
        }
    }
    if !cached {
        o_core::usage::budget::refresh_if_stale(home, Utc::now());
    }
    let report = match o_core::usage::budget::report(home, Utc::now()) {
        Ok(report) => report,
        Err(error) => return fail(&error.to_string()),
    };
    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
        return 0;
    }
    println!("하루 사용 예산: 주간 한도의 {}% (settings.json {})", report.percent, o_core::usage::budget::PERCENT_KEY);
    for b in &report.providers {
        let Some(spent) = b.spent else {
            println!("{:<6} 주간 기록 없음 — 조절하지 않음", b.provider.as_str());
            continue;
        };
        let group = b.group.as_deref().map(|g| format!(" ({g})")).unwrap_or_default();
        let pace = b.pace.map_or("-".to_string(), |p| format!("{p:.2}"));
        let efforts: Vec<String> = b.efforts.iter().map(|c| format!("{}→{}", c.from.as_deref().unwrap_or("미지정"), c.to)).collect();
        let adjust = match b.state {
            o_core::usage::budget::Pace::Over => format!("새 카드 거부 ({})", b.message),
            _ if efforts.is_empty() => "effort 그대로".to_string(),
            _ => format!("effort {}단계 낮춤: {}", b.steps_down, efforts.join(", ")),
        };
        println!(
            "{:<6} 주간 {:.1}% · 오늘 {spent:.1}/{:.0}%p · {:.1}시간 남음 · 속도 {pace}{group} — {adjust}",
            b.provider.as_str(),
            b.weekly_used.unwrap_or_default(),
            b.allowance,
            b.remaining_hours.unwrap_or_default(),
        );
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
