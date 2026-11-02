//! `o msg send|read|post`: the run's team channel.

use std::collections::HashMap;
use std::path::Path;

use o_core::run::team::{self, Caller, Refused};

use super::{fail, help, print_violations, Command};

pub const MSG: Command = Command {
    name: "msg",
    usage: "  o msg send [--to <카드|lead|all>] \"내용\"   팀 대화에 보내기 (카드 안에서는 실행·카드가 자동)
  o msg read [--all]         나에게 온 새 메시지 (--all: 전체 대화)
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
        if arg == "--all" {
            flags.insert("all".into(), "1".into());
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
    let send = |to: &str, text: &str| match team::send(home, &speaker, to, text) {
        Ok(()) => {
            println!("보냄: {} → {to}", speaker.me);
            0
        }
        Err(Refused::Policy(violations)) => {
            print_violations(&violations);
            2
        }
        Err(Refused::Io(error)) => fail(&error.to_string()),
    };
    match verb {
        "send" => send(flags.get("to").map_or("all", String::as_str), &words.join(" ")),
        "post" => {
            // For CLIs that may only run fixed commands: the message waits in the card's outbox file.
            let path = team::outbox(home, &speaker);
            let Ok(content) = std::fs::read(&path) else {
                return fail(&format!("보낼 메시지 파일이 없습니다: {}", path.to_string_lossy()));
            };
            let (to, text) = team::parse_outbox(&String::from_utf8_lossy(&content));
            let code = send(&to, &text);
            if code == 0 {
                let _ = std::fs::remove_file(&path);
            }
            code
        }
        "read" => match team::read(home, &speaker, flags.contains_key("all")) {
            Ok(fresh) => {
                if fresh.is_empty() {
                    println!("새 메시지가 없습니다.");
                }
                for m in fresh {
                    println!("{}", m.line());
                }
                0
            }
            Err(error) => fail(&error.to_string()),
        },
        _ => fail(help()),
    }
}
