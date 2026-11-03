use std::fs;
use chrono::Utc;
use o_core::skills::{harness, select};

#[test]
fn selects_matching_skills_and_rejects_paths_outside_the_catalog() {
    let project = std::env::temp_dir().join(format!("o-skills-{}-{}", std::process::id(), Utc::now().timestamp_nanos_opt().unwrap()));
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
