//! Port of tests/guard.test.ts. Forbidden strings are split with `concat!` so
//! this file stays clean for any scanner whose allow list predates the Rust paths.

use o_guard::rules::{code_rules, find_hits, is_allowed_path, OWNER, PROJECT_REMOTE, READ_ONLY_BRANCHES};
use o_guard::{review, GitProbe};
use serde_json::{json, Value};

/// A project_O checkout on `name`; protected-branch checkouts are mirrors.
struct OnBranch(&'static str);

impl GitProbe for OnBranch {
    fn branch(&self, _dir: &str) -> String {
        self.0.to_string()
    }
    fn is_mirror(&self, _dir: &str) -> bool {
        READ_ONLY_BRANCHES.contains(&self.0)
    }
}

/// Folders under C:/ws/test_2 are a mirror on test_2; everything else is younchan.
struct ByFolder;

fn in_mirror(dir: &str) -> bool {
    dir.replace('\\', "/").starts_with("C:/ws/test_2")
}

impl GitProbe for ByFolder {
    fn branch(&self, dir: &str) -> String {
        if in_mirror(dir) { "test_2" } else { "younchan" }.to_string()
    }
    fn is_mirror(&self, dir: &str) -> bool {
        in_mirror(dir)
    }
}

struct OtherRepoOnMain;

impl GitProbe for OtherRepoOnMain {
    fn branch(&self, _dir: &str) -> String {
        "main".to_string()
    }
    fn is_mirror(&self, _dir: &str) -> bool {
        false
    }
}

fn bash(command: &str) -> Value {
    json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "C:/repo" })
}

fn blocked(input: &Value, git: &dyn GitProbe) -> bool {
    !review(input, git).is_empty()
}

fn passes(input: &Value, git: &dyn GitProbe) -> bool {
    let reasons = review(input, git);
    assert!(reasons.is_empty(), "{input} → {reasons:?}");
    true
}

const CREDENTIALS_FILE: &str = concat!(".credentials", ".json");
const OAUTH_TOKEN: &str = concat!("CLAUDE_CODE_", "OAUTH_TOKEN");
const CODEX_AUTH: &str = concat!("auth", ".json");
const SKIP_PERMISSIONS: &str = concat!("--dangerously-", "skip-permissions");
const BYPASS_SANDBOX: &str = concat!("--dangerously-", "bypass-approvals-and-sandbox");

#[test]
fn code_that_touches_credentials_backends_or_bypass_flags_is_found() {
    let samples = [
        format!("readFileSync(join(home, '.claude', '{CREDENTIALS_FILE}'))"),
        format!("env.{OAUTH_TOKEN} = token"),
        format!("const p = join(codexHome, '{CODEX_AUTH}')"),
        format!("fetch('https://{}/v1/messages')", concat!("api.anthropic", ".com")),
        format!("fetch('https://{}/codex/responses')", concat!("chatgpt.com", "/backend-api")),
        format!("url = 'https://{}/v1internal'", concat!("cloudcode-pa", ".googleapis.com")),
        format!("args.push('{SKIP_PERMISSIONS}')"),
    ];
    for sample in &samples {
        assert!(!find_hits(sample, code_rules()).is_empty(), "{sample}");
    }
    assert!(find_hits("spawn('claude', ['-p', '--output-format', 'stream-json'])", code_rules()).is_empty());
}

#[test]
fn only_the_rule_file_its_tests_and_docs_may_spell_the_patterns() {
    assert!(is_allowed_path("crates/o-guard/src/rules.rs"));
    assert!(is_allowed_path("crates/o-guard/tests/guard.rs"));
    assert!(is_allowed_path("crates/o-core/tests/policy.rs"));
    assert!(is_allowed_path("docs\\DESIGN.md"));
    assert!(!is_allowed_path("crates/o-core/src/providers.rs"));
    assert!(!is_allowed_path("crates/o-guard/src/hook.rs"));
}

#[test]
fn hook_blocks_shell_access_to_credential_files() {
    assert!(blocked(&bash(&format!("cat ~/.claude/{CREDENTIALS_FILE}")), &OnBranch("a/x")));
    assert!(blocked(&bash(&format!("codex exec {BYPASS_SANDBOX} hi")), &OnBranch("a/x")));
    assert!(passes(&bash("node src/cli/o.ts status"), &OnBranch("a/x")));
    assert!(passes(&bash("o status"), &OnBranch("a/x")));
}

#[test]
fn hook_enforces_the_branch_flow() {
    assert!(blocked(&bash("git push origin main"), &OnBranch("a/x")));
    assert!(blocked(&bash("git push origin HEAD:main"), &OnBranch("a/x")));
    assert!(blocked(&bash("git commit -m wip"), &OnBranch("main")));
    assert!(blocked(&bash("git push --force origin test_2"), &OnBranch("test_2")));
    assert!(blocked(&bash("git push origin test_2"), &OnBranch("younchan")));
    assert!(blocked(&bash("git push origin younchan:test_1"), &OnBranch("younchan")));
    assert!(passes(&bash("git push origin younchan"), &OnBranch("younchan")));
    assert!(passes(&bash("git commit -m wip && git push origin younchan-o-cli"), &OnBranch("younchan-o-cli")));
}

#[test]
fn hook_enforces_branch_names() {
    assert!(blocked(&bash("git switch -c feature-x"), &OnBranch("younchan")));
    assert!(blocked(&bash("git checkout -b younchan/x"), &OnBranch("younchan")));
    assert!(blocked(&bash("git switch -c a/x"), &OnBranch("younchan")));
    assert!(passes(&bash("git switch -c yongwoo-account-vault"), &OnBranch("yongwoo")));
    assert!(passes(&bash("git switch -c younchan"), &OnBranch("yongwoo")));
    assert!(passes(&bash("git branch -d younchan-old"), &OnBranch("younchan")));
}

#[test]
fn protected_branch_checkouts_are_read_only_for_agents() {
    let mirror = |command: &str| json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "C:/ws/test_2" });
    let allowed = [
        "git status",
        "git pull --ff-only",
        "git log --oneline -5",
        "npm test",
        "node --test tests/x.test.ts",
        "cargo test -p o-guard",
        "o status",
        "git diff 2>&1 | head -20",
        "ls > /dev/null",
    ];
    for ok in allowed {
        assert!(passes(&mirror(ok), &OnBranch("test_2")), "{ok}");
    }
    let sneaky = ["ls & rm -rf src", "echo $(rm -rf src)", "echo `rm -rf src`", "rg --pre=calc x", "git grep -Ocalc x", "git diff --output=f", "grafly build"];
    let refused = ["git commit -m x", "git pull", "git checkout younchan", "npm install left-pad", "echo hi > notes.md", "rm -rf src", "git status && touch x"];
    for bad in refused.iter().chain(sneaky.iter()) {
        assert!(blocked(&mirror(bad), &OnBranch("test_2")), "{bad}");
    }
    assert!(blocked(&mirror("sed -i s/a/b/ README.md"), &OnBranch("main")));
    assert!(passes(&mirror("sed -i s/a/b/ README.md"), &OnBranch("younchan")));

    // An edit is judged by the branch of the folder the file lives in, wherever the session runs.
    let write = |file_path: &str| json!({ "tool_name": "Edit", "tool_input": { "file_path": file_path, "new_string": "x" }, "cwd": "C:/ws/younchan" });
    assert!(blocked(&write("C:/ws/test_2/src/a.ts"), &ByFolder));
    assert!(blocked(&write("C:\\ws\\test_2\\new\\dir\\b.ts"), &ByFolder));
    assert!(passes(&write("C:/ws/younchan/src/a.ts"), &ByFolder));
    let notebook = json!({ "tool_name": "NotebookEdit", "tool_input": { "notebook_path": "C:/ws/test_2/n.ipynb", "new_source": "x" }, "cwd": "C:/ws/younchan" });
    assert!(blocked(&notebook, &ByFolder));

    // Naming the mirror from another folder is the same as working in it.
    let elsewhere = |command: &str| json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "C:/ws/younchan" });
    assert!(blocked(&elsewhere("git -C C:/ws/test_2 commit -m x"), &ByFolder));
    assert!(blocked(&elsewhere("git -C \"C:/ws/test_2\" commit -m x"), &ByFolder));
    assert!(blocked(&elsewhere("rm C:/ws/test_2/src/a.ts"), &ByFolder));
    assert!(passes(&elsewhere("git -C C:/ws/test_2 status"), &ByFolder));
}

/// Relative targets are resolved against the folder the command runs in, following `cd`;
/// a move or path the hook cannot resolve allows only read-only commands.
#[test]
fn relative_shell_targets_reach_the_mirror_too() {
    let personal = |command: &str| json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "C:/ws/younchan" });
    let refused = [
        "echo x > ../test_2/a.txt",
        "echo x >../test_2/a.txt",
        "touch ..\\test_2\\a.txt",
        "cp a.txt ../test_2/",
        "cd ../test_2 && touch a.txt",
        "cd ..\\test_2; Remove-Item a.txt",
        "Set-Location -Path ../test_2\nRemove-Item a.txt",
        "cd src && echo x > ../../test_2/a.txt",
        "git -C ../test_2 commit -m x",
        "npm install --prefix=../test_2",
        // Unresolvable moves or paths: only read-only commands may follow.
        "cd $o_guard_unset_dir && touch a.txt",
        "touch $o_guard_unset_dir/a.txt",
        "cd - && touch a.txt",
        "cd $(git rev-parse --show-toplevel) && touch a.txt",
        // More folders than the hook asks git about.
        "touch C:/x/1 C:/x/2 C:/x/3 C:/x/4 C:/x/5 C:/x/6 C:/x/7 C:/x/8 C:/ws/test_2/a.txt",
    ];
    for bad in refused {
        assert!(blocked(&personal(bad), &ByFolder), "{bad:?}");
    }
    let allowed = [
        "cd ../test_2 && git status",
        "cat ../test_2/README.md",
        "git -C ../test_2 log -1",
        "touch ../younchan-other/a.txt",
        "echo x > out/a.txt",
        "cd src && cargo test",
        "cd ../younchan-other && touch a.txt",
        "git diff origin/test_2..HEAD -- crates/o-guard/src/hook.rs",
        "cd $o_guard_unset_dir && git status",
        "ls C:/x/1 C:/x/2 C:/x/3 C:/x/4 C:/x/5 C:/x/6 C:/x/7 C:/x/8 C:/x/9",
    ];
    for ok in allowed {
        assert!(passes(&personal(ok), &ByFolder), "{ok:?}");
    }
}

/// A patch that moves a file is judged by both ends of the move.
#[test]
fn patch_moves_are_checked_at_source_and_destination() {
    let patch = |from: &str, to: &str, line: &str| {
        let body = format!("*** Begin Patch\n*** Update File: {from}\n*** Move to: {to}\n@@\n-old\n+{line}\n*** End Patch");
        json!({ "tool_name": "apply_patch", "tool_input": { "command": body }, "cwd": "C:/ws/younchan" })
    };
    assert!(blocked(&patch("src/a.rs", "C:/ws/test_2/src/a.rs", "x"), &ByFolder));
    assert!(blocked(&patch("src/a.rs", "../test_2/src/a.rs", "x"), &ByFolder));
    assert!(blocked(&patch("C:/ws/test_2/src/a.rs", "src/a.rs", "x"), &ByFolder));
    assert!(passes(&patch("src/a.rs", "src/b.rs", "x"), &ByFolder));
    // Code only the rule file and its tests may spell, moved out of the allowed folder.
    let bad = format!("let t = \"{OAUTH_TOKEN}\";");
    assert!(blocked(&patch("crates/o-guard/tests/x.rs", "crates/o-core/src/x.rs", &bad), &ByFolder));
    assert!(passes(&patch("crates/o-guard/tests/x.rs", "crates/o-guard/tests/y.rs", &bad), &ByFolder));
}

#[test]
fn sign_in_files_are_off_limits_to_read_tools_too() {
    let read = |file_path: String| json!({ "tool_name": "Read", "tool_input": { "file_path": file_path } });
    assert!(blocked(&read(format!("C:/Users/me/.codex/{CODEX_AUTH}")), &OnBranch("younchan")));
    assert!(passes(&read("C:/repo/README.md".to_string()), &OnBranch("younchan")));
    let grep = json!({ "tool_name": "Grep", "tool_input": { "pattern": "x", "glob": format!("**/{CREDENTIALS_FILE}") } });
    assert!(blocked(&grep, &OnBranch("younchan")));
}

#[test]
fn another_repository_on_main_is_not_a_mirror() {
    let edit = json!({ "tool_name": "Edit", "tool_input": { "file_path": "C:/other/app/index.html", "new_string": "x" }, "cwd": "C:/ws/younchan" });
    assert!(passes(&edit, &OtherRepoOnMain));
    assert!(PROJECT_REMOTE.is_match("https://github.com/chan12392/project_O.git"));
    assert!(PROJECT_REMOTE.is_match("git@github.com:chan12392/project_O"));
    assert!(!PROJECT_REMOTE.is_match("https://github.com/someone/project_Other.git"));
    assert!(!PROJECT_REMOTE.is_match(""));
}

#[test]
fn hook_blocks_edits_that_introduce_forbidden_code_outside_allowed_files() {
    let edit = |file_path: &str, content: &str| json!({ "tool_name": "Write", "tool_input": { "file_path": file_path, "content": content }, "cwd": "C:/repo" });
    let bad = format!("let t = std::env::var(\"{OAUTH_TOKEN}\")");
    assert!(blocked(&edit("C:/repo/crates/o-core/src/x.rs", &bad), &OnBranch("a/x")));
    assert!(passes(&edit("C:/repo/crates/o-guard/src/rules.rs", &bad), &OnBranch("a/x")));
    assert!(passes(&edit("C:\\repo\\crates\\o-guard\\tests\\guard.rs", &bad), &OnBranch("a/x")));
    assert!(passes(&edit("C:/repo/crates/o-core/src/x.rs", "pub const OK: u8 = 1;"), &OnBranch("a/x")));

    let patch = format!(
        "*** Begin Patch\n*** Update File: crates/o-core/src/x.rs\n+fetch('https://{}/v1/responses')\n*** End Patch",
        concat!("api.openai", ".com")
    );
    assert!(blocked(&json!({ "tool_name": "apply_patch", "tool_input": { "command": patch }, "cwd": "C:/repo" }), &OnBranch("a/x")));
}

/// ADR-0014: the YOLO launch flags live in the provider files (and their test); everywhere
/// else — another file, a shell command, a patch that also touches another file — they stay blocked.
#[test]
fn only_provider_files_may_introduce_bypass_switches() {
    let edit = |file_path: &str, content: &str| json!({ "tool_name": "Edit", "tool_input": { "file_path": file_path, "new_string": content }, "cwd": "C:/repo" });
    let flag = format!("args.push(\"{BYPASS_SANDBOX}\".into());");
    assert!(passes(&edit("C:/repo/crates/o-core/src/providers/codex.rs", &flag), &OnBranch("younchan")));
    assert!(passes(&edit("C:/repo/crates/o-core/tests/providers.rs", &flag), &OnBranch("younchan")));
    assert!(blocked(&edit("C:/repo/crates/o-core/src/lead/mod.rs", &flag), &OnBranch("younchan")));
    assert!(blocked(&edit("C:/repo/app/src/features/chat/Composer.tsx", &flag), &OnBranch("younchan")));
    // A provider file is exempt from the bypass rule only.
    assert!(blocked(&edit("C:/repo/crates/o-core/src/providers/claude.rs", &format!("env::var(\"{OAUTH_TOKEN}\")")), &OnBranch("younchan")));
    // A command that turns the checks off is blocked wherever it runs.
    assert!(blocked(&bash(&format!("claude -p {SKIP_PERMISSIONS} hi")), &OnBranch("younchan")));
    let both = format!("*** Begin Patch\n*** Update File: crates/o-core/src/providers/agy.rs\n+{flag}\n*** Update File: crates/o-core/src/policy.rs\n+x\n*** End Patch");
    assert!(blocked(&json!({ "tool_name": "apply_patch", "tool_input": { "command": both }, "cwd": "C:/repo" }), &OnBranch("younchan")));
    let one = format!("*** Begin Patch\n*** Update File: crates/o-core/src/providers/agy.rs\n+{flag}\n*** End Patch");
    assert!(passes(&json!({ "tool_name": "apply_patch", "tool_input": { "command": one }, "cwd": "C:/repo" }), &OnBranch("younchan")));
}

#[test]
fn reasons_carry_the_korean_wording() {
    let reasons = review(&bash("git push origin main"), &OnBranch("younchan"));
    assert_eq!(reasons, vec!["main에는 직접 push할 수 없습니다. PR로만 바뀝니다 (개인·작업 브랜치 → test_2 → test_1 → main)".to_string()]);
    let edit = json!({ "tool_name": "Edit", "tool_input": { "file_path": "C:/ws/test_2/a.rs", "new_string": "x" }, "cwd": "C:/ws/younchan" });
    let reasons = review(&edit, &ByFolder);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].starts_with("project_O의 test_1·test_2·main 작업 폴더는 읽기 전용입니다") && reasons[0].ends_with("(C:/ws/test_2/a.rs)"));
}

/// A personal checkout whose gh knows PRs 1 (→ test_1), 2 (→ test_2), 3 (→ main),
/// 4 (→ younchan) and the current branch's PR (→ test_2), signed in as `login`.
/// `-R` other than this repo, or an unknown PR, fails the lookup.
struct Gh {
    login: Option<&'static str>,
}

impl GitProbe for Gh {
    fn branch(&self, _dir: &str) -> String {
        "younchan".to_string()
    }
    fn is_mirror(&self, _dir: &str) -> bool {
        false
    }
    fn pr_base(&self, _dir: &str, selector: Option<&str>, repo: Option<&str>) -> Option<String> {
        if repo.is_some_and(|r| r != "chan12392/project_O") {
            return None;
        }
        let base = match selector {
            None => "test_2",
            Some("1" | "https://github.com/chan12392/project_O/pull/1" | "test_2") => "test_1",
            Some("2" | "younchan") => "test_2",
            Some("3" | "test_1") => "main",
            Some("4") => "younchan",
            _ => return None,
        };
        Some(base.to_string())
    }
    fn gh_login(&self, _dir: &str) -> Option<String> {
        self.login.map(str::to_string)
    }
}

const OWNER_GH: Gh = Gh { login: Some(OWNER) };
const MEMBER_GH: Gh = Gh { login: Some("someone-else") };
const NO_LOGIN_GH: Gh = Gh { login: None };

#[test]
fn every_protected_branch_refuses_direct_pushes_and_commits() {
    for branch in READ_ONLY_BRANCHES {
        assert!(blocked(&bash(&format!("git push origin {branch}")), &OnBranch("younchan")), "{branch}");
        assert!(blocked(&bash(&format!("git push origin HEAD:refs/heads/{branch}")), &OnBranch("younchan")), "{branch}");
        assert!(blocked(&bash("git push"), &OnBranch(branch)), "{branch}");
        assert!(blocked(&bash("git commit -m x"), &OnBranch(branch)), "{branch}");
        assert!(blocked(&bash("git merge younchan"), &OnBranch(branch)), "{branch}");
    }
    assert!(passes(&bash("git push -u origin younchan-team-rules"), &OnBranch("younchan-team-rules")));
    assert!(passes(&bash("git push origin younchan-main-base"), &OnBranch("younchan")));
    assert!(passes(&bash("git commit -m x"), &OnBranch("younchan-main-base")));
}

#[test]
fn only_the_owner_merges_prs_into_test_1_and_main() {
    for into_protected in ["gh pr merge 1", "gh pr merge 3 --merge", "gh pr merge https://github.com/chan12392/project_O/pull/1", "gh pr merge test_2 --merge"] {
        assert!(passes(&bash(into_protected), &OWNER_GH), "{into_protected}");
        let reasons = review(&bash(into_protected), &MEMBER_GH);
        assert_eq!(reasons.len(), 1, "{into_protected}");
        assert!(reasons[0].contains("chan12392") && reasons[0].contains("someone-else"), "{}", reasons[0]);
    }
    let reasons = review(&bash("gh pr merge 3"), &MEMBER_GH);
    assert!(reasons[0].starts_with("main에는 대표(chan12392)만 병합할 수 있습니다"), "{}", reasons[0]);
    // Flags with values before the PR, and -R with the PR's repository.
    assert!(blocked(&bash("gh pr merge --subject \"release 1\" -R chan12392/project_O 1"), &MEMBER_GH));
    assert!(passes(&bash("gh pr merge --subject 'release 1' -R chan12392/project_O 2"), &MEMBER_GH));
}

#[test]
fn anyone_merges_into_test_2_and_personal_branches() {
    for free in ["gh pr merge 2 --merge", "gh pr merge younchan", "gh pr merge 4 -d", "gh pr merge", "gh pr merge --merge --delete-branch"] {
        assert!(passes(&bash(free), &MEMBER_GH), "{free}");
        assert!(passes(&bash(free), &NO_LOGIN_GH), "{free}");
    }
}

#[test]
fn merge_lookups_that_fail_block() {
    // Unknown PR or another repository: the base cannot be confirmed.
    assert!(blocked(&bash("gh pr merge 99"), &MEMBER_GH));
    assert!(blocked(&bash("gh pr merge -R someone/other 2"), &MEMBER_GH));
    // Base is test_1 but who is signed in cannot be told.
    let reasons = review(&bash("gh pr merge 1"), &NO_LOGIN_GH);
    assert!(reasons.len() == 1 && reasons[0].starts_with("gh 로그인 계정을 확인하지 못해 test_1 병합을 막습니다"), "{reasons:?}");
    // A token on the command line is someone the login lookup never saw.
    assert!(blocked(&bash("GH_TOKEN=x gh pr merge 1"), &OWNER_GH));
    // The default probe knows no gh at all: fail closed, even for the owner's own repo.
    assert!(blocked(&bash("gh pr merge 2"), &OnBranch("younchan")));
}

#[test]
fn direct_merge_apis_follow_the_same_rule() {
    let pr_merge = "gh api -X PUT repos/chan12392/project_O/pulls/1/merge";
    assert!(blocked(&bash(pr_merge), &MEMBER_GH));
    assert!(passes(&bash(pr_merge), &OWNER_GH));
    assert!(passes(&bash("gh api --method PUT repos/{owner}/{repo}/pulls/2/merge"), &MEMBER_GH));
    assert!(blocked(&bash("gh api repos/chan12392/project_O/merges -f base=main -f head=test_1"), &MEMBER_GH));
    assert!(blocked(&bash("gh api repos/chan12392/project_O/merges -f head=test_2"), &MEMBER_GH));
    assert!(passes(&bash("gh api repos/chan12392/project_O/merges -f base=test_2 -f head=younchan"), &MEMBER_GH));
    assert!(passes(&bash("gh api repos/chan12392/project_O/merges -f base=main -f head=test_1"), &OWNER_GH));
    assert!(blocked(&bash("gh api graphql -f query='mutation { mergePullRequest(input: {pullRequestId: \"x\"}) { clientMutationId } }'"), &MEMBER_GH));
    assert!(passes(&bash("gh api repos/chan12392/project_O/pulls/1"), &MEMBER_GH));
    assert!(passes(&bash("gh pr view 1 --json baseRefName"), &MEMBER_GH));
}

/// A line break (LF or CRLF), a trailing command or a quoted separator must not hide a merge from the owner check.
#[test]
fn merge_checks_see_every_line_of_a_command() {
    let into_protected = [
        "gh pr merge 1\necho x",
        "gh pr merge 1\r\necho x",
        "gh pr merge 1 --merge\ngit log -1",
        "gh pr merge --subject \"a; b\" 1",
        "gh pr merge 1 --body \"first line\nsecond line\"",
        "gh api -X PUT repos/chan12392/project_O/pulls/1/merge\necho x",
        "gh api -X PUT repos/chan12392/project_O/pulls/1/merge\r\necho x",
        "echo start\r\ngh api repos/chan12392/project_O/merges -f base=main -f head=test_1\r\nexit 0",
        "gh api graphql -f query='mutation {\n  mergePullRequest(input: {pullRequestId: \"x\"}) { clientMutationId }\n}'",
    ];
    for command in into_protected {
        assert!(passes(&bash(command), &OWNER_GH), "{command:?}");
        let reasons = review(&bash(command), &MEMBER_GH);
        assert_eq!(reasons.len(), 1, "{command:?} → {reasons:?}");
        assert!(reasons[0].contains("someone-else"), "{command:?} → {reasons:?}");
    }
    for free in ["gh pr merge 2\necho x", "gh pr merge 4 -d\r\necho x", "echo x\ngh api --method PUT repos/{owner}/{repo}/pulls/2/merge\necho y"] {
        assert!(passes(&bash(free), &MEMBER_GH), "{free:?}");
    }
}

fn project_hooks(path: &str) -> Vec<Value> {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
    let config: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    config["hooks"]["PreToolUse"].as_array().unwrap().iter().flat_map(|m| m["hooks"].as_array().unwrap().clone()).collect()
}

/// A hook that cannot run (no `o` on PATH, a crash, a timeout) must block, not wave the call through:
/// Claude Code lets any failure other than exit 2 pass unless the hook sets `onFailure: "block"`.
#[test]
fn claude_project_hook_blocks_when_it_fails() {
    let hooks = project_hooks(".claude/settings.json");
    assert!(!hooks.is_empty());
    for hook in hooks {
        assert_eq!(hook["command"], "o hook");
        assert_eq!(hook["onFailure"], "block", "{hook}");
    }
}

/// Codex has no fail-closed switch and runs hooks with `cmd.exe /C` (Windows) or `sh -lc`, so the
/// command turns a missing `o` (or any error) into exit 2, Codex's blocking code. Run as Codex does,
/// with a PATH where `o` cannot be found.
#[test]
fn codex_project_hook_blocks_when_o_is_missing() {
    let hooks = project_hooks(".codex/hooks.json");
    assert!(!hooks.is_empty());
    let empty = std::env::temp_dir().join(format!("o-guard-no-o-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    for hook in hooks {
        let line = hook["command"].as_str().unwrap();
        assert_eq!(line, "o hook || exit 2");
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::CommandExt;
            let shell = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
            std::process::Command::new(shell).arg("/C").raw_arg(format!("\"{line}\"")).env("PATH", &empty).status().unwrap()
        };
        #[cfg(not(windows))]
        let status = std::process::Command::new("/bin/sh").arg("-c").arg(line).env("PATH", &empty).status().unwrap();
        assert_eq!(status.code(), Some(2), "{line}");
    }
    std::fs::remove_dir(&empty).ok();
}
