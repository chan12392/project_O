//! Provider-policy patterns shared by the static scanner, the agent hooks and
//! the dispatch policy. This file is the only place allowed to spell them out.
//!
//! The line we hold (see docs/DESIGN.md §8): every model call goes through an
//! unmodified official CLI that signs itself in. Our code never touches a
//! provider's credentials, never calls a provider backend directly and never
//! turns off a CLI's own permission checks.

use std::sync::LazyLock;

use regex::Regex;

pub struct Rule {
    pub id: &'static str,
    pub pattern: Regex,
    pub reason: &'static str,
}

fn rule(id: &'static str, pattern: &str, reason: &'static str) -> Rule {
    Rule { id, pattern: Regex::new(pattern).expect("rule pattern"), reason }
}

/// Credential stores and tokens that belong to the official CLIs.
pub static CREDENTIAL_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule("claude-credentials", r"(?i)\.credentials\.json", "Claude Code 로그인 정보 파일에 접근할 수 없습니다"),
        rule("claude-oauth-token", r"CLAUDE_CODE_OAUTH_TOKEN|sk-ant-oat\d*-|\bsetup-token\b", "Claude 구독 OAuth 토큰을 다룰 수 없습니다"),
        rule("codex-auth", r"(?i)\bauth\.json\b", "Codex 로그인 정보 파일(auth.json)에 접근할 수 없습니다"),
        rule("gemini-oauth", r"(?i)oauth_creds\.json|google_accounts\.json", "Gemini/Antigravity 로그인 정보 파일에 접근할 수 없습니다"),
    ]
});

/// Provider backends. Talking to them ourselves is what the terms forbid.
static BACKEND_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![
        rule("anthropic-api", r"(?i)api\.anthropic\.com|anthropic-beta[^\n]{0,40}oauth", "Anthropic 서버를 직접 호출할 수 없습니다 — claude CLI를 거치세요"),
        rule("openai-backend", r"(?i)chatgpt\.com/backend-api|api\.openai\.com", "OpenAI 서버를 직접 호출할 수 없습니다 — codex CLI를 거치세요"),
        rule(
            "google-backend",
            r"(?i)cloudcode-pa\.googleapis\.com|generativelanguage\.googleapis\.com|daily-cloudcode-pa",
            "Google 모델 서버를 직접 호출할 수 없습니다 — agy CLI를 거치세요",
        ),
    ]
});

/// Switches that disable a CLI's own permission checks.
static BYPASS_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    vec![rule(
        "permission-bypass",
        r"dangerously-skip-permissions|dangerously-bypass-approvals-and-sandbox|bypassPermissions|--yolo\b",
        "CLI의 권한 확인을 끄는 옵션은 쓸 수 없습니다",
    )]
});

/// Every rule code and plans are checked against.
pub fn code_rules() -> impl Iterator<Item = &'static Rule> {
    CREDENTIAL_RULES.iter().chain(BACKEND_RULES.iter()).chain(BYPASS_RULES.iter())
}

/// Protected branches: they only change by merging PRs (no direct commits or
/// pushes), and checkouts on them are mirrors agents may read and test in,
/// never change. Keep in sync with .github/workflows/branch-flow.yml and
/// crates/o-core/src/mirror.rs.
pub const READ_ONLY_BRANCHES: [&str; 3] = ["test_1", "test_2", "main"];

/// The owner's GitHub login (member A): the only account that may merge into
/// `test_1` and `main`; his agents run under the same `gh` login. Shared
/// boundary — keep in sync with .github/workflows/merge-guard.yml.
pub const OWNER: &str = "chan12392";

/// Mirrors are project_O checkouts only; other repositories on main are ordinary work.
pub static PROJECT_REMOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)[/:]project_O(\.git)?/?$").unwrap());

/// Shell commands an agent may run inside a read-only checkout.
pub static READ_ONLY_COMMANDS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"^git\s+(status|log|diff|show|fetch|rev-parse|ls-files|grep|blame|shortlog|describe)\b",
        r"^git\s+(remote(\s+-v)?|worktree\s+list|pull\s+--ff-only)\s*$",
        r"^git\s+branch(\s+(-a|-r|-v|-vv|--list|--show-current))*\s*$",
        r"^(ls|dir|pwd|cd|cat|type|head|tail|wc|grep|rg|echo|which|where|Get-ChildItem|Get-Content|Select-String|Get-Location)\b",
        r"^node\s+--test\b",
        r"^npm\s+(test|ci|run\s+(test|check))\s*$",
        r"^npx\s+tsc\s+--noEmit(\s+-p\s+app)?\s*$",
        r"^cargo\s+(test|check|clippy)\b",
        r"^o\s+(status|events|check|scan)\b",
        r"^grafly\s+(ask|grep|skeleton|callers|map)\b",
        r"^gh\s+(pr|run)\s+(view|list|checks|diff)\b",
    ]
    .iter()
    .map(|p| Regex::new(p).unwrap())
    .collect()
});

/// Paths that must mention the patterns to do their job. Changing this list is
/// a shared-boundary change (ADR).
static ALLOWED_PATHS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [r"^crates/o-guard/src/rules\.rs$", r"^crates/o-guard/tests/", r"^crates/o-core/tests/policy\.rs$", r"^docs/"]
        .iter()
        .map(|p| Regex::new(p).unwrap())
        .collect()
});

pub struct Hit {
    pub rule: &'static str,
    pub reason: &'static str,
    pub line: usize,
}

/// The first match of each rule, with its 1-based line.
pub fn find_hits<'a>(text: &str, rules: impl IntoIterator<Item = &'a Rule>) -> Vec<Hit> {
    rules
        .into_iter()
        .filter_map(|rule| {
            rule.pattern.find(text).map(|m| Hit { rule: rule.id, reason: rule.reason, line: text[..m.start()].matches('\n').count() + 1 })
        })
        .collect()
}

pub fn is_allowed_path(repo_relative: &str) -> bool {
    let normalized = repo_relative.replace('\\', "/");
    let normalized = normalized.strip_prefix("./").unwrap_or(&normalized);
    ALLOWED_PATHS.iter().any(|p| p.is_match(normalized))
}
