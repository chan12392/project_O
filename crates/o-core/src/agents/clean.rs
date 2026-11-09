//! Adapted from stablyai/orca (MIT); see THIRD_PARTY_NOTICES.md.
//! Clean agent profiles: the official CLIs with none of the owner's own
//! harness (user hooks, global instructions, skills, plugins, MCP servers,
//! memories, settings), without touching his normal setup.
//!
//! ```text
//! %LOCALAPPDATA%/project_O-agents/<provider>/home   the CLI's home (USERPROFILE/HOME)
//!   claude: .claude/settings.json            CLAUDE_CONFIG_DIR; only claudeMdExcludes
//!   codex:  .codex/                          CODEX_HOME
//!   agy:    .gemini/                         made by agy itself
//!   opencode: .config/opencode/opencode.json  XDG_CONFIG_HOME; O's permission rules, no plugin or MCP
//!   (a CLI whose config cannot move, `catalog::profile_support`, gets no profile: "프로필 미지원")
//!   devin:  (none)                           프로필 미지원: its config folder cannot be moved
//! ```
//!
//! A clean launch sees a short list of system variables plus that home, and
//! its provider's authentication variables, and nothing else from the parent. Sign-in stays the CLI's own: O never creates
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
use super::catalog;

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
    let mut env: BTreeMap<String, String> = parent.into_iter().filter(|(name, _)| keeps(name) || super::auth_env_names(provider).iter().any(|auth| name.eq_ignore_ascii_case(auth))).collect();
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

/// The clean environment for a provider, its profile made first if missing. A CLI that
/// cannot run on the profile (`Provider::profile_unsupported`) gets none: never its own setup instead.
pub fn environment(root: &Path, provider: ProviderId) -> io::Result<BTreeMap<String, String>> {
    if let Some(why) = providers::get(provider).profile_unsupported() {
        return Err(io::Error::other(why));
    }
    let home = ensure_profile(root, provider, &user_home())?;
    let parent = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)));
    Ok(clean_env_from(parent, &home, provider))
}

/// Instruction files Claude reads in every folder above the one it starts in.
pub(crate) const INSTRUCTION_FILES: &[&str] = &["AGENTS.md", "CLAUDE.md", "CLAUDE.local.md", ".claude/CLAUDE.md", ".claude/AGENTS.md"];

/// Makes a provider's profile home if it is missing, with what that CLI needs
/// in it (`Provider::prepare_profile`: Claude its settings, Codex its CODEX_HOME).
/// A CLI whose config cannot move into a profile (`catalog::profile_support`) gets none: it is
/// refused rather than run with the person's own config.
pub fn ensure_profile(root: &Path, provider: ProviderId, real_home: &Path) -> io::Result<PathBuf> {
    if let Err(why) = catalog::profile_support(provider) {
        return Err(io::Error::other(unsupported(provider, why)));
    }
    let home = profile_home(root, provider);
    fs::create_dir_all(&home)?;
    providers::get(provider).prepare_profile(&home, real_home)?;
    Ok(home)
}

/// The official CLI with the clean environment, run in the current folder.
pub fn command(root: &Path, provider: ProviderId, args: &[&str]) -> io::Result<Command> {
    if provider == ProviderId::Agy { return Err(io::Error::other(crate::policy::AGY_UNAVAILABLE)); }
    if let Some(why) = providers::get(provider).profile_unsupported() {
        return Err(io::Error::other(why));
    }
    crate::sys::resolve::ensure_paths();
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

/// "프로필 미지원(<id>): …" — why a CLI cannot run with the project_O profile.
pub fn unsupported(provider: ProviderId, why: &str) -> String {
    let name = providers::get(provider).official().name;
    format!("프로필 미지원({provider}): {name}은(는) project_O 프로필을 쓸 수 없습니다 — {why}. 설정 → 에이전트에서 'project_O 프로필 사용'을 끄면 내 설정으로 실행합니다.")
}

/// "로그인 필요(<id>): …" — the profile exists but its CLI is signed out there. The app reads
/// the provider id in the parentheses to offer the official sign-in (`agents::open_login`).
pub fn login_needed(provider: ProviderId) -> String {
    let name = providers::get(provider).official().name;
    format!("로그인 필요({provider}): {name}의 project_O 프로필에 로그인되어 있지 않습니다. 설정 → 에이전트에서 '연결'을 누르거나 o agents login {provider}로 공식 로그인을 마친 뒤 다시 실행하세요.")
}

/// Why `provider`'s project_O profile under `root` cannot do a sandboxed card's work yet
/// (`Provider::profile_not_ready`, ADR-0040): a one-time setup the person finishes in the CLI.
/// `None` when it can, or when the CLI cannot have a profile at all (`profile_unsupported`).
pub fn profile_not_ready(root: &Path, provider: ProviderId) -> Option<String> {
    let p = providers::get(provider);
    if p.profile_unsupported().is_some() {
        return None;
    }
    p.profile_not_ready(&profile_home(root, provider))
}

/// Before a card or lead starts with the project_O profile (ADR-0015): each provider's profile
/// is made if missing, and a provider that cannot have one, or whose CLI says it is signed out
/// there, maps to the reason it must not start. A CLI that signs in through the system (no
/// login command) or whose status check cannot tell is not refused. Checks run in parallel.
pub fn not_ready(root: &Path, providers: &[ProviderId]) -> std::collections::HashMap<ProviderId, String> {
    let mut wanted: Vec<ProviderId> = providers.to_vec();
    wanted.sort();
    wanted.dedup();
    std::thread::scope(|s| {
        let checks: Vec<_> = wanted
            .into_iter()
            .map(|provider| {
                s.spawn(move || {
                    if let Err(why) = catalog::profile_support(provider) {
                        return Some((provider, unsupported(provider, why)));
                    }
                    crate::providers::get(provider).login_args()?;
                    (login_state(root, provider).logged_in == Some(false)).then(|| (provider, login_needed(provider)))
                })
            })
            .collect();
        checks.into_iter().filter_map(|check| check.join().ok().flatten()).collect()
    })
}

/// A connected CLI's project_O profile as Settings → 에이전트 shows it (ADR-0015).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileState {
    /// Made, and signed in or the CLI cannot tell (a system sign-in).
    Created,
    /// Made; the CLI says it is signed out there.
    LoginNeeded,
    /// The CLI's config cannot move into a profile.
    Unsupported,
}

impl ProfileState {
    pub fn of(provider: ProviderId, logged_in: Option<bool>) -> ProfileState {
        if catalog::profile_support(provider).is_err() {
            ProfileState::Unsupported
        } else if logged_in == Some(false) {
            ProfileState::LoginNeeded
        } else {
            ProfileState::Created
        }
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
    pub api_key_env: Vec<&'static str>,
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
                ProfileReport { provider, home: profile_home(root, provider).to_string_lossy().into_owned(), login, issues: inspect(root, provider), api_key_env: super::present_auth_env(provider) }
            })
            .collect()
    })
}
