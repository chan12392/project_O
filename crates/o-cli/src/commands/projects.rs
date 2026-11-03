//! `o projects`: the projects root and registered folders the app lists (ADR-0009).
//! Every change prints the new list; nothing here deletes a folder.

use std::path::Path;

use o_core::projects::{self, Kind, Listing};
use o_core::run::store::user_home;
use serde_json::{json, Value};

use super::{fail, help, Command};

pub const PROJECTS: Command = Command {
    name: "projects",
    usage: "  o projects [list] [--json]   앱 왼쪽 목록의 프로젝트: 루트의 하위 폴더, test 묶음, 등록한 폴더
  o projects add <폴더> | remove <폴더> [--json]
                             루트 밖 폴더를 목록에 등록 / 목록에서 빼기 (폴더는 지우지 않음)
  o projects new <이름> [--json]   루트에 빈 프로젝트 폴더 만들기
  o projects root [<폴더> | --default] [--create] [--json]
                             프로젝트 루트 보기·바꾸기(기본 %USERPROFILE%\\.project_o\\project), --create: 없으면 만들기
  o projects check <폴더>... --json   목록에 넣을 수 있는 폴더인지 확인",
    run: projects_command,
};

fn projects_command(home: &Path, args: &[String]) -> i32 {
    let json_out = args.iter().any(|a| a == "--json");
    let rest: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--json").collect();
    let user = user_home();
    let (verb, operands) = match rest.split_first() {
        Some((verb, operands)) => (*verb, operands),
        None => ("list", &[][..]),
    };
    if let ("open", [path]) = (verb, operands) {
        return match projects::open(home, &user, path) { Ok(()) => { println!("{{}}"); 0 }, Err(e) => fail(&e) };
    }
    let result = match (verb, operands) {
        ("list", []) => projects::list(home, &user).map(|l| (l, None)),
        ("add", [path]) => projects::add(home, &user, path).map(|l| (l, None)),
        ("remove", [path]) => projects::remove(home, &user, path).map(|l| (l, None)),
        ("new", [name]) => projects::new_project(home, &user, name).map(|(path, l)| (l, Some(path))),
        ("root", rest) => root(home, &user, rest),
        ("check", paths) if !paths.is_empty() => return check(home, &user, paths),
        _ => return fail(&format!("o projects 인자가 올바르지 않습니다\n\n{}", help())),
    };
    match result {
        Ok((listing, created)) => {
            if json_out {
                let mut value = serde_json::to_value(&listing).unwrap_or(Value::Null);
                if let (Some(path), Some(map)) = (created, value.as_object_mut()) {
                    map.insert("created".into(), json!(path));
                }
                println!("{value}");
            } else {
                print_listing(&listing, created.as_deref());
            }
            0
        }
        Err(message) => fail(&message),
    }
}

fn root(home: &Path, user: &Path, args: &[&str]) -> Result<(Listing, Option<String>), String> {
    let create = args.contains(&"--create");
    let target: Vec<&str> = args.iter().copied().filter(|a| *a != "--create").collect();
    let listing = match target.as_slice() {
        [] => projects::list(home, user)?,
        ["--default"] => projects::set_root(home, user, None)?,
        [path] if !path.starts_with("--") => projects::set_root(home, user, Some(path))?,
        _ => return Err(format!("o projects root 인자가 올바르지 않습니다\n\n{}", help())),
    };
    if create && !listing.root_exists {
        return projects::create_root(home, user).map(|l| (l, None));
    }
    Ok((listing, None))
}

/// One answer per folder; a refusal is an answer here, not a failure.
fn check(home: &Path, user: &Path, paths: &[&str]) -> i32 {
    let answers: Vec<Value> = paths
        .iter()
        .map(|path| match projects::check(home, user, path) {
            Ok(()) => json!({ "path": path, "ok": true }),
            Err(reason) => json!({ "path": path, "ok": false, "reason": reason }),
        })
        .collect();
    println!("{}", Value::Array(answers));
    0
}

fn print_listing(listing: &Listing, created: Option<&str>) {
    if let Some(path) = created {
        println!("만들었습니다: {path}");
    }
    let state = if listing.root_exists { "" } else { " (아직 없음 — o projects root --create)" };
    let which = if listing.default_root { "기본" } else { "설정" };
    println!("프로젝트 루트({which}): {}{state}", listing.root);
    for (kind, title) in [(Kind::Root, "루트"), (Kind::Test, "test 묶음"), (Kind::Registered, "등록한 폴더")] {
        let rows: Vec<_> = listing.projects.iter().filter(|p| p.kind == kind).collect();
        if rows.is_empty() {
            continue;
        }
        println!("\n{title}");
        for p in rows {
            let gone = if p.missing { "  (폴더 없음)" } else { "" };
            println!("  {}  {}{gone}", p.name, p.path);
        }
    }
}
