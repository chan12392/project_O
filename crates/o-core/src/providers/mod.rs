//! Each official agent CLI behind one `Provider` table: how a card launches it
//! headless and reads its stream, how it is installed and signed in, what its
//! clean profile holds, and how its subscription windows are read. One file per
//! CLI (`claude`, `codex`, `agy`); [`get`] is the registry. Only these files know
//! provider flags.
//!
//! Every launch keeps the CLI's own sign-in: no token is injected. With YOLO off
//! (the default) it keeps the CLI's permission checks too and no flag that disables
//! approvals is added. The one opening is narrow: each CLI may run `o msg`, the run's
//! team channel, and the read-only `o context` search (ADR-0005; Antigravity only gets
//! the section injected into its prompt), and nothing more. A lead chat turn
//! (`Provider::lead`, ADR-0008) gets less: reading the project, nothing else.
//!
//! YOLO mode (ADR-0014), the person's switch: `yolo_launch` and `yolo_lead` use the CLI's
//! official "never ask" flag. These files (and their test) are the only code allowed to
//! spell such flags (`o_guard::rules`); the commands that send work (`AGENT_ONLY_COMMANDS`)
//! stay denied, and Claude keeps O's guard hook through `--settings`.

pub mod agy;
pub mod claude;
pub mod codex;
mod stream;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io;
use std::path::Path;
use std::process::Command;

use serde_json::{Map, Value};

use crate::agents::clean::LoginState;
use crate::run::events::StreamSignal;
use crate::run::plan::{Card, Plan, ProviderId};
use crate::sys;
use crate::usage::UsageWindow;

pub use codex::unwrap_shell;
pub use stream::is_limit_text;

/// What O knows about one official CLI. Adding a CLI: a `ProviderId` variant, a
/// file implementing this, and its line in [`get`] (docs/ARCHITECTURE.md).
pub trait Provider: Sync {
    // Cards: headless launch and stream.

    /// The card's launch: argv with the permission mode, the commands it may run
    /// without a prompt, and the model and effort flags. Never a flag that turns
    /// approvals off.
    fn launch(&self, card: &Card, plan: &Plan, team: &TeamChannel, prompt: String, env: BTreeMap<String, String>) -> LaunchSpec;
    /// API key variables the CLI prefers over its own sign-in when it runs headless.
    /// Left on the PC (an old key in a shell profile or `.env`), they would quietly
    /// bill the API instead of the subscription the card is meant to use, so a
    /// card's launch takes them out (`LaunchSpec::env_remove`).
    fn api_key_vars(&self) -> &'static [&'static str];
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

    // Lead chat (`lead`, ADR-0008): a read-only headless turn that may resume the CLI's session.

    /// The effort values a lead turn of this CLI takes. Empty: the CLI cannot be a lead
    /// (no headless read-only mode with session resume that O can rely on).
    fn lead_efforts(&self) -> &'static [&'static str] {
        &[]
    }
    /// The lead's launch: read-only (it reads the project, never edits or runs commands),
    /// no flag that turns approvals off, resumed when the turn names a session.
    /// `None` when the CLI cannot be a lead.
    fn lead(&self, _turn: &LeadTurn) -> Option<LaunchSpec> {
        None
    }
    /// The models a lead turn may name, from the CLI's own source (its model list command
    /// or its documented aliases). Empty: only the CLI's default (no `--model`).
    fn lead_models(&self) -> Result<Vec<ModelChoice>, String> {
        Ok(Vec::new())
    }

    // YOLO mode (ADR-0014): the person's switch, never a plan's. Only these two launches may
    // carry the CLI's own "never ask" flag; each keeps `AGENT_ONLY_COMMANDS` denied where the
    // CLI has deny rules. The default is `None`: the card or lead then runs as with YOLO off.

    /// The card's launch with YOLO on.
    fn yolo_launch(&self, _card: &Card, _plan: &Plan, _team: &TeamChannel, _prompt: String, _env: BTreeMap<String, String>) -> Option<LaunchSpec> {
        None
    }
    /// The lead's launch with YOLO on: it may edit files and run commands; still resumed like `lead`.
    fn yolo_lead(&self, _turn: &LeadTurn) -> Option<LaunchSpec> {
        None
    }

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
    /// Inherited variables taken out before launch (see `Provider::api_key_vars`).
    pub env_remove: Vec<String>,
}

/// Names to take out of the inherited environment: the listed ones, plus any
/// inherited spelling of them in another case (Windows reads names without case).
pub fn names_to_remove(remove: &[String], inherited: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let mut names: Vec<OsString> = remove.iter().map(OsString::from).collect();
    for key in inherited {
        let listed = key.to_str().is_some_and(|k| remove.iter().any(|r| r.eq_ignore_ascii_case(k)));
        if listed && !names.contains(&key) {
            names.push(key);
        }
    }
    names
}

/// Takes `names` (in any inherited spelling) out of what `command` inherits.
pub fn remove_inherited(command: &mut Command, names: &[String]) {
    for name in names_to_remove(names, std::env::vars_os().map(|(k, _)| k)) {
        command.env_remove(name);
    }
}

/// Takes the provider's API key variables out of a command that runs its CLI
/// with the person's own environment, so it sees the sign-in a card would.
pub fn without_api_keys(command: &mut Command, provider: ProviderId) {
    remove_inherited(command, &strings(get(provider).api_key_vars()));
}

/// One lead chat turn as a provider launches it.
#[derive(Clone, Debug, Default)]
pub struct LeadTurn<'a> {
    /// The thread's project, where the CLI starts and what it reads.
    pub project: &'a str,
    pub prompt: String,
    /// The CLI's own session id from an earlier turn of this thread, to resume.
    pub session: Option<&'a str>,
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    /// The thread's attachment folder (outside O's state folder), opened for reading.
    pub files_dir: Option<&'a str>,
    /// Image attachments, for CLIs that take images natively.
    pub images: &'a [String],
}

/// Whether a provider can be the lead (`Provider::lead_efforts` is not empty).
pub fn can_lead(id: ProviderId) -> bool {
    !get(id).lead_efforts().is_empty()
}

/// One model a lead may run with: the id the CLI takes after `--model`, its name for people,
/// and, for an alias, the model it stands for now (Claude: `opus` → `claude-opus-5-5`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ModelChoice {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// The lead's model list for a provider, keeping only ids a card's `model` field would take
/// (`policy::is_model_id`). Asks the CLI each time; the shells cache the answer.
pub fn lead_models(id: ProviderId) -> Result<Vec<ModelChoice>, String> {
    if !can_lead(id) {
        return Err(format!("{id}는 총괄로 쓸 수 없습니다"));
    }
    let mut models = get(id).lead_models()?;
    models.retain(|m| crate::policy::is_model_id(&m.id));
    Ok(models)
}

/// Runs one of the CLI's own read-only listing commands (no model turn): argv only, no shell,
/// no window, its API key variables taken out. Returns stdout.
fn list_command(bin: &str, args: &[&str], provider: ProviderId, env: &[(&str, &str)]) -> Result<String, String> {
    let cli = sys::resolve::resolve_cli(bin).ok_or_else(|| format!("{bin} CLI를 찾지 못했습니다"))?;
    let mut command = Command::new(&cli.command);
    command.args(&cli.prefix).args(args).envs(env.iter().copied());
    without_api_keys(&mut command, provider);
    let (status, out, err) = sys::proc::capture(&mut command, std::time::Duration::from_secs(30)).map_err(|e| format!("{bin} {}: {e}", args.join(" ")))?;
    if !status.success() {
        let why = err.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or("시간 초과 또는 실패").to_string();
        return Err(format!("{bin} {} 실패: {why}", args.join(" ")));
    }
    Ok(out)
}

pub struct TeamChannel<'a> {
    pub run_id: &'a str,
    /// The run's folder, where `o msg` appends; opened for writing to sandboxed CLIs.
    pub run_dir: &'a str,
}

/// The team channel command each CLI may run without a prompt.
const TEAM_COMMAND: &str = "o msg";

/// The `o` commands no card or lead may run, in any mode (ADR-0014): only a person sends work.
/// `o` itself refuses them inside a card or a lead turn (`run::inside`); CLIs with deny rules
/// also get these, in YOLO mode, as rules that hold even when nothing else asks.
pub const AGENT_ONLY_COMMANDS: [&str; 5] = ["o dispatch", "o lead", "o review", "o _run", "o _review"];

/// The card prompt's tool line with YOLO on, in place of the CLI's own `tool_note`.
const YOLO_NOTE: &str = "YOLO 모드: 파일 수정과 셸 명령을 승인 없이 실행합니다. 맡은 일 밖의 파일·명령은 건드리지 마세요. o dispatch·o lead·o review는 쓸 수 없습니다(작업은 사람이 보냅니다).";

pub fn launch_spec(card: &Card, plan: &Plan, team: &TeamChannel) -> LaunchSpec {
    launch_spec_with(card, plan, team, None)
}

/// `launch_spec` with the card's shared context section (`context::prepare`), if any.
pub fn launch_spec_with(card: &Card, plan: &Plan, team: &TeamChannel, context: Option<&str>) -> LaunchSpec {
    launch_spec_for(card, plan, team, context, false)
}

/// `launch_spec_with` in the run's mode: with `yolo` (ADR-0014) the CLI's YOLO launch, or its
/// usual one when it has none. YOLO off is byte for byte the launch it always was.
pub fn launch_spec_for(card: &Card, plan: &Plan, team: &TeamChannel, context: Option<&str>, yolo: bool) -> LaunchSpec {
    let env = || BTreeMap::from([("O_RUN_ID".to_string(), team.run_id.to_string()), ("O_CARD_ID".to_string(), card.id.clone())]);
    let provider = get(card.provider);
    if yolo {
        if let Some(spec) = provider.yolo_launch(card, plan, team, prompt_text(card, plan, team, context, true), env()) {
            return spec;
        }
    }
    provider.launch(card, plan, team, prompt_text(card, plan, team, context, false), env())
}

/// The lead's launch in the turn's mode: with `yolo` its YOLO launch (or the read-only one when
/// the CLI has none). `None` when the CLI cannot lead.
pub fn lead_spec(id: ProviderId, turn: &LeadTurn, yolo: bool) -> Option<LaunchSpec> {
    let provider = get(id);
    let yolo = if yolo { provider.yolo_lead(turn) } else { None };
    yolo.or_else(|| provider.lead(turn))
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
    prompt_text(card, plan, team, context, false)
}

fn prompt_text(card: &Card, plan: &Plan, team: &TeamChannel, context: Option<&str>, yolo: bool) -> String {
    let provider = get(card.provider);
    let others: Vec<&Card> = plan.cards.iter().filter(|c| c.id != card.id).collect();
    let role = card.role.as_deref().filter(|r| !r.is_empty()).map(|r| format!(" · {r}")).unwrap_or_default();
    let mut lines = vec![format!("[O 작업카드] {}{role}", plan.title), format!("프로젝트: {}", plan.project)];
    if !others.is_empty() {
        lines.push("같은 폴더에서 다른 카드가 동시에 일합니다. 맡은 파일 밖의 파일은 지우거나 고치지 마세요.".into());
    }
    if yolo {
        lines.push(YOLO_NOTE.into());
    } else {
        lines.extend(provider.tool_note());
    }
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
    /// A limit was already reported for this card.
    limited: bool,
}

impl StreamReader {
    pub fn new(provider: ProviderId) -> Self {
        StreamReader { read: get(provider).stream_reader(), limited: false }
    }

    pub fn read(&mut self, line: &str) -> Vec<StreamSignal> {
        let Ok(Value::Object(data)) = serde_json::from_str::<Value>(line) else { return Vec::new() };
        let mut out = (self.read)(&data);
        // One limit per card: a later guess from the error text (no reset time, so the
        // default cooldown) must not stretch the reset time a limit event already gave.
        if self.limited {
            out.retain(|s| !matches!(s, StreamSignal::Limit { until } if until.is_empty()));
        }
        self.limited |= out.iter().any(|s| matches!(s, StreamSignal::Limit { .. }));
        out
    }
}
