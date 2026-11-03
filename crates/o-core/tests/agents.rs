//! Agent CLI status, install and sign-in on a PATH with no CLI, npm or node:
//! what the desktop app shows and does when nothing is installed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

use o_core::agents::{install, is_https_url, open_login, status};
use o_core::run::plan::ProviderId;
use serde_json::{json, Value};

/// This test binary's own PATH: one empty folder.
fn empty_path() -> PathBuf {
    static SET: Once = Once::new();
    let dir = std::env::temp_dir().join(format!("o-agents-empty-path-{}", std::process::id()));
    SET.call_once(|| {
        fs::create_dir_all(&dir).unwrap();
        std::env::set_var("PATH", &dir);
    });
    dir
}

#[test]
fn missing_clis_show_the_official_installers() {
    let root = empty_path().join("profiles");
    let list = status(&root, false);
    let value = json!(list);
    let ids: Vec<&str> = value.as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["claude", "codex", "agy", "opencode", "devin"]);
    for agent in value.as_array().unwrap() {
        assert_eq!(agent["installed"], false);
        assert_eq!(agent["loggedIn"], Value::Null, "unknown, not false: {agent}");
        assert!(agent.get("version").is_none() && agent.get("path").is_none() && agent.get("profile").is_none());
        // Without npm nothing installs in-app; the hint is the vendor's own installer.
        assert_eq!(agent["canInstall"], false);
        let hint = agent["installHint"].as_str().unwrap();
        if agent["id"] == "opencode" {
            // OpenCode v2 has no Windows installer script: its page's zip, or npm.
            assert!(hint.contains("https://opencode.ai/v2/docs/") && hint.contains("npm install -g @opencode/cli"), "{agent}");
        } else {
            assert!(hint.starts_with("PowerShell: irm https://"), "{agent}");
        }
        assert!(agent["installUrl"].as_str().unwrap().starts_with("https://"));
    }
    assert_eq!(value[0]["name"], "Claude Code");
    assert_eq!(value[0]["installUrl"], "https://code.claude.com/docs/en/setup");
    assert_eq!(value[1]["installUrl"], "https://github.com/openai/codex");
    // The clean variant reads the same and makes no profile for a CLI that is not there.
    let clean = json!(status(&root, true));
    assert_eq!(&clean.as_array().unwrap()[..4], &value.as_array().unwrap()[..4]);
    assert!(clean[4]["profileUnsupported"].as_str().unwrap().contains("프로필"));
    assert!(!root.exists());
}

#[test]
fn install_without_npm_points_to_the_official_page() {
    empty_path();
    for provider in ProviderId::ALL {
        let result = install(provider);
        assert!(!result.ok);
        assert!(result.message.contains("https://"), "{}", result.message);
        assert!(result.log.is_none());
    }
}

#[test]
fn sign_in_needs_the_cli() {
    let root = empty_path().join("profiles");
    for provider in [ProviderId::Claude, ProviderId::Codex] {
        let result = open_login(&root, provider, true);
        assert!(!result.ok);
        assert!(result.message.contains("설치되어 있지 않습니다"), "{}", result.message);
    }
}

#[test]
fn console_command_lines_quote_like_the_c_runtime() {
    let line = o_core::sys::proc::command_line(Path::new(r"C:\Program Files\nodejs\node.exe"), &[
        r"C:\npm\codex.js".into(),
        "login".into(),
        "two words".into(),
        String::new(),
        r#"say "hi""#.into(),
        r"C:\dir with space\".into(),
    ]);
    assert_eq!(line, r#""C:\Program Files\nodejs\node.exe" C:\npm\codex.js login "two words" "" "say \"hi\"" "C:\dir with space\\""#);
}

/// Opens (and at once closes) a real console window, so it runs only when asked:
/// `cargo test -p o-core --test agents -- --ignored`.
#[cfg(windows)]
#[test]
#[ignore]
fn console_window_starts_with_its_own_environment() {
    let dir = empty_path();
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let env = std::collections::BTreeMap::from([("SystemRoot".to_string(), system.clone())]);
    let program = PathBuf::from(&system).join("System32").join("hostname.exe");
    o_core::sys::proc::spawn_console(&program, &[], Some(&env), Some(&dir)).unwrap();
    assert!(o_core::sys::proc::spawn_console(&dir.join("missing.exe"), &[], None, None).is_err());
}

#[test]
fn only_https_pages_open() {
    assert!(is_https_url("https://code.claude.com/docs/en/setup"));
    assert!(is_https_url("https://github.com/openai/codex?tab=readme#install"));
    for url in ["http://example.com", "file:///C:/Windows", "javascript:alert(1)", "https://", "https:///path", "https://user@host/", "https://a b", "HTTPS://example.com", "ms-settings:"] {
        assert!(!is_https_url(url), "{url}");
    }
}
