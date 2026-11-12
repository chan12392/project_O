//! `o agents …`: the official agent CLIs — clean test profiles (see `o_core::agents::clean`),
//! install and sign-in status, the official install and sign-in.

use std::path::Path;
use std::process::Command as Process;

use o_core::agents::{self, catalog, clean, ActionResult};
use o_core::providers;
use o_core::run::plan::ProviderId;
use o_core::run::store;
use serde_json::json;

use super::{fail, help, Command};

pub const AGENTS: Command = Command {
    name: "agents",
    usage: "  o agents path              깨끗한 에이전트 프로필(개인 훅·스킬·MCP·전역 지시 없음) 위치
  o agents login <claude|codex|agy|opencode>   그 프로필로 공식 로그인 (CLI마다 한 번)
  o agents shell <claude|codex|opencode> [인자…]   지금 폴더에서 깨끗한 CLI를 대화형으로 실행
  o agents doctor [--json]   로그인 상태와 프로필 오염 검사 (깨끗하면 종료 코드 0)
  o agents status [--clean] [--json]   CLI별 설치·버전·로그인 상태 (--clean: 깨끗한 프로필 기준)
  o agents install <claude|codex|agy|opencode|devin> [--json]   공식 npm 패키지로 설치 (없으면 공식 안내)
  o agents open-login <claude|codex|agy|opencode|devin> [--clean] [--json]   새 콘솔 창에서 공식 로그인
  o agents setup <codex> [--json]   창 없이 Codex의 Windows 샌드박스 설정을 마치고 카드처럼 명령 하나를 실제로 실행해 점검 (모델 호출 1번, 결과는 프로필 옆 sandbox-check.json)
  o agents terminal [--json]   독립 터미널 열기
  o agents list [--all] [--json]   다른 에이전트 CLI 목록과 이 PC 감지(--version만 실행). --all 없이는 연결된 것만
  o agents models <opencode> [--json]   그 CLI의 모델 목록 (작업카드 model 값)",
    run,
};

fn provider_arg(value: Option<&String>) -> Option<ProviderId> {
    let found = value.and_then(|v| ProviderId::parse(v));
    if found.is_none() {
        eprintln!("에이전트를 {} 중에서 지정하세요\n\n{}", ProviderId::ALL.map(ProviderId::as_str).join(", "), help());
    }
    found
}

/// Runs a CLI attached to this console and passes on its exit code.
fn attached(command: std::io::Result<Process>) -> i32 {
    match command.and_then(|mut c| c.status()) {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => fail(&error.to_string()),
    }
}

fn run(home: &Path, args: &[String]) -> i32 {
    let root = clean::default_root();
    let (verb, rest) = match args.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("", &[][..]),
    };
    if matches!(verb, "status" | "install" | "open-login" | "setup" | "login" | "shell") {
        o_core::sys::resolve::ensure_paths();
    }
    match verb {
        "path" => {
            for provider in ProviderId::ALL {
                match providers::get(provider).profile_unsupported() {
                    Some(why) => println!("{:<6} 프로필 미지원 — {why}", provider.as_str()),
                    None => println!("{:<6} {}", provider.as_str(), clean::profile_home(&root, provider).to_string_lossy()),
                }
            }
            0
        }
        "login" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            match providers::get(provider).login_args() {
                Some(args) => attached(clean::command(&root, provider, args)),
                None => {
                    let name = providers::get(provider).official().name;
                    println!("{name}는 이 PC의 시스템 로그인(OS 자격 증명 저장소)을 그대로 씁니다. 따로 로그인하지 않습니다.");
                    let state = clean::login_state(&root, provider);
                    println!("{:<6} {}", provider.as_str(), state.detail);
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
            let list = agents::status(&root, rest.iter().any(|a| a == "--clean"));
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(list));
            } else {
                for a in &list {
                    let state = match (a.installed, a.logged_in) {
                        (true, _) if a.profile_unsupported.is_some() => format!("프로필 미지원 — {}", a.profile_unsupported.as_deref().unwrap_or_default()),
                        (true, Some(true)) if a.profile_not_ready.is_some() => format!("로그인됨 · {}", a.profile_not_ready.as_deref().unwrap_or_default()),
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
            action(agents::install(provider), rest)
        }
        "open-login" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            action(agents::open_login(&root, provider, rest.iter().any(|a| a == "--clean")), rest)
        }
        "setup" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            if !rest.iter().any(|a| a == "--json") {
                eprintln!("{} 프로필을 설정하고 실제 명령으로 점검합니다(몇 분 걸릴 수 있고, Codex 모델 호출이 한 번 있습니다)…", provider.as_str());
            }
            action(agents::check_setup(home, &root, provider), rest)
        }
        "terminal" => action(agents::open_terminal(), rest),
        "list" => {
            let all = rest.iter().any(|a| a == "--all");
            let list: Vec<_> = catalog::list().into_iter().filter(|e| all || e.status == "connected").collect();
            if rest.iter().any(|a| a == "--json") {
                println!("{}", json!(list));
            } else {
                for e in &list {
                    let found = match (&e.version, e.detected) {
                        (Some(v), _) => v.clone(),
                        (None, true) => "감지됨".into(),
                        (None, false) => "-".into(),
                    };
                    let status = match e.status {
                        "connected" => "연결됨".to_string(),
                        "planned" => format!("연결 예정({})", e.order.unwrap_or_default()),
                        _ => format!("지원 안 함 — {}", e.reason),
                    };
                    let profile = if e.profile_supported { "" } else { " · 프로필 미지원" };
                    println!("{:<10} {:<24} {:<28} {status}{profile}", e.id, e.name, found);
                }
            }
            0
        }
        "models" => {
            let Some(provider) = provider_arg(rest.first()) else { return 1 };
            let own = rest.iter().any(|a| a == "--own") || rest.windows(2).any(|w| w[0] == "--profile" && w[1] == "own");
            let cwd = rest.windows(2).find(|w| w[0] == "--project").map(|w| std::path::PathBuf::from(&w[1])).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
            match providers::models::catalog(home, provider, (!own).then_some(root.as_path()), &cwd, rest.iter().any(|a| a == "--refresh")) {
                Err(error) => fail(&error),
                Ok(catalog) => {
                    if rest.iter().any(|a| a == "--json") {
                        println!("{}", json!(catalog));
                    } else {
                        for m in &catalog.models {
                            println!("{}", m.id);
                        }
                        if let Some(error) = catalog.error { eprintln!("{error}"); }
                    }
                    0
                }
            }
        }
        _ => fail(help()),
    }
}

/// Prints an install or sign-in result: JSON for the app (exit 0 once printed), or words.
fn action(result: ActionResult, args: &[String]) -> i32 {
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
            if !p.api_key_env.is_empty() {
                let consequence = if p.provider == ProviderId::Claude && p.api_key_env.contains(&"ANTHROPIC_API_KEY") { "헤드리스 실행에서 키를 사용하면 구독 대신 API로 과금됩니다" } else { "인증 방식과 과금이 달라질 수 있습니다" };
                println!("         - 환경변수 {}: {consequence}.", p.api_key_env.join(", "));
            }
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
            println!("\n안내: 지금 폴더가 사용자 홈({}) 안입니다. 상위 폴더의 프로젝트 지시 파일은 깨끗한 프로필에서도 읽힐 수 있습니다.", real_home.to_string_lossy());
        }
    }
    if is_clean {
        0
    } else {
        1
    }
}
