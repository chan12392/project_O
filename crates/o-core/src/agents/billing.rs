//! The API billing mode (ADR-0035 개정 1): what O does when a CLI's authentication variable
//! (`auth_env_names`) is in the environment it would hand that CLI for agent work.
//! - `warn` (경고만, the default): pass it on and warn by name, as ADR-0035 decided.
//! - `subscription` (구독만): do not start that card, lead turn, review or Graph Deep run, and say
//!   which variable(s) by name. O still never removes, reads or changes the variables: it declines to
//!   launch instead of restricting the CLI's own authentication.
//!
//! Only names are looked at, never values. Read-only status, usage and model probes are not agent
//! work and run in both modes. Kept in `<O home>/settings.json` beside the daily budget and RAM cap;
//! `o billing`, Settings → 에이전트 → API 과금.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::run::plan::ProviderId;
use crate::sys::launch::{EnvironmentBase, Launch};
use crate::usage::budget::{read_settings, write_setting};

pub const KEY: &str = "apiBillingMode";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// 경고만: pass the variables and warn (ADR-0035).
    Warn,
    /// 구독만: refuse to start agent work while a variable the CLI uses is present.
    Subscription,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Warn => "warn",
            Mode::Subscription => "subscription",
        }
    }

    pub fn parse(value: &str) -> Option<Mode> {
        match value {
            "warn" => Some(Mode::Warn),
            "subscription" => Some(Mode::Subscription),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BillingReport {
    pub mode: Mode,
}

/// The saved mode; a missing or unknown value is `warn` (the default).
pub fn read_mode(home: &Path) -> Mode {
    read_settings(home).get(KEY).and_then(Value::as_str).and_then(Mode::parse).unwrap_or(Mode::Warn)
}

/// Saves the mode; anything but `warn` or `subscription` is refused.
pub fn write_mode(home: &Path, value: &str) -> Result<Mode, String> {
    let mode = Mode::parse(value).ok_or_else(|| format!("API 과금 모드는 warn(경고만) 또는 subscription(구독만)입니다: {value}"))?;
    write_setting(home, KEY, Value::from(mode.as_str()))?;
    Ok(mode)
}

pub fn report(home: &Path) -> BillingReport {
    BillingReport { mode: read_mode(home) }
}

fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if cfg!(windows) {
        a.to_str().zip(b.to_str()).is_some_and(|(a, b)| a.eq_ignore_ascii_case(b)) || a == b
    } else {
        a == b
    }
}

/// Applies environment changes to a list of names, as a process start would (names only).
fn apply<'a>(mut names: Vec<OsString>, changes: impl IntoIterator<Item = (&'a OsStr, bool)>) -> Vec<OsString> {
    for (name, set) in changes {
        names.retain(|existing| !same_name(existing, name));
        if set {
            names.push(name.to_os_string());
        }
    }
    names
}

fn parent_names() -> Vec<OsString> {
    std::env::vars_os().map(|(name, _)| name).collect()
}

/// The names of the environment `launch` would start with, given the inherited names.
pub fn launch_names(launch: &Launch, inherited: Vec<OsString>) -> Vec<OsString> {
    let base = match launch.environment_base {
        EnvironmentBase::Inherit => inherited,
        EnvironmentBase::Replace => Vec::new(),
    };
    apply(base, launch.environment_changes.iter().map(|(name, value)| (name.as_os_str(), value.is_some())))
}

/// The authentication variables `provider` uses that are among `names`, in `auth_env_names` order.
pub fn present_in(provider: ProviderId, names: &[OsString]) -> Vec<&'static str> {
    super::auth_env_names(provider).iter().copied().filter(|expected| names.iter().any(|name| same_name(name, OsStr::new(expected)))).collect()
}

/// Why agent work for `provider` may not start in `mode` with an environment of `names`.
pub fn refusal(mode: Mode, provider: ProviderId, names: &[OsString]) -> Option<String> {
    if mode != Mode::Subscription {
        return None;
    }
    let found = present_in(provider, names);
    (!found.is_empty()).then(|| {
        format!(
            "구독만 모드(settings.json {KEY}=subscription)라 {provider} 실행을 시작하지 않았습니다: 환경변수 {}가 있어 구독 대신 API 요금이 청구될 수 있습니다. \
             환경에서 이 변수를 지우고 O를 다시 시작하거나, 경고만 모드로 바꾸세요(o billing --warn, 설정 → 에이전트 → API 과금). O는 이 변수의 값을 읽거나 지우지 않습니다.",
            found.join(", ")
        )
    })
}

/// Checks the launch O is about to start for agent work (cards, follow-ups, reviews, lead turns).
pub fn check(home: &Path, provider: ProviderId, launch: &Launch) -> Result<(), String> {
    match refusal(read_mode(home), provider, &launch_names(launch, parent_names())) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// `check` for a plain command (Graph Deep). `cleared`: the command was built with `env_clear`
/// (the project_O profile), so it inherits nothing but what it sets.
pub fn check_command(home: &Path, provider: ProviderId, command: &std::process::Command, cleared: bool) -> Result<(), String> {
    let base = if cleared { Vec::new() } else { parent_names() };
    let names = apply(base, command.get_envs().map(|(name, value)| (name, value.is_some())));
    match refusal(read_mode(home), provider, &names) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("o-billing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn names(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn the_mode_defaults_to_warn_and_saves_only_known_values() {
        let home = home("settings");
        assert_eq!(read_mode(&home), Mode::Warn);
        assert_eq!(write_mode(&home, "subscription"), Ok(Mode::Subscription));
        assert_eq!(read_mode(&home), Mode::Subscription);
        assert!(write_mode(&home, "off").is_err());
        assert_eq!(read_mode(&home), Mode::Subscription);
        assert_eq!(write_mode(&home, "warn"), Ok(Mode::Warn));
        assert_eq!(serde_json::to_value(report(&home)).unwrap(), serde_json::json!({ "mode": "warn" }));
        std::fs::write(home.join("settings.json"), r#"{"apiBillingMode":"api"}"#).unwrap();
        assert_eq!(read_mode(&home), Mode::Warn);
    }

    #[test]
    fn subscription_refuses_only_the_variables_that_cli_uses_and_names_them() {
        let env = names(&["PATH", "OPENAI_API_KEY"]);
        assert_eq!(refusal(Mode::Warn, ProviderId::Codex, &env), None);
        assert_eq!(refusal(Mode::Subscription, ProviderId::Claude, &env), None);
        let why = refusal(Mode::Subscription, ProviderId::Codex, &env).unwrap();
        assert!(why.contains("OPENAI_API_KEY") && why.contains("o billing --warn"), "{why}");
        assert!(!why.contains("PATH,"));
        let both = refusal(Mode::Subscription, ProviderId::Claude, &names(&["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY"])).unwrap();
        assert!(both.contains("ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN"), "{both}");
        assert_eq!(refusal(Mode::Subscription, ProviderId::Agy, &names(&["ANTHROPIC_API_KEY"])), None);
    }

    #[test]
    fn the_launch_environment_is_read_by_name_after_its_changes() {
        let mut launch = Launch::new("cli");
        launch.env("ANTHROPIC_API_KEY", "fake-value");
        assert_eq!(present_in(ProviderId::Claude, &launch_names(&launch, names(&["PATH"]))), ["ANTHROPIC_API_KEY"]);
        launch.env_remove("ANTHROPIC_API_KEY");
        assert!(present_in(ProviderId::Claude, &launch_names(&launch, names(&["ANTHROPIC_API_KEY"]))).is_empty());
        let mut clean = Launch::new("cli");
        clean.env_clear().env("PATH", "x");
        assert!(present_in(ProviderId::Claude, &launch_names(&clean, names(&["ANTHROPIC_API_KEY"]))).is_empty());
        if cfg!(windows) {
            assert_eq!(present_in(ProviderId::Claude, &names(&["anthropic_api_key"])), ["ANTHROPIC_API_KEY"]);
        }
    }

    #[test]
    fn check_uses_the_saved_mode() {
        let home = home("check");
        let mut launch = Launch::new("cli");
        launch.env_clear().env("OPENAI_API_KEY", "fake-value");
        assert_eq!(check(&home, ProviderId::Codex, &launch), Ok(()));
        write_mode(&home, "subscription").unwrap();
        assert!(check(&home, ProviderId::Codex, &launch).unwrap_err().contains("OPENAI_API_KEY"));
        let mut command = std::process::Command::new("cli");
        command.env_clear().env("OPENAI_API_KEY", "fake-value");
        assert!(check_command(&home, ProviderId::Codex, &command, true).is_err());
        command.env_remove("OPENAI_API_KEY");
        assert_eq!(check_command(&home, ProviderId::Codex, &command, true), Ok(()));
    }
}
