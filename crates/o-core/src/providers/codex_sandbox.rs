//! Whether the project_O profile's Codex can run a sandboxed card's commands on Windows
//! (ADR-0040 개정 1). Neither the `[windows] sandbox` line in config.toml nor Codex's own
//! `windowsSandbox/readiness` proves it: on some PCs `codex exec -s workspace-write` hangs before
//! its first command starts (https://github.com/openai/codex/issues/50725). Only running one does.
//!
//! The check, run only when a person asks (Settings → 에이전트, `o agents setup codex`):
//! 1. `codex app-server`: `windowsSandbox/readiness`, and `windowsSandbox/setupStart`
//!    (`unelevated`) until `windowsSandbox/setupCompleted` when it is not ready.
//! 2. One card-like `codex exec` in a fresh folder, asked to run `cmd /c "echo o-sandbox-ok> probe.txt"`
//!    (one small model call). It passes only when probe.txt says `o-sandbox-ok`.
//! 3. The outcome is kept beside the profile home (`<agents root>/codex/sandbox-check.json`) with
//!    the Codex version and a hash of the profile's config.toml.
//!
//! Codex is ready only while that record says ok and both still match (`decide`).

use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::agents::catalog::connection;
use crate::sys::proc::{self, Contained};
use crate::sys::resolve::resolve_cli;

/// The record's file name, next to the profile home (outside CODEX_HOME, so Codex never reads it).
pub const RECORD: &str = "sandbox-check.json";

/// What the probe card writes, and the one command it is asked to run.
pub const PROBE_FILE: &str = "probe.txt";
pub const PROBE_TEXT: &str = "o-sandbox-ok";
pub const PROBE_COMMAND: &str = "cmd /c \"echo o-sandbox-ok> probe.txt\"";

/// The open Codex bug behind a probe that never starts its command.
pub const HANG_ISSUE: &str = "https://github.com/openai/codex/issues/50725";

/// Why Codex cannot do a card's work on the project_O profile yet; shown by the policy refusal,
/// the lead's prompt, `o agents doctor` and Settings → 에이전트.
pub const SANDBOX_NOT_SET: &str = "샌드박스 설정 필요(codex): project_O 프로필의 Codex에 Windows 샌드박스 설정([windows] sandbox)이 없어 Codex 카드가 파일을 고치거나 명령을 실행하지 못합니다. 설정 → 에이전트에서 'Codex 샌드박스 점검'을 누르거나 o agents setup codex를 실행해 설정과 점검을 마친 뒤 다시 실행하세요. 그때까지는 다른 에이전트(예: claude)에 맡기세요.";
pub const NOT_CHECKED: &str = "샌드박스 점검 필요(codex): project_O 프로필의 Codex가 샌드박스 안에서 실제로 명령을 실행하는지 아직 점검하지 않았습니다. 설정 → 에이전트에서 'Codex 샌드박스 점검'을 누르거나 o agents setup codex를 실행하세요. 그때까지는 다른 에이전트(예: claude)에 맡기세요.";
pub const CHANGED: &str = "샌드박스 재점검 필요(codex): 마지막 점검 뒤 Codex 버전이나 project_O 프로필의 config.toml이 바뀌었습니다. 설정 → 에이전트에서 'Codex 샌드박스 점검'을 누르거나 o agents setup codex로 다시 점검하세요. 그때까지는 다른 에이전트(예: claude)에 맡기세요.";

/// "샌드박스 점검 실패(codex): …" — the last check failed, with what it found.
pub fn failed_reason(record: &Record) -> String {
    format!(
        "샌드박스 점검 실패(codex): {} (점검 {}). 그때까지는 다른 에이전트(예: claude)에 맡기세요. 고친 뒤에는 설정 → 에이전트의 'Codex 샌드박스 점검'이나 o agents setup codex로 다시 점검하세요.",
        record.detail, record.checked_at
    )
}

/// One check's outcome (`<agents root>/codex/sandbox-check.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub ok: bool,
    /// The first line `codex --version` printed.
    pub version: String,
    /// SHA-256 of the profile's config.toml after the check (`config_hash`).
    pub config_hash: String,
    pub checked_at: String,
    /// In Korean: what passed, or why it failed.
    pub detail: String,
}

/// `<agents root>/codex/sandbox-check.json` for the profile home `<agents root>/codex/home`.
pub fn record_path(home: &Path) -> PathBuf {
    home.parent().unwrap_or(home).join(RECORD)
}

pub fn read_record(home: &Path) -> Option<Record> {
    serde_json::from_slice(&fs::read(record_path(home)).ok()?).ok()
}

/// Writes the record whole or not at all (a temporary file renamed over it).
pub fn write_record(home: &Path, record: &Record) -> io::Result<()> {
    let path = record_path(home);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension(format!("{}.tmp", crate::run::store::new_run_id(Utc::now())));
    fs::write(&temp, serde_json::to_vec_pretty(record).map_err(io::Error::other)?)?;
    fs::rename(&temp, &path).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
}

/// SHA-256 (hex) of the profile's `.codex/config.toml` without its `[projects…]` tables; a
/// missing file hashes as empty. Codex adds a `[projects.'<folder>']` table itself when a card
/// runs in a new project (run 20261110-131428-sqm9), which says nothing about the sandbox: with
/// those tables hashed, every first card in a project would ask for the check again.
pub fn config_hash(home: &Path) -> String {
    let text = String::from_utf8_lossy(&fs::read(home.join(".codex").join("config.toml")).unwrap_or_default()).into_owned();
    Sha256::digest(without_projects(&text).as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

/// `toml` without its `[projects]` and `[projects.…]` tables (header to the next table header)
/// and without the blank lines around them.
fn without_projects(toml: &str) -> String {
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            skipping = trimmed.starts_with("[projects.") || trimmed.starts_with("[projects]");
        }
        if !skipping && !trimmed.is_empty() {
            kept.push(trimmed);
        }
    }
    kept.join("\n")
}

/// `sandbox = …` inside `[windows]`, or the dotted `windows.sandbox = …`.
static WINDOWS_SANDBOX: LazyLock<regex::Regex> =
    // Inside the table: any lines that do not open another table, then the key. A comment
    // after the header and brackets inside a value or a comment do not end the table.
    LazyLock::new(|| regex::Regex::new(r"(?m)^\s*windows\.sandbox\s*=|^[ \t]*\[windows\][ \t]*(?:#[^\n]*)?\r?\n(?:[ \t]*(?:[^\[\s][^\n]*)?\r?\n)*?[ \t]*sandbox\s*=").unwrap());

/// Whether config.toml names a Windows sandbox at all: the cheap first reason.
pub fn sandbox_selected(home: &Path) -> bool {
    WINDOWS_SANDBOX.is_match(&fs::read_to_string(home.join(".codex").join("config.toml")).unwrap_or_default())
}

/// Ready (`None`) only when the last check passed and neither the config nor Codex changed since.
/// `version` is asked last, and only when everything else already matches.
pub fn decide(record: Option<&Record>, config_hash: &str, version: impl FnOnce() -> Option<String>) -> Option<String> {
    let Some(record) = record else { return Some(NOT_CHECKED.into()) };
    if !record.ok {
        return Some(failed_reason(record));
    }
    if record.config_hash != config_hash || version().as_deref() != Some(record.version.as_str()) {
        return Some(CHANGED.into());
    }
    None
}

/// The profile at `home`'s reason not to take a sandboxed card (Windows only; the caller checks the
/// platform): no sandbox selection, then the recorded check (`decide`).
pub fn not_ready(home: &Path, version: impl FnOnce() -> Option<String>) -> Option<String> {
    if !sandbox_selected(home) {
        return Some(SANDBOX_NOT_SET.into());
    }
    decide(read_record(home).as_ref(), &config_hash(home), version)
}

/// How long `installed_version` trusts its last answer.
const VERSION_FOR: Duration = Duration::from_secs(60);

/// The resolved CLI's command line and its last `--version` line, with when it was asked.
type KnownVersion = (String, Instant, Option<String>);
static VERSION: LazyLock<Mutex<Option<KnownVersion>>> = LazyLock::new(|| Mutex::new(None));

fn cli_key() -> Option<(String, crate::sys::resolve::CliCommand)> {
    let cli = resolve_cli("codex")?;
    Some((format!("{:?} {:?}", cli.command, cli.prefix), cli))
}

/// The first line of `codex --version` for the Codex on PATH, asked at most once a minute: the
/// policy, the runner and each lead turn ask for readiness often.
pub fn installed_version() -> Option<String> {
    let (key, cli) = cli_key()?;
    let mut cached = VERSION.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((known, at, version)) = cached.as_ref() {
        if *known == key && at.elapsed() < VERSION_FOR {
            return version.clone();
        }
    }
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).arg("--version");
    let version = version_of(&mut command, DEADLINES.reply);
    *cached = Some((key, Instant::now(), version.clone()));
    version
}

/// After a check: readiness compares with the version the check just saw.
pub fn remember_version(version: &str) {
    if let Some((key, _)) = cli_key() {
        *VERSION.lock().unwrap_or_else(|e| e.into_inner()) = Some((key, Instant::now(), Some(version.to_string())));
    }
}

fn version_of(command: &mut Command, deadline: Duration) -> Option<String> {
    let (status, out, err) = proc::capture(command, deadline).ok()?;
    let text = if out.trim().is_empty() { err } else { out };
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    status.success().then(|| line.chars().take(80).collect())
}

/// How long each step may take.
#[derive(Clone, Copy, Debug)]
pub struct Deadlines {
    /// `--version` and each app-server answer.
    pub reply: Duration,
    /// Codex's own sandbox setup, until `windowsSandbox/setupCompleted`.
    pub setup: Duration,
    /// The probe card, start to exit.
    pub probe: Duration,
}

pub const DEADLINES: Deadlines = Deadlines { reply: Duration::from_secs(20), setup: Duration::from_secs(300), probe: Duration::from_secs(120) };

/// What the app-server said before the probe.
#[derive(Debug, PartialEq)]
pub enum Setup {
    /// `windowsSandbox/readiness` was already `ready`.
    AlreadyReady,
    /// It was not; the unelevated setup ran and reported success.
    Completed,
}

fn send(output: &mut impl Write, message: &Value) -> Result<(), String> {
    writeln!(output, "{message}").and_then(|_| output.flush()).map_err(|e| format!("codex app-server에 쓰지 못했습니다: {e}"))
}

/// Reads app-server lines until `found` takes one, or the deadline passes.
fn wait<T>(lines: &Receiver<String>, until: Instant, what: &str, mut found: impl FnMut(&Value) -> Option<Result<T, String>>) -> Result<T, String> {
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let line = match lines.recv_timeout(left) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => return Err(format!("codex app-server가 {what}에 시간 안에 답하지 않았습니다")),
            Err(RecvTimeoutError::Disconnected) => return Err(format!("codex app-server가 {what}에 답하기 전에 끝났습니다")),
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if let Some(result) = found(&message) {
            return result;
        }
    }
}

/// The answer to request `id`: its result, or its error's message.
fn answer(message: &Value, id: u64) -> Option<Result<Value, String>> {
    if message.get("id").and_then(Value::as_u64) != Some(id) || message.get("method").is_some() {
        return None;
    }
    Some(match message.get("error").filter(|e| !e.is_null()) {
        Some(error) => Err(error.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| error.to_string())),
        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
    })
}

/// The Windows sandbox handshake over a connected app-server (lines in, JSON lines out):
/// `initialize`, `initialized`, `windowsSandbox/readiness`, and when that is not `ready`,
/// `windowsSandbox/setupStart` (`unelevated`, in `cwd`) until `windowsSandbox/setupCompleted`.
pub fn setup_over(lines: &Receiver<String>, mut output: impl Write, cwd: &Path, deadlines: &Deadlines) -> Result<Setup, String> {
    send(&mut output, &json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"project_o_sandbox_check","title":null,"version":env!("CARGO_PKG_VERSION")},"capabilities":null}}))?;
    wait(lines, Instant::now() + deadlines.reply, "initialize", |m| answer(m, 1))
        .map_err(|e| format!("Codex app-server를 시작하지 못했습니다: {e}"))?;
    send(&mut output, &json!({"method":"initialized"}))?;
    send(&mut output, &json!({"id":2,"method":"windowsSandbox/readiness"}))?;
    let readiness = wait(lines, Instant::now() + deadlines.reply, "windowsSandbox/readiness", |m| answer(m, 2))
        .map_err(|e| format!("Codex가 샌드박스 준비 상태를 알려 주지 않았습니다: {e}"))?;
    let status = readiness.get("status").and_then(Value::as_str).unwrap_or_default().to_string();
    if status == "ready" {
        return Ok(Setup::AlreadyReady);
    }
    send(&mut output, &json!({"id":3,"method":"windowsSandbox/setupStart","params":{"mode":"unelevated","cwd":cwd.to_string_lossy()}}))?;
    // The completion may come before or after setupStart's own answer.
    let mut started = false;
    let mut completed: Option<Result<(), String>> = None;
    let until = Instant::now() + deadlines.setup;
    loop {
        let step = wait(lines, until, "windowsSandbox/setupStart", |m| {
            if let Some(reply) = answer(m, 3) {
                return Some(reply.map(|r| (Some(r.get("started").and_then(Value::as_bool).unwrap_or(false)), None)));
            }
            if m.get("method").and_then(Value::as_str) == Some("windowsSandbox/setupCompleted") {
                let p = m.get("params").cloned().unwrap_or(Value::Null);
                let done = if p.get("success").and_then(Value::as_bool) == Some(true) {
                    Ok(())
                } else {
                    Err(p.get("error").and_then(Value::as_str).filter(|e| !e.trim().is_empty()).unwrap_or("Codex가 사유를 알려 주지 않았습니다").to_string())
                };
                return Some(Ok((None, Some(done))));
            }
            None
        })
        .map_err(|e| format!("Codex 샌드박스 설정({status})을 마치지 못했습니다: {e}"))?;
        match step {
            (Some(false), _) => return Err(format!("Codex가 샌드박스 설정({status})을 시작하지 않았습니다")),
            (Some(true), _) => started = true,
            (None, Some(done)) => completed = Some(done),
            (None, None) => {}
        }
        if let Some(done) = &completed {
            if let Err(error) = done {
                return Err(format!("Codex 샌드박스 설정이 실패했습니다: {error}"));
            }
            if started {
                return Ok(Setup::Completed);
            }
        }
    }
}

/// The probe card's prompt.
pub fn probe_prompt() -> String {
    format!(
        "This is an automated sandbox check. Run exactly this one shell command in the current folder, then stop. \
         Do not run anything else, do not edit any other file, and do not explain.\n\n{PROBE_COMMAND}\n"
    )
}

/// Whether the probe card did its one command: `file` is probe.txt's text (if any), `stdout` the
/// `codex exec --json` stream, `code` its exit code. `Err` says in Korean what happened instead.
pub fn probe_outcome(code: Option<i64>, stdout: &str, file: Option<&str>) -> Result<(), String> {
    if file.is_some_and(|text| text.contains(PROBE_TEXT)) {
        return Ok(());
    }
    let mut error = None;
    let mut refused = None;
    let mut message = None;
    for line in stdout.lines() {
        let Ok(event) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match event.get("type").and_then(Value::as_str) {
            Some("turn.failed" | "error") => {
                let text = event.pointer("/error/message").or_else(|| event.get("message")).and_then(Value::as_str).unwrap_or_default();
                if !text.trim().is_empty() {
                    error = Some(text.trim().to_string());
                }
            }
            Some("item.completed") => {
                let item = event.get("item").cloned().unwrap_or(Value::Null);
                match item.get("type").and_then(Value::as_str) {
                    Some("command_execution") if item.get("status").and_then(Value::as_str) != Some("completed") || item.get("exit_code").and_then(Value::as_i64).is_some_and(|c| c != 0) => {
                        let output = item.get("aggregated_output").and_then(Value::as_str).unwrap_or_default().trim().to_string();
                        let status = item.get("status").and_then(Value::as_str).unwrap_or("?");
                        refused = Some(if output.is_empty() { format!("명령 상태 {status}") } else { format!("명령 상태 {status}: {output}") });
                    }
                    Some("agent_message") => message = item.get("text").and_then(Value::as_str).map(|t| t.trim().to_string()),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    let said = error.or(refused).or(message).map(|t| t.chars().take(300).collect::<String>());
    let tail = said.map(|t| format!(": {t}")).unwrap_or_default();
    Err(match code {
        Some(0) => format!("Codex가 끝났지만 점검 파일({PROBE_FILE})에 {PROBE_TEXT}가 없습니다{tail}"),
        Some(code) => format!("Codex 점검 실행이 실패했습니다(종료 코드 {code}){tail}"),
        None => format!("Codex 점검 실행이 비정상 종료했습니다{tail}"),
    })
}

/// What a probe that never finished means.
pub fn hang_detail(deadline: Duration) -> String {
    format!(
        "Codex가 샌드박스 안에서 첫 명령을 시작하기 전에 멈췄습니다({}초 안에 끝나지 않아 중지했습니다). Windows 샌드박스에서 알려진 Codex 버그입니다: {HANG_ISSUE}",
        deadline.as_secs()
    )
}

/// Makes the official CLI with these arguments: the clean environment a card uses, in production.
pub type MakeCommand<'a> = &'a dyn Fn(&[&str]) -> io::Result<Command>;

/// Runs the whole check for the profile at `home` and keeps its record. `make` builds the Codex
/// command (`agents::check_setup`: `clean::command`, the CLI and environment a real card gets).
pub fn check(home: &Path, make: MakeCommand<'_>, deadlines: &Deadlines) -> io::Result<Record> {
    let version = make(&["--version"]).ok().and_then(|mut c| version_of(&mut c, deadlines.reply)).unwrap_or_default();
    let probe_dir = home.parent().unwrap_or(home).join(format!("sandbox-probe-{}", crate::run::store::new_run_id(Utc::now())));
    fs::create_dir_all(&probe_dir)?;
    let outcome = if version.is_empty() {
        Err("Codex 버전을 확인하지 못했습니다(codex --version이 답하지 않음)".to_string())
    } else {
        sandbox_setup(make, &probe_dir, deadlines).and_then(|_| probe(make, &probe_dir, deadlines))
    };
    // The sandbox may have changed the folder's access rules; a folder left behind is harmless.
    let _ = fs::remove_dir_all(&probe_dir);
    let record = Record {
        ok: outcome.is_ok(),
        version,
        config_hash: config_hash(home),
        checked_at: crate::sys::js::iso(Utc::now()),
        detail: outcome.err().unwrap_or_else(|| "Codex가 샌드박스 안에서 명령을 실행하고 파일을 썼습니다.".into()),
    };
    write_record(home, &record)?;
    Ok(record)
}

/// Codex's own setup through a short-lived `codex app-server` in `cwd`.
fn sandbox_setup(make: MakeCommand<'_>, cwd: &Path, deadlines: &Deadlines) -> Result<Setup, String> {
    let mut command = make(&["app-server"]).map_err(|e| format!("codex app-server를 시작하지 못했습니다: {e}"))?;
    command.current_dir(cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut server = Contained::spawn(&mut command).map_err(|e| format!("codex app-server를 시작하지 못했습니다: {e}"))?;
    let stdin = server.child.stdin.take().ok_or("codex app-server stdin")?;
    let stdout = server.child.stdout.take().ok_or("codex app-server stdout")?;
    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let result = setup_over(&rx, stdin, cwd, deadlines);
    // Ends the server and anything it started (`Contained` closes its job).
    drop(server);
    result
}

/// One card-like `codex exec` in `dir`: the fixed card arguments, the project folder, the prompt on stdin.
fn probe(make: MakeCommand<'_>, dir: &Path, deadlines: &Deadlines) -> Result<(), String> {
    let folder = dir.to_string_lossy().into_owned();
    let mut args: Vec<&str> = connection("codex").headless.fixed.to_vec();
    args.extend(["-C", folder.as_str(), "--skip-git-repo-check", "-"]);
    let mut command = make(&args).map_err(|e| format!("codex exec를 시작하지 못했습니다: {e}"))?;
    command.current_dir(dir);
    match proc::capture_input(&mut command, Some(&probe_prompt()), deadlines.probe) {
        Ok((status, stdout, _)) => {
            let file = fs::read_to_string(dir.join(PROBE_FILE)).ok();
            probe_outcome(proc::exit_code(status), &stdout, file.as_deref())
        }
        Err(error) if error == proc::TIMED_OUT => Err(hang_detail(deadlines.probe)),
        Err(error) => Err(format!("codex exec {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(ok: bool) -> Record {
        Record { ok, version: "codex-cli 1.2.3".into(), config_hash: "abc".into(), checked_at: "2026-11-10T00:00:00.000Z".into(), detail: "멈춤".into() }
    }

    #[test]
    fn ready_only_with_a_matching_successful_check() {
        let v = || Some("codex-cli 1.2.3".to_string());
        assert_eq!(decide(None, "abc", v).as_deref(), Some(NOT_CHECKED));
        let failed = decide(Some(&record(false)), "abc", || panic!("a failed check needs no version"));
        assert!(failed.as_deref().is_some_and(|r| r.starts_with("샌드박스 점검 실패(codex): 멈춤")), "{failed:?}");
        assert_eq!(decide(Some(&record(true)), "abc", v), None);
        assert_eq!(decide(Some(&record(true)), "abd", || panic!("a changed config needs no version")).as_deref(), Some(CHANGED));
        assert_eq!(decide(Some(&record(true)), "abc", || Some("codex-cli 1.2.4".into())).as_deref(), Some(CHANGED));
        assert_eq!(decide(Some(&record(true)), "abc", || None).as_deref(), Some(CHANGED));
    }

    #[test]
    fn project_tables_codex_adds_do_not_change_the_config_hash() {
        let base = "[windows]\nsandbox = \"unelevated\"\n";
        let trusted = "[windows]\r\nsandbox = \"unelevated\"\r\n\r\n[projects.'c:\\work\\app']\r\ntrust_level = \"trusted\"\r\n";
        assert_eq!(without_projects(trusted), without_projects(base));
        // What follows a projects table is kept, and any other change still shows.
        assert_eq!(without_projects("[projects.'a']\ntrust_level = \"trusted\"\n[features]\nhooks = true\n"), "[features]\nhooks = true");
        assert_ne!(without_projects("[windows]\nsandbox = \"elevated\"\n"), without_projects(base));
        assert_ne!(without_projects("[windows]\nsandbox = \"unelevated\"\n[projects_extra]\nx = 1\n"), without_projects(base));
    }

    #[test]
    fn the_windows_sandbox_is_found_however_the_table_is_written() {
        let set = |toml: &str| WINDOWS_SANDBOX.is_match(toml);
        assert!(set("windows.sandbox = \"elevated\"\n"));
        assert!(set("[windows]\nsandbox = \"elevated\"\n"));
        assert!(set("model = \"x\"\r\n\r\n[windows] # 샌드박스\r\n# [see docs]\r\nnotes = [\"a\", \"b\"]\r\n\r\n  sandbox = \"unelevated\"\r\n"));
        // Another table's key, a commented key, and no key at all.
        assert!(!set("[windows]\nnotes = 1\n[other]\nsandbox = \"elevated\"\n"));
        assert!(!set("[windows]\n# sandbox = \"elevated\"\n"));
        assert!(!set("[windows_extra]\nsandbox = \"elevated\"\n"));
        assert!(!set(""));
    }

    #[test]
    fn the_probe_passes_only_when_its_file_says_so() {
        assert_eq!(probe_outcome(Some(0), "", Some("o-sandbox-ok \r\n")), Ok(()));
        // Even a failing exit: the command ran, which is what the check asks.
        assert_eq!(probe_outcome(Some(1), "", Some("o-sandbox-ok\r\n")), Ok(()));
        let declined = r#"{"type":"item.completed","item":{"type":"command_execution","status":"declined","aggregated_output":"rejected: blocked by policy"}}"#;
        let said = format!("{declined}\n{}", r#"{"type":"item.completed","item":{"type":"agent_message","text":"막혔습니다"}}"#);
        let error = probe_outcome(Some(0), &said, None).unwrap_err();
        assert!(error.contains("probe.txt") && error.contains("rejected: blocked by policy"), "{error}");
        let failed = r#"{"type":"turn.failed","error":{"message":"usage limit"}}"#;
        let error = probe_outcome(Some(1), failed, Some("other")).unwrap_err();
        assert!(error.contains("종료 코드 1") && error.ends_with(": usage limit"), "{error}");
        assert!(probe_outcome(None, "not json", None).unwrap_err().contains("비정상 종료"));
        assert_eq!(probe_outcome(Some(0), r#"{"type":"item.completed","item":{"type":"agent_message","text":"끝"}}"#, None).unwrap_err(), "Codex가 끝났지만 점검 파일(probe.txt)에 o-sandbox-ok가 없습니다: 끝");
        assert!(hang_detail(Duration::from_secs(120)).contains(HANG_ISSUE));
    }

    /// A fake app-server on a thread: answers each request it reads with `reply(method)`'s lines.
    fn server(reply: impl Fn(&str, &Value) -> Vec<Value> + Send + 'static) -> (Receiver<String>, mpsc::Sender<String>, thread::JoinHandle<Vec<String>>) {
        let (to_client, lines) = mpsc::channel::<String>();
        let (to_server, requests) = mpsc::channel::<String>();
        let keep = to_client.clone();
        let handle = thread::spawn(move || {
            let mut seen = Vec::new();
            for request in requests {
                let message: Value = serde_json::from_str(&request).unwrap();
                let method = message["method"].as_str().unwrap_or_default().to_string();
                for line in reply(&method, &message) {
                    let _ = keep.send(line.to_string());
                }
                seen.push(method);
            }
            seen
        });
        drop(to_client);
        (lines, to_server, handle)
    }

    /// The client writes JSON lines to a `Write`; this one forwards each line to the fake server.
    struct Lines(mpsc::Sender<String>, Vec<u8>);
    impl Write for Lines {
        fn write(&mut self, data: &[u8]) -> io::Result<usize> {
            self.1.extend_from_slice(data);
            while let Some(end) = self.1.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = self.1.drain(..=end).collect();
                let _ = self.0.send(String::from_utf8_lossy(&line).trim().to_string());
            }
            Ok(data.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn quick() -> Deadlines {
        Deadlines { reply: Duration::from_secs(10), setup: Duration::from_secs(10), probe: Duration::from_secs(10) }
    }

    fn handshake(status: &'static str, completion: Option<Value>, completion_first: bool) -> (Result<Setup, String>, Vec<String>) {
        let (lines, to_server, handle) = server(move |method, message| match method {
            "initialize" => {
                assert_eq!(message["params"]["clientInfo"]["name"], "project_o_sandbox_check");
                vec![json!({"id":1,"result":{"userAgent":"fake"}})]
            }
            "windowsSandbox/readiness" => vec![json!({"method":"thread/started","params":{}}), json!({"id":2,"result":{"status":status}})],
            "windowsSandbox/setupStart" => {
                assert_eq!(message["params"]["mode"], "unelevated");
                assert_eq!(message["params"]["cwd"], "C:/probe");
                let done = completion.clone().map(|c| json!({"method":"windowsSandbox/setupCompleted","params":c}));
                let started = json!({"id":3,"result":{"started":true}});
                match (done, completion_first) {
                    (Some(done), true) => vec![done, started],
                    (Some(done), false) => vec![started, done],
                    (None, _) => vec![started],
                }
            }
            _ => vec![],
        });
        let result = setup_over(&lines, Lines(to_server, Vec::new()), Path::new("C:/probe"), &quick());
        (result, handle.join().unwrap())
    }

    #[test]
    fn a_ready_sandbox_needs_no_setup() {
        let (result, seen) = handshake("ready", None, false);
        assert_eq!(result, Ok(Setup::AlreadyReady));
        assert_eq!(seen, ["initialize", "initialized", "windowsSandbox/readiness"]);
    }

    #[test]
    fn a_sandbox_not_set_up_runs_the_unelevated_setup_to_its_end() {
        for first in [false, true] {
            let (result, seen) = handshake("notConfigured", Some(json!({"mode":"unelevated","success":true,"error":null})), first);
            assert_eq!(result, Ok(Setup::Completed));
            assert_eq!(seen, ["initialize", "initialized", "windowsSandbox/readiness", "windowsSandbox/setupStart"]);
        }
        let (result, _) = handshake("updateRequired", Some(json!({"mode":"unelevated","success":false,"error":"helper failed"})), false);
        assert_eq!(result, Err("Codex 샌드박스 설정이 실패했습니다: helper failed".into()));
    }

    #[test]
    fn a_silent_or_finished_server_fails_by_its_deadline() {
        let (lines, to_server, handle) = server(|method, _| if method == "initialize" { vec![json!({"id":1,"result":{}})] } else { vec![] });
        let short = Deadlines { reply: Duration::from_millis(50), ..quick() };
        let error = setup_over(&lines, Lines(to_server, Vec::new()), Path::new("C:/probe"), &short).unwrap_err();
        assert!(error.contains("windowsSandbox/readiness") && error.contains("시간 안에"), "{error}");
        handle.join().unwrap();

        let (to_client, lines) = mpsc::channel::<String>();
        drop(to_client);
        let error = setup_over(&lines, io::sink(), Path::new("C:/probe"), &quick()).unwrap_err();
        assert!(error.contains("끝났습니다"), "{error}");

        let (lines, to_server, handle) = server(|method, _| match method {
            "initialize" => vec![json!({"id":1,"result":{}})],
            "windowsSandbox/readiness" => vec![json!({"id":2,"error":{"code":-32601,"message":"method not found"}})],
            _ => vec![],
        });
        let error = setup_over(&lines, Lines(to_server, Vec::new()), Path::new("C:/probe"), &quick()).unwrap_err();
        assert!(error.ends_with("method not found"), "{error}");
        handle.join().unwrap();
    }
}
