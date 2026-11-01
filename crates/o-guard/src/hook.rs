//! PreToolUse hook for Claude Code and Codex working in this repository.
//! Exit code 2 blocks the tool call and hands the reason back to the agent.
//!
//! It refuses: shell commands that touch provider credentials or disable a
//! CLI's permission checks, code edits that introduce those, commits or
//! pushes to main, branch names outside the <personal>-<topic> rule, and any
//! change inside a test/main checkout (those are pull-only mirrors).

use std::cell::OnceCell;
use std::io::Read;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::rules::{code_rules, find_hits, is_allowed_path, CREDENTIAL_RULES, PROJECT_REMOTE, READ_ONLY_BRANCHES, READ_ONLY_COMMANDS};

/// Keep in sync with .github/workflows/branch-flow.yml.
pub const PERSONAL_BRANCHES: [&str; 2] = ["younchan", "yongwoo"];

static BRANCH_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^({})(-[a-z0-9][a-z0-9._-]*)?$", PERSONAL_BRANCHES.join("|"))).unwrap());
static CREATE_BRANCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgit\s+(?:switch\s+(?:-c|-C|--create)|checkout\s+(?:-b|-B)|branch)\s+([^\s-]\S*)").unwrap()
});

static SEGMENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||;|\|").unwrap());
static LINE_SEGMENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||;|\||\n").unwrap());
static GIT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b").unwrap());
static PUSH_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpush\b").unwrap());
static PUSH_TO_MAIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\s|:)(refs/heads/)?main\b").unwrap());
static FORCE_FLAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(--force\b|--force-with-lease\b|\s-f\b)").unwrap());
static TEST_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\stest\b").unwrap());
static PUSH_WITH_TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpush\s+\S+\s+\S+").unwrap());
static HISTORY_CHANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(commit|merge|cherry-pick|rebase)\b").unwrap());

static GIT_DASH_C: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\bgit\s+-C\s+("[^"]+"|'[^']+'|\S+)"#).unwrap());
static ABSOLUTE_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:[A-Za-z]:[\\/]|/)[^\s"'|;&<>]+"#).unwrap());
static LEADING_GIT_DASH_C: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^git\s+-C\s+("[^"]+"|'[^']+'|\S+)\s+"#).unwrap());
static RISKY_OPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s(--pre\b|-O\S*|--open-files-in-pager\b|--output\b)").unwrap());
static FD_REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d?>&\d").unwrap());
static NULL_REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\d?>\s*(/dev/null|\$null|NUL)\b").unwrap());

static PATCH_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\*\*\* (?:Add|Update|Delete) File: (.+)$").unwrap());
static ROOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z]:)?[\\/]").unwrap());

/// What the hook asks git about a directory. Both answer "" / false outside a repo.
pub trait GitProbe {
    fn branch(&self, dir: &str) -> String;
    /// A project_O checkout on test or main — not any repository that happens to be on main.
    fn is_mirror(&self, dir: &str) -> bool;
}

const READ_ONLY_REASON: &str =
    "project_O의 test·main 작업 폴더는 읽기 전용입니다(풀만 받는 거울). 자기 개인 브랜치 폴더에서 작업하고 PR로 올리세요";

/// Reviews one tool call (`{ tool_name, tool_input, cwd }`); every returned reason blocks it.
pub fn review(input: &Value, git: &dyn GitProbe) -> Vec<String> {
    let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let empty = Value::Object(Default::default());
    let args = match input.get("tool_input") {
        Some(v) if !v.is_null() => v,
        _ => &empty,
    };
    let cwd = match input.get("cwd").and_then(Value::as_str) {
        Some(dir) => dir.to_string(),
        None => std::env::current_dir().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
    };
    let branch = OnceCell::new();
    let current_branch = || branch.get_or_init(|| git.branch(&cwd)).clone();
    let mut reasons = Vec::new();

    if matches!(tool, "Bash" | "shell" | "PowerShell") {
        let command = string_of(args.get("command"));
        reasons.extend(find_hits(&command, code_rules()).into_iter().map(|hit| hit.reason.to_string()));
        reasons.extend(git_reasons(&command, &current_branch));
        // A mirror is read-only whether the command runs in it or names it from elsewhere.
        if git.is_mirror(&cwd) || paths_in(&command).iter().any(|p| git.is_mirror(p)) {
            reasons.extend(read_only_shell_reasons(&command));
        }
        return reasons;
    }

    // Reading a CLI's sign-in files is as off-limits as copying them in a shell.
    if matches!(tool, "Read" | "Grep" | "Glob") {
        return find_hits(&args.to_string(), CREDENTIAL_RULES.iter()).into_iter().map(|hit| hit.reason.to_string()).collect();
    }

    let raw_paths = edited_paths(tool, args);
    for path in &raw_paths {
        // Forward slashes so dirname works the same for Windows and POSIX paths.
        let absolute = if ROOTED.is_match(path) { path.clone() } else { format!("{cwd}/{path}") }.replace('\\', "/");
        if git.is_mirror(dirname(&absolute)) {
            reasons.push(format!("{READ_ONLY_REASON} ({path})"));
            return reasons;
        }
    }
    let paths: Vec<String> = raw_paths.iter().map(|p| repo_relative(p, &cwd)).collect();
    if !paths.is_empty() && paths.iter().all(|p| is_allowed_path(p)) {
        return reasons;
    }
    if !paths.is_empty() || tool == "apply_patch" {
        let place = if paths.is_empty() { tool.to_string() } else { paths.join(", ") };
        for hit in find_hits(&args.to_string(), code_rules()) {
            reasons.push(format!("{} ({place})", hit.reason));
        }
    }
    reasons
}

/// `String(value ?? "")`: absent or null is empty, other non-strings are their JSON text.
fn string_of(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// The first of `keys` that is present and not null (`a ?? b ?? c`).
fn first_present<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| args.get(*k)).find(|v| !v.is_null())
}

fn git_reasons(command: &str, current_branch: &dyn Fn() -> String) -> Vec<String> {
    let mut reasons = Vec::new();
    for segment in SEGMENT_SPLIT.split(command) {
        if !GIT_WORD.is_match(segment) {
            continue;
        }
        if PUSH_WORD.is_match(segment) {
            if PUSH_TO_MAIN.is_match(segment) {
                reasons.push("main에는 직접 push할 수 없습니다. test → main PR로 병합하세요".to_string());
            } else if FORCE_FLAG.is_match(segment) && TEST_WORD.is_match(segment) {
                reasons.push("test 브랜치는 강제 push할 수 없습니다".to_string());
            } else if !PUSH_WITH_TARGET.is_match(segment) && current_branch() == "main" {
                reasons.push("main 브랜치에서 push할 수 없습니다".to_string());
            }
        }
        if HISTORY_CHANGE.is_match(segment) && current_branch() == "main" {
            reasons.push(
                "main 브랜치에서는 커밋·병합하지 않습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치로 옮기세요".to_string(),
            );
        }
    }
    for captures in CREATE_BRANCH.captures_iter(command) {
        let name = &captures[1];
        if !BRANCH_NAME.is_match(name) {
            reasons.push(format!(
                "브랜치 이름 {name}은 규칙({} 또는 <개인 브랜치>-<주제>)에 맞지 않습니다",
                PERSONAL_BRANCHES.join(", ")
            ));
        }
    }
    reasons
}

/// Absolute paths and `git -C` targets a command names, so a mirror can't be changed from another folder.
fn paths_in(command: &str) -> Vec<String> {
    let found = GIT_DASH_C
        .captures_iter(command)
        .map(|c| c.get(1).unwrap().as_str())
        .chain(ABSOLUTE_PATH.find_iter(command).map(|m| m.as_str()));
    let mut paths: Vec<String> = Vec::new();
    for raw in found {
        let unquoted = strip_quotes(raw).to_string();
        if !paths.contains(&unquoted) {
            paths.push(unquoted);
        }
    }
    paths.truncate(6);
    paths
}

/// Drops one leading and one trailing quote (`/^["']|["']$/g`).
fn strip_quotes(text: &str) -> &str {
    let text = text.strip_prefix(['"', '\'']).unwrap_or(text);
    text.strip_suffix(['"', '\'']).unwrap_or(text)
}

/// A `&` that is neither part of `&&` nor a redirect like `2>&1` or `&>`.
/// (The TS version used a lookbehind, which the regex crate does not support.)
fn has_background_ampersand(command: &str) -> bool {
    let bytes = command.as_bytes();
    bytes.iter().enumerate().any(|(i, &b)| {
        b == b'&'
            && !(i > 0 && matches!(bytes[i - 1], b'&' | b'>'))
            && !matches!(bytes.get(i + 1), Some(b'&' | b'>'))
    })
}

fn read_only_shell_reasons(command: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    // Substitution or a background `&` would run a second command the allow list never saw.
    if command.contains("$(") || command.contains('`') || has_background_ampersand(command) {
        reasons.push(format!("{READ_ONLY_REASON} — 명령 치환이나 &로 이은 명령은 허용하지 않습니다"));
    }
    // Read-only commands with options that run a program or write a file.
    if RISKY_OPTION.is_match(command) {
        reasons.push(format!("{READ_ONLY_REASON} — 다른 프로그램을 실행하거나 파일을 쓰는 옵션은 허용하지 않습니다"));
    }
    for raw in LINE_SEGMENT_SPLIT.split(command) {
        // `git -C <mirror> status` is judged as `git status`.
        let segment = LEADING_GIT_DASH_C.replace(raw.trim(), "git ");
        if segment.is_empty() {
            continue;
        }
        let without_fd = FD_REDIRECT.replace_all(&segment, "");
        let without_safe_redirects = NULL_REDIRECT.replace_all(&without_fd, "");
        if without_safe_redirects.contains('>') || !READ_ONLY_COMMANDS.iter().any(|p| p.is_match(&segment)) {
            let shown: String = segment.chars().take(80).collect();
            reasons.push(format!("{READ_ONLY_REASON} — 허용되지 않은 명령: {shown}"));
        }
    }
    reasons
}

fn edited_paths(tool: &str, args: &Value) -> Vec<String> {
    if let Some(Value::String(path)) = first_present(args, &["file_path", "notebook_path"]) {
        return vec![path.clone()];
    }
    if tool == "apply_patch" {
        let patch = string_of(first_present(args, &["command", "patch", "input"]));
        return PATCH_FILE.captures_iter(&patch).map(|c| c[1].trim().to_string()).collect();
    }
    Vec::new()
}

/// The folder part of a forward-slash path (`node:path` posix `dirname`), keeping `C:/` for a drive root.
fn dirname(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return if path.is_empty() { "." } else { "/" };
    }
    match trimmed.rfind('/') {
        None => ".",
        Some(0) => "/",
        Some(i) => {
            let parent = trimmed[..i].trim_end_matches('/');
            if parent.is_empty() {
                "/"
            } else if parent.len() == 2 && parent.ends_with(':') {
                &trimmed[..3] // "C:/"
            } else {
                parent
            }
        }
    }
}

/// String-based on purpose: hosts send Windows paths whatever OS the hook runs on.
fn repo_relative(path: &str, cwd: &str) -> String {
    let file = path.replace('\\', "/");
    let root = cwd.replace('\\', "/");
    let root = root.trim_end_matches('/');
    let n = root.len();
    let inside = file.len() > n && file.as_bytes()[..n].eq_ignore_ascii_case(root.as_bytes()) && file.as_bytes()[n] == b'/';
    if inside {
        file[n + 1..].to_string()
    } else {
        file
    }
}

/// `git` without a console window (the hook runs on every tool call).
pub(crate) fn git_command() -> Command {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// New files live in folders that may not exist yet; ask git from the nearest existing parent.
fn git_at(dir: &str, args: &[&str]) -> String {
    let mut existing = PathBuf::from(dir);
    while !existing.exists() {
        match existing.parent() {
            Some(parent) if parent.as_os_str().is_empty() => existing = PathBuf::from("."),
            Some(parent) => existing = parent.to_path_buf(),
            None => break,
        }
    }
    // A command may name a file; git runs in its folder.
    if existing.exists() && !existing.is_dir() {
        existing = existing.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")).to_path_buf();
    }
    let output = git_command().args(args).current_dir(&existing).stdin(Stdio::null()).stderr(Stdio::null()).output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

pub struct RealGit;

impl GitProbe for RealGit {
    fn branch(&self, dir: &str) -> String {
        git_at(dir, &["branch", "--show-current"])
    }
    fn is_mirror(&self, dir: &str) -> bool {
        READ_ONLY_BRANCHES.contains(&self.branch(dir).as_str())
            && PROJECT_REMOTE.is_match(&git_at(dir, &["remote", "get-url", "origin"]))
    }
}

/// Whether `dir` (or the folder of a file path) is a project_O test/main mirror.
pub fn is_mirror(dir: &str) -> bool {
    RealGit.is_mirror(dir)
}

fn review_stdin() -> Result<Vec<String>, String> {
    let mut raw = Vec::new();
    std::io::stdin().read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let input: Value = serde_json::from_str(&String::from_utf8_lossy(&raw)).map_err(|e| e.to_string())?;
    if !input.is_object() {
        return Err("입력이 JSON 객체가 아닙니다".to_string());
    }
    Ok(review(&input, &RealGit))
}

/// `o hook`: reads the tool call JSON on stdin, prints reasons, returns the exit code (2 blocks).
pub fn hook_main() -> i32 {
    // Fail closed: a hook that cannot read its input or crashes blocks the call instead of waving it through.
    // (This relies on unwinding panics; the workspace must not set `panic = "abort"`.)
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let outcome = panic::catch_unwind(AssertUnwindSafe(review_stdin));
    panic::set_hook(previous_hook);
    let reasons = match outcome {
        Ok(Ok(reasons)) => reasons,
        Ok(Err(message)) => vec![format!("정책 훅이 입력을 검사하지 못했습니다: {message}")],
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "알 수 없는 오류".to_string());
            vec![format!("정책 훅이 입력을 검사하지 못했습니다: {message}")]
        }
    };
    if reasons.is_empty() {
        return 0;
    }
    let mut unique: Vec<&str> = Vec::new();
    for reason in &reasons {
        if !unique.contains(&reason.as_str()) {
            unique.push(reason);
        }
    }
    let lines: Vec<String> = unique.iter().map(|r| format!("  - {r}")).collect();
    eprintln!(
        "project_O 정책 훅이 차단했습니다:\n{}\n규칙: AGENTS.md · docs/DESIGN.md §8. 우회하지 말고 사용자에게 보고하세요.",
        lines.join("\n")
    );
    2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_ampersand_is_found_but_chains_and_redirects_are_not() {
        assert!(has_background_ampersand("ls & rm -rf src"));
        assert!(has_background_ampersand("ls&"));
        assert!(!has_background_ampersand("git status && git log"));
        assert!(!has_background_ampersand("git diff 2>&1 | head"));
        assert!(!has_background_ampersand("ls &> out"));
    }

    #[test]
    fn dirname_matches_posix_and_keeps_drive_roots() {
        assert_eq!(dirname("C:/ws/test/src/a.ts"), "C:/ws/test/src");
        assert_eq!(dirname("C:/a.ts"), "C:/");
        assert_eq!(dirname("/a"), "/");
        assert_eq!(dirname("a.ts"), ".");
        assert_eq!(dirname("C:/ws/test/"), "C:/ws");
    }

    #[test]
    fn repo_relative_strips_the_cwd_case_insensitively() {
        assert_eq!(repo_relative("c:\\Repo\\src\\a.rs", "C:/repo/"), "src/a.rs");
        assert_eq!(repo_relative("C:/repository/a.rs", "C:/repo"), "C:/repository/a.rs");
        assert_eq!(repo_relative("src/a.rs", "C:/repo"), "src/a.rs");
    }

    #[test]
    fn paths_in_unquotes_dedupes_and_caps() {
        assert_eq!(paths_in(r#"git -C "C:/ws/test" status && cat C:/ws/test"#), vec!["C:/ws/test".to_string()]);
        assert_eq!(paths_in("ls /a /b /c /d /e /f /g").len(), 6);
    }
}
