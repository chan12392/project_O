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
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::providers;
use crate::run::plan::ProviderId;
use crate::run::store::user_home;
use crate::sys::proc;
use crate::sys::resolve::resolve_cli;

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
    let (name, value) = providers::get(provider).clean_env(home);
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
pub(crate) const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CLAUDE.local.md", ".claude/CLAUDE.md", ".claude/AGENTS.md"];

/// Makes a provider's profile home if it is missing, with what that CLI needs
/// in it (`Provider::prepare_profile`: Claude its settings, Codex its CODEX_HOME).
pub fn ensure_profile(root: &Path, provider: ProviderId, real_home: &Path) -> io::Result<PathBuf> {
    let home = profile_home(root, provider);
    fs::create_dir_all(&home)?;
    providers::get(provider).prepare_profile(&home, real_home)?;
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
    let mut command = match command(root, provider, providers::get(provider).status_args()) {
        Ok(command) => command,
        Err(error) => return unknown(error.to_string()),
    };
    command.current_dir(profile_home(root, provider));
    match proc::capture(&mut command, Duration::from_secs(30)) {
        Ok((status, out, err)) => read_login(provider, status.success(), &out, &err),
        Err(error) => unknown(format!("상태 확인 {error}")),
    }
}

/// Reads a status command's answer (`Provider::read_login`); when the CLI's answer
/// cannot tell, the first line it printed.
pub fn read_login(provider: ProviderId, success: bool, stdout: &str, stderr: &str) -> LoginState {
    providers::get(provider).read_login(success, stdout, stderr).unwrap_or_else(|| {
        let said = format!("{stdout}\n{stderr}");
        let first = said.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("응답 없음");
        LoginState { logged_in: None, detail: format!("상태를 알 수 없음: {}", first.chars().take(160).collect::<String>()) }
    })
}

pub(crate) fn has_content(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// Nothing but empty objects, empty lists and nulls.
pub(crate) fn is_blank(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Array(items) => items.is_empty(),
        Value::Object(fields) => fields.values().all(is_blank),
        _ => false,
    }
}

pub(crate) fn read_json(path: &Path) -> Option<Result<Value, String>> {
    let text = fs::read_to_string(path).ok()?;
    if text.trim().is_empty() {
        return Some(Ok(Value::Null));
    }
    Some(serde_json::from_str(&text).map_err(|e| format!("{} 읽기 실패: {e}", path.to_string_lossy())))
}

/// What in a provider's profile home would bring a personal harness back
/// (`Provider::inspect_profile`). Empty when the profile is clean.
pub fn inspect(root: &Path, provider: ProviderId) -> Vec<String> {
    let home = profile_home(root, provider);
    let mut issues = Vec::new();
    let mut flag = |found: bool, what: String| {
        if found {
            issues.push(what);
        }
    };
    flag(home.join(".agents").exists(), format!("사용자 스킬·플러그인 폴더가 있음: {}", home.join(".agents").to_string_lossy()));
    providers::get(provider).inspect_profile(&home, &mut flag);
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
