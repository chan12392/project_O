//! `o lead`: the lead chat (ADR-0008, ADR-0034). A message goes to a lead CLI that
//! proposes a plan; `o lead dispatch` runs a stored plan once, after the same check as `o dispatch`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::sync::Arc;

use o_core::agents::clean;
use o_core::lead::{self, DispatchError, SendRequest};
use o_core::run::mode::{take_flags, RunMode};
use o_core::run::plan::ProviderId;
use o_core::run::runner::{official_command, Spawner};

pub const WATCH: super::Command = super::Command {
    name: "_lead-watch",
    usage: "",
    run: |home, _| {
        let (keep, stop) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
            drop(keep);
        });
        o_core::lead::recovery::watch_until(home.to_path_buf(), Arc::new(official_command), stop, std::time::Duration::from_secs(2));
        0
    },
};
/// `o _lead-approve <home> <threadId> <token> <project>`: one normal-mode Claude lead turn's
/// approval server (ADR-0008 개정 3), started by Claude Code as its only MCP server. It asks the
/// person and waits; it never decides anything itself.
pub const APPROVE: super::Command = super::Command {
    name: o_core::lead::approvals::COMMAND,
    usage: "",
    run: |_, args| {
        let [home, thread_id, token, project] = args else { return fail("o _lead-approve <home> <threadId> <token> <project>") };
        let server = o_core::lead::approvals::Server {
            home: PathBuf::from(home),
            thread_id: thread_id.clone(),
            token: token.clone(),
            project: project.clone(),
            wait: o_core::lead::approvals::WAIT,
        };
        o_core::lead::approvals::serve(server, std::io::stdin().lock(), std::io::stdout());
        0
    },
};
use o_core::run::yolo;
use o_core::sys::proc;
use serde_json::json;

use super::{fail, print_violations, Command};

pub const LEAD: Command = Command {
    name: "lead",
    usage: "  o lead send --project <폴더>|--overall [--thread T] [--provider claude|codex|agy] [--model M] [--effort E] [--attach <파일>]... [--new] [--manual-dispatch] [--profile project-o|own] [--mode yolo|normal] <메시지|->
                             총괄에게 메시지, 새 이벤트 JSON 출력
                             --profile project-o(기본)면 총괄도 O의 깨끗한 프로필로, own이면 평소 CLI 설정으로 실행
                             --mode yolo면 직접 수정 담당 탭이 파일을 고치고 명령을 실행 (주지 않으면 o yolo 설정, ADR-0014)
  o lead models --provider claude|codex|agy  총괄이 쓸 수 있는 모델 목록 JSON (CLI의 공식 목록·별칭)
  o lead show [--project <폴더>|--overall] [--thread T] [--json]  총괄 대화와 열린 탭·프로젝트 목록
  o lead log-page --project <폴더> <runId> <SHA-256|-> <바이트 위치>  완료된 실행의 원본 로그 읽기(사람 전용)
                             처음은 - 0, 다음에는 응답의 sha256과 nextOffset을 사용
  o lead note <threadId> -                  설계 주석 추가·삭제 (stdin JSON)
  o lead new --project <폴더>|--overall     새 대화 시작 (--overall: 전체 총괄, ADR-0032)
  o lead close <threadId>                  탭 닫기 (대화 기록은 보존, 사람 전용)
  o lead writer <threadId>                 직접 수정할 총괄 탭 지정 (사람 전용)
  o lead wake <runId>                      대기 중인 카드 메시지를 현재 총괄에 전달
  o lead retry <runId> <messageId>          불명 메시지를 확인 후 복구
  o lead attach --project <폴더> [--thread T] --name <이름> <파일|->   첨부 저장 (이미지·문서, 10MB까지)
  o lead dispatch <planId> [--profile project-o|own] [--no-context] [--mode yolo|normal]
                             제안된 계획을 정책 검사 후 한 번 실행, runId 출력 (--profile은 o dispatch와 같음)
  o lead forward <제안 id>                 전체 총괄이 제안한 요청을 프로젝트 총괄에게 전달 (사람 전용, ADR-0036)
  o lead decide <승인 id> allow|deny         총괄이 기다리는 작업 하나를 허용하거나 거부 (사람 전용)
  o lead stop [--project <폴더>] [--thread T]  답하는 중인 총괄 중지 (탭·프로젝트·전체)
  o lead stop-all --project <폴더>           이 대화의 총괄과 실행을 모두 중지",
    run: lead_main,
};

fn lead_main(home: &Path, args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("close" | "writer" | "forward"))
        && agent_env()
    {
        eprintln!("에이전트 안에서는 총괄 탭을 닫거나 직접 수정을 맡기거나 요청을 전달할 수 없습니다. 사용자가 실행해 주세요.");
        return 2;
    }
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("send") => send(home, rest),
        Some("show") => show(home, rest),
        Some("log-page") => log_page(home, rest),
        Some("note") => note(home, rest),
        Some("new") => new(home, rest),
        Some("close") => close(home, rest),
        Some("writer") => writer(home, rest),
        Some("wake") => wake(home, rest),
        Some("retry") => retry(home, rest),
        Some("attach") => attach(home, rest),
        Some("dispatch") => dispatch(home, rest),
        Some("forward") => forward(home, rest),
        Some("decide") => decide(home, rest),
        Some("stop") => stop(home, rest),
        Some("stop-all") => stop_all(home, rest),
        Some("models") => models(rest),
        _ => fail(LEAD.usage),
    }
}

fn wake(home: &Path, args: &[String]) -> i32 {
    let [run_id] = args else { return fail("o lead wake <runId>가 필요합니다") };
    match lead::wake_run(home, run_id, &official_command) {
        Ok(processed) => { println!("{}", json!({"processed": processed})); 0 }
        Err(error) => fail(&error),
    }
}

fn retry(home: &Path, args: &[String]) -> i32 {
    let [run_id, message_id] = args else { return fail("o lead retry <runId> <messageId>가 필요합니다") };
    match lead::retry_card(home, run_id, message_id) {
        Ok(state) => { println!("{}", json!({"state": state})); 0 }
        Err(error) => fail(&error),
    }
}

fn log_page(home: &Path, args: &[String]) -> i32 {
    if std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) || std::env::var_os("O_CARD_ID").is_some_and(|v| !v.is_empty()) {
        return fail("원본 로그는 사람만 읽을 수 있습니다");
    }
    let [flag, project, run, hash, offset] = args else { return fail("o lead log-page --project <폴더> <runId> <SHA-256|-> <바이트 위치>가 필요합니다") };
    if flag != "--project" { return fail("--project가 필요합니다"); }
    let Ok(offset) = offset.parse() else { return fail("바이트 위치가 올바르지 않습니다") };
    match lead::log_page::read(home, project, run, hash, offset) {
        Ok(page) => { println!("{}", json!(page)); 0 },
        Err(error) => fail(&error),
    }
}

fn note(home: &Path, args: &[String]) -> i32 {
    if args.len() != 2 || args[1] != "-" { return fail("o lead note <threadId> - 가 필요합니다"); }
    let change = stdin_bytes().and_then(|bytes| serde_json::from_slice::<lead::notes::Change>(&bytes).map_err(|e| e.to_string()));
    match change.and_then(|change| lead::notes::change(home, &args[0], change).map_err(|e| e.to_string())) {
        Ok(notes) => { println!("{}", json!(notes)); 0 }
        Err(e) => fail(&e),
    }
}

fn models(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let Some(provider) = a.provider.as_deref().and_then(ProviderId::parse) else { return fail("--provider claude|codex|agy가 필요합니다") };
    match o_core::providers::lead_models(provider) {
        Ok(models) => {
            println!("{}", json!({ "provider": provider, "models": models }));
            0
        }
        Err(e) => fail(&e),
    }
}

/// Flags with a value, repeatable ones collected, switches, and the words left over.
#[derive(Default)]
struct Args {
    project: Option<String>,
    thread: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    name: Option<String>,
    attach: Vec<PathBuf>,
    fresh: bool,
    manual_dispatch: bool,
    /// `--overall`: the overall lead (ADR-0032), whose folder is the projects root.
    overall: bool,
    /// The profile and context choice (`run::mode::take_flags`, ADR-0013).
    mode: RunMode,
    /// `--mode yolo|normal` (ADR-0014); none: the saved switch.
    yolo: Option<bool>,
    json: bool,
    words: Vec<String>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (mode, args) = take_flags(args)?;
    let (yolo, args) = yolo::take_flag(args)?;
    let mut out = Args { mode, yolo, ..Args::default() };
    let mut items = args.iter();
    while let Some(arg) = items.next() {
        let mut value = |flag: &str| items.next().cloned().ok_or_else(|| format!("{flag} 뒤에 값이 필요합니다"));
        match arg.as_str() {
            "--project" => out.project = Some(value("--project")?),
            "--thread" => out.thread = Some(value("--thread")?),
            "--provider" => out.provider = Some(value("--provider")?),
            "--model" => out.model = Some(value("--model")?),
            "--effort" => out.effort = Some(value("--effort")?),
            "--name" => out.name = Some(value("--name")?),
            "--attach" => out.attach.push(PathBuf::from(value("--attach")?)),
            "--new" => out.fresh = true,
            "--manual-dispatch" => out.manual_dispatch = true,
            "--overall" => out.overall = true,
            "--json" => out.json = true,
            other if other.starts_with("--") => return Err(format!("알 수 없는 인자: {other}")),
            other => out.words.push(other.to_string()),
        }
    }
    Ok(out)
}

impl Args {
    /// `--project`, or with `--overall` the projects root (both together are refused).
    fn project(&self, home: &Path) -> Result<Option<String>, String> {
        match (self.overall, &self.project) {
            (true, Some(_)) => Err("--overall과 --project는 함께 쓸 수 없습니다".into()),
            (true, None) => lead::overall::root(home).map(Some).ok_or_else(|| "프로젝트 루트 폴더가 없어 전체 총괄을 쓸 수 없습니다".into()),
            (false, project) => Ok(project.clone()),
        }
    }
}

/// `o _lead-forward <threadId> <n>` for a request the overall lead passed on (ADR-0032).
fn forward_start(thread_id: &str, index: usize) -> std::io::Result<()> {
    proc::spawn_detached(Process::new(std::env::current_exe()?).args(["_lead-forward", thread_id, &index.to_string()])).map(|_| ())
}

fn run_start(run_id: &str) -> std::io::Result<()> {
    proc::spawn_detached(Process::new(std::env::current_exe()?).args(["_run", run_id])).map(|_| ())
}

fn agent_env() -> bool {
    std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) || std::env::var_os("O_CARD_ID").is_some_and(|v| !v.is_empty())
}

/// `o lead forward <proposalId>`: the person sends the overall lead's proposed requests on (ADR-0036).
fn forward(home: &Path, args: &[String]) -> i32 {
    let [proposal_id] = args else { return fail("o lead forward <제안 id>가 필요합니다") };
    match lead::overall::send_on(home, proposal_id, &forward_start) {
        Ok(events) => { println!("{}", json!({ "events": events })); 0 }
        Err(e) => fail(&e),
    }
}

/// `o _lead-forward <threadId> <n>`: sends the overall thread's n-th request to its project's
/// lead (`lead::overall::forward`) and has the overall lead review the plan it makes. Started by
/// O after the person sends a proposal on, or after a review asks for a revision; never by an agent.
pub const FORWARD: Command = Command {
    name: "_lead-forward",
    usage: "",
    run: |home, args| {
        if agent_env() {
            return fail("요청 전달은 O만 시작합니다");
        }
        let [thread_id, index] = args else { return fail("o _lead-forward <threadId> <번호>") };
        let Ok(index) = index.parse() else { return fail("번호가 올바르지 않습니다") };
        let starts = lead::overall::Starts { run: &run_start, forward: &forward_start };
        match lead::overall::forward(home, thread_id, index, &official_command, &starts) {
            Ok(events) => { println!("{}", json!({ "events": events })); 0 }
            Err(e) => fail(&e),
        }
    },
};

fn stdin_bytes() -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn send(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let project = match a.project(home) {
        Ok(Some(project)) => project,
        Ok(None) => return fail("--project <폴더> 또는 --overall이 필요합니다"),
        Err(e) => return fail(&e),
    };
    let project = project.as_str();
    let provider = match a.provider.as_deref().map(|p| ProviderId::parse(p).ok_or(p)) {
        None => ProviderId::Claude,
        Some(Ok(p)) => p,
        Some(Err(p)) => return fail(&format!("알 수 없는 에이전트: {p}")),
    };
    let text = if a.words.len() == 1 && a.words[0] == "-" {
        match stdin_bytes() {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => return fail(&e),
        }
    } else {
        a.words.join(" ")
    };
    let root = a.mode.clean().then(clean::default_root);
    let mut req = SendRequest {
        project,
        thread: a.thread.as_deref(),
        provider,
        model: a.model.as_deref(),
        effort: a.effort.as_deref(),
        text: &text,
        attach: &a.attach,
        fresh: a.fresh,
        clean: root.as_deref(),
        yolo: yolo::resolve(home, a.yolo),
    };
    if let Err(e) = lead::check_request(home, &req) { return fail(&e); }
    let thread = match lead::select_thread(home, project, req.thread, req.fresh) {
        Ok(thread) => thread,
        Err(e) => return fail(&e),
    };
    req.thread = Some(&thread);
    req.fresh = false;
    let spawner: Arc<Spawner> = Arc::new(official_command);
    let start = |run_id: &str| -> std::io::Result<()> {
        proc::spawn_detached(Process::new(std::env::current_exe()?).args(["_run", run_id])).map(|_| ())
    };
    let resume = |run_id: &str| -> std::io::Result<()> {
        proc::spawn_detached(Process::new(std::env::current_exe()?).args(["_followup", run_id])).map(|_| ())
    };
    let result = if a.manual_dispatch { lead::send_with_followups(home, &req, spawner.as_ref(), &resume) } else { lead::send_with_autorun_and_followups(home, &req, spawner.as_ref(), &start, &resume) };
    match result {
        Ok(events) => {
            // A turn that ended in lead.error is still on record: the events say what happened.
            println!("{}", json!({ "threadId": thread, "events": events }));
            0
        }
        Err(e) => fail(&e),
    }
}

fn show(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let project = match a.project(home) {
        Ok(project) => project,
        Err(e) => return fail(&e),
    };
    let view = match lead::show(home, project.as_deref(), a.thread.as_deref()) {
        Ok(view) => view,
        Err(e) => return fail(&e),
    };
    if a.json {
        println!("{view}");
        return 0;
    }
    let Some(thread) = view.get("thread").filter(|t| !t.is_null()) else {
        println!("총괄 대화가 없습니다.");
        return 0;
    };
    println!("대화 {} — {}", thread["threadId"].as_str().unwrap_or_default(), thread["project"].as_str().unwrap_or_default());
    for event in thread["events"].as_array().into_iter().flatten() {
        let text = |key: &str| event[key].as_str().unwrap_or_default().to_string();
        match event["t"].as_str().unwrap_or_default() {
            "lead.user" => println!("\n[나] {}", text("text")),
            "lead.reply" => println!("\n[총괄 {}] {}", text("provider"), text("text")),
            "lead.plan" => println!(
                "\n[계획 {}] {} — {}",
                text("planId"),
                event["plan"]["title"].as_str().unwrap_or_default(),
                if event["ok"] == true { "정책 통과" } else { "정책 차단" }
            ),
            "lead.dispatch" => println!("[실행] {} → {}", text("planId"), text("runId")),
            "lead.warning" => println!("\n[주의] {}", text("text")),
            "lead.error" => println!("\n[오류] {}", text("text")),
            _ => {}
        }
    }
    if let Some(busy) = thread["busy"].as_str() {
        if busy == "unknown" { println!("\n(이전 총괄의 종료를 확인할 수 없어 새 총괄을 시작할 수 없습니다)"); }
        else { println!("\n({busy} 총괄이 답하는 중)"); }
    }
    0
}

fn new(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let project = match a.project(home) {
        Ok(Some(project)) => project,
        Ok(None) => return fail("--project <폴더> 또는 --overall이 필요합니다"),
        Err(e) => return fail(&e),
    };
    match lead::start(home, &project) {
        Ok(thread) => {
            println!("{}", json!({ "threadId": thread, "project": project }));
            0
        }
        Err(e) => fail(&e),
    }
}

fn close(home: &Path, args: &[String]) -> i32 {
    let [thread] = args else { return fail("o lead close <threadId>") };
    match lead::close(home, thread) {
        Ok(()) => { println!("{}", json!({ "threadId": thread })); 0 }
        Err(e) => fail(&e),
    }
}

fn writer(home: &Path, args: &[String]) -> i32 {
    let [thread] = args else { return fail("o lead writer <threadId>") };
    match lead::set_writer(home, thread) {
        Ok(()) => { println!("{}", json!({ "threadId": thread })); 0 }
        Err(e) => fail(&e),
    }
}

fn attach(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let (Some(project), Some(name), [file]) = (a.project.as_deref(), a.name.as_deref(), a.words.as_slice()) else {
        return fail("o lead attach --project <폴더> [--thread T] --name <이름> <파일|->");
    };
    let thread = match lead::check_folder(home, project).and_then(|()| lead::attachment_thread(home, project, a.thread.as_deref())) {
        Ok(thread) => thread,
        Err(e) => return fail(&e),
    };
    let saved = if file == "-" {
        stdin_bytes().and_then(|bytes| {
            if bytes.len() as u64 > lead::files::MAX_BYTES {
                return Err("첨부 파일은 하나에 10MB까지입니다".into());
            }
            lead::files::save(home, &thread, name, &bytes)
        })
    } else {
        lead::files::import(home, &thread, Path::new(file))
    };
    match saved {
        Ok(attachment) => {
            println!("{}", json!(attachment));
            0
        }
        Err(e) => fail(&e),
    }
}

fn dispatch(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let [plan_id] = a.words.as_slice() else { return fail("o lead dispatch <planId> [--profile project-o|own]") };
    let start = |run_id: &str| -> std::io::Result<()> {
        let exe = std::env::current_exe()?;
        proc::spawn_detached(Process::new(exe).args(["_run", run_id])).map(|_| ())
    };
    let mode = RunMode { yolo: yolo::resolve(home, a.yolo), ..a.mode };
    match lead::dispatch(home, plan_id, mode, &start) {
        Ok(done) => {
            println!("{}", json!(done));
            0
        }
        Err(DispatchError::Blocked(violations)) => {
            print_violations(&violations);
            2
        }
        Err(DispatchError::Other(message)) => fail(&message),
    }
}

/// The person's answer to one waiting request. Agents never run this: `main` refuses it in
/// an agent's environment, and a lead's shell is denied `o lead` altogether.
fn decide(home: &Path, args: &[String]) -> i32 {
    let [id, answer] = args else { return fail("o lead decide <승인 id> allow|deny가 필요합니다") };
    let allow = match answer.as_str() {
        "allow" => true,
        "deny" => false,
        _ => return fail("allow나 deny를 적으세요"),
    };
    match lead::decide(home, id, allow) {
        Ok(()) => { println!("{}", json!({ "id": id, "decision": answer })); 0 }
        Err(error) => fail(&error),
    }
}

fn stop(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => return fail(&e),
    };
    let project = match a.project(home) { Ok(project) => project, Err(e) => return fail(&e) };
    let stopped = match lead::stop(home, project.as_deref(), a.thread.as_deref()) {
        Ok(stopped) => stopped,
        Err(e) => return fail(&e),
    };
    println!("{}", json!({ "stopped": stopped }));
    0
}

fn stop_all(home: &Path, args: &[String]) -> i32 {
    let a = match parse(args) { Ok(a) => a, Err(e) => return fail(&e) };
    let Some(project) = a.project.as_deref() else { return fail("--project <폴더>가 필요합니다") };
    match lead::stop_all(home, project) {
        Ok(stopped) => { println!("{}", json!({ "stopped": stopped })); 0 },
        Err(error) => fail(&error),
    }
}
