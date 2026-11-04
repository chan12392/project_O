//! A lead turn's steps as the chat shows them (ADR-0008 개정 3): what the CLI read, ran,
//! searched or changed, read from its stream. Pure: it only folds stream lines.
//!
//! Claude Code: an `assistant` line's `tool_use` starts a step, the `user` line with its
//! `tool_result` ends it; a file edit's `+`/`−` counts come from `tool_use_result.structuredPatch`
//! (or the created file's text). Codex app-server, as `codex_app_server::stream_line` maps
//! it: `item.started` starts a step, `item.completed` ends it, `o.patch` updates a file change's diff.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sys::js::{js_head, js_len};

/// Steps kept for one turn; later ones are counted but neither shown nor stored.
pub const MAX_STEPS: usize = 200;
/// A step's label (a path, a command, a pattern) is cut at this many characters.
const LABEL_CHARS: usize = 300;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// The CLI's own id for the tool call (`#<n>` for each file of a Codex change).
    pub id: String,
    /// `read` · `search` · `run` · `edit` · `web` · `other`.
    pub kind: String,
    /// What it worked on: a path, a command, a pattern, a query or a URL.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<u64>,
    /// `running` · `done` · `failed` · `denied` · `incomplete`.
    pub status: String,
}

#[derive(Default)]
pub struct Tracker {
    steps: Vec<Step>,
    /// Every step seen, also past `MAX_STEPS`.
    pub total: usize,
}

fn clip(text: &str) -> String {
    let line = text.trim();
    if js_len(line) > LABEL_CHARS { format!("{}…", js_head(line, LABEL_CHARS)) } else { line.to_string() }
}

fn text_lines(text: &str) -> u64 {
    text.lines().count() as u64
}

/// `+`/`−` lines of a unified diff, headers left out.
pub fn diff_counts(diff: &str) -> (u64, u64) {
    let (mut added, mut removed) = (0, 0);
    for line in diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") { continue; }
        if line.starts_with('+') { added += 1; } else if line.starts_with('-') { removed += 1; }
    }
    (added, removed)
}

/// A Codex change's counts: a unified diff for an update, the whole text for an add or delete.
pub fn change_counts(change: &Value) -> (u64, u64) {
    let diff = change["diff"].as_str().unwrap_or("");
    if diff.lines().any(|line| line.starts_with("@@")) { return diff_counts(diff); }
    match change.pointer("/kind/type").and_then(Value::as_str) {
        Some("add") => (text_lines(diff), 0),
        Some("delete") => (0, text_lines(diff)),
        _ => diff_counts(diff),
    }
}

/// Windows Codex runs every command as `pwsh -Command "<script>"`; the script is what was asked.
fn shell_body(command: &str) -> &str {
    match command.find(" -Command ") {
        Some(at) => command[at + " -Command ".len()..].trim().trim_matches('"'),
        None => command,
    }
}

impl Tracker {
    /// Every step this turn, oldest first (at most `MAX_STEPS`).
    pub fn steps(&self) -> &[Step] {
        &self.steps
    }

    fn start(&mut self, step: Step) {
        self.total += 1;
        if self.steps.len() < MAX_STEPS && !self.steps.iter().any(|s| s.id == step.id) { self.steps.push(step); }
    }

    /// Ends a running step; returns it as finished, or `None` when it is not a running one here.
    fn finish(&mut self, id: &str, status: &str, counts: Option<(u64, u64)>) -> Option<Step> {
        let step = self.steps.iter_mut().find(|s| s.id == id && s.status == "running")?;
        step.status = status.to_string();
        if let Some((added, removed)) = counts { (step.added, step.removed) = (Some(added), Some(removed)); }
        Some(step.clone())
    }

    /// Steps still running when the turn ended, now `incomplete` (stopped, timed out or cut off).
    pub fn unfinished(&mut self) -> Vec<Step> {
        self.steps.iter_mut().filter(|s| s.status == "running").map(|s| { s.status = "incomplete".into(); s.clone() }).collect()
    }

    /// Reads one stream line; returns the steps it finished.
    pub fn read(&mut self, value: &Value) -> Vec<Step> {
        match value["type"].as_str() {
            Some("assistant") => { self.claude_calls(value); Vec::new() }
            Some("user") => self.claude_results(value),
            Some("item.started") => { self.codex_started(&value["item"]); Vec::new() }
            Some("item.completed") => self.codex_completed(&value["item"]),
            Some("o.patch") => { self.codex_patch(value); Vec::new() }
            _ => Vec::new(),
        }
    }

    fn claude_calls(&mut self, value: &Value) {
        for part in value.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
            if part["type"] != "tool_use" { continue; }
            let (Some(id), Some(name)) = (part["id"].as_str(), part["name"].as_str()) else { continue };
            let input = &part["input"];
            let text = |key: &str| input[key].as_str().unwrap_or("").to_string();
            let (kind, label, file) = match name {
                "Read" => ("read", text("file_path"), Some(text("file_path"))),
                "Grep" | "Glob" => ("search", text("pattern"), None),
                "Edit" | "MultiEdit" | "Write" => ("edit", text("file_path"), Some(text("file_path"))),
                "NotebookEdit" => ("edit", text("notebook_path"), Some(text("notebook_path"))),
                "Bash" | "PowerShell" => ("run", text("command"), None),
                "WebSearch" => ("web", text("query"), None),
                "WebFetch" => ("web", text("url"), None),
                other => ("other", other.to_string(), None),
            };
            self.start(Step { id: id.to_string(), kind: kind.into(), label: clip(&label), file: file.filter(|f| !f.is_empty()), added: None, removed: None, status: "running".into() });
        }
    }

    fn claude_results(&mut self, value: &Value) -> Vec<Step> {
        let results: Vec<&Value> = value.pointer("/message/content").and_then(Value::as_array).into_iter().flatten()
            .filter(|part| part["type"] == "tool_result").collect();
        let mut done = Vec::new();
        for part in &results {
            let Some(id) = part["tool_use_id"].as_str() else { continue };
            let rejected = value["tool_result_meta"].as_array().into_iter().flatten()
                .any(|meta| meta["id"] == id && meta.pointer("/permission_decision/decision").and_then(Value::as_str) == Some("reject"));
            let status = if rejected { "denied" } else if part["is_error"] == true { "failed" } else { "done" };
            // `tool_use_result` belongs to the line's one result; a line with several says nothing per call.
            let counts = (status == "done" && results.len() == 1).then(|| claude_counts(&value["tool_use_result"], self.steps.iter().find(|s| s.id == id))).flatten();
            done.extend(self.finish(id, status, counts));
        }
        done
    }

    fn codex_started(&mut self, item: &Value) {
        let Some(id) = item["id"].as_str() else { return };
        match item["type"].as_str() {
            Some("command_execution") => {
                let command = item["command"].as_str().unwrap_or("");
                let actions: Vec<&Value> = item["commandActions"].as_array().into_iter().flatten().collect();
                let all = |kinds: &[&str]| !actions.is_empty() && actions.iter().all(|a| a["type"].as_str().is_some_and(|t| kinds.contains(&t)));
                let (kind, label, file) = if all(&["read"]) {
                    let path = actions[0]["path"].as_str().or(actions[0]["name"].as_str()).unwrap_or("").to_string();
                    ("read", path.clone(), Some(path))
                } else if all(&["search", "listFiles"]) {
                    let what = actions[0]["query"].as_str().or(actions[0]["path"].as_str()).unwrap_or(shell_body(command)).to_string();
                    ("search", what, None)
                } else {
                    ("run", shell_body(command).to_string(), None)
                };
                self.start(Step { id: id.into(), kind: kind.into(), label: clip(&label), file: file.filter(|f| !f.is_empty()), added: None, removed: None, status: "running".into() });
            }
            Some("file_change") => self.codex_changes(id, &item["changes"]),
            Some("web_search") => {
                let query = item["query"].as_str().unwrap_or("");
                self.start(Step { id: id.into(), kind: "web".into(), label: clip(query), file: None, added: None, removed: None, status: "running".into() });
            }
            _ => {}
        }
    }

    fn codex_patch(&mut self, value: &Value) {
        if let Some(id) = value["itemId"].as_str() { self.codex_changes(id, &value["changes"]); }
    }

    /// A Codex change's files as running steps, new ones started and known ones recounted.
    fn codex_changes(&mut self, id: &str, changes: &Value) {
        for (i, change) in changes.as_array().into_iter().flatten().enumerate() {
            let (added, removed) = change_counts(change);
            let step_id = format!("{id}#{i}");
            match self.steps.iter_mut().find(|s| s.id == step_id) {
                Some(step) if step.status == "running" => (step.added, step.removed) = (Some(added), Some(removed)),
                Some(_) => {}
                None => {
                    let path = change["path"].as_str().unwrap_or("").to_string();
                    self.start(Step { id: step_id, kind: "edit".into(), label: clip(&path), file: Some(path).filter(|p| !p.is_empty()), added: Some(added), removed: Some(removed), status: "running".into() });
                }
            }
        }
    }

    fn codex_completed(&mut self, item: &Value) -> Vec<Step> {
        let Some(id) = item["id"].as_str() else { return Vec::new() };
        let status = match item["status"].as_str() {
            Some("declined") => "denied",
            Some("failed") => "failed",
            _ if item["exitCode"].as_i64().is_some_and(|code| code != 0) => "failed",
            _ => "done",
        };
        match item["type"].as_str() {
            Some("file_change") => {
                // A completed item carries the final diff; a change seen only now still becomes steps.
                self.codex_changes(id, &item["changes"]);
                let ids: Vec<String> = self.steps.iter().filter(|s| s.id.starts_with(&format!("{id}#"))).map(|s| s.id.clone()).collect();
                ids.iter().filter_map(|step| self.finish(step, status, None)).collect()
            }
            Some("command_execution" | "web_search") => self.finish(id, status, None).into_iter().collect(),
            _ => Vec::new(),
        }
    }
}

/// A finished Claude edit's counts: the patch it applied, else the file it created, else
/// the strings it swapped.
fn claude_counts(result: &Value, step: Option<&Step>) -> Option<(u64, u64)> {
    if step.is_none_or(|s| s.kind != "edit") { return None; }
    if let Some(hunks) = result["structuredPatch"].as_array().filter(|h| !h.is_empty()) {
        let (mut added, mut removed) = (0, 0);
        for line in hunks.iter().flat_map(|h| h["lines"].as_array().into_iter().flatten()).filter_map(Value::as_str) {
            if line.starts_with('+') { added += 1; } else if line.starts_with('-') { removed += 1; }
        }
        return Some((added, removed));
    }
    if result["type"] == "create" { return result["content"].as_str().map(|text| (text_lines(text), 0)); }
    match (result["oldString"].as_str(), result["newString"].as_str()) {
        (Some(old), Some(new)) => Some((text_lines(new), text_lines(old))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_tool_calls_become_steps_with_patch_counts() {
        let mut t = Tracker::default();
        t.read(&json!({"type":"assistant","message":{"content":[
            {"type":"tool_use","id":"r1","name":"Read","input":{"file_path":"C:/p/src/a.rs"}},
            {"type":"tool_use","id":"e1","name":"Edit","input":{"file_path":"C:/p/src/a.rs","old_string":"a","new_string":"b"}}]}}));
        assert_eq!(t.steps().len(), 2);
        assert!(t.steps().iter().all(|s| s.status == "running"));
        let done = t.read(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"r1","content":"..."}]}}));
        assert_eq!(done[0].kind, "read");
        assert_eq!(done[0].status, "done");
        let done = t.read(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"e1","content":"ok"}]},
            "tool_use_result":{"type":"update","filePath":"C:/p/src/a.rs","structuredPatch":[{"lines":[" x","-a","-c","+b"]}]}}));
        assert_eq!((done[0].added, done[0].removed), (Some(1), Some(2)));
    }

    #[test]
    fn claude_created_files_count_their_lines_and_rejections_are_denied() {
        let mut t = Tracker::default();
        t.read(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"C:/p/hello.txt","content":"a\nb\n"}}]}}));
        let done = t.read(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"w1"}]},
            "tool_use_result":{"type":"create","content":"a\nb\n","structuredPatch":[]}}));
        assert_eq!((done[0].added, done[0].removed), (Some(2), Some(0)));
        t.read(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"b1","name":"Bash","input":{"command":"npm test"}}]}}));
        let done = t.read(&json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"b1","is_error":true}]},
            "tool_result_meta":[{"id":"b1","permission_decision":{"decision":"reject"}}]}));
        assert_eq!((done[0].kind.as_str(), done[0].label.as_str(), done[0].status.as_str()), ("run", "npm test", "denied"));
    }

    #[test]
    fn codex_items_become_steps_and_file_changes_count_their_diff() {
        let mut t = Tracker::default();
        t.read(&json!({"type":"item.started","item":{"type":"command_execution","id":"c1","command":"\"pwsh.exe\" -Command \"Get-Content a.txt\"",
            "commandActions":[{"type":"read","command":"Get-Content a.txt","name":"a.txt","path":"C:/p/a.txt"}]}}));
        t.read(&json!({"type":"item.started","item":{"type":"command_execution","id":"c2","command":"\"pwsh.exe\" -Command \"npm test\"","commandActions":[{"type":"unknown","command":"npm test"}]}}));
        t.read(&json!({"type":"item.started","item":{"type":"file_change","id":"f1","changes":[
            {"path":"C:/p/a.txt","kind":{"type":"update"},"diff":"@@ -1 +1,2 @@\n-a\n+b\n+c\n"},
            {"path":"C:/p/new.txt","kind":{"type":"add"},"diff":"x\ny\n"}]}}));
        assert_eq!(t.steps()[0].kind, "read");
        assert_eq!(t.steps()[1].label, "npm test");
        assert_eq!((t.steps()[2].added, t.steps()[2].removed), (Some(2), Some(1)));
        assert_eq!((t.steps()[3].added, t.steps()[3].removed), (Some(2), Some(0)));
        let done = t.read(&json!({"type":"item.completed","item":{"type":"command_execution","id":"c2","status":"completed","exitCode":1}}));
        assert_eq!(done[0].status, "failed");
        let done = t.read(&json!({"type":"item.completed","item":{"type":"file_change","id":"f1","status":"declined","changes":[]}}));
        assert_eq!(done.len(), 2);
        assert!(done.iter().all(|s| s.status == "denied"));
        let left = t.unfinished();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].status, "incomplete");
    }

    #[test]
    fn steps_stop_at_the_cap_and_labels_are_cut() {
        let mut t = Tracker::default();
        for n in 0..MAX_STEPS + 5 {
            t.read(&json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":format!("t{n}"),"name":"Bash","input":{"command":"x".repeat(400)}}]}}));
        }
        assert_eq!(t.steps().len(), MAX_STEPS);
        assert_eq!(t.total, MAX_STEPS + 5);
        assert!(js_len(&t.steps()[0].label) <= LABEL_CHARS + 1);
    }
}
