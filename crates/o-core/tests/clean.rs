//! Clean agent profiles against a throwaway profile root.

use std::fs;
use std::path::{Path, PathBuf};

use o_core::agents::clean::{ancestor_instructions, clean_env_from, ensure_profile, inspect, profile_home, read_login, root_in};
use o_core::run::plan::ProviderId;
use serde_json::{json, Value};

#[test]
fn antigravity_shell_login_models_and_usage_do_not_start_the_cli() {
    let root = Path::new("unused");
    assert!(o_core::agents::clean::command(root, ProviderId::Agy, &["-p", "test"]).unwrap_err().to_string().contains("Antigravity"));
    assert!(o_core::agents::clean::login_state(root, ProviderId::Agy).detail.contains("Antigravity"));
    assert!(o_core::providers::lead_models(ProviderId::Agy).unwrap_err().contains("Antigravity"));
    assert!(o_core::providers::get(ProviderId::Agy).usage_windows().unwrap().unwrap_err().contains("Antigravity"));
}

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
fn clean_profiles_pass_only_their_provider_auth_environment() {
    let parent = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CODEX_API_KEY", "OPENAI_API_KEY", "OPENROUTER_API_KEY", "UNRELATED_SECRET"]
        .map(|name| (name.to_string(), format!("fixture-{name}")));
    let home = Path::new("profile");
    for (provider, expected) in [
        (ProviderId::Claude, &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"][..]),
        (ProviderId::Codex, &["CODEX_API_KEY", "OPENAI_API_KEY"][..]),
        (ProviderId::OpenCode, &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "OPENAI_API_KEY", "OPENROUTER_API_KEY"][..]),
    ] {
        let env = clean_env_from(parent.clone(), home, provider);
        for (name, value) in &parent {
            assert_eq!(env.get(name), expected.contains(&name.as_str()).then_some(value), "{provider:?}: {name}");
        }
    }
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
    for provider in ProviderId::ALL.into_iter().filter(|p| *p != ProviderId::Devin) {
        ensure_profile(&root, provider, &real_home).unwrap();
    }
    let settings = profile_home(&root, ProviderId::Claude).join(".claude").join("settings.json");
    let opencode = profile_home(&root, ProviderId::OpenCode).join(".config").join("opencode").join("opencode.json");
    let mut expected = vec![settings.clone(), opencode.clone()];
    expected.sort();
    let mut found = files_under(&root);
    found.sort();
    assert_eq!(found, expected);
    assert!(profile_home(&root, ProviderId::Codex).join(".codex").is_dir());

    // OpenCode's only file is O's permission rules: deny first, edits in, the team and context
    // commands with nothing joined to them, no plugin and no MCP server (ADR-0015).
    let config: Value = serde_json::from_str(&fs::read_to_string(&opencode).unwrap()).unwrap();
    let keys: Vec<&String> = config.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["$schema", "permissions", "plugins"]);
    assert_eq!(config["plugins"], json!([]));
    let rules: Vec<(String, String, String)> = config["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["action"].as_str().unwrap().into(), r["resource"].as_str().unwrap().into(), r["effect"].as_str().unwrap().into()))
        .collect();
    let rule = |a: &str, r: &str, e: &str| (a.to_string(), r.to_string(), e.to_string());
    assert_eq!(rules[0], rule("*", "*", "deny"));
    for allowed in [rule("edit", "*", "allow"), rule("shell", "o msg *", "allow"), rule("shell", "o context *", "allow")] {
        assert!(rules.contains(&allowed), "{allowed:?}");
    }
    // OpenCode applies the last matching rule: every chain or redirect comes after the allows.
    let last_allow = rules.iter().rposition(|r| r.2 == "allow").unwrap();
    for join in ["*&*", "*|*", "*;*", "*>*"] {
        let at = rules.iter().position(|r| *r == rule("shell", join, "deny")).unwrap();
        assert!(at > last_allow, "{join}");
    }
    assert!(!rules.iter().any(|r| r.0 == "shell" && r.1 == "*" && r.2 == "allow"));

    let value: Value = serde_json::from_str(&fs::read_to_string(&settings).unwrap()).unwrap();
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["claudeMdExcludes"]);
    let excludes: Vec<&str> = value["claudeMdExcludes"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    let base = real_home.to_string_lossy().replace('\\', "/");
    for name in ["AGENTS.md", "CLAUDE.md", "CLAUDE.local.md"] {
        assert!(excludes.contains(&format!("{base}/{name}").as_str()), "{name}: {excludes:?}");
    }
    for provider in ProviderId::ALL.into_iter().filter(|p| !(cfg!(windows) && *p == ProviderId::Codex)) {
        assert_eq!(inspect(&root, provider), Vec::<String>::new(), "{provider}");
    }
    // A new Codex profile has no Windows sandbox selection, so its cards could not write.
    let codex = inspect(&root, ProviderId::Codex);
    assert_eq!(codex.len(), usize::from(cfg!(windows)), "{codex:?}");
    // The doctor's flag and the readiness the policy and the lead use are the same text (ADR-0040).
    let not_ready = o_core::agents::clean::profile_not_ready(&root, ProviderId::Codex);
    assert_eq!(not_ready.is_some(), cfg!(windows));
    assert_eq!(codex, not_ready.into_iter().collect::<Vec<_>>());
    for provider in [ProviderId::Claude, ProviderId::OpenCode, ProviderId::Devin, ProviderId::Agy] {
        assert_eq!(o_core::agents::clean::profile_not_ready(&root, provider), None, "{provider}");
    }
    let config = profile_home(&root, ProviderId::Codex).join(".codex").join("config.toml");
    for setting in ["[windows]\r\nsandbox = \"unelevated\"\r\n", "model = \"m\"\n[windows]\n# kept\nsandbox = \"elevated\"\n", "windows.sandbox = \"unelevated\"\n"] {
        write(&config, setting);
        assert_eq!(inspect(&root, ProviderId::Codex), Vec::<String>::new(), "{setting:?}");
        assert_eq!(o_core::agents::clean::profile_not_ready(&root, ProviderId::Codex), None, "{setting:?}");
    }
    write(&config, "[windows]\n[other]\nsandbox = \"x\"\n");
    assert_eq!(inspect(&root, ProviderId::Codex).len(), usize::from(cfg!(windows)));
}

#[test]
fn doctor_flags_planted_hooks_skills_and_mcp_servers() {
    let root = temp("dirty");
    let real_home = temp("realhome2");
    for provider in ProviderId::ALL.into_iter().filter(|p| *p != ProviderId::Devin) {
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
    write(&codex.join("config.toml"), "model = \"m\"\n[windows]\nsandbox = \"unelevated\"\n[mcp_servers.m]\ncommand = \"m\"\n");
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

    // OpenCode: the person's own config copied in (allow-all rule, a plugin, an MCP server),
    // a global instruction file and a skill.
    let opencode = profile_home(&root, ProviderId::OpenCode).join(".config").join("opencode");
    write(
        &opencode.join("opencode.json"),
        r#"{ "permissions": [{ "action": "*", "resource": "*", "effect": "allow" }], "plugins": ["p"], "mcp": { "servers": { "m": { "type": "local", "command": ["m"] } } } }"#,
    );
    write(&opencode.join("AGENTS.md"), "x");
    write(&opencode.join("skills").join("x").join("SKILL.md"), "x");
    let issues = inspect(&root, ProviderId::OpenCode);
    assert_eq!(issues.len(), 5, "{issues:?}");
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

    // OpenCode `auth list --format json`: only the count of connections is kept, never a name or label.
    let empty = read_login(ProviderId::OpenCode, true, "[]", "");
    assert_eq!((empty.logged_in, empty.detail.as_str()), (Some(false), "로그인 안 됨 — o agents login opencode"));
    let list = r#"[{"id":"opencode","name":"Secret Person","connections":[{"type":"credential","id":"cred_1","label":"me@example.com","method":"oauth"}]},
        {"id":"openrouter","name":"OpenRouter","connections":[{"type":"credential","id":"cred_2","label":"key","method":"key"}]}]"#;
    let signed_in = read_login(ProviderId::OpenCode, true, list, "");
    assert_eq!((signed_in.logged_in, signed_in.detail.as_str()), (Some(true), "로그인됨 (OpenCode 연결 2개)"));
    assert!(!signed_in.detail.contains("example.com") && !signed_in.detail.contains("Secret"));
    // A Claude sign-in connected to OpenCode is named: Anthropic does not allow it in third-party apps.
    let claude = read_login(ProviderId::OpenCode, true, r#"[{"id":"anthropic","connections":[{"method":"oauth"}]}]"#, "");
    assert!(claude.logged_in == Some(true) && claude.detail.contains("약관"), "{}", claude.detail);
    assert_eq!(read_login(ProviderId::OpenCode, false, "", "boom").logged_in, None);
    use o_core::providers::opencode::check_auth_list;
    assert!(check_auth_list(r#"[{"id":"anthropic","connections":[{"method":"oauth"}]}]"#).is_err());
    assert!(check_auth_list(r#"[{"id":"anthropic","connections":[{"method":"key"}]}]"#).is_ok());
    assert!(check_auth_list(r#"[{"id":"opencode","connections":[]}]"#).is_ok());
    assert!(check_auth_list("invalid").is_err());
}

#[test]
fn opencode_profile_env_moves_every_folder_into_the_profile() {
    let home = Path::new("C:/agents/opencode/home");
    let env = clean_env_from([("PATH".to_string(), "C:/bin".to_string()), ("XDG_DATA_HOME".to_string(), "C:/real".to_string())], home, ProviderId::OpenCode);
    assert_eq!(env.get("USERPROFILE").map(String::as_str), Some("C:/agents/opencode/home"));
    assert_eq!(env.get("XDG_CONFIG_HOME"), Some(&home.join(".config").to_string_lossy().into_owned()));
    // A parent's XDG folder never leaks in: OpenCode's data then follows HOME into the profile.
    assert!(!env.contains_key("XDG_DATA_HOME"));
}

#[test]
fn every_connected_cli_can_have_a_profile_and_unsupported_ones_are_named() {
    use o_core::agents::catalog::{entry, profile_support, Profile};
    for provider in ProviderId::ALL.into_iter().filter(|p| entry(p.as_str()).is_none_or(|e| matches!(e.profile, Profile::Relocatable(_)))) {
        assert!(profile_support(provider).is_ok(), "{provider}");
    }
    // Devin's config cannot move (research §2.2): the profile layer says "프로필 미지원" for it.
    assert!(matches!(entry("devin").map(|e| e.profile), Some(Profile::Unsupported(_))));
    let text = o_core::agents::clean::unsupported(ProviderId::OpenCode, "이유");
    assert!(text.starts_with("프로필 미지원(opencode):") && text.contains("이유"));
    assert!(o_core::agents::clean::login_needed(ProviderId::OpenCode).starts_with("로그인 필요(opencode):"));
}

/// Devin's config folder cannot be moved: no clean environment is made, and nothing runs it with the person's own.
#[test]
fn devin_gets_no_clean_profile_and_says_why() {
    use o_core::agents::clean::{environment, login_state};
    let root = temp("devin");
    let why = "Devin은 project_O 프로필을 지원하지 않습니다 — 내 설정으로 실행하려면 프로필 사용을 끄세요";
    assert_eq!(environment(&root, ProviderId::Devin).unwrap_err().to_string(), why);
    let state = login_state(&root, ProviderId::Devin);
    assert_eq!((state.logged_in, state.detail.as_str()), (None, why));
    assert!(!profile_home(&root, ProviderId::Devin).exists());
    assert!(environment(&root, ProviderId::Codex).is_ok());
}

#[test]
fn new_clean_codex_profile_never_trusts_a_project() {
    // #35: an untrusted project's `.codex/` layers (config, MCP servers, hooks) are skipped by
    // Codex, so a new profile must not mark the first project trusted on the person's behalf.
    use o_core::agents::clean::environment;
    let base = temp("trust");
    let root = base.join("project_O-agents");
    let home = ensure_profile(&root, ProviderId::Codex, &base).unwrap();
    assert!(environment(&root, ProviderId::Codex).is_ok());
    assert!(!home.join(".codex/config.toml").exists());
    std::fs::write(home.join(".codex/config.toml"), "# user configuration\n").unwrap();
    assert!(environment(&root, ProviderId::Codex).is_ok());
    assert_eq!(std::fs::read_to_string(home.join(".codex/config.toml")).unwrap(), "# user configuration\n");
}
