//! project_O 프로필 사용 (ADR-0013): which agent profile a run's cards and the lead use.
//!
//! `project-o` (the default) is Project O's clean agent profiles (`agents::clean`): no
//! personal hooks, global instructions, skills, MCP or memory. `own` is the person's
//! normal CLI setup. The switch decides the profile only: the shared context section
//! (`context::prepare`, ADR-0005) goes in either way unless `--no-context` asks otherwise,
//! and the daily budget (blocking, effort lowering) follows its own switch (`usage::budget`).
//! `LIMITS` and the guard rules apply the same either way. Pure.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Project O's clean agent profiles.
    #[default]
    ProjectO,
    /// The person's own CLI setup (skills, hooks, memory).
    Own,
}

impl Profile {
    pub fn parse(value: &str) -> Option<Profile> {
        match value {
            "project-o" => Some(Profile::ProjectO),
            "own" => Some(Profile::Own),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Profile::ProjectO => "project-o",
            Profile::Own => "own",
        }
    }

    /// How people read it: "project_O 프로필" / "내 설정".
    pub fn label(self) -> &'static str {
        match self {
            Profile::ProjectO => "project_O 프로필",
            Profile::Own => "내 설정",
        }
    }

    pub fn of_clean(clean: bool) -> Profile {
        if clean {
            Profile::ProjectO
        } else {
            Profile::Own
        }
    }

    pub fn is_clean(self) -> bool {
        self == Profile::ProjectO
    }
}

/// What a run's cards get. Saved in the run folder as the `clean` and `no-context`
/// markers (`store::mark_mode`) and recorded as `run.mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunMode {
    pub profile: Profile,
    /// Earlier work of the project injected into each card's prompt (ADR-0005).
    pub context: bool,
}

impl RunMode {
    /// A profile with the shared context on.
    pub fn of(profile: Profile) -> RunMode {
        RunMode { profile, context: true }
    }

    /// Clean agent profiles (no personal harness).
    pub fn clean(self) -> bool {
        self.profile.is_clean()
    }
}

impl Default for RunMode {
    fn default() -> RunMode {
        RunMode::of(Profile::ProjectO)
    }
}

/// `--profile project-o|own` (or `--profile=own`), `--clean` (the same as `--profile project-o`)
/// and `--no-context` taken out of `args`: the run mode they make (project-o with context
/// when unnamed) and the arguments left, in order.
pub fn take_flags(args: &[String]) -> Result<(RunMode, Vec<String>), String> {
    let mut profile = None;
    let mut no_context = false;
    let mut rest = Vec::new();
    let mut items = args.iter();
    fn set(parsed: Profile, profile: &mut Option<Profile>) -> Result<(), String> {
        if profile.is_some_and(|p| p != parsed) {
            return Err("--profile(또는 --clean)을 서로 다르게 두 번 주었습니다".into());
        }
        *profile = Some(parsed);
        Ok(())
    }
    while let Some(arg) = items.next() {
        let value = match arg.as_str() {
            "--clean" => {
                set(Profile::ProjectO, &mut profile)?;
                continue;
            }
            "--no-context" => {
                no_context = true;
                continue;
            }
            "--profile" => items.next().map(String::as_str).unwrap_or_default(),
            other => match other.strip_prefix("--profile=") {
                Some(value) => value,
                None => {
                    rest.push(arg.clone());
                    continue;
                }
            },
        };
        let parsed = Profile::parse(value).ok_or_else(|| format!("--profile은 project-o 또는 own입니다: {value}"))?;
        set(parsed, &mut profile)?;
    }
    Ok((RunMode { profile: profile.unwrap_or_default(), context: !no_context }, rest))
}
