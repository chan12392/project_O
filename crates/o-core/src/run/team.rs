//! The run's team channel. Cards get O_RUN_ID and O_CARD_ID from the runner;
//! the lead or a person passes --run and --as. Messages are events in the
//! run's events.jsonl, checked by the same policy rules as tasks (ADR-0002).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::run::events::OEvent;
use crate::sys::js::now_iso;
use crate::policy::{check_message, Parties, Violation};
use crate::providers::outbox_path;
use crate::run::store::{append_event, is_run_id, plan_card_ids, read_events, read_plan, run_dir};

/// Who is speaking, in which run.
pub struct Speaker {
    pub run_id: String,
    pub me: String,
    pub cards: Vec<String>,
}

/// Where the caller stands: the card's environment, and the flags a lead or person passed.
pub struct Caller {
    pub env_card: Option<String>,
    pub env_run: Option<String>,
    pub flag_run: Option<String>,
    pub flag_as: Option<String>,
}

impl Caller {
    pub fn from_env(flag_run: Option<String>, flag_as: Option<String>) -> Self {
        Caller { env_card: std::env::var("O_CARD_ID").ok(), env_run: std::env::var("O_RUN_ID").ok(), flag_run, flag_as }
    }
}

/// Fixes who and where. Inside a card the runner's identity wins: a card cannot
/// speak as the lead or reach another run. `me` names files (outbox, read
/// cursor), so it must be a party of this run. Errors are the message to print.
pub fn speaker(home: &Path, caller: Caller) -> Result<Speaker, String> {
    let in_card = caller.env_card.as_deref().is_some_and(|c| !c.is_empty());
    let run_id = if in_card { caller.env_run } else { caller.flag_run.or(caller.env_run) };
    let me = if in_card { caller.env_card.unwrap_or_default() } else { caller.flag_as.unwrap_or_else(|| "lead".into()) };
    let run_id = match run_id {
        Some(id) if !id.is_empty() && is_run_id(&id) && run_dir(home, &id).exists() => id,
        other => return Err(format!("실행을 찾지 못했습니다: {}", other.unwrap_or_else(|| "(--run 없음)".into()))),
    };
    let cards = read_plan(home, &run_id).and_then(|plan| plan_card_ids(&plan)).map_err(|e| e.to_string())?;
    if me != "lead" && !cards.contains(&me) {
        return Err(format!("이 실행의 카드가 아닙니다: {me}"));
    }
    Ok(Speaker { run_id, me, cards })
}

/// Why a message was not sent.
pub enum Refused {
    /// The policy blocked it.
    Policy(Vec<Violation>),
    /// The channel could not be written.
    Io(io::Error),
}

/// Checks and appends one message.
pub fn send(home: &Path, speaker: &Speaker, to: &str, text: &str) -> Result<(), Refused> {
    let violations = check_message(text, &Parties { from: &speaker.me, to, known: &speaker.cards });
    if !violations.is_empty() {
        return Err(Refused::Policy(violations));
    }
    let event = OEvent::Msg { at: now_iso(), from: speaker.me.clone(), to: to.to_string(), text: text.to_string() };
    append_event(home, &speaker.run_id, &event).map_err(Refused::Io)
}

static TO_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^to:\s*(\S+)").unwrap());

/// The card's outbox file, for CLIs that may only run fixed commands.
pub fn outbox(home: &Path, speaker: &Speaker) -> PathBuf {
    PathBuf::from(outbox_path(&run_dir(home, &speaker.run_id).to_string_lossy(), &speaker.me))
}

/// Reads an outbox file: the first line `to: <party>` names the receiver (else everyone).
pub fn parse_outbox(content: &str) -> (String, String) {
    let lines: Vec<&str> = content.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let first = lines.first().copied().unwrap_or_default();
    match TO_LINE.captures(first) {
        Some(to) => (to[1].to_string(), lines[1..].join("\n").trim().to_string()),
        None => ("all".to_string(), lines.join("\n").trim().to_string()),
    }
}

/// A team message as `o msg read` prints it: from, to and the text.
pub struct Received {
    pub from: String,
    pub to: String,
    pub text: String,
}

impl Received {
    /// One line per message: a newline in the text can't fake a second sender.
    pub fn line(&self) -> String {
        format!("[{} → {}] {}", self.from, self.to, self.text.replace("\r\n", " ⏎ ").replace('\n', " ⏎ "))
    }
}

/// New messages for the speaker (or, with `all`, the whole channel). Without
/// `all` it moves the speaker's read cursor past what it returns.
pub fn read(home: &Path, speaker: &Speaker, all: bool) -> io::Result<Vec<Received>> {
    let messages: Vec<Received> = read_events(home, &speaker.run_id)
        .into_iter()
        .filter_map(|e| match e {
            OEvent::Msg { from, to, text, .. } => Some(Received { from, to, text }),
            _ => None,
        })
        .collect();
    let mine: Vec<Received> = if all {
        messages
    } else {
        messages.into_iter().filter(|m| m.from != speaker.me && (m.to == speaker.me || m.to == "all")).collect()
    };
    let cursor = run_dir(home, &speaker.run_id).join(format!("read.{}", speaker.me));
    let seen = if all { 0 } else { fs::read_to_string(&cursor).ok().and_then(|t| t.trim().parse::<usize>().ok()).unwrap_or(0) };
    if !all {
        fs::write(&cursor, mine.len().to_string())?;
    }
    Ok(mine.into_iter().skip(seen).collect())
}
