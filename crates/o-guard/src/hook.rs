//! PreToolUse hook for Claude Code and Codex working in this repository.
//! Exit code 2 blocks the tool call and hands the reason back to the agent.
//!
//! It refuses: shell commands that touch provider credentials or disable a
//! CLI's permission checks, code edits that introduce those, commits or
//! pushes to the protected branches (test_1, test_1_hotfix, test_2, main_hotfix, main), PR merges into
//! test_1_hotfix/main_hotfix/main by anyone but the owner, branch names outside the
//! <personal>-<topic> rule, and any change inside a protected-branch checkout
//! (those are pull-only mirrors).

use std::cell::OnceCell;
use std::io::Read;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::rules::{
    code_rules, find_hits, is_allowed_path, rules_for_paths, CREDENTIAL_RULES, GUARD_FILE, GUARD_FILE_NAMED, GUARD_FILE_REASON, OWNER, PROJECT_REMOTE,
    READ_ONLY_BRANCHES, READ_ONLY_COMMANDS,
};

/// Keep in sync with .github/workflows/branch-flow.yml.
pub const PERSONAL_BRANCHES: [&str; 4] = ["younchan", "yongwoo", "eunmi", "yeeun"];

/// The free integration branch: anyone may merge PRs into it.
const FREE_MERGE_BRANCH: &str = "test_2";

/// Any member may merge into it once another member approved the PR: the GitHub ruleset
/// on test_1 enforces that approval (the owner bypasses it), so the hook does not (ADR-0031).
const APPROVAL_MERGE_BRANCH: &str = "test_1";

static BRANCH_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^({})(-[a-z0-9][a-z0-9._-]*)?$", PERSONAL_BRANCHES.join("|"))).unwrap());
static CREATE_BRANCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgit\s+(?:switch\s+(?:-c|-C|--create)|checkout\s+(?:-b|-B)|branch)\s+([^\s-]\S*)").unwrap()
});

static SEGMENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||;|\|").unwrap());
static LINE_SEGMENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||;|\||\n").unwrap());
static GIT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b").unwrap());
static AGENT_DISPATCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(?:^|[\s/\\])o(?:\.exe)?["']?\s+(?:dispatch|followup|_run|_followup|_lead-forward|_lead-approve|review|_review|sync-mirror|events|runs|lead\s+(?:send|dispatch|forward|decide|stop-all|stop|attach|new))\b"#).unwrap());
static AGENT_CONFIRMED_RECOVERY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)(?:^|[\s/\\])o(?:\.exe)?["']?\s+resources\s+recover\b"#).unwrap());
/// A push naming a protected branch as (part of) its refspec: `main`, `HEAD:test_1`, `refs/heads/test_2`.
static PUSH_TO_PROTECTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?:\s|:)(?:refs/heads/)?({})(?:\s|$)", READ_ONLY_BRANCHES.join("|"))).unwrap());
static PUSH_WITH_TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpush\s+\S+\s+\S+").unwrap());
static HISTORY_CHANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(commit|merge|cherry-pick|rebase)\b").unwrap());
/// Git subcommands that move the branch they run on or rewrite what it holds.
const HISTORY_COMMANDS: [&str; 8] = ["commit", "merge", "cherry-pick", "rebase", "pull", "revert", "am", "reset"];
/// Options before the subcommand that take the next word as their value.
const GIT_VALUE_OPTIONS: [&str; 7] = ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--config-env", "--super-prefix"];
/// `git commit -n` (and `-an`, `-nq`): the short form of `--no-verify`. Clusters that carry a value (`-m…`) are not read.
static SHORT_NO_VERIFY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-[aeopqsSuv]*n[A-Za-z]*$").unwrap());

static ABSOLUTE_PATH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:[A-Za-z]:[\\/]|/)[^\s"'|;&<>]+"#).unwrap());
static LEADING_GIT_DASH_C: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^git\s+-C\s+("[^"]+"|'[^']+'|\S+)\s+"#).unwrap());
static RISKY_OPTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s(--pre\b|-O\S*|--open-files-in-pager\b|--output\b)").unwrap());
static FD_REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d?>&\d").unwrap());
static NULL_REDIRECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\d?>\s*(/dev/null|\$null|NUL)\b").unwrap());

/// How many folders a shell command may name before the hook stops asking git about each one
/// and allows only read-only commands.
const MAX_SHELL_TARGETS: usize = 8;
/// Commands that change the folder the rest of the command runs in (lower case).
const MOVE_COMMANDS: [&str; 6] = ["cd", "chdir", "pushd", "set-location", "sl", "push-location"];
const POP_COMMANDS: [&str; 2] = ["popd", "pop-location"];
/// A redirect (`>`, `2>>`, `<`) or `--flag=` / `-Flag:` glued to the front of a path word.
static PATH_WORD_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(?:\d*(?:>>?|<)&?|--?[A-Za-z][\w-]*[=:])").unwrap());
/// A word that starts with an environment variable: `${NAME}`, `$env:NAME`, `$NAME`, `%NAME%` (rest in group 5).
static VARIABLE_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^(?:\$\{(\w+)\}|\$env:(\w+)|\$(\w+)|%(\w+)%)(.*)$").unwrap());
static PERCENT_VARIABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"%\w+%").unwrap());
static PARENT_STEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?:^|[\\/])\.\.(?:[\\/]|$)").unwrap());

/// Every file a patch touches: added, updated, deleted, and the destination of a move.
static PATCH_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\*\*\* (?:(?:Add|Update|Delete) File|Move to): (.+)$").unwrap());
static ROOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z]:)?[\\/]").unwrap());

/// `gh pr merge` (group 1) or `gh api`, wherever it appears: quoted or not, on any line.
static GH_MERGE_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bgh(?:\.exe)?\s+(?:(pr\s+merge)|api)\b").unwrap());
static API_PR_MERGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:https://api\.github\.com)?/?repos/([^/\s]+/[^/\s]+)/pulls/(\d+)/merge/?$").unwrap());
static API_MERGES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:https://api\.github\.com)?/?repos/[^/\s]+/[^/\s]+/merges/?$").unwrap());
static GRAPHQL_MERGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(mergePullRequest|mergeBranch|enablePullRequestAutoMerge)\b").unwrap());
/// A program that runs a string as a command, so single quotes handed to it do not keep a call literal.
static STRING_RUNNER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:bash|sh|zsh|dash|ksh|fish|pwsh|powershell|cmd|eval|iex|invoke-expression|invoke-command|start-process|ssh|wsl|sudo|su|xargs|watch|python3?|py|node|perl|ruby)\b").unwrap()
});
/// Credentials named on the command line make gh act as someone the login lookup never saw.
static GH_IDENTITY_OVERRIDE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(GH_TOKEN|GITHUB_TOKEN|GH_ENTERPRISE_TOKEN|GH_CONFIG_DIR|GH_HOST)\b|--hostname\b").unwrap());

/// What the hook asks git and gh about a directory. The git answers are "" / false
/// outside a repo; the gh answers are `None` when the lookup fails. The gh
/// defaults answer `None` (fail closed) so a probe that does not know gh stays offline.
pub trait GitProbe {
    fn branch(&self, dir: &str) -> String;
    /// A project_O checkout on a protected branch — not any repository that happens to be on main.
    fn is_mirror(&self, dir: &str) -> bool;
    /// The base branch of a PR (`gh pr view [<selector>] [-R <repo>] --json baseRefName`);
    /// no selector means the current branch's PR.
    fn pr_base(&self, _dir: &str, _selector: Option<&str>, _repo: Option<&str>) -> Option<String> {
        None
    }
    /// The signed-in GitHub login (`gh api user --jq .login`).
    fn gh_login(&self, _dir: &str) -> Option<String> {
        None
    }
}

const READ_ONLY_REASON: &str =
    "project_O의 test_1·test_1_hotfix·test_2·main_hotfix·main 작업 폴더는 읽기 전용입니다(풀만 받는 거울). 자기 개인 브랜치 폴더에서 작업하고 PR로 올리세요";

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

    if matches!(tool, "Bash" | "shell" | "PowerShell" | "exec_command") || (tool != "apply_patch" && first_present(args, &["command", "cmd"]).is_some()) {
        let command = string_of(first_present(args, &["command", "cmd"]));
        let plain = plain_command(&command);
        if (input["inside_agent"].as_bool() == Some(true) || std::env::var_os("O_AGENT_ROLE").is_some_and(|v| !v.is_empty()) || std::env::var_os("O_CARD_ID").is_some_and(|v| !v.is_empty())) && (AGENT_DISPATCH.is_match(&plain) || AGENT_CONFIRMED_RECOVERY.is_match(&plain)) {
            reasons.push("에이전트 안에서는 작업을 시작할 수 없습니다. 사용자가 실행해 주세요".into());
        }
        reasons.extend(find_hits(&command, code_rules()).into_iter().map(|hit| hit.reason.to_string()));
        reasons.extend(find_hits(&plain, code_rules()).into_iter().map(|hit| hit.reason.to_string()));
        reasons.extend(git_reasons(&command, &current_branch));
        if plain != command { reasons.extend(git_reasons(&plain, &current_branch)); }
        reasons.extend(merge_reasons(&command, &cwd, git));
        reasons.extend(gh_write_reasons(&command));
        if plain != command { reasons.extend(gh_write_reasons(&plain)); }
        // Reading a guard file is fine; anything else that names one may change it.
        // Pointing git at the repository's hooks (what `npm install` does) names the folder, not a change to it.
        static HOOKS_PATH_SET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)core\.hookspath(?:\s+|=)["']?\.githooks/?["']?(?:\s|$)"#).unwrap());
        if GUARD_FILE_NAMED.is_match(&HOOKS_PATH_SET.replace_all(&command, " ")) && !read_only_shell_reasons(&command).is_empty() {
            reasons.push(GUARD_FILE_REASON.into());
        }
        // A mirror is read-only whether the command runs in it or names it from elsewhere.
        let targets = shell_targets(&command, &cwd);
        if (protected_state(&cwd) || targets.as_ref().is_ok_and(|found| found.iter().any(|p| protected_state(p)))) && !read_only_shell_reasons(&command).is_empty() {
            reasons.push("O 상태·프로필 폴더는 도구로 직접 바꿀 수 없습니다. O의 정해진 명령을 사용하세요".into());
        }
        if git.is_mirror(&cwd) || targets.as_ref().is_ok_and(|found| found.iter().any(|p| git.is_mirror(p))) {
            reasons.extend(read_only_shell_reasons(&command));
        } else if let Err(unknown) = &targets {
            if !read_only_shell_reasons(&command).is_empty() {
                reasons.push(format!(
                    "명령이 고칠 수 있는 폴더를 확인하지 못했습니다({unknown}). 거울(test_1·test_1_hotfix·test_2·main_hotfix·main 작업 폴더)일 수 있어 읽기 전용 명령만 허용합니다. 경로를 변수 없이 그대로 적으세요"
                ));
            }
        }
        let mut seen = std::collections::HashSet::new();
        reasons.retain(|reason| seen.insert(reason.clone()));
        return reasons;
    }

    // Reading a CLI's sign-in files is as off-limits as copying them in a shell.
    if matches!(tool, "Read" | "Grep" | "Glob") {
        return find_hits(&args.to_string(), CREDENTIAL_RULES.iter()).into_iter().map(|hit| hit.reason.to_string()).collect();
    }

    // A page fetched by the CLI's own tool is a request like one made in a shell.
    if tool == "WebFetch" {
        return find_hits(&string_of(first_present(args, &["url", "uri"])), code_rules()).into_iter().map(|hit| hit.reason.to_string()).collect();
    }

    let raw_paths = edited_paths(tool, args);
    for path in &raw_paths {
        // Forward slashes so dirname works the same for Windows and POSIX paths; `..` resolved lexically.
        let absolute = resolve(path, &cwd, None).unwrap_or_else(|| path.clone());
        if git.is_mirror(dirname(&absolute)) {
            reasons.push(format!("{READ_ONLY_REASON} ({path})"));
            return reasons;
        }
        if GUARD_FILE.is_match(&absolute) {
            reasons.push(format!("{GUARD_FILE_REASON} ({path})"));
            return reasons;
        }
        if protected_state(&absolute) {
            reasons.push(format!("O 상태·프로필 폴더는 직접 편집할 수 없습니다 ({path})"));
            return reasons;
        }
    }
    let paths: Vec<String> = raw_paths.iter().map(|p| repo_relative(p, &cwd)).collect();
    if !paths.is_empty() && paths.iter().all(|p| is_allowed_path(p)) {
        return reasons;
    }
    if !paths.is_empty() || tool == "apply_patch" {
        let place = if paths.is_empty() { tool.to_string() } else { paths.join(", ") };
        // A provider file may spell the bypass switches (ADR-0014); every other rule still applies.
        for hit in find_hits(&args.to_string(), rules_for_paths(&paths)) {
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

/// Supplement literal checks; this is not a shell interpreter or an OS sandbox.
fn plain_command(command: &str) -> String {
    command.replace("`\r\n", "").replace("`\n", "").replace("\\\n", "").replace(['"', '\'', '\\', '^', '`'], "")
}

fn gh_write_reasons(command: &str) -> Vec<String> {
    let mut reasons = Vec::new();
    for segment in split_unquoted(command) {
        let words = shell_words(segment);
        for start in 0..words.len() {
            let program = words[start].rsplit(['/', '\\']).next().unwrap_or_default();
            if !matches!(program.to_ascii_lowercase().as_str(), "gh" | "gh.exe") { continue; }
            let args = &words[start + 1..];
            let Some(sub) = args.first().map(String::as_str) else { continue };
            if !["api", "auth", "pr", "issue", "repo", "release", "run", "workflow", "search", "status", "browse", "attestation", "version", "help", "--version", "--help"].contains(&sub) {
                reasons.push("gh 별칭·확장 명령은 검사할 수 없습니다. 공식 명령을 그대로 쓰세요".into());
            }
            if (sub == "workflow" && args.get(1).is_some_and(|arg| ["run", "enable", "disable"].contains(&arg.as_str())))
                || (sub == "run" && args.get(1).is_some_and(|arg| arg == "rerun")) {
                reasons.push("워크플로 실행·설정 변경은 사람이 직접 수행해야 합니다".into());
            }
            if sub != "api" { continue; }
            let (positional, flags) = split_flags(&args[1..], &["-X", "--method", "-f", "--raw-field", "-F", "--field", "--input", "-H", "--header", "--jq", "-q", "--hostname"]);
            let endpoint = positional.first().map(String::as_str).unwrap_or_default().split(['?', '#']).next().unwrap_or_default().trim_end_matches('/');
            let fields = flag_values(&flags, &["-f", "--raw-field", "-F", "--field", "--input"]);
            let method = flag_values(&flags, &["-X", "--method"]).first().map(|s| s.to_ascii_uppercase()).unwrap_or_else(|| if fields.is_empty() { "GET".into() } else { "POST".into() });
            let write = !["GET", "HEAD"].contains(&method.as_str());
            let repository_write = endpoint.rfind("repos/").is_some_and(|at| endpoint[at..].split('/').count() == 3);
            let sensitive = endpoint.contains("/rulesets") || endpoint.contains("/protection") || endpoint.contains("/branches/") || endpoint.contains("/git/") || endpoint.contains("/contents/") || endpoint.ends_with("/contents") || endpoint.contains("/actions/") || endpoint.ends_with("/dispatches") || endpoint.contains('%') || repository_write;
            let opaque_body = flags.iter().any(|(f, _)| f == "--input") || fields.iter().any(|f| f.contains("=@"));
            let graphql = endpoint == "graphql" && (command.contains("mutation") || opaque_body);
            if (write && sensitive) || graphql {
                reasons.push("gh api로 저장소 보호·Git ref·파일·워크플로를 직접 변경할 수 없습니다. 정식 PR 경로를 사용하세요".into());
            }
        }
    }
    reasons
}

/// Lexical spelling plus the nearest existing parent's real path (junctions and short names).
fn real_path(path: &str) -> String {
    let path = Path::new(path);
    for ancestor in path.ancestors() {
        if let Ok(real) = ancestor.canonicalize() {
            return normalize(&real.join(path.strip_prefix(ancestor).unwrap_or(Path::new(""))).to_string_lossy().replace('\\', "/"));
        }
    }
    normalize(&path.to_string_lossy().replace('\\', "/"))
}

pub fn within_state(path: &str, state: &str) -> bool {
    let state = real_path(state);
    let path = real_path(path);
    ["", "-agents", "-lead", "-term", "-worktrees"].iter().any(|suffix| {
        let root = format!("{state}{suffix}");
        path.eq_ignore_ascii_case(&root) || inside(&path, &root)
    })
}

fn protected_state(path: &str) -> bool {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).or_else(|| {
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(|home| PathBuf::from(home).join(".local/state"))
    });
    base.is_some_and(|base| within_state(path, &base.join("project_O").to_string_lossy()))
}

/// The first of `keys` that is present and not null (`a ?? b ?? c`).
fn first_present<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().filter_map(|k| args.get(*k)).find(|v| !v.is_null())
}

fn git_reasons(command: &str, current_branch: &dyn Fn() -> String) -> Vec<String> {
    let mut reasons = Vec::new();
    if command.to_ascii_uppercase().contains("GIT_CONFIG_") {
        reasons.push("GIT_CONFIG_*로 git 설정을 주입할 수 없습니다".into());
    }
    for segment in SEGMENT_SPLIT.split(command) {
        if !GIT_WORD.is_match(segment) {
            continue;
        }
        if git_calls(segment).iter().any(|call| call.sub == "push") {
            if let Some(target) = PUSH_TO_PROTECTED.captures(segment) {
                reasons.push(format!(
                    "{}에는 직접 push할 수 없습니다. PR로만 바뀝니다 (개인·작업 브랜치 → test_2 또는 test_1_hotfix → test_1 → main, 정식 핫픽스는 main_hotfix → main)",
                    &target[1]
                ));
            } else if !PUSH_WITH_TARGET.is_match(segment) && is_protected(&current_branch()) {
                reasons.push(format!("보호 브랜치 {}에서는 push할 수 없습니다. 자기 개인 브랜치에서 PR로 올리세요", current_branch()));
            }
        }
        if HISTORY_CHANGE.is_match(segment) && is_protected(&current_branch()) {
            reasons.push(format!(
                "보호 브랜치 {}에서는 커밋·병합하지 않습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치로 옮기세요",
                current_branch()
            ));
        }
    }
    for call in git_calls(command) {
        reasons.extend(git_call_reasons(&call, current_branch));
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
    let mut unique: Vec<String> = Vec::new();
    for reason in reasons {
        push_unique(&mut unique, reason);
    }
    unique
}

/// One `git` invocation read as words: the `-c` settings before the subcommand, the subcommand
/// (lower case) and what follows it.
#[derive(PartialEq)]
struct GitCall {
    config: Vec<String>,
    sub: String,
    args: Vec<String>,
}

/// Whether a command word runs git, however it is spelled: a path, `git.exe`, or with quotes
/// (dropped by `shell_words`) or escape characters inside the name.
fn is_git(word: &str) -> bool {
    let lowered = word.trim_start_matches(['(', '{', '$', '`', '&', '!']).to_ascii_lowercase();
    let base = lowered.rsplit(['/', '\\']).next().unwrap_or("");
    let unescaped: String = lowered.chars().filter(|c| !matches!(c, '\\' | '^' | '`')).collect();
    [base, unescaped.as_str()].iter().any(|name| name.strip_suffix(".exe").unwrap_or(name) == "git")
}

/// Every git call in a command, read from its words so quoting cannot hide the program, the
/// subcommand or a branch name (`g""it`, `"main"`). When the command hands a string to another
/// shell, the words inside that string are read too.
fn git_calls(command: &str) -> Vec<GitCall> {
    let mut texts = vec![command.to_string()];
    if STRING_RUNNER.is_match(command) {
        texts.push(command.replace(['"', '\''], " "));
    }
    let mut calls: Vec<GitCall> = Vec::new();
    for text in &texts {
        for segment in split_unquoted(text) {
            let words = shell_words(segment);
            for start in (0..words.len()).filter(|i| is_git(&words[*i])) {
                let mut config = Vec::new();
                let mut sub = None;
                let mut rest = words[start + 1..].iter();
                while let Some(word) = rest.next() {
                    if GIT_VALUE_OPTIONS.contains(&word.as_str()) {
                        let value = rest.next();
                        if word == "-c" || word == "--config-env" {
                            config.extend(value.cloned());
                        }
                    } else if let Some(value) = word.strip_prefix("--config-env=") {
                        config.push(value.to_string());
                    } else if let Some(value) = word.strip_prefix("-c").filter(|value| !value.is_empty()) {
                        config.push(value.to_string());
                    } else if !word.starts_with('-') {
                        sub = Some(word.to_ascii_lowercase());
                        break;
                    }
                }
                let Some(sub) = sub else { continue };
                let args = rest.map(|word| word.trim_end_matches([')', '}', '`']).to_string()).collect();
                let call = GitCall { config, sub, args };
                if !calls.contains(&call) {
                    calls.push(call);
                }
            }
        }
    }
    calls
}

/// Why one git call is refused: it reaches a protected branch (a push, a move of the branch, a
/// switch onto it, history changed while on it) or turns the git hooks off.
fn git_call_reasons(call: &GitCall, current_branch: &dyn Fn() -> String) -> Vec<String> {
    let mut reasons = Vec::new();
    let sub = call.sub.as_str();
    // Aliases and external git-* programs can hide another command and its hook-bypass flags.
    const BUILTINS: &str = "add am apply archive bisect blame branch bundle cat-file check-attr check-ignore check-mailmap check-ref-format checkout checkout-index cherry cherry-pick clean clone commit commit-tree config describe diff diff-files diff-index diff-tree fetch for-each-ref format-patch fsck gc grep hash-object help init log ls-files ls-remote ls-tree maintenance merge merge-base merge-tree mktree mv notes pull push range-diff read-tree rebase reflog remote repack replace reset restore revert rev-list rev-parse rm show show-ref sparse-checkout stash status submodule switch symbolic-ref tag update-index update-ref verify-commit verify-tag whatchanged worktree write-tree version";
    if !BUILTINS.split_whitespace().any(|known| known == sub) {
        reasons.push("git 별칭·외부 명령은 검사할 수 없습니다. 공식 명령을 그대로 쓰세요".into());
    }
    let flag = |names: &[&str]| call.args.iter().any(|arg| names.contains(&arg.as_str()));
    let positional: Vec<&str> = call.args.iter().map(String::as_str).take_while(|arg| *arg != "--").filter(|arg| !arg.starts_with('-')).collect();
    let heads = |name: &str| name.strip_prefix("refs/heads/").unwrap_or(name).to_string();
    let moved = |branch: &str| format!("보호 브랜치 {branch}를 직접 옮기거나 지우거나 바꿀 수 없습니다. PR 병합으로만 바뀝니다");

    let hooks_path = |text: &str| text.to_ascii_lowercase().starts_with("core.hookspath");
    let routing = |text: &str| {
        let lower = text.to_ascii_lowercase();
        lower.starts_with("alias.") || lower.starts_with("push.") || (lower.starts_with("branch.") && [".merge", ".remote", ".pushremote"].iter().any(|key| lower.split('=').next().unwrap_or_default().ends_with(key)))
    };
    if call.config.iter().any(|setting| routing(setting)) || (sub == "config" && positional.iter().position(|arg| routing(arg)).is_some_and(|at| positional.get(at + 1).is_some())) {
        reasons.push("git 별칭·push 대상 설정은 에이전트가 바꿀 수 없습니다. 브랜치 이름을 명시하세요".into());
    }
    let sets_hooks_path = sub == "config"
        && positional.iter().position(|arg| hooks_path(arg)).is_some_and(|at| {
            flag(&["--unset", "--unset-all"]) || positional.contains(&"unset") || positional.get(at + 1).is_some_and(|value| *value != ".githooks")
        });
    let short_no_verify = sub == "commit" && call.args.iter().take_while(|arg| *arg != "--").any(|arg| SHORT_NO_VERIFY.is_match(arg));
    if flag(&["--no-verify"]) || short_no_verify || sets_hooks_path || call.config.iter().any(|setting| hooks_path(setting)) {
        reasons.push("git 훅을 건너뛰거나 끄는 옵션(--no-verify, core.hooksPath 변경)은 쓸 수 없습니다. 훅이 막으면 사유를 그대로 보고하세요".to_string());
    }

    if sub == "push" {
        let specs = positional.get(1..).unwrap_or(&[]);
        if flag(&["--all", "--mirror", "--branches"]) || specs.iter().any(|spec| spec.contains('*') || spec.trim_start_matches('+') == ":") {
            reasons.push("여러 브랜치를 한꺼번에 push하는 옵션(--all, --mirror, 와일드카드)은 보호 브랜치까지 올릴 수 있어 쓸 수 없습니다. 올릴 브랜치 이름을 적으세요".to_string());
        }
        for spec in specs {
            let target = heads(spec.trim_start_matches('+').rsplit(':').next().unwrap_or(spec));
            let target = if matches!(target.as_str(), "HEAD" | "@") { current_branch() } else { target };
            if is_protected(&target) {
                reasons.push(format!(
                    "{target}에는 직접 push할 수 없습니다. PR로만 바뀝니다 (개인·작업 브랜치 → test_2 또는 test_1_hotfix → test_1 → main, 정식 핫픽스는 main_hotfix → main)"
                ));
            } else if !target.starts_with("refs/tags/") && !BRANCH_NAME.is_match(&target) {
                reasons.push(format!("push 대상 브랜치 {target}은 개인·작업 브랜치 이름 규칙에 맞지 않습니다"));
            }
        }
        if specs.is_empty() {
            reasons.push("push할 브랜치 이름을 명시하세요. 설정된 upstream은 보호 브랜치를 가리킬 수 있습니다".into());
        }
    }
    // The mirror's own `git pull --ff-only` only follows the remote.
    if HISTORY_COMMANDS.contains(&sub) && !(sub == "pull" && flag(&["--ff-only"])) && is_protected(&current_branch()) {
        reasons.push(format!(
            "보호 브랜치 {}에서는 커밋·병합하지 않습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치로 옮기세요",
            current_branch()
        ));
    }
    // The branch is read once, before the command runs: a switch onto a protected branch
    // would let the rest of the same command commit there unseen.
    if matches!(sub, "checkout" | "switch") && !flag(&["-b", "-B", "-c", "-C", "--create", "--force-create", "--orphan", "--detach", "-d"]) && !call.args.iter().any(|arg| arg == "--") {
        if let Some(branch) = positional.first().filter(|branch| is_protected(branch)) {
            reasons.push(format!(
                "작업 폴더를 보호 브랜치 {branch}로 전환하지 않습니다. 파일이 필요하면 git show {branch}:<경로>나 git checkout {branch} -- <경로>를, 갈라 나올 때는 git switch -c <개인 브랜치>-<주제> {branch}를 쓰세요"
            ));
        }
    }
    // The branch a call creates or re-creates, read from the call's words so `git.exe`, a path or
    // quotes do not hide it: it follows the naming rule, and `-B main` would move a protected branch.
    let created = match sub {
        "checkout" | "switch" => {
            let create = ["-b", "-B", "-c", "-C", "--create", "--force-create", "--orphan"];
            call.args.iter().position(|arg| create.contains(&arg.as_str())).and_then(|at| call.args.get(at + 1)).map(String::as_str)
        }
        "branch" if call.args.iter().all(|arg| !arg.starts_with('-') || matches!(arg.as_str(), "-f" | "--force")) => positional.first().copied(),
        _ => None,
    };
    if let Some(name) = created.filter(|name| !name.starts_with('-')) {
        if is_protected(name) {
            reasons.push(moved(name));
        }
        if !BRANCH_NAME.is_match(name) {
            reasons.push(format!("브랜치 이름 {name}은 규칙({} 또는 <개인 브랜치>-<주제>)에 맞지 않습니다", PERSONAL_BRANCHES.join(", ")));
        }
    }
    if sub == "worktree" && positional.first() == Some(&"add") && !flag(&["-b", "-B", "--detach", "-d", "--orphan"]) {
        if let Some(branch) = positional.get(2).filter(|branch| is_protected(branch)) {
            reasons.push(format!("보호 브랜치 {branch}의 작업 폴더(거울)는 사람이 설정합니다. 에이전트는 만들지 않습니다"));
        }
    }
    if sub == "branch" && flag(&["-f", "--force", "-d", "-D", "--delete", "-m", "-M", "--move", "-c", "-C", "--copy"]) {
        reasons.extend(positional.iter().filter(|branch| is_protected(branch)).map(|branch| moved(branch)));
    }
    if sub == "update-ref" || (sub == "symbolic-ref" && positional.len() >= 2) {
        reasons.extend(positional.iter().filter_map(|name| name.strip_prefix("refs/heads/")).filter(|branch| is_protected(branch)).map(moved));
    }
    if matches!(sub, "fetch" | "pull") {
        for spec in positional.get(1..).unwrap_or(&[]) {
            if let Some(branch) = spec.rsplit_once(':').map(|(_, to)| heads(to)).filter(|branch| is_protected(branch)) {
                reasons.push(moved(&branch));
            }
        }
    }
    reasons
}

fn is_protected(branch: &str) -> bool {
    READ_ONLY_BRANCHES.contains(&branch)
}

/// What a merge command merges into: a PR to look up, or a base branch it names itself
/// (`None` when the command does not say).
enum MergeTarget {
    Pr { selector: Option<String>, repo: Option<String> },
    Base(Option<String>),
}

/// Merges into test_1_hotfix/main_hotfix/main are the owner's: `gh pr merge` and gh's direct merge APIs
/// pass only when the base is confirmed free (test_2, test_1, a personal or work branch)
/// or the signed-in gh user is the owner. Any lookup that fails blocks, the owner's too:
/// a PR whose base can't be read is refused before the owner check.
fn merge_reasons(command: &str, cwd: &str, git: &dyn GitProbe) -> Vec<String> {
    let mut reasons = Vec::new();
    // Quotes inside the program name (`g""h`, `"gh"`) hide the call from the reading below:
    // when dropping the quotes shows more calls, the command is judged without them.
    let plain = plain_command(command);
    let command = if GH_MERGE_CALL.find_iter(&plain).count() > GH_MERGE_CALL.find_iter(command).count() { plain.as_str() } else { command };
    for (is_pr_merge, arguments, starts_command) in gh_calls(command) {
        let Some(target) = merge_target(is_pr_merge, arguments) else { continue };
        let base = match &target {
            MergeTarget::Pr { selector, repo } => git.pr_base(cwd, selector.as_deref(), repo.as_deref()),
            MergeTarget::Base(base) => base.clone(),
        };
        // Only a call that starts a command: the words quoted in a commit message or PR body
        // name no PR, and for those the owner check below still decides.
        if starts_command && base.is_none() && matches!(target, MergeTarget::Pr { .. }) {
            reasons.push(format!(
                "병합할 PR의 base 브랜치를 확인하지 못했습니다(gh pr view 실패). 보호 브랜치 병합일 수 있어 막습니다. test_1_hotfix·main_hotfix·main 병합은 대표({OWNER})만 합니다"
            ));
            continue;
        }
        if base.as_deref().is_some_and(is_free_base) {
            continue;
        }
        // A token on the command line would merge as someone the lookup below never saw.
        let login = if GH_IDENTITY_OVERRIDE.is_match(command) { None } else { git.gh_login(cwd) };
        if login.as_deref() == Some(OWNER) {
            continue;
        }
        let into = base.clone().unwrap_or_else(|| "확인 못 한 브랜치".to_string());
        reasons.push(match &login {
            None => format!(
                "gh 로그인 계정을 확인하지 못해 {into} 병합을 막습니다(gh api user 실패 또는 명령에 토큰 지정). test_1_hotfix·main_hotfix·main 병합은 대표({OWNER})만 합니다"
            ),
            Some(user) => format!(
                "{into}에는 대표({OWNER})만 병합할 수 있습니다(지금 gh 로그인: {user}). test_2는 누구나, test_1은 다른 팀원 1명이 승인한 뒤 누구나 병합하고, test_1_hotfix·main_hotfix·main은 대표가 병합합니다"
            ),
        });
    }
    reasons
}

/// test_2, test_1 and the personal/work branches are free to merge into here (test_1's
/// approval is GitHub's to enforce); anything else needs the owner.
fn is_free_base(base: &str) -> bool {
    base == FREE_MERGE_BRANCH || base == APPROVAL_MERGE_BRANCH || BRANCH_NAME.is_match(base)
}

/// Each `gh pr merge` (`true`) or `gh api` (`false`) call with its arguments: the text from the call
/// to the first `;`, `&`, `|` or line break (LF, CR) outside quotes. Calls are found anywhere, so a
/// line break or quoting cannot hide one; one inside an `echo` only costs a lookup. The third value
/// says whether the call starts a command (nothing but `;`, `&`, `|`, `(`, a backtick or a line break before it)
/// and is not literal text inside single quotes (#56).
fn gh_calls(command: &str) -> Vec<(bool, &str, bool)> {
    GH_MERGE_CALL
        .captures_iter(command)
        .map(|c| {
            let whole = c.get(0).unwrap();
            let rest = &command[whole.end()..];
            let before = command[..whole.start()].trim_end_matches([' ', '\t']);
            let starts_command = (before.is_empty() || before.ends_with([';', '&', '|', '(', '`', '\n', '\r']))
                && !in_literal_quote(command, whole.start());
            (c.get(1).is_some(), &rest[..unquoted_end(rest)], starts_command)
        })
        .collect()
}

/// Whether `at` lies inside single quotes, where bash and PowerShell run nothing. Read with both
/// escape characters (bash `\`, PowerShell `` ` ``) and true only when both readings agree. Double
/// quotes do not count (`$(…)` runs inside them), nor does text handed to a program that runs a string.
/// Quoting this reading cannot follow (command substitutions, `$'…'`, here-documents, comments, curly quotes)
/// counts as not literal, so the call keeps its full check.
fn in_literal_quote(command: &str, at: usize) -> bool {
    const CURLY_QUOTES: [char; 7] = ['\u{2018}', '\u{2019}', '\u{201A}', '\u{201B}', '\u{201C}', '\u{201D}', '\u{201E}'];
    if STRING_RUNNER.is_match(command) || command.contains("$(") || command.contains('`') || command.contains("$'") || command.contains("<<") || command.contains(CURLY_QUOTES) {
        return false;
    }
    ['\\', '`'].into_iter().all(|escape| {
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for c in command[..at].chars() {
            match quote {
                Some('\'') if c == '\'' => quote = None,
                Some('\'') => {}
                _ if escaped => escaped = false,
                _ if c == escape => escaped = true,
                Some(_) if c == '"' => quote = None,
                Some(_) => {}
                None if c == '"' || c == '\'' => quote = Some(c),
                None if c == '#' => return false,
                None => {}
            }
        }
        quote == Some('\'')
    })
}

/// Where the command part starting `text` ends: the first `;`, `&`, `|`, LF or CR outside quotes.
fn unquoted_end(text: &str) -> usize {
    let mut quote: Option<char> = None;
    for (i, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if matches!(c, ';' | '&' | '|' | '\n' | '\r') => return i,
            None => {}
        }
    }
    text.len()
}

/// Recognizes `gh pr merge …`, `gh api …/pulls/<n>/merge`, `gh api …/merges` and GraphQL merge mutations
/// from the arguments that follow `gh pr merge` / `gh api`.
fn merge_target(is_pr_merge: bool, arguments: &str) -> Option<MergeTarget> {
    let words = shell_words(arguments);
    if is_pr_merge {
        let (positional, flags) = split_flags(&words, &["-A", "--author-email", "-b", "--body", "-F", "--body-file", "--match-head-commit", "-R", "--repo", "-t", "--subject"]);
        let repo = flag_values(&flags, &["-R", "--repo"]).into_iter().next();
        return Some(MergeTarget::Pr { selector: positional.into_iter().next(), repo });
    }
    let (positional, flags) = split_flags(
        &words,
        &["-X", "--method", "-f", "--raw-field", "-F", "--field", "-H", "--header", "-q", "--jq", "-t", "--template", "--input", "--hostname", "--cache", "-p", "--preview"],
    );
    // A query string or fragment does not change which endpoint is called.
    let endpoint = positional.first()?.split(['?', '#']).next().unwrap_or_default();
    if let Some(pr) = API_PR_MERGE.captures(endpoint) {
        let repo = Some(pr[1].to_string()).filter(|r| !r.contains('{'));
        return Some(MergeTarget::Pr { selector: Some(pr[2].to_string()), repo });
    }
    if API_MERGES.is_match(endpoint) {
        let fields = flag_values(&flags, &["-f", "--raw-field", "-F", "--field"]);
        let base = fields.iter().find_map(|f| f.strip_prefix("base=")).map(str::to_string);
        return Some(MergeTarget::Base(base));
    }
    // A GraphQL body read from a file or stdin cannot be checked for a merge mutation: treated as one.
    let fields = flag_values(&flags, &["-f", "--raw-field", "-F", "--field"]);
    let unseen_body = flags.iter().any(|(flag, _)| flag == "--input") || fields.iter().any(|field| field.split_once('=').is_some_and(|(_, value)| value.starts_with('@')));
    (endpoint.eq_ignore_ascii_case("graphql") && (GRAPHQL_MERGE.is_match(arguments) || unseen_body)).then_some(MergeTarget::Base(None))
}

/// Positional words and `(flag, value)` pairs; `takes_value` flags consume the next word unless written `--flag=value`.
fn split_flags(words: &[String], takes_value: &[&str]) -> (Vec<String>, Vec<(String, String)>) {
    let mut positional = Vec::new();
    let mut flags = Vec::new();
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        if !word.starts_with('-') || word == "-" {
            positional.push(word.clone());
        } else if let Some((flag, value)) = word.split_once('=').filter(|(f, _)| f.starts_with("--")) {
            flags.push((flag.to_string(), value.to_string()));
        } else if takes_value.contains(&word.as_str()) {
            flags.push((word.clone(), iter.next().cloned().unwrap_or_default()));
        } else if word.len() > 2 && word.is_ascii() && takes_value.contains(&&word[..2]) && !word.starts_with("--") {
            flags.push((word[..2].to_string(), word[2..].to_string()));
        } else {
            flags.push((word.clone(), String::new()));
        }
    }
    (positional, flags)
}

fn flag_values(flags: &[(String, String)], names: &[&str]) -> Vec<String> {
    flags.iter().filter(|(f, _)| names.contains(&f.as_str())).map(|(_, v)| v.clone()).collect()
}

/// Splits on whitespace outside single or double quotes, dropping the quotes.
fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => word.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                in_word = true;
            }
            None if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            None => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

/// The folders and files a shell command names outside the folder it runs in, so a mirror can't be
/// changed from another folder: absolute paths anywhere in it, `git -C` targets, `cd`/`Set-Location`/
/// `pushd` targets, and `..`, `~` or `$VAR/` paths resolved against the folder each part runs in
/// (following those moves). Both the quoted words and the text with quotes dropped are read, so a
/// nested `bash -c "cd ../x && …"` is seen too. `Err` says why the targets can't be told: a move or
/// path built from an unknown variable or substitution, `cd -`, or more than `MAX_SHELL_TARGETS`.
/// Plain relative words inside the current folder stay in the folder already checked.
pub fn shell_targets(command: &str, cwd: &str) -> Result<Vec<String>, String> {
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok();
    let home = home.as_deref();
    let mut targets: Vec<String> = Vec::new();
    for found in ABSOLUTE_PATH.find_iter(command) {
        // `../x`, `~/x`, `origin/main`, `s/a/b`: the tail of a relative word, not an absolute path (`-o/abs` still is one).
        let tail_of_word = found.as_str().starts_with('/')
            && match command.as_bytes()[..found.start()] {
                [.., b'-', p] if p.is_ascii_alphanumeric() => false,
                [.., p] => matches!(p, b'.' | b'~' | b'_') || p.is_ascii_alphanumeric(),
                [] => false,
            };
        if !tail_of_word {
            push_unique(&mut targets, normalize(&strip_quotes(found.as_str()).replace('\\', "/")));
        }
    }
    let start = normalize(&cwd.replace('\\', "/"));
    for text in [command.to_string(), command.replace(['"', '\''], " ")] {
        let mut here = start.clone();
        let mut pushed: Vec<String> = Vec::new();
        for segment in split_unquoted(&text) {
            let words = shell_words(segment);
            let Some(first) = words.first() else { continue };
            let name = first.to_ascii_lowercase();
            if POP_COMMANDS.contains(&name.as_str()) {
                here = pushed.pop().ok_or_else(|| format!("{first}로 돌아갈 폴더"))?;
                continue;
            }
            if MOVE_COMMANDS.contains(&name.as_str()) {
                if words[1..].iter().any(|w| w == "-") {
                    return Err(format!("{first} -"));
                }
                let to = words[1..].iter().find(|w| !w.starts_with('-') && !w.eq_ignore_ascii_case("/d")).map_or("~", String::as_str);
                let dir = resolve(to, &here, home).ok_or_else(|| format!("{first} {to}"))?;
                if name.contains("push") {
                    pushed.push(here);
                }
                push_unique(&mut targets, dir.clone());
                here = dir;
                continue;
            }
            for (i, word) in words.iter().enumerate() {
                let after_git_dash_c = i >= 2 && words[i - 1] == "-C" && matches!(words[i - 2].as_str(), "git" | "git.exe");
                let word = PATH_WORD_PREFIX.replace(word, "");
                let is_variable = VARIABLE_PREFIX.captures(&word).is_some_and(|c| !c[5].is_empty());
                if !(after_git_dash_c || is_variable || word.starts_with('~') || PARENT_STEP.is_match(&word)) {
                    continue;
                }
                let path = resolve(&word, &here, home).ok_or_else(|| word.to_string())?;
                if after_git_dash_c || !inside(&path, &here) {
                    push_unique(&mut targets, path);
                }
            }
        }
    }
    if targets.len() > MAX_SHELL_TARGETS {
        return Err(format!("경로 {}개 — {MAX_SHELL_TARGETS}개까지만 확인", targets.len()));
    }
    Ok(targets)
}

fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

/// The command parts of `text` between `;`, `&`, `|` and line breaks outside quotes.
fn split_unquoted(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut rest = text;
    loop {
        let end = unquoted_end(rest);
        parts.push(&rest[..end]);
        if end == rest.len() {
            return parts;
        }
        rest = &rest[end + 1..]; // every separator is one ASCII byte
    }
}

/// `word` as an absolute forward-slash path from `base`, expanding a leading `~` or environment
/// variable; `None` when that can't be done (unknown variable, `~user`, a `$` or `%VAR%` left inside).
pub fn resolve(word: &str, base: &str, home: Option<&str>) -> Option<String> {
    let word = word.replace('\\', "/");
    let expanded = if let Some(rest) = word.strip_prefix('~') {
        if !(rest.is_empty() || rest.starts_with('/')) {
            return None;
        }
        format!("{}{rest}", home?.replace('\\', "/"))
    } else if let Some(c) = VARIABLE_PREFIX.captures(&word) {
        let name = (1..=4).find_map(|i| c.get(i))?.as_str();
        format!("{}{}", std::env::var(name).ok()?.replace('\\', "/"), &c[5])
    } else {
        word
    };
    if expanded.contains('$') || expanded.contains('`') || PERCENT_VARIABLE.is_match(&expanded) {
        return None;
    }
    let joined = if ROOTED.is_match(&expanded) { expanded } else { format!("{base}/{expanded}") };
    Some(normalize(&joined))
}

/// Drops `.`, empty steps and `..` (with the step before it) from a forward-slash path, keeping `C:/` or `/`.
fn normalize(path: &str) -> String {
    let unc;
    let path = if let Some(rest) = path.strip_prefix("//?/UNC/").or_else(|| path.strip_prefix("//./UNC/")) {
        unc = format!("//{rest}");
        unc.as_str()
    } else { path.strip_prefix("//?/").or_else(|| path.strip_prefix("//./")).unwrap_or(path) };
    let bytes = path.as_bytes();
    let root_len = if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        3
    } else {
        if path.starts_with("//") { 2 } else { usize::from(path.starts_with('/')) }
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in path[root_len..].split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            step => parts.push(step),
        }
    }
    format!("{}{}", &path[..root_len], parts.join("/"))
}

/// Whether `path` lies strictly under `dir` (both normalized; drive letters compare case-insensitively).
fn inside(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches('/');
    let n = dir.len();
    path.len() > n && path.as_bytes()[..n].eq_ignore_ascii_case(dir.as_bytes()) && path.as_bytes()[n] == b'/'
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
    hidden_command("git")
}

/// Security probes never search the working directory or PATH. This does not authenticate a
/// binary replaced by the same OS user in a supported user installation (ADR-0046).
pub fn installed_program(program: &str) -> Option<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(windows)]
    {
        for root in ["ProgramW6432", "ProgramFiles", "LOCALAPPDATA"] {
            let Some(base) = std::env::var_os(root).map(PathBuf::from) else { continue };
            let base = if root == "LOCALAPPDATA" { base.join("Programs") } else { base };
            let relative = match program { "git" => "Git/cmd/git.exe", "gh" => "GitHub CLI/gh.exe", _ => return None };
            paths.push(base.join(relative));
        }
    }
    #[cfg(not(windows))]
    if matches!(program, "git" | "gh") {
        for dir in ["/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"] { paths.push(Path::new(dir).join(program)); }
    }
    paths.into_iter().find(|path| path.is_absolute() && path.is_file())
}

fn hidden_command(program: &str) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))] // only Windows sets a creation flag
    let mut command = Command::new(installed_program(program).unwrap_or_default());
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
    run_at("git", dir, args)
}

fn run_at(program: &str, dir: &str, args: &[&str]) -> String {
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
    let output = hidden_command(program).args(args).current_dir(&existing).stdin(Stdio::null()).stderr(Stdio::null()).output();
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
    /// A project_O checkout is told by any remote that points at the repository, whatever it is
    /// called, or by the guard's own rule file in HEAD: worktrees share one config, so renaming
    /// or removing `origin` from a personal checkout must not turn the mirrors writable.
    fn is_mirror(&self, dir: &str) -> bool {
        READ_ONLY_BRANCHES.contains(&self.branch(dir).as_str())
            && (git_at(dir, &["config", "--get-regexp", r"^remote\..*\.url$"]).lines().any(|line| line.split_whitespace().nth(1).is_some_and(|url| PROJECT_REMOTE.is_match(url)))
                || git_at(dir, &["cat-file", "-t", "HEAD:crates/o-guard/src/rules.rs"]) == "blob")
    }
    fn pr_base(&self, dir: &str, selector: Option<&str>, repo: Option<&str>) -> Option<String> {
        let mut args = vec!["pr", "view"];
        args.extend(selector);
        if let Some(repo) = repo {
            args.extend(["-R", repo]);
        }
        args.extend(["--json", "baseRefName", "--jq", ".baseRefName"]);
        Some(run_at("gh", dir, &args)).filter(|base| !base.is_empty())
    }
    fn gh_login(&self, dir: &str) -> Option<String> {
        Some(run_at("gh", dir, &["api", "user", "--jq", ".login"])).filter(|login| !login.is_empty())
    }
}

/// Whether `dir` (or the folder of a file path) is a project_O protected-branch mirror.
pub fn is_mirror(dir: &str) -> bool {
    RealGit.is_mirror(dir)
}

fn review_stdin(inside_agent: bool) -> Result<Vec<String>, String> {
    let mut raw = Vec::new();
    std::io::stdin().read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let mut input: Value = serde_json::from_str(&String::from_utf8_lossy(&raw)).map_err(|e| e.to_string())?;
    if !input.is_object() {
        return Err("입력이 JSON 객체가 아닙니다".to_string());
    }
    input["inside_agent"] = Value::Bool(inside_agent);
    Ok(review(&input, &RealGit))
}

/// `o hook`: reads the tool call JSON on stdin, prints reasons, returns the exit code (2 blocks).
pub fn hook_main(inside_agent: bool) -> i32 {
    // Fail closed: a hook that cannot read its input or crashes blocks the call instead of waving it through.
    // (This relies on unwinding panics; the workspace must not set `panic = "abort"`.)
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| review_stdin(inside_agent)));
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
    fn agent_cannot_stop_leads() {
        assert!(AGENT_DISPATCH.is_match("o lead stop"));
        assert!(AGENT_DISPATCH.is_match("o lead stop-all"));
        assert!(AGENT_DISPATCH.is_match("o lead forward 20261109-x-f1"));
        assert!(!AGENT_DISPATCH.is_match("o lead status"));
        assert!(AGENT_DISPATCH.is_match("o _lead-forward 20261107-x 0"));
    }

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
    fn shell_targets_unquote_dedupe_resolve_and_refuse_too_many() {
        assert_eq!(shell_targets(r#"git -C "C:/ws/test" status && cat C:/ws/test"#, "C:/ws/me"), Ok(vec!["C:/ws/test".to_string()]));
        assert_eq!(shell_targets("cd .. && cd other && touch ../x/a", "C:\\ws\\me"), Ok(vec!["C:/ws".into(), "C:/ws/other".into(), "C:/ws/x/a".into()]));
        assert_eq!(shell_targets("touch src/a.rs ./b ../me/c", "C:/ws/me"), Ok(vec![]));
        assert!(shell_targets("ls /a /b /c /d /e /f /g /h /i", "/").is_err());
        assert!(shell_targets("pushd ../x && popd && popd", "/ws/me").is_err());
    }

    #[test]
    fn normalize_drops_dot_steps_and_keeps_roots() {
        assert_eq!(normalize("C:/ws/me/../test_2/./a.txt"), "C:/ws/test_2/a.txt");
        assert_eq!(normalize("C:/../.."), "C:/");
        assert_eq!(normalize("/ws//me/.."), "/ws");
    }
}
