//! What the desktop app shows about the official agent CLIs: whether each is
//! installed and signed in, and the official ways to install it and sign in.
//! Install runs only an official, non-interactive package command (npm); the
//! vendors' install scripts are shown to the person, never fetched and run here.
//! Sign-in opens the CLI's own login in a console window the person drives.

pub mod billing;
pub mod catalog;
pub mod clean;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use serde::Serialize;

use crate::providers;
use crate::run::plan::ProviderId;
use crate::run::store::user_home;
use crate::sys::proc;
use crate::sys::resolve::{resolve_cli, which, CliCommand};
use clean::LoginState;

/// One CLI as the Settings and onboarding screens show it (`AgentStatus` in app/src/platform/backend/types.ts).
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
    pub api_key_env: Vec<&'static str>,
    pub install_hint: String,
    pub can_install: bool,
    pub install_url: String,
    /// Checked with the project_O profile: that profile's state (ADR-0015). Absent for the person's own setup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<clean::ProfileState>,
    /// With the project_O profile on: why this CLI cannot run on it (프로필 미지원). Its sign-in
    /// is then read from the person's own setup, and its cards are refused while the profile is on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_unsupported: Option<String>,
    /// With the project_O profile on: why this CLI's profile cannot do a card's work until the
    /// person finishes a one-time setup in the CLI (`clean::profile_not_ready`, ADR-0040).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_not_ready: Option<String>,
}

/// Authentication environment names understood by each CLI; values stay with the CLI.
pub fn auth_env_names(provider: ProviderId) -> &'static [&'static str] {
    match provider {
        ProviderId::Claude => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        ProviderId::Codex => &["CODEX_API_KEY", "OPENAI_API_KEY"],
        ProviderId::OpenCode => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"],
        ProviderId::Devin => &["WINDSURF_API_KEY"],
        ProviderId::Agy => &[],
    }
}

pub fn present_auth_env(provider: ProviderId) -> Vec<&'static str> {
    let names: Vec<_> = std::env::vars_os().map(|(name, _)| name).collect();
    auth_env_names(provider).iter().copied().filter(|expected| names.iter().any(|name| name.to_str().is_some_and(|name| {
        if cfg!(windows) { name.eq_ignore_ascii_case(expected) } else { name == *expected }
    }))).collect()
}

/// What an install or sign-in did (`ActionResult` in app/src/platform/backend/types.ts).
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
    command.envs(providers::get(provider).check_env().iter().copied());
    let (status, out, err) = proc::capture(&mut command, Duration::from_secs(15)).ok()?;
    let text = if out.trim().is_empty() { err } else { out };
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    status.success().then(|| line.chars().take(80).collect())
}

/// Signed in, per the CLI's own status command: with the clean profile (as
/// `o agents doctor` asks) or with the person's normal setup.
fn login(root: &Path, provider: ProviderId, cli: &CliCommand, clean: bool) -> LoginState {
    if provider == ProviderId::Agy { return LoginState { logged_in: None, detail: crate::policy::AGY_UNAVAILABLE.into() }; }
    if clean {
        return clean::login_state(root, provider);
    }
    let p = providers::get(provider);
    let mut command = cli_command(cli, p.status_args());
    command.envs(p.check_env().iter().copied());
    let home = user_home();
    if home.is_dir() {
        command.current_dir(home);
    }
    match proc::capture(&mut command, Duration::from_secs(30)) {
        Ok((status, out, err)) => clean::read_login(provider, status.success(), &out, &err),
        Err(error) => LoginState { logged_in: None, detail: format!("상태 확인 {error}") },
    }
}

/// One CLI's status. A CLI on PATH that cannot be started without a shell counts as not installed.
pub fn status_of(root: &Path, provider: ProviderId, clean: bool) -> AgentStatus {
    let o = providers::get(provider).official();
    let npm_ready = o.npm_package.is_some() && npm().is_some();
    let install_hint = match o.npm_package {
        Some(package) if npm_ready => format!("npm install -g {package}"),
        _ if o.script.starts_with("irm ") => format!("PowerShell: {}", o.script),
        _ => o.script.to_string(),
    };
    let unsupported = providers::get(provider).profile_unsupported().filter(|_| clean);
    let found: Option<PathBuf> = which(provider.as_str());
    let cli = resolve_cli(provider.as_str());
    let (version, logged_in) = match &cli {
        Some(cli) => thread::scope(|s| {
            let version = s.spawn(|| version(provider, cli));
            let login = s.spawn(|| if unsupported.is_some() { LoginState { logged_in: None, detail: String::new() } } else { login(root, provider, cli, clean) });
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
        api_key_env: present_auth_env(provider),
        install_hint,
        can_install: npm_ready,
        install_url: o.url.into(),
        profile: (clean && cli.is_some()).then(|| clean::ProfileState::of(provider, logged_in)),
        profile_unsupported: unsupported.map(str::to_string),
        profile_not_ready: if clean && cli.is_some() { clean::profile_not_ready(root, provider) } else { None },
    }
}

/// Every CLI at once (each status check may take a while), in ProviderId::ALL order.
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
    let o = providers::get(provider).official();
    let manual = |why: &str| ActionResult::new(false, format!("{why} 공식 안내({})를 따라 설치하세요: {}", o.url, o.script));
    let Some(package) = o.npm_package else {
        return manual(&format!("{}는 공식 설치 스크립트로만 설치합니다.", o.name));
    };
    let Some(npm) = npm() else {
        return manual("npm을 찾지 못했습니다.");
    };
    let mut command = cli_command(&npm, &["install", "-g", package]);
    match proc::capture(&mut command, INSTALL_TIMEOUT) {
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
    let p = providers::get(provider);
    let o = p.official();
    let Some(args) = p.login_args() else {
        let opened = proc::open_url(o.url).is_ok();
        let then = if opened { "공식 안내 페이지를 열었습니다." } else { "공식 안내 페이지를 열지 못했습니다." };
        return ActionResult::new(opened, format!("{}는 이 PC의 시스템 로그인(Google 계정)을 그대로 씁니다. {then} {}", o.name, o.url));
    };
    let Some(cli) = resolve_cli(provider.as_str()) else {
        return ActionResult::new(false, format!("{}가 설치되어 있지 않습니다.", o.name));
    };
    let mut argv = cli.prefix.clone();
    argv.extend(args.iter().map(|a| a.to_string()));
    let started = if clean { clean_console(root, provider, &cli, &argv) } else { proc::spawn_console(&cli.command, &argv, None, None) };
    let shown = format!("{} {}", provider.as_str(), args.join(" "));
    match started {
        Ok(()) => {
            let profile = if clean { " (깨끗한 테스트 프로필)" } else { "" };
            ActionResult::new(true, format!("새 창에서 {} 공식 로그인({shown})을 진행하세요{profile}. 끝나면 상태를 다시 확인합니다.", o.name))
        }
        Err(error) => ActionResult::new(false, format!("{shown} 창을 열지 못했습니다: {error}")),
    }
}

/// The CLI in a console window of its own with the project_O profile's environment, started in
/// that profile's home (as `o agents shell` runs it, but not attached to this process).
fn clean_console(root: &Path, provider: ProviderId, cli: &CliCommand, argv: &[String]) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let root = std::path::absolute(root)?;
    #[cfg(not(target_os = "macos"))]
    let root = root.to_path_buf();
    let env = clean::environment(&root, provider)?;
    proc::spawn_console(&cli.command, argv, Some(&env), Some(&clean::profile_home(&root, provider)))
}

/// Opens the CLI itself, interactive, in a console window with the project_O profile, so the
/// person can finish the one-time setup its cards need there (`Provider::profile_not_ready`:
/// Codex's Windows sandbox, ADR-0040). O writes none of that setting itself.
pub fn open_setup(root: &Path, provider: ProviderId) -> ActionResult {
    let o = providers::get(provider).official();
    if clean::profile_not_ready(root, provider).is_none() {
        return ActionResult::new(true, format!("{}의 project_O 프로필은 따로 설정할 것이 없습니다.", o.name));
    }
    let Some(cli) = resolve_cli(provider.as_str()) else {
        return ActionResult::new(false, format!("{}가 설치되어 있지 않습니다.", o.name));
    };
    match clean_console(root, provider, &cli, &cli.prefix) {
        Ok(()) => ActionResult::new(true, format!("새 창에서 {}가 project_O 프로필로 열렸습니다. 첫 화면의 안내대로 설정(Windows 샌드박스)을 마치고 창을 닫은 뒤 다시 확인을 누르세요.", o.name)),
        Err(error) => ActionResult::new(false, format!("{} 창을 열지 못했습니다: {error}", provider.as_str())),
    }
}

/// Opens an independent shell; the user starts any agent themselves.
pub fn open_terminal() -> ActionResult {
    #[cfg(windows)]
    {
        let system = std::env::var_os("SystemRoot").map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
        let shell = system.join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        match proc::spawn_console(&shell, &[], None, None) {
            Ok(()) => ActionResult::new(true, "터미널을 열었습니다. 사용할 폴더로 이동한 뒤 agy를 직접 입력하세요."),
            Err(error) => ActionResult::new(false, format!("터미널을 열지 못했습니다: {error}")),
        }
    }
    #[cfg(not(windows))]
    ActionResult::new(false, "터미널 열기는 Windows에서 지원합니다. 직접 터미널을 열어 agy를 입력하세요.")
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
