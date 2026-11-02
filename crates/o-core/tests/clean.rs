//! Clean agent profiles against a throwaway profile root.

use std::fs;
use std::path::{Path, PathBuf};

use o_core::agents::clean::{ancestor_instructions, clean_env_from, ensure_profile, inspect, profile_home, read_login, root_in};
use o_core::run::plan::ProviderId;
use serde_json::Value;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("o-clean-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn clean_env_keeps_system_variables_and_points_home_at_the_profile() {
    let parent = [
        ("Path", r"C:\Windows\system32"),
        ("SystemRoot", r"C:\Windows"),
        ("ProgramFiles(x86)", r"C:\Program Files (x86)"),
        ("PROCESSOR_ARCHITECTURE", "AMD64"),
        ("LC_ALL", "C"),
        ("USERPROFILE", r"C:\Users\someone"),
        ("ORCA_TERMINAL_ID", "t1"),
        ("CLAUDECODE", "1"),
        ("CODEX_THREAD_ID", "x"),
        ("BEAKHO_RELEASE", "pinned"),
        ("GRAFLY_NO_AUTO_INDEX", "1"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()));
    let home = Path::new(r"D:\profiles\claude\home");
    let env = clean_env_from(parent.clone(), home, ProviderId::Claude);
    for kept in ["Path", "SystemRoot", "ProgramFiles(x86)", "PROCESSOR_ARCHITECTURE", "LC_ALL"] {
        assert!(env.contains_key(kept), "{kept}");
    }
    for dropped in ["ORCA_TERMINAL_ID", "CLAUDECODE", "CODEX_THREAD_ID", "BEAKHO_RELEASE", "GRAFLY_NO_AUTO_INDEX"] {
        assert!(!env.contains_key(dropped), "{dropped}");
    }
    assert_eq!(env["USERPROFILE"], home.to_string_lossy());
    assert_eq!(env["HOME"], home.to_string_lossy());
    assert_eq!(env["HOMEDRIVE"], "D:");
    assert_eq!(env["HOMEPATH"], r"\profiles\claude\home");
    assert_eq!(env["CLAUDE_CONFIG_DIR"], home.join(".claude").to_string_lossy());

    let codex = clean_env_from(parent.clone(), home, ProviderId::Codex);
    assert_eq!(codex["CODEX_HOME"], home.join(".codex").to_string_lossy());
    assert!(!codex.contains_key("CLAUDE_CONFIG_DIR"));
    let agy = clean_env_from(parent, home, ProviderId::Agy);
    assert_eq!(agy["AGY_CLI_DISABLE_AUTO_UPDATE"], "true");
}

#[test]
fn profiles_live_next_to_the_state_folder_not_inside_it() {
    let base = Path::new("C:/Users/me/AppData/Local");
    let root = root_in(base);
    assert_eq!(root, base.join("project_O-agents"));
    assert!(!root.starts_with(base.join("project_O")));
    assert_eq!(profile_home(&root, ProviderId::Codex), base.join("project_O-agents").join("codex").join("home"));
}

#[test]
fn a_new_profile_holds_only_the_claude_settings() {
    let root = temp("new");
    let real_home = temp("realhome");
    for provider in ProviderId::ALL {
        ensure_profile(&root, provider, &real_home).unwrap();
    }
    let settings = profile_home(&root, ProviderId::Claude).join(".claude").join("settings.json");
    assert_eq!(files_under(&root), vec![settings.clone()]);
    assert!(profile_home(&root, ProviderId::Codex).join(".codex").is_dir());

    let value: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["claudeMdExcludes"]);
    let excludes: Vec<&str> = value["claudeMdExcludes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    let base = real_home.to_string_lossy().replace('\\', "/");
    for name in ["AGENTS.md", "CLAUDE.md", "CLAUDE.local.md"] {
        assert!(excludes.contains(&format!("{base}/{name}").as_str()), "{name}: {excludes:?}");
    }
    for provider in ProviderId::ALL {
        assert_eq!(inspect(&root, provider), Vec::<String>::new(), "{provider}");
    }
}

#[test]
fn doctor_flags_planted_hooks_skills_and_mcp_servers() {
    let root = temp("dirty");
    let real_home = temp("realhome2");
    for provider in ProviderId::ALL {
        ensure_profile(&root, provider, &real_home).unwrap();
    }
    let claude = profile_home(&root, ProviderId::Claude).join(".claude");
    write(&claude.join("settings.json"), r#"{ "claudeMdExcludes": [], "hooks": { "PreToolUse": [] } }"#);
    write(&claude.join("skills").join("x").join("SKILL.md"), "x");
    write(&claude.join(".claude.json"), r#"{ "projects": { "C:/p": { "mcpServers": { "m": { "command": "m" } } } } }"#);
    let issues = inspect(&root, ProviderId::Claude);
    assert_eq!(issues.len(), 3, "{issues:?}");

    let codex_home = profile_home(&root, ProviderId::Codex);
    let codex = codex_home.join(".codex");
    write(&codex.join("config.toml"), "model = \"m\"\n[mcp_servers.m]\ncommand = \"m\"\n");
    write(&codex.join("hooks.json"), "{}");
    write(&codex.join("skills").join(".system").join("a").join("SKILL.md"), "a");
    write(&codex.join("skills").join("mine").join("SKILL.md"), "b");
    write(&codex_home.join(".agents").join("skills").join("c").join("SKILL.md"), "c");
    let issues = inspect(&root, ProviderId::Codex);
    assert_eq!(issues.len(), 4, "{issues:?}");

    let gemini = profile_home(&root, ProviderId::Agy).join(".gemini");
    write(&gemini.join("config").join("hooks.json"), r#"{ "hooks": {} }"#);
    assert_eq!(inspect(&root, ProviderId::Agy), Vec::<String>::new());
    write(&gemini.join("config").join("mcp_config.json"), r#"{ "mcpServers": { "m": { "command": "m" } } }"#);
    write(&gemini.join("skills").join("x").join("SKILL.md"), "x");
    let issues = inspect(&root, ProviderId::Agy);
    assert_eq!(issues.len(), 2, "{issues:?}");
}

#[test]
fn ancestor_instruction_files_outside_the_real_home_are_reported() {
    let top = temp("ancestors");
    let real_home = top.join("home");
    let project = real_home.join("work").join("app");
    fs::create_dir_all(&project).unwrap();
    write(&top.join("CLAUDE.md"), "x");
    write(&real_home.join("AGENTS.md"), "x");
    write(&real_home.join("work").join(".claude").join("CLAUDE.md"), "x");
    write(&project.join("CLAUDE.md"), "the project's own");
    let found: Vec<PathBuf> = ancestor_instructions(&project, &real_home).into_iter().filter(|p| p.starts_with(&top)).collect();
    assert_eq!(found, vec![real_home.join("work").join(".claude").join("CLAUDE.md"), top.join("CLAUDE.md")]);
}

#[test]
fn login_state_keeps_only_yes_or_no() {
    let claude = read_login(ProviderId::Claude, false, r#"{"loggedIn": false, "authMethod": "none"}"#, "");
    assert_eq!(claude.logged_in, Some(false));
    assert_eq!(read_login(ProviderId::Claude, true, r#"{"loggedIn": true, "email": "a@b.c"}"#, "").detail, "로그인됨");
    assert_eq!(read_login(ProviderId::Codex, false, "", "Not logged in").logged_in, Some(false));
    assert_eq!(read_login(ProviderId::Codex, true, "Logged in using ChatGPT", "").logged_in, Some(true));
    let usage = r#"{"command":{"name":"usage","data":{"groups":[{"name":"G","buckets":[{"id":"g-weekly","remaining_fraction":0.5,"reset_time":"2026-11-08T09:17:28Z"}]}]}}}"#;
    assert_eq!(read_login(ProviderId::Agy, true, usage, "").logged_in, Some(true));
    assert_eq!(read_login(ProviderId::Agy, true, "{}", "").logged_in, None);
    assert_eq!(read_login(ProviderId::Agy, false, "", "boom").logged_in, None);
}
