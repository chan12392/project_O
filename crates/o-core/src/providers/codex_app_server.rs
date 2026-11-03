//! Codex app-server's JSONL conversation protocol. The caller owns the child process.

use std::collections::VecDeque;
use std::io::{BufRead, Write};

use serde_json::{json, Value};

use super::LeadTurn;

/// Events from one Codex turn, in the order received from app-server.
#[derive(Debug, PartialEq)]
pub enum Event {
    Session(String),
    Delta(String),
    ApprovalDenied,
    Item(Value),
    Usage(Value),
    Finished { ok: bool, error: String },
}

/// Map completed app-server events to the provider stream already consumed by O.
pub fn stream_line(event: Event) -> Option<String> {
    let line = match event {
        Event::Session(session_id) => json!({"type":"thread.started","thread_id":session_id}),
        Event::Delta(delta) => json!({"type":"o.delta","delta":delta}),
        Event::ApprovalDenied => json!({"type":"o.approval_denied"}),
        Event::Item(mut item) => {
            let kind = item["type"].as_str().unwrap_or("");
            let kind = match kind {
                "agentMessage" => "agent_message",
                "commandExecution" => "command_execution",
                "fileChange" => "file_change",
                "webSearch" => "web_search",
                _ => return None,
            };
            item["type"] = json!(kind);
            json!({"type":"item.completed","item":item})
        }
        Event::Usage(usage) => json!({"type":"turn.usage","usage":{
            "input_tokens":usage["inputTokens"],"cached_input_tokens":usage["cachedInputTokens"],"output_tokens":usage["outputTokens"]
        }}),
        Event::Finished { ok: true, .. } => json!({"type":"turn.completed"}),
        Event::Finished { ok: false, error } => json!({"type":"turn.failed","error":{"message":error}}),
    };
    Some(line.to_string())
}

fn send(output: &mut impl Write, message: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *output, message).map_err(|e| e.to_string())?;
    output.write_all(b"\n").and_then(|_| output.flush()).map_err(|e| e.to_string())
}

/// The version this protocol was checked against; an older CLI may lack `app-server` or its v2 API.
const UNSUPPORTED: &str = "Codex app-server를 시작하지 못했습니다. Codex CLI를 0.166.0 이상으로 업데이트하세요";

/// Approvals stay with the user (never an auto reviewer) and are never asked: the lead is read-only (ADR-0008).
const APPROVALS: [(&str, &str); 2] = [("approvalPolicy", "never"), ("approvalsReviewer", "user")];

/// A refusal for a server request O cannot answer, in that request's own response shape;
/// `None` for request types without one (they get a JSON-RPC error). Nothing is ever granted.
fn refusal(method: &str) -> Option<Value> {
    Some(match method {
        "item/permissions/requestApproval" => json!({"permissions":{},"scope":"turn"}),
        "mcpServer/elicitation/request" => json!({"action":"decline"}),
        "item/tool/requestUserInput" => json!({"answers":{}}),
        _ => return None,
    })
}

/// Drive one turn over a connected stdio transport. Command and file approvals are declined,
/// other server requests refused; the caller must not mark a message delivered until `Session` is emitted.
/// A transport that never finishes `initialize` reports the Codex version it needs.
pub fn run(turn: &LeadTurn<'_>, input: impl BufRead, output: impl Write, emit: impl FnMut(Event)) -> Result<(), String> {
    let mut phase = 1;
    drive(turn, input, output, emit, &mut phase).map_err(|error| if phase == 1 { format!("{UNSUPPORTED} ({error})") } else { error })
}

fn drive(
    turn: &LeadTurn<'_>,
    input: impl BufRead,
    mut output: impl Write,
    mut emit: impl FnMut(Event),
    phase: &mut u8,
) -> Result<(), String> {
    send(&mut output, &json!({"id":1,"method":"initialize","params":{
        "clientInfo":{"name":"project_o","title":"Project O","version":env!("CARGO_PKG_VERSION")}
    }}))?;

    let mut thread_id = String::new();
    let mut turn_id = String::new();
    let mut usage_base: Option<Value> = None;
    let mut usage_total: Option<Value> = None;
    let mut pending = VecDeque::new();
    let mut lines = input.lines();
    loop {
        let message: Value = if *phase == 4 && !pending.is_empty() {
            pending.pop_front().unwrap()
        } else {
            let Some(line) = lines.next() else { break };
            let line = line.map_err(|e| e.to_string())?;
            serde_json::from_str(&line).map_err(|e| format!("Codex app-server 응답 오류: {e}"))?
        };
        if let Some(method) = message.get("method").and_then(Value::as_str) {
            if let Some(id) = message.get("id") {
                if matches!(method, "item/commandExecution/requestApproval" | "item/fileChange/requestApproval") {
                    let same_thread = message.pointer("/params/threadId").and_then(Value::as_str) == Some(thread_id.as_str());
                    let request_turn = message.pointer("/params/turnId").and_then(Value::as_str).filter(|id| !id.is_empty());
                    let same_turn = request_turn.is_some() && (*phase == 3 || request_turn == Some(turn_id.as_str()));
                    if !(*phase == 3 || *phase == 4) || !same_thread || !same_turn || !(id.is_string() || id.is_i64() || id.is_u64()) {
                        return Err("Codex 승인 요청의 세션 정보가 올바르지 않습니다".into());
                    }
                    send(&mut output, &json!({"id":id,"result":{"decision":"decline"}}))?;
                    emit(Event::ApprovalDenied);
                    continue;
                }
                if !(id.is_string() || id.is_i64() || id.is_u64()) { return Err(format!("Codex 요청 ID가 올바르지 않습니다: {method}")); }
                match refusal(method) {
                    Some(result) => send(&mut output, &json!({"id":id,"result":result}))?,
                    None => send(&mut output, &json!({"id":id,"error":{"code":-32601,"message":"Project O 총괄은 이 요청을 지원하지 않습니다"}}))?,
                }
                emit(Event::ApprovalDenied);
                continue;
            }
            if matches!(method, "item/agentMessage/delta" | "item/completed" | "thread/tokenUsage/updated" | "turn/completed") {
                if *phase == 3 {
                    if pending.len() >= 10_000 { return Err("Codex 응답 대기열이 너무 큽니다".into()); }
                    pending.push_back(message);
                    continue;
                }
                let same_thread = message.pointer("/params/threadId").and_then(Value::as_str) == Some(thread_id.as_str());
                let event_turn = if method == "turn/completed" { message.pointer("/params/turn/id") } else { message.pointer("/params/turnId") };
                if *phase != 4 || !same_thread || event_turn.and_then(Value::as_str) != Some(turn_id.as_str()) { continue; }
            }
            match method {
                "item/agentMessage/delta" => {
                    if let Some(delta) = message.pointer("/params/delta").and_then(Value::as_str) {
                        emit(Event::Delta(delta.to_string()));
                    }
                }
                "item/completed" => {
                    if let Some(item) = message.pointer("/params/item") { emit(Event::Item(item.clone())); }
                }
                "thread/tokenUsage/updated" => {
                    let usage = &message["params"]["tokenUsage"];
                    if usage["total"].is_object() && usage["last"].is_object() {
                        if usage_base.is_none() {
                            let mut base = json!({});
                            for key in ["inputTokens", "cachedInputTokens", "outputTokens"] {
                                let total = usage["total"][key].as_u64().unwrap_or(0);
                                let last = usage["last"][key].as_u64().unwrap_or(0);
                                base[key] = json!(total.saturating_sub(last));
                            }
                            usage_base = Some(base);
                        }
                        usage_total = Some(usage["total"].clone());
                    }
                }
                "turn/completed" => {
                    if let (Some(base), Some(total)) = (&usage_base, &usage_total) {
                        let mut usage = json!({});
                        for key in ["inputTokens", "cachedInputTokens", "outputTokens"] {
                            usage[key] = json!(total[key].as_u64().unwrap_or(0).saturating_sub(base[key].as_u64().unwrap_or(0)));
                        }
                        emit(Event::Usage(usage));
                    }
                    let turn = &message["params"]["turn"];
                    let status = turn["status"].as_str().unwrap_or("");
                    let ok = status == "completed";
                    let error = turn.pointer("/error/message").and_then(Value::as_str).filter(|message| !message.trim().is_empty())
                        .unwrap_or(match status {
                            "completed" => "",
                            "interrupted" => "Codex 응답이 중단됐습니다",
                            _ => "Codex가 응답을 완료하지 못했습니다",
                        }).to_string();
                    emit(Event::Finished { ok, error });
                    return Ok(());
                }
                _ => {}
            }
            continue;
        }
        if let Some(error) = message.get("error") {
            return Err(error.get("message").and_then(Value::as_str).unwrap_or("Codex app-server 오류").to_string());
        }
        if message.get("id").is_some() && !message.get("result").is_some_and(Value::is_object) {
            return Err("Codex app-server 응답에 결과가 없습니다".into());
        }
        match message.get("id").and_then(Value::as_u64) {
            Some(1) if *phase == 1 => {
                send(&mut output, &json!({"method":"initialized","params":{}}))?;
                let mut params = json!({"cwd":turn.project,"sandbox":"read-only"});
                for (key, value) in APPROVALS { params[key] = json!(value); }
                let method = if let Some(session) = turn.session.filter(|s| !s.is_empty()) {
                    params["threadId"] = json!(session);
                    "thread/resume"
                } else {
                    "thread/start"
                };
                if let Some(model) = turn.model { params["model"] = json!(model); }
                send(&mut output, &json!({"id":2,"method":method,"params":params}))?;
                *phase = 2;
            }
            Some(2) if *phase == 2 => {
                thread_id = message.pointer("/result/thread/id").and_then(Value::as_str)
                    .filter(|id| !id.is_empty()).ok_or("Codex 스레드 ID가 없습니다")?.to_string();
                if turn.session.is_some_and(|session| !session.is_empty() && session != thread_id) {
                    return Err("Codex가 요청과 다른 스레드를 재개했습니다".into());
                }
                let mut content = vec![json!({"type":"text","text":turn.prompt})];
                content.extend(turn.images.iter().map(|path| json!({"type":"localImage","path":path})));
                let mut params = json!({"threadId":thread_id,"cwd":turn.project,"input":content,"sandboxPolicy":{"type":"readOnly"}});
                for (key, value) in APPROVALS { params[key] = json!(value); }
                if let Some(model) = turn.model { params["model"] = json!(model); }
                if let Some(effort) = turn.effort { params["effort"] = json!(effort); }
                send(&mut output, &json!({"id":3,"method":"turn/start","params":params}))?;
                *phase = 3;
            }
            Some(3) if *phase == 3 => {
                if thread_id.is_empty() { return Err("Codex 스레드가 시작되지 않았습니다".into()); }
                turn_id = message.pointer("/result/turn/id").and_then(Value::as_str)
                    .filter(|id| !id.is_empty()).ok_or("Codex 턴 ID가 없습니다")?.to_string();
                emit(Event::Session(thread_id.clone()));
                *phase = 4;
            }
            Some(_) => return Err("Codex app-server 응답 순서가 올바르지 않습니다".into()),
            None => {}
        }
    }
    Err("Codex app-server가 답변을 끝내기 전에 종료됐습니다".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn resume_sends_read_only_turn_and_delivers_stream() {
        let images = vec!["/tmp/image.png".to_string()];
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), session: Some("session-1"), model: Some("m-1"),
            effort: Some("high"), files_dir: None, images: &images, app_server: true };
        let input = concat!(
            "{\"id\":1,\"result\":{}}\n",
            "{\"id\":2,\"result\":{\"thread\":{\"id\":\"session-1\"}}}\n",
            "{\"id\":3,\"result\":{\"turn\":{\"id\":\"turn-1\"}}}\n",
            "{\"method\":\"item/agentMessage/delta\",\"params\":{\"threadId\":\"session-1\",\"turnId\":\"turn-1\",\"delta\":\"hi\"}}\n",
            "{\"method\":\"item/completed\",\"params\":{\"threadId\":\"session-1\",\"turnId\":\"turn-1\",\"item\":{\"type\":\"agentMessage\",\"text\":\"hi\"}}}\n",
            "{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"session-1\",\"turn\":{\"id\":\"turn-1\",\"status\":\"completed\"}}}\n"
        );
        let mut output = Vec::new();
        let mut events = Vec::new();
        run(&turn, Cursor::new(input), &mut output, |e| events.push(e)).unwrap();
        let sent: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
        assert_eq!(sent[2]["method"], "thread/resume");
        assert_eq!(sent[2]["params"]["sandbox"], "read-only");
        assert_eq!(sent[3]["params"]["sandboxPolicy"]["type"], "readOnly");
        for at in [2, 3] {
            assert_eq!(sent[at]["params"]["approvalPolicy"], "never");
            assert_eq!(sent[at]["params"]["approvalsReviewer"], "user");
        }
        assert_eq!(sent[3]["params"]["input"][0]["text"], "hello");
        assert_eq!(sent[3]["params"]["model"], "m-1");
        assert_eq!(sent[3]["params"]["effort"], "high");
        assert_eq!(sent[3]["params"]["input"][1]["path"], "/tmp/image.png");
        assert_eq!(events[0], Event::Session("session-1".into()));
        assert_eq!(events[1], Event::Delta("hi".into()));
        assert_eq!(events.last(), Some(&Event::Finished { ok: true, error: String::new() }));
    }

    #[test]
    fn server_error_or_permission_request_never_reports_success() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        for response in ["{\"id\":1,\"error\":{\"message\":\"no access\"}}\n",
            "{\"id\":1}\n",
            "{\"id\":5,\"method\":\"item/commandExecution/requestApproval\",\"params\":{}}\n",
            "{\"method\":\"turn/completed\",\"params\":{\"turn\":{\"status\":\"completed\"}}}\n"] {
            let mut events = Vec::new();
            assert!(run(&turn, Cursor::new(response), Vec::new(), |e| events.push(e)).is_err());
            assert!(events.is_empty());
        }
    }

    #[test]
    fn unsuccessful_turns_report_a_reason_without_an_error_message() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        for (status, message) in [("interrupted", "Codex 응답이 중단됐습니다"), ("failed", "Codex가 응답을 완료하지 못했습니다")] {
            let input = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
                json!({"id":3,"result":{"turn":{"id":"t"}}}),
                json!({"method":"turn/completed","params":{"threadId":"s","turn":{"id":"t","status":status}}})]
                .into_iter().map(|v| format!("{v}\n")).collect::<String>();
            let mut events = Vec::new();
            run(&turn, Cursor::new(input), Vec::new(), |event| events.push(event)).unwrap();
            assert_eq!(events.last(), Some(&Event::Finished { ok: false, error: message.into() }));
        }
    }

    #[test]
    fn read_only_approvals_are_declined_and_the_turn_continues() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        let input = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
            json!({"id":"approval-1","method":"item/commandExecution/requestApproval","params":{"threadId":"s","turnId":"t"}}),
            json!({"id":3,"result":{"turn":{"id":"t"}}}),
            json!({"id":42,"method":"item/fileChange/requestApproval","params":{"threadId":"s","turnId":"t"}}),
            json!({"method":"item/completed","params":{"threadId":"s","turnId":"t","item":{"type":"agentMessage","text":"권한이 필요합니다"}}}),
            json!({"method":"turn/completed","params":{"threadId":"s","turn":{"id":"t","status":"completed"}}})]
            .into_iter().map(|v| format!("{v}\n")).collect::<String>();
        let mut output = Vec::new();
        let mut events = Vec::new();
        run(&turn, Cursor::new(input), &mut output, |e| events.push(e)).unwrap();
        let sent: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
        assert!(sent.contains(&json!({"id":"approval-1","result":{"decision":"decline"}})));
        assert!(sent.contains(&json!({"id":42,"result":{"decision":"decline"}})));
        assert_eq!(events.iter().filter(|event| **event == Event::ApprovalDenied).count(), 2);
        assert_eq!(events.last(), Some(&Event::Finished { ok: true, error: String::new() }));

        let wrong = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
            json!({"id":3,"result":{"turn":{"id":"t"}}}),
            json!({"id":4,"method":"item/commandExecution/requestApproval","params":{"threadId":"other","turnId":"t"}})]
            .into_iter().map(|v| format!("{v}\n")).collect::<String>();
        let mut output = Vec::new();
        assert!(run(&turn, Cursor::new(wrong), &mut output, |_| {}).is_err());
        assert!(!String::from_utf8(output).unwrap().contains("decision"));
    }

    #[test]
    fn new_threads_never_ask_for_approval() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        let input = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
            json!({"id":3,"result":{"turn":{"id":"t"}}}),
            json!({"method":"turn/completed","params":{"threadId":"s","turn":{"id":"t","status":"completed"}}})]
            .into_iter().map(|v| format!("{v}
")).collect::<String>();
        let mut output = Vec::new();
        run(&turn, Cursor::new(input), &mut output, |_| {}).unwrap();
        let sent: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
        assert_eq!(sent[2]["method"], "thread/start");
        for at in [2, 3] {
            assert_eq!(sent[at]["params"]["approvalPolicy"], "never");
            assert_eq!(sent[at]["params"]["approvalsReviewer"], "user");
        }
    }

    #[test]
    fn other_server_requests_are_refused_and_the_turn_continues() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        let input = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
            json!({"id":3,"result":{"turn":{"id":"t"}}}),
            json!({"id":10,"method":"item/permissions/requestApproval","params":{"threadId":"s","turnId":"t","permissions":{"network":{"enabled":true}}}}),
            json!({"id":11,"method":"mcpServer/elicitation/request","params":{"threadId":"s","serverName":"x","mode":"url"}}),
            json!({"id":12,"method":"item/tool/requestUserInput","params":{"threadId":"s","turnId":"t","questions":[{"id":"q"}]}}),
            json!({"id":"13","method":"item/tool/call","params":{"threadId":"s","turnId":"t"}}),
            json!({"id":14,"method":"account/chatgptAuthTokens/refresh","params":{}}),
            json!({"method":"item/completed","params":{"threadId":"s","turnId":"t","item":{"type":"agentMessage","text":"done"}}}),
            json!({"method":"turn/completed","params":{"threadId":"s","turn":{"id":"t","status":"completed"}}})]
            .into_iter().map(|v| format!("{v}
")).collect::<String>();
        let mut output = Vec::new();
        let mut events = Vec::new();
        run(&turn, Cursor::new(input), &mut output, |e| events.push(e)).unwrap();
        let sent: Vec<Value> = String::from_utf8(output).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
        assert!(sent.contains(&json!({"id":10,"result":{"permissions":{},"scope":"turn"}})));
        assert!(sent.contains(&json!({"id":11,"result":{"action":"decline"}})));
        assert!(sent.contains(&json!({"id":12,"result":{"answers":{}}})));
        for id in [json!("13"), json!(14)] {
            let reply = sent.iter().find(|m| m["id"] == id).unwrap();
            assert_eq!(reply["error"]["code"], -32601);
            assert!(reply.get("result").is_none());
        }
        assert_eq!(events.iter().filter(|event| **event == Event::ApprovalDenied).count(), 5);
        assert_eq!(events.last(), Some(&Event::Finished { ok: true, error: String::new() }));
    }

    #[test]
    fn a_cli_without_app_server_names_the_version_it_needs() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        for input in ["", "{\"id\":1,\"error\":{\"message\":\"unknown method\"}}
"] {
            let error = run(&turn, Cursor::new(input), Vec::new(), |_| {}).unwrap_err();
            assert!(error.starts_with(UNSUPPORTED), "{error}");
        }
        let input = [json!({"id":1,"result":{}}), json!({"id":2,"error":{"message":"no thread"}})]
            .into_iter().map(|v| format!("{v}
")).collect::<String>();
        assert_eq!(run(&turn, Cursor::new(input), Vec::new(), |_| {}).unwrap_err(), "no thread");
    }

    #[test]
    fn resumed_thread_usage_counts_only_this_turn_once() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), session: Some("s"), ..Default::default() };
        let first = json!({"inputTokens":100,"cachedInputTokens":40,"outputTokens":10});
        let second = json!({"inputTokens":200,"cachedInputTokens":80,"outputTokens":20});
        let input = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
            json!({"id":3,"result":{"turn":{"id":"t"}}}),
            json!({"method":"thread/tokenUsage/updated","params":{"threadId":"s","turnId":"t","tokenUsage":{"last":first,"total":{"inputTokens":1100,"cachedInputTokens":540,"outputTokens":110}}}}),
            json!({"method":"thread/tokenUsage/updated","params":{"threadId":"s","turnId":"t","tokenUsage":{"last":second,"total":{"inputTokens":1300,"cachedInputTokens":620,"outputTokens":130}}}}),
            json!({"method":"turn/completed","params":{"threadId":"s","turn":{"id":"t","status":"completed"}}})]
            .into_iter().map(|v| format!("{v}\n")).collect::<String>();
        let mut events = Vec::new();
        run(&turn, Cursor::new(input), Vec::new(), |e| events.push(e)).unwrap();
        let usage: Vec<_> = events.into_iter().filter_map(|e| if let Event::Usage(u) = e { Some(u) } else { None }).collect();
        assert_eq!(usage, [json!({"inputTokens":300,"cachedInputTokens":120,"outputTokens":30})]);
    }

    #[test]
    fn notifications_before_start_response_are_not_lost() {
        let turn = LeadTurn { project: "/project", prompt: "hello".into(), ..Default::default() };
        let input = [json!({"id":1,"result":{}}), json!({"id":2,"result":{"thread":{"id":"s"}}}),
            json!({"method":"item/agentMessage/delta","params":{"threadId":"s","turnId":"t","delta":"answer"}}),
            json!({"method":"turn/completed","params":{"threadId":"s","turn":{"id":"t","status":"completed"}}}),
            json!({"id":3,"result":{"turn":{"id":"t"}}})]
            .into_iter().map(|v| format!("{v}\n")).collect::<String>();
        let mut events = Vec::new();
        run(&turn, Cursor::new(input), Vec::new(), |e| events.push(e)).unwrap();
        assert_eq!(events, [Event::Session("s".into()), Event::Delta("answer".into()), Event::Finished { ok: true, error: String::new() }]);
    }
}
