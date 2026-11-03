use std::path::Path;
use o_core::run::store;
use super::{fail, Command};

pub const IMAGE: Command = Command { name: "image", usage: "  o image <read|open|folder> <project> <run-or-dash> <card-or-dash> <path>   로컬 이미지 확인", run: image };

fn image(home: &Path, args: &[String]) -> i32 {
    let [action, project, run, card, path] = args else { return fail(IMAGE.usage); };
    let run = (run != "-").then_some(run.as_str());
    let card = (card != "-").then_some(card.as_str());
    let user = store::user_home();
    let result = match action.as_str() {
        "read" => o_core::images::read(home, &user, project, run, card, path).map(|image| serde_json::to_string(&image).unwrap()),
        "open" | "folder" => o_core::images::open(home, &user, project, run, card, path, action == "folder").map(|_| "{}".into()),
        _ => return fail(IMAGE.usage),
    };
    match result { Ok(text) => { println!("{text}"); 0 }, Err(error) => fail(&error) }
}
