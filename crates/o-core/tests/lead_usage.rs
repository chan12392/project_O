mod support;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use o_core::lead::{self, LeadEvent, SendRequest};
use o_core::run::plan::ProviderId;

#[test]
fn interrupted_codex_turn_keeps_observed_usage_as_incomplete() {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    let base = std::env::temp_dir().join(format!("o-lead-usage-{}-{stamp}", std::process::id()));
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let source = base.join("fake.rs");
    let binary = base.join(if cfg!(windows) { "fake.exe" } else { "fake" });
    std::fs::write(&source, r####"
use std::io::{self, BufRead, Write};
fn main() {
    if std::env::args().any(|arg| arg == "--help") { println!("Usage: codex app-server"); return; }
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        if line.contains("\"id\":1") { println!("{}", r#"{"id":1,"result":{}}"#); }
        if line.contains("\"id\":2") { println!("{}", r#"{"id":2,"result":{"thread":{"id":"s"}}}"#); }
        if line.contains("\"id\":3") {
            println!("{}", r#"{"id":3,"result":{"turn":{"id":"t"}}}"#);
            println!("{}", r#"{"method":"thread/tokenUsage/updated","params":{"threadId":"s","turnId":"t","tokenUsage":{"last":{"inputTokens":10,"cachedInputTokens":4,"outputTokens":3},"total":{"inputTokens":110,"cachedInputTokens":54,"outputTokens":13}}}}"#);
            io::stdout().flush().unwrap();
            return;
        }
        io::stdout().flush().unwrap();
    }
}
"####).unwrap();
    let mut compiler = Command::new("rustc");
    compiler.arg(&source).arg("-o").arg(&binary);
    assert!(o_core::sys::proc::hide_window(&mut compiler).status().unwrap().success());
    let spawner = move |spec: &o_core::providers::LaunchSpec, _: &str| -> std::io::Result<o_core::sys::launch::Launch> {
        let mut command = support::fake_worker_launch()?;
        command.executable = binary.clone();
        if spec.args.iter().any(|arg| arg == "--help") { command.arg("--help"); }
        Ok(command)
    };
    let project = project.to_string_lossy();
    let request = SendRequest { project: &project, thread: None, provider: ProviderId::Codex, model: None, effort: None, text: "test", attach: &[], fresh: false, clean: None, yolo: false };
    let events = lead::send(&base.join("state"), &request, &spawner).unwrap();
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Usage { input, cached, output, complete: Some(false), .. }
        if *input == 6.0 && *cached == 4.0 && *output == 3.0)));
    assert!(events.iter().any(|event| matches!(event, LeadEvent::Error { .. })));
}
