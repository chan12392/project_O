//! `o term` and the detached terminal host.
use std::io::{self, Read};
use std::path::Path;
use o_core::term::{self, OpenRequest};
use super::{fail, inside_agent, Command};

pub const TERM: Command = Command { name: "term", usage: "  o term open|list|read|write|resize|paste|close  터미널 세션", run: command };
pub const HOST: Command = Command { name: "_term", usage: "", run: host };

fn host(home: &Path, args: &[String]) -> i32 {
    if inside_agent() { return fail("에이전트 안에서는 터미널 호스트를 시작할 수 없습니다"); }
    match args { [id] => match term::host(home, id) { Ok(()) => 0, Err(e) => fail(&e.to_string()) }, _ => fail("터미널 ID가 필요합니다") }
}
fn command(home: &Path, args: &[String]) -> i32 {
    let Some((verb, rest)) = args.split_first() else { return fail("터미널 명령이 필요합니다") };
    if matches!(verb.as_str(), "open" | "write" | "paste" | "close") && inside_agent() { return fail("에이전트 안에서는 이 터미널 동작을 할 수 없습니다. 사용자가 실행해 주세요."); }
    let result: io::Result<serde_json::Value> = (|| {
        let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "잘못된 터미널 인자");
        match verb.as_str() {
            "open" => {
                let mut req = OpenRequest { provider: String::new(), project: String::new(), thread: None, cols: None, rows: None };
                let mut args = rest.iter();
                while let Some(arg) = args.next() {
                    match arg.as_str() {
                        "--json" => (),
                        "--provider" => req.provider = args.next().ok_or_else(invalid)?.clone(),
                        "--project" => req.project = args.next().ok_or_else(invalid)?.clone(),
                        "--thread" => req.thread = Some(args.next().ok_or_else(invalid)?.clone()),
                        "--cols" => req.cols = Some(args.next().ok_or_else(invalid)?.parse().map_err(|_| invalid())?),
                        "--rows" => req.rows = Some(args.next().ok_or_else(invalid)?.parse().map_err(|_| invalid())?),
                        _ => return Err(invalid()),
                    }
                }
                Ok(serde_json::to_value(term::open(home, req)?).map_err(io::Error::other)?)
            }
            "list" if rest.is_empty() || rest == ["--json"] => Ok(serde_json::to_value(term::list(home)?).map_err(io::Error::other)?),
            "read" if rest.len() == 3 || rest.len() == 4 => {
                if rest[1] != "--offset" || (rest.len() == 4 && rest[3] != "--json") { return Err(invalid()); }
                let offset = rest[2].parse().map_err(|_| invalid())?;
                Ok(serde_json::to_value(term::read(home, &rest[0], offset)?).map_err(io::Error::other)?)
            }
            "write" if rest.len() == 2 && rest[1] == "-" => { let mut bytes = Vec::new(); io::stdin().read_to_end(&mut bytes)?; term::write(home, &rest[0], &bytes)?; Ok(serde_json::Value::Null) }
            "resize" if rest.len() == 3 => { term::resize(home, &rest[0], rest[1].parse().map_err(|_| invalid())?, rest[2].parse().map_err(|_| invalid())?)?; Ok(serde_json::Value::Null) }
            "paste" if rest.len() == 1 => { term::paste(home, &rest[0])?; Ok(serde_json::Value::Null) }
            "close" if rest.len() == 1 => { term::close(home, &rest[0])?; Ok(serde_json::Value::Null) }
            _ => Err(invalid()),
        }
    })();
    match result { Ok(value) => { println!("{value}"); 0 }, Err(error) => fail(&error.to_string()) }
}
