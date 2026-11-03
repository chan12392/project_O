//! Locally registered skill metadata; selection never reads or executes skill content.

use std::fs;
use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct Skill {
    name: String,
    path: String,
    #[serde(default)]
    triggers: Vec<String>,
}

/// Explicit `$name` mentions always win; otherwise at most five trigger matches are selected.
pub fn select(project: &Path, task: &str) -> Vec<Value> {
    select_under(project, ".oskills", task, true)
}

/// Optional, locally configured O harness blocks; the provider's own instructions stay in force.
pub fn harness(home: &Path, task: &str) -> Vec<Value> {
    select_under(home, "harness", task, false).into_iter().take(5).filter_map(|item| {
        let path = item["path"].as_str()?;
        if !fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= 4096) { return None; }
        let body = fs::read_to_string(path).ok()?;
        Some(json!({"name": item["name"], "text": body}))
    }).collect()
}

fn select_under(base: &Path, directory: &str, task: &str, is_skill: bool) -> Vec<Value> {
    let root = base.join(directory);
    let Ok(root) = root.canonicalize() else { return Vec::new() };
    let Ok(base) = base.canonicalize() else { return Vec::new() };
    if !root.starts_with(base) { return Vec::new(); }
    let Ok(index) = root.join("index.json").canonicalize() else { return Vec::new() };
    if !index.starts_with(&root) || !fs::metadata(&index).is_ok_and(|m| m.is_file() && m.len() <= 65_536) { return Vec::new(); }
    let Ok(data) = fs::read_to_string(index) else { return Vec::new() };
    let Ok(skills) = serde_json::from_str::<Vec<Skill>>(&data) else { return Vec::new() };
    let text = task.to_lowercase();
    let mut matches = Vec::new();
    let mut names = HashSet::new();
    for skill in skills.into_iter().take(200) {
        if skill.name.is_empty() || skill.name.len() > 64 || !skill.name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') { continue; }
        if !names.insert(skill.name.clone()) { continue; }
        let Ok(path) = root.join(&skill.path).canonicalize() else { continue };
        if !path.starts_with(&root) || !path.is_file() { continue; }
        if is_skill && path.file_name().is_none_or(|n| n != "SKILL.md") { continue; }
        if !is_skill && path.extension().is_none_or(|ext| ext != "md") { continue; }
        let explicit = text.split(|c: char| !c.is_alphanumeric() && c != '$' && c != '-').any(|word| word == format!("${}", skill.name));
        let score = skill.triggers.iter().take(32).filter(|term| term.len() >= 2 && term.len() <= 80 && text.contains(&term.to_lowercase())).count();
        if explicit || score > 0 { matches.push((explicit, score, skill.name, path)); }
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)).then_with(|| a.2.cmp(&b.2)));
    matches.into_iter().enumerate().filter(|(i, item)| *i < 5 || item.0)
        .map(|(_, (_, _, name, path))| json!({"name": name, "path": path.to_string_lossy()})).collect()
}
