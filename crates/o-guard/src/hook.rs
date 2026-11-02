//! PreToolUse hook for Claude Code and Codex working in this repository.
//! Exit code 2 blocks the tool call and hands the reason back to the agent.
//!
//! It refuses: shell commands that touch provider credentials or disable a
//! CLI's permission checks, code edits that introduce those, commits or
//! pushes to the protected branches (test_1, test_2, main), PR merges into
//! test_1/main by anyone but the owner, branch names outside the
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
    code_rules, find_hits, is_allowed_path, CREDENTIAL_RULES, OWNER, PROJECT_REMOTE, READ_ONLY_BRANCHES, READ_ONLY_COMMANDS,
};

/// Keep in sync with .github/workflows/branch-flow.yml.
pub const PERSONAL_BRANCHES: [&str; 2] = ["younchan", "yongwoo"];

/// The free integration branch: anyone may merge PRs into it.
const FREE_MERGE_BRANCH: &str = "test_2";

static BRANCH_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^({})(-[a-z0-9][a-z0-9._-]*)?$", PERSONAL_BRANCHES.join("|"))).unwrap());
static CREATE_BRANCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\bgit\s+(?:switch\s+(?:-c|-C|--create)|checkout\s+(?:-b|-B)|branch)\s+([^\s-]\S*)").unwrap()
});

static SEGMENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||;|\|").unwrap());
static LINE_SEGMENT_SPLIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&&|\|\||;|\||\n").unwrap());
static GIT_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgit\b").unwrap());
static PUSH_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpush\b").unwrap());
/// A push naming a protected branch as (part of) its refspec: `main`, `HEAD:test_1`, `refs/heads/test_2`.
static PUSH_TO_PROTECTED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?:\s|:)(?:refs/heads/)?({})(?:\s|$)", READ_ONLY_BRANCHES.join("|"))).unwrap());
static PUSH_WITH_TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bpush\s+\S+\s+\S+").unwrap());
static HISTORY_CHANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(commit|merge|cherry-pick|rebase)\b").unwrap());

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
static GH_MERGE_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bgh\s+(?:(pr\s+merge)|api)\b").unwrap());
static API_PR_MERGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?repos/([^/\s]+/[^/\s]+)/pulls/(\d+)/merge/?$").unwrap());
static API_MERGES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^/?repos/[^/\s]+/[^/\s]+/merges/?$").unwrap());
static GRAPHQL_MERGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(mergePullRequest|mergeBranch|enablePullRequestAutoMerge)\b").unwrap());
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
    "project_O의 test_1·test_2·main 작업 폴더는 읽기 전용입니다(풀만 받는 거울). 자기 개인 브랜치 폴더에서 작업하고 PR로 올리세요";

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
        reasons.extend(merge_reasons(&command, &cwd, git));
        // A mirror is read-only whether the command runs in it or names it from elsewhere.
        let targets = shell_targets(&command, &cwd);
        if git.is_mirror(&cwd) || targets.as_ref().is_ok_and(|found| found.iter().any(|p| git.is_mirror(p))) {
            reasons.extend(read_only_shell_reasons(&command));
        } else if let Err(unknown) = &targets {
            if !read_only_shell_reasons(&command).is_empty() {
                reasons.push(format!(
                    "명령이 고칠 수 있는 폴더를 확인하지 못했습니다({unknown}). 거울(test_1·test_2·main 작업 폴더)일 수 있어 읽기 전용 명령만 허용합니다. 경로를 변수 없이 그대로 적으세요"
                ));
            }
        }
        return reasons;
    }

    // Reading a CLI's sign-in files is as off-limits as copying them in a shell.
    if matches!(tool, "Read" | "Grep" | "Glob") {
        return find_hits(&args.to_string(), CREDENTIAL_RULES.iter()).into_iter().map(|hit| hit.reason.to_string()).collect();
    }

    let raw_paths = edited_paths(tool, args);
    for path in &raw_paths {
        // Forward slashes so dirname works the same for Windows and POSIX paths; `..` resolved lexically.
        let absolute = normalize(&if ROOTED.is_match(path) { path.clone() } else { format!("{cwd}/{path}") }.replace('\\', "/"));
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
            if let Some(target) = PUSH_TO_PROTECTED.captures(segment) {
                reasons.push(format!(
                    "{}에는 직접 push할 수 없습니다. PR로만 바뀝니다 (개인·작업 브랜치 → test_2 → test_1 → main)",
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

fn is_protected(branch: &str) -> bool {
    READ_ONLY_BRANCHES.contains(&branch)
}

/// What a merge command merges into: a PR to look up, or a base branch it names itself
/// (`None` when the command does not say).
enum MergeTarget {
    Pr { selector: Option<String>, repo: Option<String> },
    Base(Option<String>),
}

/// Merges into test_1/main are the owner's: `gh pr merge` and gh's direct merge APIs
/// pass only when the base is confirmed free (test_2, a personal or work branch)
/// or the signed-in gh user is the owner. Any lookup that fails blocks.
fn merge_reasons(command: &str, cwd: &str, git: &dyn GitProbe) -> Vec<String> {
    let mut reasons = Vec::new();
    for (is_pr_merge, arguments) in gh_calls(command) {
        let Some(target) = merge_target(is_pr_merge, arguments) else { continue };
        let base = match &target {
            MergeTarget::Pr { selector, repo } => git.pr_base(cwd, selector.as_deref(), repo.as_deref()),
            MergeTarget::Base(base) => base.clone(),
        };
        if base.as_deref().is_some_and(is_free_base) {
            continue;
        }
        // A token on the command line would merge as someone the lookup below never saw.
        let login = if GH_IDENTITY_OVERRIDE.is_match(command) { None } else { git.gh_login(cwd) };
        if login.as_deref() == Some(OWNER) {
            continue;
        }
        let into = base.clone().unwrap_or_else(|| "확인 못 한 브랜치".to_string());
        reasons.push(match (&base, &login) {
            (None, _) if matches!(target, MergeTarget::Pr { .. }) => format!(
                "병합할 PR의 base 브랜치를 확인하지 못했습니다(gh pr view 실패). test_1·main 병합일 수 있어 막습니다. test_1·main 병합은 대표({OWNER})만 합니다"
            ),
            (_, None) => format!(
                "gh 로그인 계정을 확인하지 못해 {into} 병합을 막습니다(gh api user 실패 또는 명령에 토큰 지정). test_1·main 병합은 대표({OWNER})만 합니다"
            ),
            (_, Some(user)) => format!(
                "{into}에는 대표({OWNER})만 병합할 수 있습니다(지금 gh 로그인: {user}). test_2 병합은 누구나 하고, test_2 → test_1 → main은 대표가 병합합니다"
            ),
        });
    }
    reasons
}

/// test_2 and the personal/work branches are free to merge into; anything else needs the owner.
fn is_free_base(base: &str) -> bool {
    base == FREE_MERGE_BRANCH || BRANCH_NAME.is_match(base)
}

/// Each `gh pr merge` (`true`) or `gh api` (`false`) call with its arguments: the text from the call
/// to the first `;`, `&`, `|` or line break (LF, CR) outside quotes. Calls are found anywhere, so a
/// line break or quoting cannot hide one; one inside an `echo` only costs a lookup.
fn gh_calls(command: &str) -> Vec<(bool, &str)> {
    GH_MERGE_CALL
        .captures_iter(command)
        .map(|c| {
            let rest = &command[c.get(0).unwrap().end()..];
            (c.get(1).is_some(), &rest[..unquoted_end(rest)])
        })
        .collect()
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
    let endpoint = positional.first()?;
    if let Some(pr) = API_PR_MERGE.captures(endpoint) {
        let repo = Some(pr[1].to_string()).filter(|r| !r.contains('{'));
        return Some(MergeTarget::Pr { selector: Some(pr[2].to_string()), repo });
    }
    if API_MERGES.is_match(endpoint) {
        let fields = flag_values(&flags, &["-f", "--raw-field", "-F", "--field"]);
        let base = fields.iter().find_map(|f| f.strip_prefix("base=")).map(str::to_string);
        return Some(MergeTarget::Base(base));
    }
    (endpoint == "graphql" && GRAPHQL_MERGE.is_match(arguments)).then_some(MergeTarget::Base(None))
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
fn shell_targets(command: &str, cwd: &str) -> Result<Vec<String>, String> {
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
fn resolve(word: &str, base: &str, home: Option<&str>) -> Option<String> {
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
    let bytes = path.as_bytes();
    let root_len = if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/' {
        3
    } else {
        usize::from(path.starts_with('/'))
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

fn hidden_command(program: &str) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))] // only Windows sets a creation flag
    let mut command = Command::new(program);
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
    fn is_mirror(&self, dir: &str) -> bool {
        READ_ONLY_BRANCHES.contains(&self.branch(dir).as_str())
            && PROJECT_REMOTE.is_match(&git_at(dir, &["remote", "get-url", "origin"]))
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
