//! Clean agent profiles: the official CLIs with none of the owner's own
//! harness (user hooks, global instructions, skills, plugins, MCP servers,
//! memories, settings), without touching his normal setup.
//!
//! ```text
//! %LOCALAPPDATA%/project_O-agents/<provider>/home   the CLI's home (USERPROFILE/HOME)
//!   claude: .claude/settings.json            CLAUDE_CONFIG_DIR; only claudeMdExcludes
//!   codex:  .codex/                          CODEX_HOME
//!   agy:    .gemini/                         made by agy itself
//! ```
//!
//! A clean launch sees a short list of system variables plus that home, and
//! nothing else from the parent. Sign-in stays the CLI's own: O never creates
//! or reads login files, and login state comes only from each CLI's status command.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Value};

use crate::run::plan::ProviderId;
use crate::usage::probe;
use crate::sys::resolve::resolve_cli;
use crate::run::store::{to_pretty, user_home};
use crate::usage::parse_agy_usage;

/// `%LOCALAPPDATA%/project_O-agents`, or `~/.local/state/project_O-agents` elsewhere. Kept out of
/// %TEMP% (Codex refuses to set up its helpers there) and out of O's state folder, which is protected.
pub fn default_root() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| user_home().join(".local").join("state"));
    root_in(&base)
}

/// The profile root inside `base`, next to O's state folder (`<base>/project_O`), never in it.
pub fn root_in(base: &Path) -> PathBuf {
    base.join("project_O-agents")
}

pub fn profile_home(root: &Path, provider: ProviderId) -> PathBuf {
    root.join(provider.as_str()).join("home")
}

/// System variables a clean CLI keeps (names compared without case, as Windows does).
const KEEP: &[&str] = &[
    "PATH", "PATHEXT", "SYSTEMROOT", "WINDIR", "COMSPEC", "SYSTEMDRIVE", "TEMP", "TMP", "APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "PROGRAMW6432",
    "COMMONPROGRAMW6432", "USERNAME", "USERDOMAIN", "COMPUTERNAME", "OS", "NUMBER_OF_PROCESSORS", "LANG", "TERM", "COLORTERM",
];
const KEEP_PREFIX: &[&str] = &["PROGRAMFILES", "COMMONPROGRAMFILES", "PROCESSOR_", "LC_"];

fn keeps(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    KEEP.contains(&name.as_str()) || KEEP_PREFIX.iter().any(|p| name.starts_with(p))
}

/// The whole environment of a clean CLI: the kept system variables of `parent`,
/// the profile home as the user's home, and the provider's own pointer into it.
pub fn clean_env_from(parent: impl IntoIterator<Item = (String, String)>, home: &Path, provider: ProviderId) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = parent.into_iter().filter(|(name, _)| keeps(name)).collect();
    let text = home.to_string_lossy().into_owned();
    env.insert("USERPROFILE".into(), text.clone());
    env.insert("HOME".into(), text.clone());
    if text.get(1..2) == Some(":") {
        let rest = &text[2..];
        env.insert("HOMEDRIVE".into(), text[..2].to_string());
        env.insert("HOMEPATH".into(), if rest.is_empty() { "\\".into() } else { rest.to_string() });
    }
    let (name, value) = match provider {
        ProviderId::Claude => ("CLAUDE_CONFIG_DIR", home.join(".claude").to_string_lossy().into_owned()),
        ProviderId::Codex => ("CODEX_HOME", home.join(".codex").to_string_lossy().into_owned()),
        ProviderId::Agy => ("AGY_CLI_DISABLE_AUTO_UPDATE", "true".into()),
    };
    env.insert(name.into(), value);
    env
}

/// The clean environment for a provider, its profile made first if missing.
pub fn environment(root: &Path, provider: ProviderId) -> io::Result<BTreeMap<String, String>> {
    let home = ensure_profile(root, provider, &user_home())?;
    let parent = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)));
    Ok(clean_env_from(parent, &home, provider))
}

/// Instruction files Claude reads in every folder above the one it starts in.
const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CLAUDE.local.md", ".claude/CLAUDE.md", ".claude/AGENTS.md"];

/// Claude's only settings in a clean profile: skip the real home's instruction
/// files, which it would otherwise read for any project under the home folder.
/// Claude matches these globs against `/`-separated absolute paths; a linked
/// file is listed by its target as well.
pub fn claude_settings(real_home: &Path) -> Value {
    let slashes = |p: &Path| p.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    let mut excludes = Vec::new();
    for name in INSTRUCTION_FILES {
        let file = real_home.join(name);
        excludes.push(slashes(&file));
        if let Ok(target) = fs::canonicalize(&file) {
            let target = slashes(&target);
            if !excludes.contains(&target) {
                excludes.push(target);
            }
        }
    }
    excludes.push(format!("{}/.claude/rules/**", slashes(real_home).trim_end_matches('/')));
    json!({ "claudeMdExcludes": excludes })
}

/// Makes a provider's profile home if it is missing. Only Claude gets a file
/// (its settings); Codex needs its CODEX_HOME folder to exist; agy makes its own.
pub fn ensure_profile(root: &Path, provider: ProviderId, real_home: &Path) -> io::Result<PathBuf> {
    let home = profile_home(root, provider);
    fs::create_dir_all(&home)?;
    match provider {
        ProviderId::Claude => {
            let settings = home.join(".claude").join("settings.json");
            if !settings.exists() {
                fs::create_dir_all(home.join(".claude"))?;
                fs::write(settings, to_pretty(&claude_settings(real_home))?)?;
            }
        }
        ProviderId::Codex => fs::create_dir_all(home.join(".codex"))?,
        ProviderId::Agy => {}
    }
    Ok(home)
}

/// The official CLI with the clean environment, run in the current folder.
pub fn command(root: &Path, provider: ProviderId, args: &[&str]) -> io::Result<Command> {
    let bin = provider.as_str();
    let cli = resolve_cli(bin).ok_or_else(|| io::Error::other(format!("{bin} CLI를 PATH에서 찾지 못했거나 셸 없이 실행할 수 없습니다")))?;
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).args(args).env_clear().envs(environment(root, provider)?);
    Ok(command)
}

/// The official login, or for agy (signed in through the system) its status check.
pub fn login_args(provider: ProviderId) -> Option<&'static [&'static str]> {
    match provider {
        ProviderId::Claude => Some(&["auth", "login"]),
        ProviderId::Codex => Some(&["login"]),
        ProviderId::Agy => None,
    }
}

/// The CLI's own sign-in status check (agy: its quota report, empty when signed out).
pub fn status_args(provider: ProviderId) -> &'static [&'static str] {
    match provider {
        ProviderId::Claude => &["auth", "status"],
        ProviderId::Codex => &["login", "status"],
        ProviderId::Agy => &probe::AGY_USAGE,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginState {
    /// `None` when the status check could not tell.
    pub logged_in: Option<bool>,
    pub detail: String,
}

/// Asks the CLI itself, with the clean environment, from inside its profile home.
pub fn login_state(root: &Path, provider: ProviderId) -> LoginState {
    let unknown = |detail: String| LoginState { logged_in: None, detail };
    let mut command = match command(root, provider, status_args(provider)) {
        Ok(command) => command,
        Err(error) => return unknown(error.to_string()),
    };
    command.current_dir(profile_home(root, provider));
    match probe::capture(&mut command, Duration::from_secs(30)) {
        Ok((status, out, err)) => read_login(provider, status.success(), &out, &err),
        Err(error) => unknown(format!("상태 확인 {error}")),
    }
}

/// Reads a status command's answer. Only the yes/no is kept: these outputs can name the account.
pub fn read_login(provider: ProviderId, success: bool, stdout: &str, stderr: &str) -> LoginState {
    let state = |logged_in: Option<bool>, detail: &str| LoginState { logged_in, detail: detail.to_string() };
    let said = format!("{stdout}\n{stderr}");
    let fallback = || {
        let first = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("응답 없음");
        state(None, &format!("상태를 알 수 없음: {}", first.chars().take(160).collect::<String>()))
    };
    match provider {
        ProviderId::Claude => match serde_json::from_str::<Value>(stdout.trim()).ok().and_then(|v| v.get("loggedIn").and_then(Value::as_bool)) {
            Some(true) => state(Some(true), "로그인됨"),
            Some(false) => state(Some(false), "로그인 안 됨 — o agents login claude"),
            None => fallback(),
        },
        ProviderId::Codex if said.contains("Not logged in") => state(Some(false), "로그인 안 됨 — o agents login codex"),
        ProviderId::Codex if success && said.contains("Logged in") => state(Some(true), "로그인됨"),
        // Signed out, agy has no quota to report.
        ProviderId::Agy if success && !parse_agy_usage(stdout).is_empty() => state(Some(true), "로그인됨 (시스템 로그인)"),
        _ => fallback(),
    }
}

static CODEX_HOOK_OR_MCP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\s*(?:\[\[?\s*)?(?:mcp_servers|hooks)\b").unwrap());

fn has_content(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// Nothing but empty objects, empty lists and nulls.
fn is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.values().all(is_blank),
        _ => false,
    }
}

fn read_json(path: &Path) -> Option<Result<Value, String>> {
    let text = fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return Some(Ok(Value::Null));
    }
    Some(serde_json::from_str(&text).map_err(|e| format!("{} 읽기 실패: {e}", path.to_string_lossy())))
}

fn mcp_in_claude_state(state: &Value) -> bool {
    let servers = |v: &Value| v.get("mcpServers").is_some_and(|s| !is_blank(s));
    servers(state) || state.get("projects").and_then(Value::as_object).is_some_and(|p| p.values().any(servers))
}

/// What in a provider's profile home would bring a personal harness back.
/// Empty when the profile is clean.
pub fn inspect(root: &Path, provider: ProviderId) -> Vec<String> {
    let home = profile_home(root, provider);
    let mut issues = Vec::new();
    let mut flag = |found: bool, what: String| {
        if found {
            issues.push(what);
        }
    };
    let shown = |p: &Path| p.to_string_lossy().into_owned();
    flag(home.join(".agents").exists(), format!("사용자 스킬·플러그인 폴더가 있음: {}", shown(&home.join(".agents"))));
    match provider {
        ProviderId::Claude => {
            let dir = home.join(".claude");
            match read_json(&dir.join("settings.json")) {
                None => flag(true, "settings.json이 없음 (홈 폴더 지시 파일 제외 설정이 빠짐)".into()),
                Some(Err(error)) => flag(true, error),
                Some(Ok(settings)) => {
                    let keys: Vec<&String> = settings.as_object().map(|o| o.keys().collect()).unwrap_or_default();
                    flag(keys.iter().any(|k| *k == "hooks"), "settings.json에 hooks가 있음".into());
                    let extra: Vec<&str> = keys.iter().map(|k| k.as_str()).filter(|k| *k != "claudeMdExcludes" && *k != "hooks").collect();
                    flag(!extra.is_empty(), format!("settings.json에 예상 밖 설정: {}", extra.join(", ")));
                }
            }
            for name in ["skills", "plugins", "agents", "commands", "rules"] {
                flag(has_content(&dir.join(name)), format!("{} 폴더에 내용이 있음", shown(&dir.join(name))));
            }
            for name in ["CLAUDE.md", "AGENTS.md"] {
                flag(dir.join(name).exists(), format!("전역 지시 파일이 있음: {}", shown(&dir.join(name))));
            }
            for state in [dir.join(".claude.json"), home.join(".claude.json")] {
                match read_json(&state) {
                    Some(Ok(value)) => flag(mcp_in_claude_state(&value), format!("{}에 mcpServers가 있음", shown(&state))),
                    Some(Err(error)) => flag(true, error),
                    None => {}
                }
            }
        }
        ProviderId::Codex => {
            let dir = home.join(".codex");
            let config = fs::read_to_string(dir.join("config.toml")).unwrap_or_default();
            flag(CODEX_HOOK_OR_MCP.is_match(&config), "config.toml에 hooks 또는 mcp_servers가 있음".into());
            for name in ["hooks.json", "AGENTS.md", "AGENTS.override.md"] {
                flag(dir.join(name).exists(), format!("{}가 있음", shown(&dir.join(name))));
            }
            let skills: Vec<String> = fs::read_dir(dir.join("skills"))
                .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != ".system").collect())
                .unwrap_or_default();
            flag(!skills.is_empty(), format!("skills에 내장(.system) 밖 스킬이 있음: {}", skills.join(", ")));
        }
        ProviderId::Agy => {
            let dir = home.join(".gemini");
            for name in ["hooks.json", "mcp_config.json"] {
                let path = dir.join("config").join(name);
                match read_json(&path) {
                    Some(Ok(value)) => flag(!is_blank(&value), format!("{}가 비어 있지 않음", shown(&path))),
                    Some(Err(error)) => flag(true, error),
                    None => {}
                }
            }
            flag(has_content(&dir.join("skills")), format!("{} 폴더에 내용이 있음", shown(&dir.join("skills"))));
            for name in ["GEMINI.md", "AGENTS.md"] {
                flag(dir.join(name).exists(), format!("전역 지시 파일이 있음: {}", shown(&dir.join(name))));
            }
        }
    }
    issues
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let text = |p: &Path| p.to_string_lossy().trim_end_matches(['\\', '/']).replace('\\', "/");
    if cfg!(windows) {
        text(a).eq_ignore_ascii_case(&text(b))
    } else {
        text(a) == text(b)
    }
}

/// Instruction files in the folders above `cwd` that a clean Claude would still
/// read: all but the real home's, which its profile excludes.
pub fn ancestor_instructions(cwd: &Path, real_home: &Path) -> Vec<PathBuf> {
    cwd.ancestors()
        .skip(1)
        .filter(|dir| !same_dir(dir, real_home))
        .flat_map(|dir| INSTRUCTION_FILES.iter().map(move |name| name.split('/').fold(dir.to_path_buf(), |path, part| path.join(part))))
        .filter(|file| file.is_file())
        .collect()
}

/// Whether `cwd` is the real home folder or inside it.
pub fn inside_home(cwd: &Path, real_home: &Path) -> bool {
    !real_home.as_os_str().is_empty() && cwd.ancestors().any(|dir| same_dir(dir, real_home))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileReport {
    pub provider: ProviderId,
    pub home: String,
    #[serde(flatten)]
    pub login: LoginState,
    pub issues: Vec<String>,
}

/// `o agents doctor`: every profile's login state and cleanliness. The status
/// checks run in parallel; each CLI may take a while to answer.
pub fn doctor(root: &Path) -> Vec<ProfileReport> {
    std::thread::scope(|s| {
        let checks: Vec<_> = ProviderId::ALL.into_iter().map(|p| (p, s.spawn(move || login_state(root, p)))).collect();
        checks
            .into_iter()
            .map(|(provider, check)| {
                let login = check.join().unwrap_or_else(|_| LoginState { logged_in: None, detail: "내부 오류".into() });
                ProfileReport { provider, home: profile_home(root, provider).to_string_lossy().into_owned(), login, issues: inspect(root, provider) }
            })
            .collect()
    })
}
