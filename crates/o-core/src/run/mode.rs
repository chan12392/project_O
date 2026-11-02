//! 토큰 효율 (ADR-0010): one switch for how a run's cards are set up.
//!
//! On (the default) is clean agent profiles (`agents::clean`), the shared context
//! section (`context::prepare`, ADR-0005) and the daily budget lowering effort
//! (ADR-0004). Off is the person's own harness, no injected context, and the effort
//! the lead or the plan chose. `--clean` and `--no-context` stay as finer overrides.
//! Safety never follows the switch: the daily budget still blocks a provider that used
//! up its day, and `LIMITS` and the guard rules apply the same either way. Pure.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Efficiency {
    #[default]
    On,
    Off,
}

impl Efficiency {
    pub fn parse(value: &str) -> Option<Efficiency> {
        match value {
            "on" => Some(Efficiency::On),
            "off" => Some(Efficiency::Off),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Efficiency::On => "on",
            Efficiency::Off => "off",
        }
    }

    /// How people read it: "토큰 효율 켜짐" / "토큰 효율 꺼짐".
    pub fn label(self) -> &'static str {
        match self {
            Efficiency::On => "토큰 효율 켜짐",
            Efficiency::Off => "토큰 효율 꺼짐",
        }
    }
}

/// What a run's cards get. Saved in the run folder (`store::mark_mode`) and recorded as `run.mode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunMode {
    pub efficiency: Efficiency,
    /// Clean agent profiles: no personal hooks, global instructions, skills, MCP or memory.
    pub clean: bool,
    /// Earlier work of the project injected into each card's prompt (ADR-0005).
    pub context: bool,
    /// The daily budget may lower a card's effort when the day runs ahead (ADR-0004).
    pub lower_effort: bool,
}

impl RunMode {
    /// The switch alone: on is all three, off is none.
    pub fn of(efficiency: Efficiency) -> RunMode {
        let on = efficiency == Efficiency::On;
        RunMode { efficiency, clean: on, context: on, lower_effort: on }
    }

    /// The finer overrides on top of the switch: `--clean` turns clean profiles on,
    /// `--no-context` turns the context off. Effort lowering follows the switch only.
    pub fn with(self, clean: bool, no_context: bool) -> RunMode {
        RunMode { clean: self.clean || clean, context: self.context && !no_context, ..self }
    }
}

impl Default for RunMode {
    fn default() -> RunMode {
        RunMode::of(Efficiency::On)
    }
}

/// `--efficiency on|off` (or `--efficiency=on`), `--clean` and `--no-context` taken out of
/// `args`: the run mode they make (on when unnamed) and the arguments left, in order.
pub fn take_flags(args: &[String]) -> Result<(RunMode, Vec<String>), String> {
    let mut efficiency = None;
    let (mut clean, mut no_context) = (false, false);
    let mut rest = Vec::new();
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        let value = match arg.as_str() {
            "--clean" => {
                clean = true;
                continue;
            }
            "--no-context" => {
                no_context = true;
                continue;
            }
            "--efficiency" => items.next().map(String::as_str).unwrap_or_default(),
            other => match other.strip_prefix("--efficiency=") {
                Some(value) => value,
                None => {
                    rest.push(arg.clone());
                    continue;
                }
            },
        };
        let parsed = Efficiency::parse(value).ok_or_else(|| format!("--efficiency는 on 또는 off입니다: {value}"))?;
        if efficiency.is_some_and(|e| e != parsed) {
            return Err("--efficiency를 서로 다르게 두 번 주었습니다".into());
        }
        efficiency = Some(parsed);
    }
    Ok((RunMode::of(efficiency.unwrap_or_default()).with(clean, no_context), rest))
}
