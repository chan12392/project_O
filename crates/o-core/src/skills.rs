//! Locally registered skill metadata; selection never reads or executes skill content.

use std::fs;
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Value};

/// Most entries picked by trigger; explicit `$name` skills come on top of these.
const PICKS: usize = 5;
/// Largest harness block that may enter a prompt.
const BLOCK_BYTES: u64 = 4096;

#[derive(Deserialize)]
struct Skill {
    name: String,
    path: String,
    #[serde(default)]
    triggers: Vec<String>,
}

/// An index entry the task matched: named explicitly, its trigger hits, its name and file.
type Match = (bool, usize, String, PathBuf);

/// Explicit `$name` mentions always win; otherwise at most five trigger matches are selected.
pub fn select(project: &Path, task: &str) -> Vec<Value> {
    select_under(project, ".oskills", task, true).into_iter().enumerate().filter(|(i, item)| *i < PICKS || item.0)
        .map(|(_, (_, _, name, path))| json!({"name": name, "path": path.to_string_lossy()})).collect()
}

/// Optional, locally configured O harness blocks; the provider's own instructions stay in force.
/// Blocks that cannot be used (too big, not text) leave their place to the next match.
pub fn harness(home: &Path, task: &str) -> Vec<Value> {
    select_under(home, "harness", task, false).into_iter()
        .filter_map(|(_, _, name, path)| Some(json!({"name": name, "text": read_block(&path)?})))
        .take(PICKS).collect()
}

/// The block's text, read once with the cap so a file that grows after the check cannot slip past it.
fn read_block(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    fs::File::open(path).ok()?.take(BLOCK_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > BLOCK_BYTES { return None; }
    String::from_utf8(bytes).ok()
}

/// Whether `text` (lowercased) holds the trigger. A trigger's English letters and digits must
/// meet a word edge, so "ui" does not match "build"; Korean triggers match inside words,
/// so "게임" still matches "게임을".
fn mentions(text: &str, term: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let (first, last) = (term.chars().next(), term.chars().next_back());
    text.match_indices(term).any(|(at, _)| {
        let (before, after) = (text[..at].chars().next_back(), text[at + term.len()..].chars().next());
        !(word(first) && word(before)) && !(word(last) && word(after))
    })
}

fn select_under(base: &Path, directory: &str, task: &str, is_skill: bool) -> Vec<Match> {
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
        let score = skill.triggers.iter().take(32).filter(|term| term.len() >= 2 && term.len() <= 80 && mentions(&text, &term.to_lowercase())).count();
        if explicit || score > 0 { matches.push((explicit, score, skill.name, path)); }
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)).then_with(|| a.2.cmp(&b.2)));
    matches
}
