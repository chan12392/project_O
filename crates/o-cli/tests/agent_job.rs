//! ADR-0043: an agent that clears the runner's variables is still inside the job the runner
//! started it in. One test only: it puts this test process itself into such a job for good.

#[cfg(windows)]
#[test]
fn a_process_in_a_cards_job_is_an_agent_whatever_its_environment_says() {
    use std::path::Path;
    use std::process::{Command, Output};
    use o_core::run::events::OEvent;
    use o_core::run::plan::{Card, Plan, ProviderId};
    use o_core::run::store::{create_run, read_events};
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-agent-job-{}-{nonce}", std::process::id()));
    let home = base.join("project_O");
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let card = |id: &str| Card { id: id.into(), provider: ProviderId::Codex, role: None, task: "x".into(), model: None, effort: None, depends_on: vec![], resources: vec![] };
    create_run(&home, "r1", &Plan { title: "t".into(), project: project.to_string_lossy().into_owned(), cards: vec![card("a"), card("b")] }).unwrap();
    let o = |args: &[&str], env: &[(&str, &str)]| -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_o"));
        command.args(args).env("LOCALAPPDATA", &base).env_remove("O_AGENT_ROLE").env_remove("O_CARD_ID").env_remove("O_RUN_ID").env_remove("O_MSG_DIR");
        for (k, v) in env { command.env(k, v); }
        command.output().unwrap()
    };
    let senders = |home: &Path| -> Vec<String> {
        read_events(home, "r1").into_iter().filter_map(|e| match e { OEvent::Msg { from, .. } => Some(from), _ => None }).collect()
    };
    let refused = |output: &Output| output.status.code() == Some(2) && String::from_utf8_lossy(&output.stderr).contains("에이전트 안에서는");

    // Outside a job, with no variables: a person.
    assert!(!refused(&o(&["dispatch"], &[])));
    assert!(o(&["msg", "send", "--run", "r1", "from the person"], &[]).status.success());
    assert_eq!(senders(&home), ["lead"]);

    // The job the runner makes for card a of run r1 (`win_launch::card_job_name`).
    let name: Vec<u16> = format!(r"Global\project_O-job-r1-a-{:032x}", nonce ^ u128::from(std::process::id())).encode_utf16().chain(Some(0)).collect();
    let job = unsafe { CreateJobObjectW(std::ptr::null(), name.as_ptr()) };
    assert!(!job.is_null());
    assert_ne!(unsafe { AssignProcessToJobObject(job, GetCurrentProcess()) }, 0, "{}", std::io::Error::last_os_error());

    // The variables are gone, the job is not: no work is started and nothing a person decides changes.
    assert!(refused(&o(&["dispatch"], &[])));
    assert!(refused(&o(&["lead", "decide", "20261104-000000-abcd-a1", "allow"], &[])));
    assert_ne!(o(&["yolo", "--on"], &[]).status.code(), Some(0));
    // It speaks only as the card of its job: not as the lead, not as another card.
    let as_lead = o(&["msg", "send", "--run", "r1", "--as", "lead", "forged"], &[]);
    assert_eq!(as_lead.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&as_lead.stderr).contains("표식"), "{}", String::from_utf8_lossy(&as_lead.stderr));
    assert_eq!(o(&["msg", "send", "forged"], &[("O_RUN_ID", "r1"), ("O_CARD_ID", "b")]).status.code(), Some(1));
    assert_eq!(o(&["msg", "send", "forged"], &[("O_AGENT_ROLE", "lead"), ("O_RUN_ID", "r1")]).status.code(), Some(1));
    assert_eq!(senders(&home), ["lead"]);
    let own = o(&["msg", "send", "from the card"], &[("O_RUN_ID", "r1"), ("O_CARD_ID", "a")]);
    assert!(own.status.success(), "{}", String::from_utf8_lossy(&own.stderr));
    assert_eq!(senders(&home), ["lead", "a"]);
}
