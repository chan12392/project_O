//! `o msg send|read|post`: the run's team channel.

use std::io::Read;
use std::path::Path;

use o_core::run::msg_relay;
use o_core::run::team::Caller;
use o_core::run::team_command::{self, Output, Request};

use super::{fail, help, Command};

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
    // ADR-0043: a card whose sandbox may not write the run folder asks the runner that started it.
    let output = match msg_relay::from_env().filter(|_| super::inside_card()) {
        Some(mailbox) => match msg_relay::ask(&mailbox, args) {
            Ok(output) => output,
            Err(error) => return fail(&error.to_string()),
        },
        None => {
            let caller = match Caller::from_env(None, None) {
                Ok(caller) => caller,
                Err(message) => return fail(&message),
            };
            let mut answer = || {
                let mut text = String::new();
                std::io::stdin().take(65_537).read_to_string(&mut text)?;
                Ok(text)
            };
            team_command::run(home, Request { caller, agent: super::inside_agent(), usage: help(), answer: &mut answer }, args)
        }
    };
    let Output { code, out, err } = output;
    print!("{out}");
    eprint!("{err}");
    code
}
