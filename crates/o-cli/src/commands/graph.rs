//! `o graph` (ADR-0033).

use std::path::Path;

use o_core::graph::{self, settings};
use serde_json::{json, Value};

use super::{fail, Command};

pub const GRAPH: Command = Command {
    name: "graph",
    usage: "  o graph view|build|check|ask|callers|blast|skeleton|map|deep|settings [옵션]\n                             프로젝트 코드 그래프 조회·인덱스·설정",
    run: graph_main,
};

fn graph_main(home: &Path, args: &[String]) -> i32 {
    let Some((command, rest)) = args.split_first() else { return fail("graph 하위 명령이 필요합니다") };
    let agent = ["O_AGENT_ROLE", "O_CARD_ID"].iter().any(|key| std::env::var_os(key).is_some_and(|v| !v.is_empty()));
    if agent && (matches!(command.as_str(), "build" | "deep") || (command == "settings" && rest.iter().any(|a| a != "--json"))) {
        eprintln!("에이전트 안에서는 작업을 시작하거나 그래프 설정을 바꿀 수 없습니다. 사용자가 실행해 주세요.");
        return 2;
    }
    match run(home, command, rest) {
        Ok(value) => { println!("{value}"); i32::from(command == "build" && value.get("ok") == Some(&Value::Bool(false))) }
        Err(error) => fail(&error),
    }
}

fn run(home: &Path, command: &str, args: &[String]) -> Result<Value, String> {
    let (options, positional) = parse(command, args)?;
    let project = options.get("project").map(String::as_str);
    let project = if command == "view" || command == "settings" { project } else {
        Some(project.ok_or("--project 뒤에 폴더 경로가 필요합니다")?)
    };
    let path = project.map(Path::new);
    if let Some(project) = project { o_core::lead::check_folder(home, project)?; }
    match command {
        "view" => graph::view(home, project).map(|v| json!(v)),
        "build" => graph::build(home, project.unwrap()),
        "check" => graph::grafly::check(home, project.unwrap()),
        "ask" => graph::grafly::ask(home, project.unwrap(), positional.unwrap()),
        "callers" | "blast" | "skeleton" | "map" => {
            let path = path.unwrap();
            let wiring = graph::wiring::load(path)?.ok_or("그래프 인덱스가 없습니다. o graph build를 실행하세요")?;
            match command {
                "callers" => {
                    let direction = if options.get("direction").is_some_and(|v| v == "out") { graph::query::Direction::Out } else { graph::query::Direction::In };
                    let depth = match options.get("depth").map(String::as_str).unwrap_or("1") { "all" => None, n => Some(n.parse().unwrap()) };
                    graph::query::callers(&wiring, positional.unwrap(), direction, depth).map(|v| json!(v))
                }
                "blast" => {
                    let mut changed = graph::git::changed(path)?;
                    if let Some(base) = options.get("base") {
                        if base.starts_with('-') { return Err("base는 git 참조여야 합니다".into()); }
                        let mut git = std::process::Command::new("git");
                        git.current_dir(path).args(["diff", "--relative", "--name-only", "--no-renames", "-z", base, "--", "."]);
                        let (status, out, err) = o_core::sys::proc::capture(&mut git, std::time::Duration::from_secs(20))?;
                        if !status.success() { return Err(format!("git diff: {err}")); }
                        changed.extend(out.split('\0').filter(|p| !p.is_empty()).map(str::to_owned));
                    }
                    let changed: Vec<String> = changed.into_iter().collect();
                    let depth = options.get("depth").map(|s| s.parse().unwrap()).unwrap_or(2);
                    Ok(json!(graph::query::blast(&wiring, &changed, depth)))
                }
                "skeleton" => Ok(json!(graph::query::skeleton(&wiring, positional.unwrap()))),
                _ => Ok(json!(graph::query::map(&wiring))),
            }
        }
        "deep" => {
            let saved = settings::read(home);
            let provider = options.get("provider").map(String::as_str).unwrap_or(&saved.deep_provider);
            let provider = o_core::run::plan::ProviderId::parse(provider).ok_or("provider는 codex 또는 claude여야 합니다")?;
            let root = (options.get("profile").map(String::as_str).unwrap_or("project-o") == "project-o").then(o_core::agents::clean::default_root);
            graph::deep::run(home, project.unwrap(), provider, options.get("model").map(String::as_str), root.as_deref()).map(|v| json!(v))
        }
        "settings" => {
            if options.is_empty() { return Ok(json!(settings::read(home))); }
            let mut change = settings::Change::default();
            for (key, value) in &options {
                match key.as_str() {
                    "effects" => change.effects = Some(on_off(value)?),
                    "particles" => change.particles = Some(on_off(value)?),
                    "warn-deep" => change.warn_deep = Some(on_off(value)?),
                    "deep-provider" => change.deep_provider = Some(value.clone()),
                    "codex-model" => change.codex_model = Some(value.clone()),
                    "claude-model" => change.claude_model = Some(value.clone()),
                    "max-nodes" => change.max_nodes = Some(value.parse().map_err(|_| "max-nodes는 숫자여야 합니다")?),
                    "depth" => change.depth = Some(value.parse().map_err(|_| "depth는 숫자여야 합니다")?),
                    _ => unreachable!(),
                }
            }
            settings::update(home, change).map(|v| json!(v))
        }
        _ => Err(format!("알 수 없는 graph 명령: {command}")),
    }
}

fn on_off(value: &str) -> Result<bool, String> {
    match value { "on" => Ok(true), "off" => Ok(false), _ => Err("on 또는 off가 필요합니다".into()) }
}

fn parse<'a>(command: &str, args: &'a [String]) -> Result<(std::collections::HashMap<String, String>, Option<&'a str>), String> {
    let allowed: &[&str] = match command {
        "view" | "build" | "check" | "ask" | "skeleton" | "map" => &["project"],
        "callers" => &["project", "direction", "depth"],
        "blast" => &["project", "base", "depth"],
        "deep" => &["project", "provider", "model", "profile"],
        "settings" => &["effects", "particles", "deep-provider", "codex-model", "claude-model", "warn-deep", "max-nodes", "depth"],
        _ => return Err(format!("알 수 없는 graph 명령: {command}")),
    };
    let mut options = std::collections::HashMap::new();
    let mut positional = None;
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        if arg == "--json" { continue; }
        if let Some(key) = arg.strip_prefix("--") {
            if !allowed.contains(&key) { return Err(format!("알 수 없는 인자: {arg}")); }
            let value = items.next().filter(|v| !v.is_empty() && !v.starts_with("--")).ok_or_else(|| format!("{arg} 뒤에 값이 필요합니다"))?;
            if options.insert(key.to_string(), value.clone()).is_some() { return Err(format!("중복 인자: {arg}")); }
        } else if matches!(command, "ask" | "callers" | "skeleton") && positional.replace(arg.as_str()).is_none() {
        } else { return Err(format!("알 수 없는 인자: {arg}")); }
    }
    if matches!(command, "ask" | "callers" | "skeleton") && positional.is_none() { return Err("질의·심볼 또는 파일이 필요합니다".into()); }
    if options.get("direction").is_some_and(|v| v != "in" && v != "out") { return Err("direction은 in 또는 out이어야 합니다".into()); }
    if options.get("provider").is_some_and(|v| v != "codex" && v != "claude") { return Err("provider는 codex 또는 claude여야 합니다".into()); }
    if options.get("profile").is_some_and(|v| v != "project-o" && v != "own") { return Err("profile은 project-o 또는 own이어야 합니다".into()); }
    if matches!(command, "callers" | "blast") && options.get("depth").is_some_and(|v| !(command == "callers" && v == "all") && !v.parse::<usize>().is_ok_and(|n| n > 0)) { return Err("depth는 양의 정수여야 합니다".into()); }
    Ok((options, positional))
}
