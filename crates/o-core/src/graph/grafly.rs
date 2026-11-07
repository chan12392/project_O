//! Non-LLM grafly operations; never invoke its Deep or vision modes.
use std::{path::Path, process::Command, time::Duration};
use serde::Serialize;
use serde_json::{json, Value};
use crate::sys::{proc::capture, resolve::{resolve_cli, which, CliCommand}};

#[derive(Clone, Debug, Serialize)]
pub struct Status {
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

fn command(args: &[&str], project: Option<&str>) -> Result<Command, String> {
    let cli = resolve_cli("grafly").ok_or("grafly가 설치되어 있지 않습니다")?;
    Ok(command_for(cli, args, project))
}

fn command_for(cli: CliCommand, args: &[&str], project: Option<&str>) -> Command {
    let mut command = Command::new(cli.command);
    command.args(cli.prefix).args(args);
    if let Some(project) = project { command.current_dir(project); }
    command.env("GRAFLY_NO_AUTO_INDEX", "1").env("GRAFLY_NO_AUTO_DEEP", "1");
    for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CODEX_API_KEY", "OPENAI_API_KEY"] { command.env_remove(key); }
    command
}

fn run(args: &[&str], project: Option<&str>, timeout: Duration) -> Result<(bool, String, String), String> {
    let (status, out, err) = capture(&mut command(args, project)?, timeout)?;
    Ok((status.success(), out, err))
}

fn success(outcome: (bool, String, String)) -> Result<String, String> {
    let (ok, out, err) = outcome;
    if ok { Ok(out) } else { Err(format!("grafly 실행 실패: {}", if err.trim().is_empty() { out.trim() } else { err.trim() })) }
}

pub fn status() -> Status {
    if which("grafly").is_none() { return Status { installed: false, version: None }; }
    match run(&["--version"], None, Duration::from_secs(10)) {
        Ok((true, out, _)) => Status { installed: true, version: out.lines().next().map(str::to_owned) },
        _ => Status { installed: true, version: None },
    }
}

pub fn build(home: &Path, project: &str) -> Result<Value, String> {
    super::check_project(home, project)?;
    let (ok, out, err) = run(&["build", project], Some(project), Duration::from_secs(600))?;
    let output = if err.trim().is_empty() { out } else { format!("{out}\n{err}") };
    let lines: Vec<_> = output.lines().collect();
    Ok(json!({"ok": ok, "output": lines[lines.len().saturating_sub(20)..].join("\n")}))
}

pub fn check(home: &Path, project: &str) -> Result<Value, String> {
    super::check_project(home, project)?;
    let (ok, out, err) = run(&["check", "--json"], Some(project), Duration::from_secs(60))?;
    let value: Value = serde_json::from_str(&out).map_err(|e| format!("grafly check: {e}: {err}"))?;
    Ok(json!({"fresh": ok, "result": value}))
}

pub fn ask(home: &Path, project: &str, query: &str) -> Result<Value, String> {
    super::check_project(home, project)?;
    if query.trim().is_empty() { return Err("검색어가 비었습니다".into()); }
    let out = success(run(&["ask", "--json", "--read-only", "--", query], Some(project), Duration::from_secs(60))?)?;
    serde_json::from_str(&out).map_err(|e| format!("grafly ask: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_command_reports_output_and_preserves_status() {
        assert_eq!(success((true, "answer".into(), String::new())).unwrap(), "answer");
        assert!(success((false, "".into(), "problem".into())).unwrap_err().contains("problem"));
    }
    #[test]
    fn empty_query_is_rejected() {
        let home = std::env::temp_dir().join("o-graph-empty-query-home");
        assert!(ask(&home, ".", "").is_err());
    }
    #[test]
    fn command_keeps_query_one_argument_and_disables_auto_deep() {
        let cli = CliCommand { command: "grafly".into(), prefix: vec![] };
        let command = command_for(cli, &["ask", "--json", "--read-only", "--", "a; b"], None);
        let args: Vec<_> = command.get_args().map(|v| v.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["ask", "--json", "--read-only", "--", "a; b"]);
        assert!(command.get_envs().any(|(k, v)| k == "GRAFLY_NO_AUTO_DEEP" && v.is_some_and(|v| v == "1")));
        assert!(command.get_envs().any(|(k, v)| k == "OPENAI_API_KEY" && v.is_none()));
    }
}
