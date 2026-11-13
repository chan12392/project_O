//! The run's team channel. Cards get O_RUN_ID and O_CARD_ID from the runner;
//! the lead or a person passes --run and --as. Messages are events in the
//! run's events.jsonl, checked by the same policy rules as tasks (ADR-0002).

use std::io;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::sys::js::now_iso;
use crate::policy::{check_message, Parties, Violation};
use crate::providers::outbox_path;
use crate::run::store::{append_value, event_text, is_run_id, plan_card_ids, read_plan, run_dir, with_events};
use serde_json::{json, Value};

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
    /// A process in a card's job (`agent_job`, ADR-0043) speaks only as that card: with the
    /// variables cleared or changed it would otherwise pass for the lead or another card.
    pub fn from_env(flag_run: Option<String>, flag_as: Option<String>) -> Result<Self, String> {
        use crate::sys::agent_job::{card_label, current, AgentJob};
        let caller = Caller { env_card: std::env::var("O_CARD_ID").ok(), env_run: std::env::var("O_RUN_ID").ok(), flag_run, flag_as };
        if let Some(AgentJob::Card(job)) = current() {
            let named = caller.env_run.as_deref().zip(caller.env_card.as_deref()).map(|(run, card)| card_label(run, card));
            if named.as_deref() != Some(job.as_str()) {
                return Err("이 카드의 실행·카드 표식(O_RUN_ID, O_CARD_ID)이 바뀌어 팀 대화를 쓸 수 없습니다".into());
            }
        }
        Ok(caller)
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
/// `key` makes retries by the same sender idempotent, including after a lost response.
pub fn send_tracked(home: &Path, speaker: &Speaker, to: &str, text: &str, question: bool, key: Option<&str>) -> Result<String, Refused> {
    send_reply(home, speaker, to, text, question, key, None)
}

/// Last recorded card state; a damaged record makes the observation inconclusive.
pub fn recipient_state(home: &Path, speaker: &Speaker, to: &str) -> io::Result<Option<&'static str>> {
    if !speaker.cards.iter().any(|card| card == to) { return Ok(None); }
    let text = crate::run::store::locked_event_text(home, &speaker.run_id)?;
    if !text.is_empty() && !text.ends_with('\n') { return Ok(Some("불명")); }
    let mut state = "불명";
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else { return Ok(Some("불명")); };
        if event.get("cardId").and_then(Value::as_str) != Some(to) { continue; }
        state = match event.get("t").and_then(Value::as_str) {
            Some("card.queued") => "대기",
            Some("card.start") => "실행",
            Some("card.end") => "종료",
            _ => state,
        };
    }
    Ok(Some(state))
}

/// An answer names a question in the same run and may only be sent back to its author.
pub fn send_reply(home: &Path, speaker: &Speaker, to: &str, text: &str, question: bool, key: Option<&str>, reply_to: Option<&str>) -> Result<String, Refused> {
    send_kind(home, speaker, to, text, if question { "question" } else { "message" }, key, reply_to)
}

/// A request for the person's decision must never be answered by an autonomous lead.
pub fn send_approval(home: &Path, speaker: &Speaker, text: &str, key: Option<&str>) -> Result<String, Refused> {
    if speaker.me == "lead" { return Err(Refused::Io(io::Error::other("카드만 승인 요청을 보낼 수 있습니다"))); }
    send_kind(home, speaker, "lead", text, "approval", key, None)
}

/// A person's answer references the approval request and keeps retries idempotent.
pub fn answer_approval(home: &Path, speaker: &Speaker, question_id: &str, text: &str) -> Result<String, Refused> {
    if speaker.me != "lead" { return Err(Refused::Io(io::Error::other("승인 요청에는 사용자가 직접 답해야 합니다"))); }
    let messages = read(home, speaker, true).map_err(Refused::Io)?;
    let request = messages.iter().find(|m| m.id == question_id && m.kind == "approval" && m.to == "lead")
        .ok_or_else(|| Refused::Io(io::Error::other("승인 요청을 찾지 못했습니다")))?;
    let id = send_reply(home, speaker, &request.from, text, false, None, Some(question_id))?;
    transition(home, speaker, question_id, "acknowledged")
        .map_err(|e| Refused::Io(io::Error::other(format!("답변은 저장됐지만 승인 요청의 상태를 확인하지 못했습니다: {e}"))))?;
    Ok(id)
}

fn checked_event_text(file: &mut fs::File) -> io::Result<String> {
    let content = event_text(file).map_err(|_| io::Error::other("메시지 기록이 손상되어 전달 상태를 확인할 수 없습니다"))?;
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let event: Value = serde_json::from_str(line).map_err(|_| io::Error::other("메시지 기록이 손상되어 전달 상태를 확인할 수 없습니다"))?;
        let string = |name: &str| event.get(name).and_then(Value::as_str).is_some();
        let optional_string = |name: &str| event.get(name).is_none_or(|value| value.is_null() || value.is_string());
        let valid = match event.get("t").and_then(Value::as_str) {
            Some("msg") => string("from") && string("to") && string("text") && optional_string("id") && optional_string("replyTo")
                && event.get("kind").is_none_or(|value| value.is_null() || matches!(value.as_str(), Some("message" | "question" | "approval"))),
            Some("msg.state") => string("id") && string("recipient") && matches!(event.get("state").and_then(Value::as_str), Some("stored" | "delivered" | "unknown" | "acknowledged" | "cancelled")),
            Some(_) => true,
            None => false,
        };
        if !valid { return Err(io::Error::other("메시지 기록이 손상되어 전달 상태를 확인할 수 없습니다")); }
    }
    Ok(content)
}

fn send_kind(home: &Path, speaker: &Speaker, to: &str, text: &str, kind: &str, key: Option<&str>, reply_to: Option<&str>) -> Result<String, Refused> {
    let violations = check_message(text, &Parties { from: &speaker.me, to, known: &speaker.cards });
    if !violations.is_empty() {
        return Err(Refused::Policy(violations));
    }
    // Nobody reads "wiki" as an inbox (ADR-0041): a question or a reply sent there would wait for ever.
    if to == crate::policy::WIKI_PARTY && !speaker.cards.iter().any(|card| card == to) && (kind != "message" || reply_to.is_some()) {
        return Err(Refused::Io(io::Error::other("wiki에게는 질문이나 답장을 보낼 수 없습니다. 남길 내용만 메시지로 보내세요")));
    }
    if key.is_some_and(|key| key.is_empty() || key.len() > 64 || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')) {
        return Err(Refused::Io(io::Error::other("메시지 키는 영문·숫자·-·_ 1~64자여야 합니다")));
    }
    with_events(home, &speaker.run_id, |file| {
        let content = checked_event_text(file)?;
        let id = key.map_or_else(|| format!("m{}", content.len()), |key| format!("k{}-{}-{key}", speaker.me.len(), speaker.me));
        let events = records(&content);
        if let Some(existing) = events.iter().find(|m| m.id == id) {
            if existing.from == speaker.me && existing.to == to && existing.text == text && existing.kind == kind && existing.reply_to.as_deref() == reply_to { return Ok(id); }
            return Err(io::Error::other("이미 다른 내용으로 사용한 메시지 키입니다"));
        }
        if let Some(question_id) = reply_to {
            let original = events.iter().find(|m| m.id == question_id).ok_or_else(|| io::Error::other("답할 질문을 찾지 못했습니다"))?;
            if kind != "message" || !matches!(original.kind.as_str(), "question" | "approval") || original.from != to || original.from == speaker.me || (original.to != speaker.me && original.to != "all") {
                return Err(io::Error::other("이 질문의 수신자만 작성자에게 답할 수 있습니다"));
            }
            if states_for(home, &speaker.run_id, &content, &speaker.me)?.get(question_id).is_some_and(|s| s == "cancelled") {
                return Err(io::Error::other("취소된 질문에는 답할 수 없습니다"));
            }
            if let Some(answer) = events.iter().find(|m| m.from == speaker.me && m.reply_to.as_deref() == Some(question_id)) {
                if answer.to == to && answer.text == text { return Ok(answer.id.clone()); }
                return Err(io::Error::other("이미 답한 질문입니다. 기존 답변을 확인해 주세요"));
            }
        }
        let mut event = json!({"t":"msg", "at":now_iso(), "from":speaker.me, "to":to, "text":text, "id":id, "kind":kind});
        if let Some(question_id) = reply_to { event["replyTo"] = json!(question_id); }
        append_value(file, &event)?;
        Ok(id)
    }).map_err(Refused::Io)
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

/// Claim the outbox before posting; a crash leaves the claim for an idempotent retry.
pub fn post(home: &Path, speaker: &Speaker) -> Result<(String, String), Refused> {
    let path = outbox(home, speaker);
    let dir = path.parent().ok_or_else(|| Refused::Io(io::Error::other("outbox 경로가 없습니다")))?;
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("{}.post.lock", speaker.me))).map_err(Refused::Io)?;
    lock.lock().map_err(Refused::Io)?;
    let prefix = format!("{}.pending-", speaker.me);
    let mut pending: Vec<_> = fs::read_dir(dir).map_err(Refused::Io)?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&prefix))).collect();
    pending.sort();
    let claimed = if let Some(path) = pending.into_iter().next() { path } else {
        let nonce = format!("{}-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(), std::process::id());
        let claimed = dir.join(format!("{prefix}{nonce}"));
        fs::rename(&path, &claimed).map_err(Refused::Io)?;
        claimed
    };
    let name = claimed.file_name().and_then(|n| n.to_str()).ok_or_else(|| Refused::Io(io::Error::other("outbox 이름 오류")))?;
    let key = name.strip_prefix(&prefix).ok_or_else(|| Refused::Io(io::Error::other("outbox 이름 오류")))?;
    let content = fs::read(&claimed).map_err(Refused::Io)?;
    let (to, text) = parse_outbox(&String::from_utf8_lossy(&content));
    let id = match send_tracked(home, speaker, &to, &text, false, Some(key)) {
        // A policy refusal is final and nothing was stored: set the claim aside (content kept) so a
        // rewritten outbox is posted next instead of the refused one again (#60). I/O errors keep the
        // claim for the idempotent retry.
        Err(Refused::Policy(violations)) => {
            fs::rename(&claimed, dir.join(format!("{}.rejected-{key}", speaker.me))).map_err(Refused::Io)?;
            return Err(Refused::Policy(violations));
        }
        result => result?,
    };
    fs::remove_file(claimed).map_err(Refused::Io)?;
    Ok((to, id))
}

/// A team message as `o msg read` prints it: from, to and the text.
pub struct Received {
    pub id: String,
    pub kind: String,
    pub from: String,
    pub to: String,
    pub text: String,
    pub state: String,
    pub reply_to: Option<String>,
    legacy: bool,
}

impl Received {
    /// One line per message: a newline in the text can't fake a second sender.
    pub fn line(&self) -> String {
        let reply = self.reply_to.as_ref().map(|id| format!(", 답변 대상: {id}")).unwrap_or_default();
        format!("[{} → {}] {} (id: {}, {}, {}{})", self.from, self.to, self.text.replace("\r\n", " ⏎ ").replace('\n', " ⏎ "), self.id, self.kind, self.state, reply)
    }
}

/// Reading is non-destructive: output does not prove that an agent processed it.
pub fn read(home: &Path, speaker: &Speaker, all: bool) -> io::Result<Vec<Received>> {
    with_events(home, &speaker.run_id, |file| {
        let content = checked_event_text(file)?;
        let states = states_for(home, &speaker.run_id, &content, &speaker.me)?;
        let messages = records(&content).into_iter().filter_map(|mut m| {
            m.state = states.get(&m.id).cloned().unwrap_or_else(|| "stored".into());
            (all || (m.from != speaker.me && (m.to == speaker.me || m.to == "all") && m.state != "acknowledged" && m.state != "cancelled")).then_some(m)
        });
        Ok(if all { messages.collect() } else { messages.take(1).collect() })
    })
}

/// Claim one message atomically. A crash after this point leaves an unknown outcome.
pub fn claim(home: &Path, speaker: &Speaker) -> io::Result<Option<Received>> {
    with_events(home, &speaker.run_id, |file| {
        let content = checked_event_text(file)?;
        let current = states_for(home, &speaker.run_id, &content, &speaker.me)?;
        let msg = records(&content).into_iter().find(|m| {
            m.from != speaker.me && (m.to == speaker.me || m.to == "all") && !matches!(current.get(&m.id).map(String::as_str), Some("acknowledged" | "cancelled"))
        });
        let Some(mut msg) = msg else { return Ok(None) };
        if matches!(current.get(&msg.id).map(String::as_str), Some("unknown" | "delivered")) {
            return Err(io::Error::other(format!("{} 접수 결과가 불명 또는 처리 대기입니다. 확인 후 ack 또는 retry 하세요", msg.id)));
        }
        append_value(file, &json!({"t":"msg.state", "at":now_iso(), "id":msg.id, "recipient":speaker.me, "state":"unknown"}))?;
        msg.state = "unknown".into();
        Ok(Some(msg))
    })
}

fn records(content: &str) -> Vec<Received> {
    let mut offset = 0;
    content.split_inclusive('\n').filter_map(|line| {
        let start = offset;
        offset += line.len();
        if !line.ends_with('\n') { return None; }
        let v: Value = serde_json::from_str(line).ok()?;
        if v["t"] != "msg" { return None; }
        Some(Received { id: v["id"].as_str().map_or_else(|| format!("m{start}"), str::to_string), kind: v["kind"].as_str().unwrap_or("message").into(), from: v["from"].as_str()?.into(), to: v["to"].as_str()?.into(), text: v["text"].as_str()?.into(), state: "stored".into(), reply_to: v["replyTo"].as_str().map(str::to_string), legacy: v["id"].as_str().is_none() })
    }).collect()
}

fn states(content: &str, recipient: &str) -> std::collections::HashMap<String, String> {
    let mut result = std::collections::HashMap::new();
    for line in content.split_inclusive('\n').filter(|line| line.ends_with('\n')) {
        let Ok(v) = serde_json::from_str::<Value>(line) else { continue };
        if v["t"] != "msg.state" { continue; }
        if let (Some(id), Some(state), Some(who)) = (v["id"].as_str(), v["state"].as_str(), v["recipient"].as_str()) {
            if who == recipient || who == "all" { result.insert(id.into(), state.into()); }
        }
    }
    result
}

fn states_for(home: &Path, run_id: &str, content: &str, recipient: &str) -> io::Result<std::collections::HashMap<String, String>> {
    let mut result = states(content, recipient);
    let cursor = run_dir(home, run_id).join(format!("read.{recipient}"));
    let seen = match fs::read_to_string(cursor) {
        Ok(value) => value.trim().parse::<usize>().map_err(|_| io::Error::other("이전 메시지 읽기 기록이 손상되었습니다"))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    // The old cursor proves a read was attempted, not that the receiver processed it.
    for msg in records(content).into_iter().filter(|m| m.legacy && m.from != recipient && (m.to == recipient || m.to == "all")).take(seen) {
        result.entry(msg.id).or_insert_with(|| "unknown".into());
    }
    Ok(result)
}

/// A recipient explicitly confirms processing; a sender may cancel an unprocessed message.
pub fn transition(home: &Path, speaker: &Speaker, id: &str, state: &str) -> io::Result<()> {
    with_events(home, &speaker.run_id, |file| {
        let content = checked_event_text(file)?;
        let messages = records(&content);
        let msg = messages.iter().find(|m| m.id == id).ok_or_else(|| io::Error::other("메시지를 찾지 못했습니다"))?;
        let recipient = match state {
            "stored" | "delivered" | "unknown" | "acknowledged" if msg.from != speaker.me && (msg.to == speaker.me || msg.to == "all") => &speaker.me,
            "cancelled" if msg.from == speaker.me => &msg.to,
            _ => return Err(io::Error::other("메시지 상태를 바꿀 권한이 없습니다")),
        };
        let current = states_for(home, &speaker.run_id, &content, recipient)?;
        if current.get(id).is_some_and(|s| s == state) { return Ok(()); }
        if state == "cancelled" && msg.to == "all" && content.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).any(|v| v["t"] == "msg.state" && v["id"] == id && v["state"] != "cancelled") {
            return Err(io::Error::other("이미 전달을 시작한 메시지는 취소할 수 없습니다"));
        }
        let old = current.get(id).map(String::as_str).unwrap_or("stored");
        if !matches!((old, state), ("stored", "delivered" | "unknown" | "acknowledged" | "cancelled") | ("delivered", "unknown" | "acknowledged") | ("unknown", "stored" | "delivered" | "acknowledged")) {
            return Err(io::Error::other("허용되지 않는 메시지 상태 변경입니다"));
        }
        if state != "cancelled" && messages.iter().take_while(|m| m.id != id).any(|m| m.from != speaker.me && (m.to == speaker.me || m.to == "all") && !matches!(current.get(&m.id).map(String::as_str), Some("acknowledged" | "cancelled"))) {
            return Err(io::Error::other("앞선 메시지부터 확인해 주세요"));
        }
        append_value(file, &json!({"t":"msg.state", "at":now_iso(), "id":id, "recipient":recipient, "state":state}))
    })
}
