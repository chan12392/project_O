//! Port of tests/guard.test.ts. Forbidden strings are split with `concat!` so
//! this file stays clean for any scanner whose allow list predates the Rust paths.

use o_guard::rules::{code_rules, find_hits, is_allowed_path, PROJECT_REMOTE};
use o_guard::{review, GitProbe};
use serde_json::{json, Value};

/// A project_O checkout on `name`; test/main checkouts are mirrors.
struct OnBranch(&'static str);

impl GitProbe for OnBranch {
    fn branch(&self, _dir: &str) -> String {
        self.0.to_string()
    }
    fn is_mirror(&self, _dir: &str) -> bool {
        self.0 == "test" || self.0 == "main"
    }
}

/// Folders under C:/ws/test are a mirror on test; everything else is younchan.
struct ByFolder;

fn in_mirror(dir: &str) -> bool {
    dir.replace('\\', "/").starts_with("C:/ws/test")
}

impl GitProbe for ByFolder {
    fn branch(&self, dir: &str) -> String {
        if in_mirror(dir) { "test" } else { "younchan" }.to_string()
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
    assert!(blocked(&bash("git push --force origin test"), &OnBranch("test")));
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
fn test_and_main_checkouts_are_read_only_for_agents() {
    let mirror = |command: &str| json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "C:/ws/test" });
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
        assert!(passes(&mirror(ok), &OnBranch("test")), "{ok}");
    }
    let sneaky = ["ls & rm -rf src", "echo $(rm -rf src)", "echo `rm -rf src`", "rg --pre=calc x", "git grep -Ocalc x", "git diff --output=f", "grafly build"];
    let refused = ["git commit -m x", "git pull", "git checkout younchan", "npm install left-pad", "echo hi > notes.md", "rm -rf src", "git status && touch x"];
    for bad in refused.iter().chain(sneaky.iter()) {
        assert!(blocked(&mirror(bad), &OnBranch("test")), "{bad}");
    }
    assert!(blocked(&mirror("sed -i s/a/b/ README.md"), &OnBranch("main")));
    assert!(passes(&mirror("sed -i s/a/b/ README.md"), &OnBranch("younchan")));

    // An edit is judged by the branch of the folder the file lives in, wherever the session runs.
    let write = |file_path: &str| json!({ "tool_name": "Edit", "tool_input": { "file_path": file_path, "new_string": "x" }, "cwd": "C:/ws/younchan" });
    assert!(blocked(&write("C:/ws/test/src/a.ts"), &ByFolder));
    assert!(blocked(&write("C:\\ws\\test\\new\\dir\\b.ts"), &ByFolder));
    assert!(passes(&write("C:/ws/younchan/src/a.ts"), &ByFolder));
    let notebook = json!({ "tool_name": "NotebookEdit", "tool_input": { "notebook_path": "C:/ws/test/n.ipynb", "new_source": "x" }, "cwd": "C:/ws/younchan" });
    assert!(blocked(&notebook, &ByFolder));

    // Naming the mirror from another folder is the same as working in it.
    let elsewhere = |command: &str| json!({ "tool_name": "Bash", "tool_input": { "command": command }, "cwd": "C:/ws/younchan" });
    assert!(blocked(&elsewhere("git -C C:/ws/test commit -m x"), &ByFolder));
    assert!(blocked(&elsewhere("git -C \"C:/ws/test\" commit -m x"), &ByFolder));
    assert!(blocked(&elsewhere("rm C:/ws/test/src/a.ts"), &ByFolder));
    assert!(passes(&elsewhere("git -C C:/ws/test status"), &ByFolder));
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

#[test]
fn reasons_carry_the_korean_wording() {
    let reasons = review(&bash("git push origin main"), &OnBranch("younchan"));
    assert_eq!(reasons, vec!["main에는 직접 push할 수 없습니다. test → main PR로 병합하세요".to_string()]);
    let edit = json!({ "tool_name": "Edit", "tool_input": { "file_path": "C:/ws/test/a.rs", "new_string": "x" }, "cwd": "C:/ws/younchan" });
    let reasons = review(&edit, &ByFolder);
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].starts_with("project_O의 test·main 작업 폴더는 읽기 전용입니다") && reasons[0].ends_with("(C:/ws/test/a.rs)"));
}
