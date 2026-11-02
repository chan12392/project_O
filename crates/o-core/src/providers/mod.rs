//! Each official agent CLI behind one `Provider` table: how a card launches it
//! headless and reads its stream, how it is installed and signed in, what its
//! clean profile holds, and how its subscription windows are read. One file per
//! CLI (`claude`, `codex`, `agy`); [`get`] is the registry. Only these files know
//! provider flags.
//!
//! Every launch keeps the CLI's own sign-in and permission checks: no token is
//! injected and no flag that disables approvals is ever added. The one opening
//! is narrow: each CLI may run `o msg`, the run's team channel, and the
//! read-only `o context` search (ADR-0005; Antigravity only gets the section
//! injected into its prompt), and nothing more.

pub mod agy;
pub mod claude;
pub mod codex;
mod stream;

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde_json::{Map, Value};

use crate::agents::clean::LoginState;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan, ProviderId};
use crate::usage::UsageWindow;

pub use codex::unwrap_shell;

/// What O knows about one official CLI. Adding a CLI: a `ProviderId` variant, a
/// file implementing this, and its line in [`get`] (docs/ARCHITECTURE.md).
pub trait Provider: Sync {
    // Cards: headless launch and stream.

    /// The card's launch: argv with the permission mode, the commands it may run
    /// without a prompt, and the model and effort flags. Never a flag that turns
    /// approvals off.
    fn launch(&self, card: &Card, plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec;
    /// A line for the card prompt on which shell commands the CLI may run headless.
    fn tool_note(&self) -> Option<String>;
    /// How a card of this CLI sends and reads team messages.
    fn team_how_to(&self, _card: &Card, _team: &TeamChannel) -> Vec<String> {
        vec![format!("보내기: {TEAM_COMMAND} send --to <카드 id|lead|all> \"내용\"   받기: {TEAM_COMMAND} read")]
    }
    /// Whether a card may run `o context` itself (ADR-0005); otherwise it only gets the injected section.
    fn searches_context(&self) -> bool;
    /// A fresh reader for one card's stdout, one JSON object per line.
    fn stream_reader(&self) -> ReadLine;

    // Lead chat (headless prompt, session resume, attachments per CLI) goes here.

    // Install and sign-in (`agents`).

    fn official(&self) -> Official;
    /// The CLI's own sign-in command; `None` when it signs in through the system.
    fn login_args(&self) -> Option<&'static [&'static str]>;
    /// The CLI's own sign-in status check.
    fn status_args(&self) -> &'static [&'static str];
    /// What the status check said; `None` when it cannot tell. Only the yes/no is kept:
    /// these outputs can name the account.
    fn read_login(&self, success: bool, stdout: &str, stderr: &str) -> Option<LoginState>;
    /// Variables set whenever O runs the CLI for a check (version, status).
    fn check_env(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }

    // Clean profile (`agents::clean`).

    /// The one variable a clean launch adds: the CLI's config folder inside the
    /// profile home, or a switch when the CLI keeps its own folder there.
    fn clean_env(&self, home: &Path) -> (&'static str, String);
    /// Makes what the profile needs before first use.
    fn prepare_profile(&self, home: &Path, real_home: &Path) -> io::Result<()>;
    /// Flags what in the profile home would bring a personal harness back.
    fn inspect_profile(&self, home: &Path, flag: &mut dyn FnMut(bool, String));

    // Usage (`usage::probe`).

    /// The CLI's subscription windows, asked without a model turn; `None` when it
    /// has no such probe (Claude reports them in each run's stream).
    fn usage_windows(&self) -> Option<Result<Vec<UsageWindow>, String>> {
        None
    }
}

/// The registry: every `ProviderId` has exactly one implementation.
pub fn get(id: ProviderId) -> &'static dyn Provider {
    match id {
        ProviderId::Claude => &claude::Claude,
        ProviderId::Codex => &codex::Codex,
        ProviderId::Agy => &agy::Agy,
    }
}

/// Each vendor's own install channels for Windows.
pub struct Official {
    pub name: &'static str,
    /// The npm package the vendor documents (`npm install -g <package>`), if any.
    pub npm_package: Option<&'static str>,
    /// The vendor's PowerShell installer, for the person to run.
    pub script: &'static str,
    pub url: &'static str,
}

/// Reads one parsed stdout line into zero or more signals.
pub type ReadLine = Box<dyn FnMut(&Map<String, Value>) -> Vec<StreamSignal> + Send>;

#[derive(Clone, Debug, PartialEq)]
pub struct LaunchSpec {
    /// Executable name looked up on PATH.
    pub bin: String,
    pub args: Vec<String>,
    /// Prompt written to stdin, when the CLI reads it from there.
    pub stdin: Option<String>,
    /// Run and card ids for `o msg`; never credentials.
    pub env: BTreeMap<String, String>,
}

pub struct TeamChannel<'a> {
    pub run_id: &'a str,
    /// The run's folder, where `o msg` appends; opened for writing to sandboxed CLIs.
    pub run_dir: &'a str,
}

/// The team channel command each CLI may run without a prompt.
const TEAM_COMMAND: &str = "o msg";

pub fn launch_spec(card: &Card, plan: &Plan, team: &TeamChannel) -> LaunchSpec {
    launch_spec_with(card, plan, team, None)
}

/// `launch_spec` with the card's shared context section (`context::prepare`), if any.
pub fn launch_spec_with(card: &Card, plan: &Plan, team: &TeamChannel, context: Option<&str>) -> LaunchSpec {
    let prompt = card_prompt_with(card, plan, team, context);
    let env = BTreeMap::from([("O_RUN_ID".to_string(), team.run_id.to_string()), ("O_CARD_ID".to_string(), card.id.clone())]);
    get(card.provider).launch(card, plan, team, prompt, env)
}

/// Where an agent whose CLI cannot pass free text to a command writes what it sends.
pub fn outbox_path(run_dir: &str, card_id: &str) -> String {
    format!("{}/outbox/{card_id}.txt", run_dir.trim_end_matches(['\\', '/']))
}

/// The work card text a provider receives. Kept short: it is paid for on every card.
pub fn card_prompt(card: &Card, plan: &Plan, team: &TeamChannel) -> String {
    card_prompt_with(card, plan, team, None)
}

/// The work card text with the shared context section after the task.
pub fn card_prompt_with(card: &Card, plan: &Plan, team: &TeamChannel, context: Option<&str>) -> String {
    let provider = get(card.provider);
    let others: Vec<&Card> = plan.cards.iter().filter(|c| c.id != card.id).collect();
    let role = card.role.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" · {r}")).unwrap_or_default();
    let mut lines = vec![format!("[O 작업카드] {}{role}", plan.title), format!("프로젝트: {}", plan.project)];
    if !others.is_empty() {
        lines.push("같은 폴더에서 다른 카드가 동시에 일합니다. 맡은 파일 밖의 파일은 지우거나 고치지 마세요.".into());
    }
    lines.extend(provider.tool_note());
    lines.push(String::new());
    lines.push(card.task.trim().to_string());
    lines.push(String::new());
    if let Some(context) = context.filter(|c| !c.is_empty()) {
        lines.push(context.to_string());
        lines.push(String::new());
    }
    lines.push(format!("팀 대화 (실행 {}, 내 카드 {}):", team.run_id, card.id));
    for c in &others {
        let model = c.model.as_deref().filter(|m| !m.is_empty()).map(|m| format!(" {m}")).unwrap_or_default();
        lines.push(format!("- {}: {} ({}{model})", c.id, c.label(), c.provider));
    }
    lines.push("- lead: 총괄".into());
    lines.extend(provider.team_how_to(card, team));
    lines.push("시작할 때 한 번 받아 보고, 다른 카드가 알아야 할 결과나 막힌 점이 생기면 짧게 보내세요.".into());
    lines.push(String::new());
    lines.push("끝나면 결과를 다섯 줄 이내로 요약하세요: 한 일, 바꾼 파일, 확인한 것, 남은 일.".into());
    lines.join("\n")
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|a| a.to_string()).collect()
}

fn opt(flag: &str, value: Option<&str>) -> Vec<String> {
    match value {
        Some(v) if !v.is_empty() => vec![flag.to_string(), v.to_string()],
        _ => Vec::new(),
    }
}

/// Turns one stdout line into zero or more signals. Unknown lines are ignored
/// so a CLI update degrades to "less detail", never to a crash.
pub struct StreamReader {
    read: ReadLine,
}

impl StreamReader {
    pub fn new(provider: ProviderId) -> Self {
        StreamReader { read: get(provider).stream_reader() }
    }

    pub fn read(&mut self, line: &str) -> Vec<StreamSignal> {
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        (self.read)(&data)
    }
}
