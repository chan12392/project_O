//! What the desktop app shows about the official agent CLIs: whether each is
//! installed and signed in, and the official ways to install it and sign in.
//! Install runs only an official, non-interactive package command (npm); the
//! vendors' install scripts are shown to the person, never fetched and run here.
//! Sign-in opens the CLI's own login in a console window the person drives.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::clean::{self, LoginState};
use crate::plan::ProviderId;
use crate::probe;
use crate::proc;
use crate::resolve::{resolve_cli, which, CliCommand};
use crate::store::user_home;

/// One CLI as the Settings and onboarding screens show it (`AgentStatus` in app/src/backend.ts).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatus {
    pub id: ProviderId,
    pub name: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `None` (null) when the CLI's status check could not tell.
    pub logged_in: Option<bool>,
    pub install_hint: String,
    pub can_install: bool,
    pub install_url: String,
}

/// What an install or sign-in did (`ActionResult` in app/src/backend.ts).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActionResult {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
}

impl ActionResult {
    fn new(ok: bool, message: impl Into<String>) -> Self {
        ActionResult { ok, message: message.into(), log: None }
    }
}

/// Each vendor's own install channels for Windows.
struct Official {
    name: &'static str,
    /// The npm package the vendor documents (`npm install -g <package>`), if any.
    npm_package: Option<&'static str>,
    /// The vendor's PowerShell installer, for the person to run.
    script: &'static str,
    url: &'static str,
}

fn official(provider: ProviderId) -> Official {
    match provider {
        // https://code.claude.com/docs/en/setup (native installer, WinGet, npm)
        ProviderId::Claude => Official {
            name: "Claude Code",
            npm_package: Some("@anthropic-ai/claude-code"),
            script: "irm https://claude.ai/install.ps1 | iex",
            url: "https://code.claude.com/docs/en/setup",
        },
        // https://github.com/openai/codex (npm, Homebrew, Windows installer)
        ProviderId::Codex => Official {
            name: "Codex",
            npm_package: Some("@openai/codex"),
            script: "irm https://chatgpt.com/codex/install.ps1 | iex",
            url: "https://github.com/openai/codex",
        },
        // https://antigravity.google/docs/getting-started (installer script only)
        ProviderId::Agy => Official {
            name: "Antigravity",
            npm_package: None,
            script: "irm https://antigravity.google/cli/install.ps1 | iex",
            url: "https://antigravity.google/docs/getting-started",
        },
    }
}

/// npm without cmd.exe: the shim's script when it can be read, otherwise the
/// `node_modules/npm/bin/npm-cli.js` next to npm.cmd, with the node.exe beside it.
pub fn npm() -> Option<CliCommand> {
    if let Some(cli) = resolve_cli("npm") {
        return Some(cli);
    }
    let dir = which("npm")?.parent()?.to_path_buf();
    let script = dir.join("node_modules").join("npm").join("bin").join("npm-cli.js");
    if !script.is_file() {
        return None;
    }
    let local = dir.join(if cfg!(windows) { "node.exe" } else { "node" });
    let node = if local.is_file() { local } else { which("node").filter(|n| n.extension().is_none_or(|e| e.eq_ignore_ascii_case("exe")))? };
    Some(CliCommand { command: node, prefix: vec![script.to_string_lossy().into_owned()] })
}

fn cli_command(cli: &CliCommand, args: &[&str]) -> Command {
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).args(args);
    command
}

/// The first line `<cli> --version` prints, within a few seconds.
fn version(provider: ProviderId, cli: &CliCommand) -> Option<String> {
    let mut command = cli_command(cli, &["--version"]);
    if provider == ProviderId::Agy {
        command.env("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
    }
    let (status, out, err) = probe::capture(&mut command, Duration::from_secs(15)).ok()?;
    let text = if out.trim().is_empty() { err } else { out };
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    status.success().then(|| line.chars().take(80).collect())
}

/// Signed in, per the CLI's own status command: with the clean profile (as
/// `o agents doctor` asks) or with the person's normal setup.
fn login(root: &Path, provider: ProviderId, cli: &CliCommand, clean: bool) -> LoginState {
    if clean {
        return clean::login_state(root, provider);
    }
    let mut command = cli_command(cli, clean::status_args(provider));
    if provider == ProviderId::Agy {
        command.env("AGY_CLI_DISABLE_AUTO_UPDATE", "true");
    }
    let home = user_home();
    if home.is_dir() {
        command.current_dir(home);
    }
    match probe::capture(&mut command, Duration::from_secs(30)) {
        Ok((status, out, err)) => clean::read_login(provider, status.success(), &out, &err),
        Err(error) => LoginState { logged_in: None, detail: format!("상태 확인 {error}") },
    }
}

/// One CLI's status. A CLI on PATH that cannot be started without a shell counts as not installed.
pub fn status_of(root: &Path, provider: ProviderId, clean: bool) -> AgentStatus {
    let o = official(provider);
    let npm_ready = o.npm_package.is_some() && npm().is_some();
    let install_hint = match o.npm_package {
        Some(package) if npm_ready => format!("npm install -g {package}"),
        _ => format!("PowerShell: {}", o.script),
    };
    let found: Option<PathBuf> = which(provider.as_str());
    let cli = resolve_cli(provider.as_str());
    let (version, logged_in) = match &cli {
        Some(cli) => thread::scope(|s| {
            let version = s.spawn(|| version(provider, cli));
            let login = s.spawn(|| login(root, provider, cli, clean));
            (version.join().ok().flatten(), login.join().ok().and_then(|l| l.logged_in))
        }),
        None => (None, None),
    };
    AgentStatus {
        id: provider,
        name: o.name.into(),
        installed: cli.is_some(),
        version,
        path: found.filter(|_| cli.is_some()).map(|p| p.to_string_lossy().into_owned()),
        logged_in,
        install_hint,
        can_install: npm_ready,
        install_url: o.url.into(),
    }
}

/// Every CLI at once (each status check may take a while), in the order claude, codex, agy.
pub fn status(root: &Path, clean: bool) -> Vec<AgentStatus> {
    thread::scope(|s| {
        let checks: Vec<_> = ProviderId::ALL.into_iter().map(|p| s.spawn(move || status_of(root, p, clean))).collect();
        checks.into_iter().map(|check| check.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic))).collect()
    })
}

const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const LOG_LIMIT: usize = 20_000;

/// Runs the vendor's npm install (`npm install -g <package>`) with no window and
/// waits for it. Without npm, or for a CLI npm does not carry, it says where the
/// official installer is instead.
pub fn install(provider: ProviderId) -> ActionResult {
    let o = official(provider);
    let manual = |why: &str| ActionResult::new(false, format!("{why} 공식 안내({})를 따라 설치하세요: {}", o.url, o.script));
    let Some(package) = o.npm_package else {
        return manual(&format!("{}는 공식 설치 스크립트로만 설치합니다.", o.name));
    };
    let Some(npm) = npm() else {
        return manual("npm을 찾지 못했습니다.");
    };
    let mut command = cli_command(&npm, &["install", "-g", package]);
    match probe::capture(&mut command, INSTALL_TIMEOUT) {
        Ok((status, out, err)) => {
            let log = format!("{out}\n{err}").trim().to_string();
            let tail: String = log.chars().rev().take(LOG_LIMIT).collect::<Vec<_>>().into_iter().rev().collect();
            let message = if status.success() {
                format!("{} 설치를 마쳤습니다 (npm install -g {package}).", o.name)
            } else {
                let code = proc::exit_code(status).map(|c| c.to_string()).unwrap_or_else(|| "?".into());
                format!("{} 설치에 실패했습니다 (npm 종료 코드 {code}). 로그를 확인하세요.", o.name)
            };
            ActionResult { ok: status.success(), message, log: Some(tail).filter(|l| !l.is_empty()) }
        }
        Err(error) => ActionResult::new(false, format!("npm install -g {package} {error}")),
    }
}

/// Opens the CLI's official sign-in (`claude auth login`, `codex login`) in a
/// console window of its own, with the clean profile when `clean`. Antigravity
/// signs in through the system: its official page opens instead.
pub fn open_login(root: &Path, provider: ProviderId, clean: bool) -> ActionResult {
    let o = official(provider);
    let Some(args) = clean::login_args(provider) else {
        let opened = proc::open_url(o.url).is_ok();
        let then = if opened { "공식 안내 페이지를 열었습니다." } else { "공식 안내 페이지를 열지 못했습니다." };
        return ActionResult::new(opened, format!("Antigravity는 이 PC의 시스템 로그인(Google 계정)을 그대로 씁니다. {then} {}", o.url));
    };
    let Some(cli) = resolve_cli(provider.as_str()) else {
        return ActionResult::new(false, format!("{}가 설치되어 있지 않습니다.", o.name));
    };
    let mut argv = cli.prefix.clone();
    argv.extend(args.iter().map(|a| a.to_string()));
    let started = if clean {
        clean::environment(root, provider).and_then(|env| proc::spawn_console(&cli.command, &argv, Some(&env), Some(&clean::profile_home(root, provider))))
    } else {
        proc::spawn_console(&cli.command, &argv, None, None)
    };
    let shown = format!("{} {}", provider.as_str(), args.join(" "));
    match started {
        Ok(()) => {
            let profile = if clean { " (깨끗한 테스트 프로필)" } else { "" };
            ActionResult::new(true, format!("새 창에서 {} 공식 로그인({shown})을 진행하세요{profile}. 끝나면 상태를 다시 확인합니다.", o.name))
        }
        Err(error) => ActionResult::new(false, format!("{shown} 창을 열지 못했습니다: {error}")),
    }
}

/// Only an https address with a host goes to the browser.
pub fn is_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty() && !host.contains('@') && url.len() <= 2048 && !url.chars().any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '<' || c == '>')
}

/// Opens an https page in the person's browser; anything else is refused.
pub fn open_external(url: &str) -> Result<(), String> {
    if !is_https_url(url) {
        return Err("https 주소만 열 수 있습니다".into());
    }
    proc::open_url(url).map_err(|e| e.to_string())
}
