//! `o billing` with the real `o` against a throwaway O home (LOCALAPPDATA/project_O), ADR-0035 개정 1.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn o(base: &Path, args: &[&str], card: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o")).args(args).env("LOCALAPPDATA", base).env("O_RUN_ID", "").env("O_CARD_ID", card).env("O_AGENT_ROLE", "").output().unwrap()
}

fn json(base: &Path, args: &[&str]) -> Value {
    let out = o(base, args, "");
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn setup(name: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("o-billing-cli-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    (base.clone(), base.join("project_O"))
}

#[test]
fn billing_defaults_to_warn_and_saves_the_chosen_mode() {
    let (base, home) = setup("modes");
    assert_eq!(json(&base, &["billing", "--json"]), serde_json::json!({ "mode": "warn" }));
    assert_eq!(json(&base, &["billing", "--json", "--subscription"])["mode"], "subscription");
    let settings: Value = serde_json::from_str(&fs::read_to_string(home.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["apiBillingMode"], "subscription");
    assert!(!o(&base, &["billing", "--warn", "--subscription"], "").status.success());
    assert!(!o(&base, &["billing", "--api"], "").status.success());
    assert_eq!(json(&base, &["billing", "--json", "--warn"])["mode"], "warn");
}

#[test]
fn a_card_reads_the_mode_but_cannot_change_it() {
    let (base, home) = setup("agent");
    let refused = o(&base, &["billing", "--warn"], "card-a");
    assert_eq!(refused.status.code(), Some(1), "{}", String::from_utf8_lossy(&refused.stdout));
    assert!(String::from_utf8_lossy(&refused.stderr).contains("에이전트 안에서는"));
    assert!(!home.join("settings.json").exists());
    let read = o(&base, &["billing", "--json"], "card-a");
    assert!(read.status.success());
}
