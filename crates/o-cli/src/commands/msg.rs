//! `o msg send|read|post`: the run's team channel.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use o_core::run::team::{self, Caller, Refused};

use super::{fail, help, print_violations, Command};

pub const MSG: Command = Command {
    name: "msg",
    usage: "  o msg send [--to <카드|lead|all>] \"내용\"   팀 대화에 보내기 (카드 안에서는 실행·카드가 자동)
  o msg read [--all]         첫 메시지 인수 (--all: 상태 변경 없는 전체 대화)
  o msg wait                 새 메시지를 최대 30초 기다린 뒤 첫 메시지 인수
  o msg send --question --id <재시도키> ...  질문 구분과 중복 방지 키
  o msg send --approval --id <재시도키> ...  사람의 승인 요청 (총괄 자동 응답 없음)
  o msg answer-approval --run <실행> --id <요청ID> -  표준 입력으로 승인 요청에 답하기
  o msg send --to <질문자> --reply-to <질문ID> ...  질문에 답하기
  o msg ack|delivered|unknown|retry|cancel --id <메시지ID>  처리 상태 기록
  o msg post                 카드의 outbox 파일(첫 줄 \"to: <상대>\")을 팀 대화로 보내기
                             카드 밖에서는 --run <runId> 와 --as <카드|lead> 로 지정",
    run: message,
};

fn message(home: &Path, args: &[String]) -> i32 {
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
    let speaker = match team::speaker(home, Caller::from_env(flags.get("run").cloned(), flags.get("as").cloned())) {
        Ok(speaker) => speaker,
        Err(message) => return fail(&message),
    };
    if verb == "wait" && flags.contains_key("all") { return fail("wait에는 --all을 사용할 수 없습니다"); }
    if flags.contains_key("approval") && (verb != "send" || flags.contains_key("question") || flags.contains_key("reply-to") || flags.get("to").is_some_and(|to| to != "lead")) {
        return fail("승인 요청은 --approval만 사용해 lead에 보내세요");
    }
    let send = |to: &str, text: &str| {
        let approval = if let Some(id) = flags.get("reply-to") {
            match team::read(home, &speaker, true) {
                Ok(messages) => messages.iter().any(|m| m.id == *id && m.kind == "approval"),
                Err(error) => return fail(&error.to_string()),
            }
        } else { false };
        if approval && (std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) || std::env::var_os("O_CARD_ID").is_some_and(|v| !v.is_empty())) {
            return fail("승인 요청에는 사용자가 직접 답해야 합니다");
        }
        let result = if flags.contains_key("approval") {
            team::send_approval(home, &speaker, text, flags.get("id").map(String::as_str))
        } else {
            team::send_reply(home, &speaker, to, text, flags.contains_key("question"), flags.get("id").map(String::as_str), flags.get("reply-to").map(String::as_str))
        };
        match result {
        Ok(id) => {
            if approval {
                if let Err(error) = team::transition(home, &speaker, flags.get("reply-to").unwrap(), "acknowledged") {
                    return fail(&format!("답변은 저장됐지만 승인 요청의 상태를 확인하지 못했습니다: {error}"));
                }
            }
            println!("저장됨: {} → {to} (id: {id})", speaker.me);
            0
        }
        Err(Refused::Policy(violations)) => {
            print_violations(&violations);
            2
        }
        Err(Refused::Io(error)) => fail(&format!("접수 결과가 불명일 수 있습니다. 같은 --id로 다시 확인해 주세요: {error}")),
    }};
    match verb {
        "answer-approval" => {
            if std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) || std::env::var_os("O_CARD_ID").is_some_and(|v| !v.is_empty()) {
                return fail("승인 요청에는 사용자가 직접 답해야 합니다");
            }
            let Some(id) = flags.get("id") else { return fail("--id가 필요합니다") };
            if words != ["-"] { return fail("답변은 표준 입력으로 보내세요") }
            let mut text = String::new();
            if let Err(error) = std::io::stdin().take(65_537).read_to_string(&mut text) { return fail(&error.to_string()); }
            if text.len() > 65_536 { return fail("답변이 너무 깁니다") }
            match team::answer_approval(home, &speaker, id, &text) {
                Ok(id) => { println!("{}", serde_json::json!({"id": id})); 0 }
                Err(Refused::Policy(violations)) => { print_violations(&violations); 2 }
                Err(Refused::Io(error)) => fail(&error.to_string()),
            }
        }
        "send" => send(flags.get("to").map_or(if flags.contains_key("approval") { "lead" } else { "all" }, String::as_str), &words.join(" ")),
        "post" => {
            match team::post(home, &speaker) {
                Ok((to, id)) => { println!("저장됨: {} → {to} (id: {id})", speaker.me); 0 }
                Err(Refused::Policy(violations)) => { print_violations(&violations); 2 }
                Err(Refused::Io(error)) => fail(&format!("접수 결과가 불명일 수 있습니다. 같은 요청으로 다시 확인해 주세요: {error}")),
            }
        }
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
                    println!("새 메시지가 없습니다.");
                }
                for m in fresh {
                    println!("{}", m.line());
                    if verb == "wait" || !flags.contains_key("all") { println!("처리 후 o msg ack --id {} (불명이면 확인 후 retry)", m.id); }
                }
                0
            }
            Err(error) => fail(&error.to_string()),
        },
        "ack" | "delivered" | "unknown" | "retry" | "cancel" => {
            let Some(id) = flags.get("id") else { return fail("--id가 필요합니다") };
            let state = match verb { "ack" => "acknowledged", "cancel" => "cancelled", "retry" => "stored", other => other };
            match team::transition(home, &speaker, id, state) {
                Ok(()) => { println!("{id}: {state}"); 0 }
                Err(error) => fail(&error.to_string()),
            }
        }
        _ => fail(help()),
    }
}
