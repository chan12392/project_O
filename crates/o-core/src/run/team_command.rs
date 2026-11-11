//! What `o msg <verb> …` does and prints, apart from the process that runs it: `o msg` itself,
//! and the runner answering for a sandboxed card that may not write the run folder
//! (`run::msg_relay`, ADR-0043).

use std::collections::HashMap;
use std::io;
use std::path::Path;

use crate::policy::Violation;
use crate::run::team::{self, Caller, Refused};

/// What the command printed and its exit code.
#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Output {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Output {
    fn say(&mut self, line: impl AsRef<str>) {
        self.out.push_str(line.as_ref());
        self.out.push('\n');
    }

    fn fail(mut self, message: impl AsRef<str>) -> Output {
        self.err.push_str(message.as_ref());
        self.err.push('\n');
        self.code = 1;
        self
    }

    fn blocked(mut self, violations: &[Violation]) -> Output {
        self.err.push_str("차단됨 — 정책 위반으로 이 계획은 실행되지 않았습니다:\n");
        for v in violations {
            let card = v.card_id.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
            self.err.push_str(&format!("  - [{}]{card} {}\n", v.rule, v.message));
        }
        self.err.push_str("위반을 우회하지 말고 사용자에게 이 내용을 그대로 보고하세요.\n");
        self.code = 2;
        self
    }
}

/// Who runs the command and what only the command line can give it.
pub struct Request<'a> {
    pub caller: Caller,
    /// A card or a lead turn runs it: it may not answer a request for the person's decision.
    pub agent: bool,
    /// Printed for a verb that does not exist.
    pub usage: &'a str,
    /// The text of `answer-approval -`, read from standard input.
    pub answer: &'a mut dyn FnMut() -> io::Result<String>,
}

/// `o msg <args>`.
pub fn run(home: &Path, request: Request, args: &[String]) -> Output {
    let Request { caller, agent, usage, answer } = request;
    let mut output = Output::default();
    let (verb, rest) = match args.split_first() {
        Some((verb, rest)) => (verb.as_str(), rest),
        None => ("", &[][..]),
    };
    let mut flags: HashMap<String, String> = HashMap::new();
    let mut words: Vec<&str> = Vec::new();
    let mut items = rest.iter();
    while let Some(arg) = items.next() {
        if arg == "--all" || arg == "--question" || arg == "--approval" {
            flags.insert(arg[2..].into(), "1".into());
        } else if let Some(name) = arg.strip_prefix("--") {
            flags.insert(name.into(), items.next().cloned().unwrap_or_default());
        } else {
            words.push(arg);
        }
    }
    let caller = Caller { flag_run: flags.get("run").cloned(), flag_as: flags.get("as").cloned(), ..caller };
    let speaker = match team::speaker(home, caller) {
        Ok(speaker) => speaker,
        Err(message) => return output.fail(message),
    };
    if verb == "wait" && flags.contains_key("all") { return output.fail("wait에는 --all을 사용할 수 없습니다"); }
    if flags.contains_key("approval") && (verb != "send" || flags.contains_key("question") || flags.contains_key("reply-to") || flags.get("to").is_some_and(|to| to != "lead")) {
        return output.fail("승인 요청은 --approval만 사용해 lead에 보내세요");
    }
    match verb {
        "answer-approval" => {
            if agent { return output.fail("승인 요청에는 사용자가 직접 답해야 합니다"); }
            let Some(id) = flags.get("id") else { return output.fail("--id가 필요합니다") };
            if words != ["-"] { return output.fail("답변은 표준 입력으로 보내세요") }
            let text = match answer() {
                Ok(text) => text,
                Err(error) => return output.fail(error.to_string()),
            };
            if text.len() > 65_536 { return output.fail("답변이 너무 깁니다") }
            match team::answer_approval(home, &speaker, id, &text) {
                Ok(id) => { output.say(serde_json::json!({"id": id}).to_string()); output }
                Err(Refused::Policy(violations)) => output.blocked(&violations),
                Err(Refused::Io(error)) => output.fail(error.to_string()),
            }
        }
        "send" => {
            let to = flags.get("to").map_or(if flags.contains_key("approval") { "lead" } else { "all" }, String::as_str);
            let text = words.join(" ");
            let approval = if let Some(id) = flags.get("reply-to") {
                match team::read(home, &speaker, true) {
                    Ok(messages) => messages.iter().any(|m| m.id == *id && m.kind == "approval"),
                    Err(error) => return output.fail(error.to_string()),
                }
            } else { false };
            if approval && agent { return output.fail("승인 요청에는 사용자가 직접 답해야 합니다"); }
            let result = if flags.contains_key("approval") {
                team::send_approval(home, &speaker, &text, flags.get("id").map(String::as_str))
            } else {
                team::send_reply(home, &speaker, to, &text, flags.contains_key("question"), flags.get("id").map(String::as_str), flags.get("reply-to").map(String::as_str))
            };
            match result {
                Ok(id) => {
                    if approval {
                        if let Err(error) = team::transition(home, &speaker, &flags["reply-to"], "acknowledged") {
                            return output.fail(format!("답변은 저장됐지만 승인 요청의 상태를 확인하지 못했습니다: {error}"));
                        }
                    }
                    output.say(format!("저장됨: {} → {to} (id: {id})", speaker.me));
                    if flags.contains_key("question") && speaker.cards.iter().any(|card| card == to) {
                        let state = team::recipient_state(home, &speaker, to).ok().flatten().unwrap_or("불명");
                        if state != "실행" {
                            output.say(format!("상대 카드의 마지막 기록: {state}. 답을 기다리며 자원을 점유하면 진행이 막힐 수 있습니다. 총괄에 알리고 가능한 독립 작업을 진행하세요."));
                        }
                    }
                    output
                }
                Err(Refused::Policy(violations)) => output.blocked(&violations),
                Err(Refused::Io(error)) => output.fail(format!("접수 결과가 불명일 수 있습니다. 같은 --id로 다시 확인해 주세요: {error}")),
            }
        }
        "post" => match team::post(home, &speaker) {
            Ok((to, id)) => { output.say(format!("저장됨: {} → {to} (id: {id})", speaker.me)); output }
            Err(Refused::Policy(violations)) => output.blocked(&violations),
            Err(Refused::Io(error)) => output.fail(format!("접수 결과가 불명일 수 있습니다. 같은 요청으로 다시 확인해 주세요: {error}")),
        },
        "read" | "wait" => match if verb == "read" && flags.contains_key("all") { team::read(home, &speaker, true) } else if verb == "wait" {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                match team::claim(home, &speaker) {
                    Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(250)),
                    result => break result.map(|m| m.into_iter().collect()),
                }
            }
        } else { team::claim(home, &speaker).map(|m| m.into_iter().collect()) } {
            Ok(fresh) => {
                if fresh.is_empty() {
                    output.say("새 메시지가 없습니다.");
                }
                for m in fresh {
                    output.say(m.line());
                    if verb == "wait" || !flags.contains_key("all") { output.say(format!("처리 후 o msg ack --id {} (불명이면 확인 후 retry)", m.id)); }
                }
                output
            }
            Err(error) => output.fail(error.to_string()),
        },
        "ack" | "delivered" | "unknown" | "retry" | "cancel" => {
            let Some(id) = flags.get("id") else { return output.fail("--id가 필요합니다") };
            let state = match verb { "ack" => "acknowledged", "cancel" => "cancelled", "retry" => "stored", other => other };
            match team::transition(home, &speaker, id, state) {
                Ok(()) => { output.say(format!("{id}: {state}")); output }
                Err(error) => output.fail(error.to_string()),
            }
        }
        _ => output.fail(usage),
    }
}
