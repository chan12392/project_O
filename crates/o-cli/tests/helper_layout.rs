#![cfg(windows)]

use std::os::windows::process::CommandExt;
use std::process::Command;

#[test]
fn fake_provider() {
    if std::env::var_os("O_HELPER_LAYOUT_PROVIDER").is_some() {
        std::fs::write(std::env::var_os("O_HELPER_LAYOUT_MARKER").unwrap(), "started").unwrap();
        println!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"layout-ok"}}"#);
    }
}

#[test]
fn desktop_host() {
    let Some(base) = std::env::var_os("O_HELPER_LAYOUT_HOST") else { return };
    let base = std::path::PathBuf::from(base);
    let project = base.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let project = project.to_string_lossy();
    let request = o_core::lead::SendRequest {
        project: &project, provider: o_core::run::plan::ProviderId::Claude,
        model: None, effort: Some("low"), text: "test", attach: &[], fresh: false, clean: None, yolo: false,
    };
    let marker = base.join("provider-started");
    let spawn = move |_: &o_core::providers::LaunchSpec, cwd: &str| {
        let mut launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
        launch.current_dir(cwd).args(["--exact", "fake_provider", "--nocapture"])
            .env("O_HELPER_LAYOUT_PROVIDER", "1").env("O_HELPER_LAYOUT_MARKER", &marker);
        Ok(launch)
    };
    let events = o_core::lead::send(&base.join("state"), &request, &spawn).unwrap();
    assert!(events.iter().any(|event| matches!(event, o_core::lead::LeadEvent::Reply { text, .. } if text == "layout-ok")), "{events:?}");
}

#[test]
fn desktop_discovers_sibling_o_exe() {
    let base = std::env::temp_dir().join(format!("o-helper-layout-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_o"), base.join("o.exe")).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), base.join("project_O.exe")).unwrap();
    let result = Command::new(base.join("project_O.exe"))
        .args(["--exact", "desktop_host", "--nocapture"])
        .env("O_HELPER_LAYOUT_HOST", &base)
        .creation_flags(0x08000000).output().unwrap();
    assert!(result.status.success(), "{}\n{}", String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr));
    assert!(base.join("provider-started").exists());
}

#[test]
fn desktop_missing_sibling_does_not_start_provider() {
    let base = std::env::temp_dir().join(format!("o-helper-absent-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::create_dir_all(&base).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), base.join("project_O.exe")).unwrap();
    let result = Command::new(base.join("project_O.exe"))
        .args(["--exact", "desktop_host", "--nocapture"])
        .env("O_HELPER_LAYOUT_HOST", &base)
        .creation_flags(0x08000000).output().unwrap();
    assert!(!result.status.success());
    assert!(!base.join("provider-started").exists());
}
