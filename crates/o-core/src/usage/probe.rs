//! `o usage`: asks the official CLIs for their subscription windows, with
//! hidden windows and timeouts, and keeps the last reading in `usage.json`.

use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::sys::js::now_iso;
use crate::sys::proc;
use crate::sys::resolve::resolve_cli;
use crate::run::store::to_pretty;
use crate::usage::{parse_agy_usage, parse_codex_rate_limits, Unavailable, UsageReading, UsageWindow};

const TIMEOUT: Duration = Duration::from_secs(20);

/// Stops a probe's process tree however the probe ends.
struct Probe {
    child: Child,
    _tree: Option<proc::KillOnClose>,
}

impl Drop for Probe {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Codex: a short-lived, read-only app-server answers account/rateLimits/read (no model turn).
fn codex_windows() -> Result<Vec<UsageWindow>, String> {
    let cli = resolve_cli("codex").ok_or("codex CLI를 찾지 못했습니다")?;
    let mut command = Command::new(&cli.command);
    command
        .args(&cli.prefix)
        .args(["-c", "features.plugins=false", "-s", "read-only", "-a", "never", "app-server"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = proc::hide_window(&mut command).spawn().map_err(|e| e.to_string())?;
    let tree = proc::contain(&child);
    let mut probe = Probe { child, _tree: tree };
    let mut stdin = probe.child.stdin.take().ok_or("codex app-server stdin")?;
    let stdout = probe.child.stdout.take().ok_or("codex app-server stdout")?;

    let (tx, rx) = mpsc::channel::<String>();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut send = |message: Value| -> Result<(), String> {
        let mut body = serde_json::Map::new();
        body.insert("jsonrpc".into(), json!("2.0"));
        if let Value::Object(fields) = message {
            body.extend(fields);
        }
        writeln!(stdin, "{}", Value::Object(body)).and_then(|_| stdin.flush()).map_err(|e| e.to_string())
    };
    send(json!({ "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "project-o", "version": "0.0.1" } } }))?;

    let timeout = || "codex app-server가 시간 안에 답하지 않았습니다".to_string();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let left = deadline.checked_duration_since(Instant::now()).ok_or_else(timeout)?;
        let line = rx.recv_timeout(left).map_err(|_| timeout())?;
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        match message.get("id").and_then(Value::as_f64) {
            Some(1.0) => {
                send(json!({ "method": "initialized", "params": {} }))?;
                send(json!({ "id": 2, "method": "account/rateLimits/read", "params": {} }))?;
            }
            Some(2.0) => {
                if let Some(error) = message.get("error").filter(|e| !e.is_null()) {
                    let text = error.get("message").and_then(Value::as_str).unwrap_or("rate limit read failed");
                    return Err(text.to_string());
                }
                return Ok(parse_codex_rate_limits(message.get("result").unwrap_or(&Value::Null)));
            }
            _ => {}
        }
    }
}

/// Antigravity: print-mode /usage reports quota without a model turn (agy 1.1.11 and later).
fn agy_windows() -> Result<Vec<UsageWindow>, String> {
    let cli = resolve_cli("agy").ok_or("agy CLI를 찾지 못했습니다")?;
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).args(AGY_USAGE).env("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
    let (status, out, _) = capture(&mut command, TIMEOUT + Duration::from_secs(10)).map_err(|e| format!("agy /usage {e}"))?;
    if !status.success() {
        let code = proc::exit_code(status).map(|c| c.to_string()).unwrap_or_else(|| "시간 초과".into());
        return Err(format!("agy /usage 실패 ({code})"));
    }
    Ok(parse_agy_usage(&out))
}

/// agy's quota report in print mode: no model turn.
pub const AGY_USAGE: [&str; 6] = ["-p", "/usage", "--output-format", "json", "--print-timeout", "20s"];

/// Runs a short, non-interactive command with no window and a deadline, and
/// returns its exit status, stdout and stderr. A command still running at the
/// deadline is ended with its tree.
pub fn capture(command: &mut Command, timeout: Duration) -> Result<(ExitStatus, String, String), String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = proc::hide_window(command).spawn().map_err(|e| format!("실행 실패: {e}"))?;
    let tree = proc::contain(&child);
    let mut probe = Probe { child, _tree: tree };
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut out);
            }
            String::from_utf8_lossy(&out).into_owned()
        })
    };
    let stdout = drain(probe.child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = drain(probe.child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let deadline = Instant::now() + timeout;
    let status = loop {
        match probe.child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => return Err("실패 (시간 초과)".into()),
            Err(e) => return Err(format!("실행 실패: {e}")),
        }
    };
    // Ends anything the command left behind holding the pipes open.
    drop(probe);
    Ok((status, stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default()))
}

/// Both CLIs at once; windows and failures keep the order codex, agy.
pub fn read_usage() -> UsageReading {
    let mut reading = UsageReading { at: now_iso(), windows: Vec::new(), unavailable: Vec::new() };
    let (codex, agy) = thread::scope(|s| {
        let codex = s.spawn(codex_windows);
        let agy = s.spawn(agy_windows);
        (codex.join(), agy.join())
    });
    for (provider, outcome) in [("codex", codex), ("agy", agy)] {
        match outcome.unwrap_or_else(|_| Err("내부 오류".into())) {
            Ok(windows) => reading.windows.extend(windows),
            Err(reason) => reading.unavailable.push(Unavailable { provider: provider.into(), reason }),
        }
    }
    reading
}

pub fn save_usage(home: &Path, reading: &UsageReading) -> io::Result<()> {
    fs::create_dir_all(home)?;
    fs::write(home.join("usage.json"), to_pretty(reading)?)?;
    // The daily budget counts today's spend from these snapshots.
    let at = crate::sys::js::parse_time(&reading.at).unwrap_or_else(chrono::Utc::now);
    crate::usage::budget::record(home, &reading.windows, at)
}

/// The last saved reading as written, without asking any CLI.
pub fn load_usage(home: &Path) -> io::Result<Option<Value>> {
    let path = home.join("usage.json");
    if !path.exists() {
        return Ok(None);
    }
    serde_json::from_str(&fs::read_to_string(path)?).map(Some).map_err(io::Error::other)
}
