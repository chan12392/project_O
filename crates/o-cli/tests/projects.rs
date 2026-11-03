//! `o projects` with the real `o` against a throwaway home: LOCALAPPDATA holds O's
//! state folder, USERPROFILE (HOME elsewhere) the projects root.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

struct Fixture {
    base: PathBuf,
    user: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let base = std::env::temp_dir().join(format!("o-cli-projects-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let user = base.join("user");
    fs::create_dir_all(&user).unwrap();
    Fixture { base, user }
}

fn o(f: &Fixture, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_o"))
        .args(args)
        .env("LOCALAPPDATA", f.base.join("local"))
        .env("USERPROFILE", &f.user)
        .env("HOME", &f.user)
        .env("O_RUN_ID", "")
        .env("O_CARD_ID", "")
        .output()
        .unwrap()
}

fn json(f: &Fixture, args: &[&str]) -> Value {
    let out = o(f, args);
    assert_eq!(out.status.code(), Some(0), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap()
}

fn slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn kinds(listing: &Value) -> Vec<(String, String)> {
    listing["projects"].as_array().unwrap().iter().map(|p| (p["kind"].as_str().unwrap().to_string(), p["name"].as_str().unwrap().to_string())).collect()
}

#[test]
fn projects_lists_the_root_creates_it_and_makes_a_new_project() {
    let f = fixture("root");
    let listing = json(&f, &["projects", "--json"]);
    let root = slashes(&f.user.join(".project_o").join("project"));
    assert_eq!(listing["root"], root.as_str());
    assert_eq!(listing["rootExists"], false);
    assert_eq!(listing["defaultRoot"], true);

    // A new project needs the root first.
    assert_eq!(o(&f, &["projects", "new", "blog"]).status.code(), Some(1));
    assert_eq!(json(&f, &["projects", "root", "--create", "--json"])["rootExists"], true);
    let made = json(&f, &["projects", "new", "blog", "--json"]);
    assert_eq!(made["created"], format!("{root}/blog").as_str());
    fs::create_dir_all(f.user.join(".project_o").join("project").join("test").join("probe")).unwrap();
    assert_eq!(kinds(&json(&f, &["projects", "list", "--json"])), [("root".to_string(), "blog".to_string()), ("test".to_string(), "probe".to_string())]);

    // Plain text names the root and each group.
    let text = String::from_utf8(o(&f, &["projects"]).stdout).unwrap();
    assert!(text.contains("프로젝트 루트(기본)") && text.contains("test 묶음") && text.contains("probe"), "{text}");
}

#[test]
fn projects_add_remove_and_check_follow_the_project_rules() {
    let f = fixture("add");
    let work = f.base.join("work").join("app");
    fs::create_dir_all(&work).unwrap();
    let path = work.to_string_lossy().into_owned();

    let added = json(&f, &["projects", "add", &path, "--json"]);
    assert_eq!(kinds(&added), [("registered".to_string(), "app".to_string())]);
    assert_eq!(added["projects"][0]["path"], slashes(&work).as_str());
    assert_eq!(o(&f, &["projects", "add", &path]).status.code(), Some(1), "listed twice");

    // The home folder itself is refused with the reason on stderr.
    let home = o(&f, &["projects", "add", &f.user.to_string_lossy()]);
    assert_eq!(home.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&home.stderr).contains("홈 폴더"));

    let answers = json(&f, &["projects", "check", &path, &f.user.to_string_lossy(), "--json"]);
    assert_eq!(answers[0]["ok"], true);
    assert_eq!(answers[1]["ok"], false);
    assert!(answers[1]["reason"].as_str().unwrap().contains("홈 폴더"));

    let removed = json(&f, &["projects", "remove", &path, "--json"]);
    assert!(removed["projects"].as_array().unwrap().is_empty());
    assert!(work.is_dir(), "taking a folder off the list keeps it");

    assert_eq!(o(&f, &["projects", "frobnicate"]).status.code(), Some(1));
    assert!(String::from_utf8(o(&f, &["help"]).stdout).unwrap().contains("o projects add <폴더>"));
}

#[test]
fn projects_root_can_move_and_come_back() {
    let f = fixture("move");
    let other = f.base.join("elsewhere");
    let moved = json(&f, &["projects", "root", &other.to_string_lossy(), "--json"]);
    assert_eq!(moved["root"], slashes(&other).as_str());
    assert_eq!(moved["defaultRoot"], false);
    assert_eq!(o(&f, &["projects", "root", &f.user.to_string_lossy()]).status.code(), Some(1));
    assert_eq!(json(&f, &["projects", "root", "--default", "--json"])["defaultRoot"], true);
}
