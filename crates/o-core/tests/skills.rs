use std::fs;
use chrono::Utc;
use o_core::skills::{harness, select};

fn temp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("o-{name}-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()))
}

#[test]
fn selects_matching_skills_and_rejects_paths_outside_the_catalog() {
    let project = temp("skills");
    let root = project.join(".oskills");
    for name in ["a", "b", "c", "d", "e", "f"] {
        fs::create_dir_all(root.join(name)).unwrap();
        fs::write(root.join(name).join("SKILL.md"), "skill").unwrap();
    }
    fs::write(project.join("SKILL.md"), "outside").unwrap();
    let mut items: Vec<_> = ["a", "b", "c", "d", "e", "f"].into_iter().map(|name| serde_json::json!({"name": name, "path": format!("{name}/SKILL.md"), "triggers": ["게임"]})).collect();
    items.push(serde_json::json!({"name": "escape", "path": "../SKILL.md", "triggers": ["게임"]}));
    fs::write(root.join("index.json"), serde_json::to_vec(&items).unwrap()).unwrap();
    assert!(select(&project, "날씨").is_empty());
    let chosen = select(&project, "게임 $f");
    assert_eq!(chosen.len(), 5);
    assert_eq!(chosen[0]["name"], "f");
    assert!(!chosen.iter().any(|item| item["name"] == "escape"));
    assert_eq!(select(&project, "$a $b $c $d $e $f").len(), 6);
    let blocks = project.join("harness");
    fs::create_dir_all(&blocks).unwrap();
    fs::write(blocks.join("game.md"), "게임 검증을 실행한다").unwrap();
    fs::write(blocks.join("index.json"), r#"[{"name":"game","path":"game.md","triggers":["게임"]}]"#).unwrap();
    assert!(harness(&project, "날씨").is_empty());
    assert_eq!(harness(&project, "게임")[0]["text"], "게임 검증을 실행한다");
}

#[test]
fn english_triggers_meet_word_edges_and_korean_ones_match_inside_words() {
    let project = temp("skill-words");
    let root = project.join(".oskills").join("ui");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("SKILL.md"), "skill").unwrap();
    fs::write(project.join(".oskills").join("index.json"), r#"[{"name":"ui","path":"ui/SKILL.md","triggers":["UI","화면"]}]"#).unwrap();
    assert!(select(&project, "fix the build").is_empty());
    assert!(select(&project, "quick-fix").is_empty());
    assert_eq!(select(&project, "Fix the UI.").len(), 1);
    assert_eq!(select(&project, "UI를 고친다").len(), 1);
    assert_eq!(select(&project, "설정화면을 고친다").len(), 1);
}

#[test]
fn a_harness_block_it_cannot_use_leaves_its_place_to_the_next() {
    let home = temp("harness");
    let blocks = home.join("harness");
    fs::create_dir_all(&blocks).unwrap();
    let mut items = Vec::new();
    for name in ["a", "b", "c", "d", "e", "f"] {
        fs::write(blocks.join(format!("{name}.md")), format!("block {name}")).unwrap();
        items.push(serde_json::json!({"name": name, "path": format!("{name}.md"), "triggers": ["test"]}));
    }
    // Too big by one byte, and not text: both are passed over.
    fs::write(blocks.join("a.md"), "x".repeat(4097)).unwrap();
    fs::write(blocks.join("b.md"), [0xff, 0xfe]).unwrap();
    fs::write(blocks.join("index.json"), serde_json::to_vec(&items).unwrap()).unwrap();
    let names = |task: &str| harness(&home, task).iter().map(|item| item["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
    assert_eq!(names("run the test"), ["c", "d", "e", "f"]);
    fs::write(blocks.join("a.md"), "x".repeat(4096)).unwrap();
    assert_eq!(names("run the test"), ["a", "c", "d", "e", "f"]);
    assert!(names("run the tests").is_empty());
}
