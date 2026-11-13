//! Static policy scan, run by git hooks and CI.
//!
//!   --all       every tracked file (CI)
//!   --staged    staged content, and no commits on a protected branch (pre-commit)
//!   --pre-push  refuses pushes that target a protected branch (pre-push; refs on stdin)

use std::io::{self, Read};
use std::process::Stdio;

use crate::hook::git_command;
use crate::rules::{find_hits, is_allowed_path, rules_for_paths, READ_ONLY_BRANCHES};

/// Runs git and returns its stdout. stderr goes to ours; output size is unbounded.
/// Paths are not quoted, so non-ASCII file names are scanned rather than missed.
fn git(args: &[&str]) -> io::Result<String> {
    let output = git_command()
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("git {} 실패 ({})", args.join(" "), output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_bytes(args: &[&str]) -> io::Result<Vec<u8>> {
    let output = git_command().args(args).stdin(Stdio::null()).stderr(Stdio::inherit()).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("git {} 실패 ({})", args.join(" "), output.status)));
    }
    Ok(output.stdout)
}

fn lines(text: &str) -> Vec<String> {
    text.split('\n').filter(|l| !l.is_empty()).map(str::to_string).collect()
}

fn scan_files(files: &[String], read: impl Fn(&str) -> io::Result<Vec<u8>>) -> io::Result<Vec<String>> {
    let mut problems = Vec::new();
    for file in files {
        if is_allowed_path(file) {
            continue;
        }
        let bytes = match read(file) {
            Ok(bytes) => bytes,
            // A tracked file deleted from the working tree has nothing to scan; any other failure must not pass silently.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(io::Error::new(error.kind(), format!("{file}: {error}"))),
        };
        // A NUL byte does not make a file exempt: it is read with the NULs dropped, which is
        // also how UTF-16 text spells the same patterns.
        let text = if bytes.contains(&0) { String::from_utf8_lossy(&bytes.iter().copied().filter(|b| *b != 0).collect::<Vec<u8>>()).into_owned() } else { String::from_utf8_lossy(&bytes).into_owned() };
        for hit in find_hits(&text, rules_for_paths(std::slice::from_ref(file))) {
            problems.push(format!("{file}:{} [{}] {}", hit.line, hit.rule, hit.reason));
        }
    }
    Ok(problems)
}

fn fail(title: &str, lines: &[String]) -> i32 {
    eprintln!("차단됨 — {title}");
    for line in lines {
        eprintln!("  {line}");
    }
    eprintln!("규칙: AGENTS.md · docs/DESIGN.md §8");
    1
}

/// The pre-push lines (`<local ref> <local sha> <remote ref> <remote sha>`) whose remote ref is a protected branch (READ_ONLY_BRANCHES).
pub fn blocked_refs(stdin: &str) -> Vec<String> {
    let protected_refs: Vec<String> = READ_ONLY_BRANCHES.iter().map(|b| format!("refs/heads/{b}")).collect();
    lines(stdin)
        .into_iter()
        .filter(|line| line.split(' ').nth(2).is_some_and(|remote| protected_refs.iter().any(|p| p == remote)))
        .collect()
}

fn run(mode: &str) -> io::Result<i32> {
    if mode == "--pre-push" {
        let mut stdin = String::new();
        io::stdin().read_to_string(&mut stdin)?;
        let blocked = blocked_refs(&stdin);
        if !blocked.is_empty() {
            return Ok(fail("보호 브랜치(test_1·test_1_hotfix·test_2·main_hotfix·main)에는 직접 push할 수 없습니다. GitHub PR 병합으로만 바뀝니다", &blocked));
        }
        return Ok(0);
    }
    if mode == "--staged" {
        let branch = git(&["branch", "--show-current"])?.trim().to_string();
        if READ_ONLY_BRANCHES.contains(&branch.as_str()) {
            return Ok(fail(
                &format!("보호 브랜치 {branch}에서는 커밋할 수 없습니다. 자기 개인 브랜치나 <개인 브랜치>-<주제> 브랜치에서 작업하고 PR로 올리세요"),
                &[],
            ));
        }
        let files = lines(&git(&["diff", "--cached", "--name-only", "--diff-filter=ACMR"])?);
        let problems = scan_files(&files, |path| git_bytes(&["show", &format!(":{path}")]))?;
        if !problems.is_empty() {
            return Ok(fail("프로바이더 정책에 어긋나는 코드가 있습니다", &problems));
        }
        return Ok(0);
    }
    let files = lines(&git(&["ls-files"])?);
    let problems = scan_files(&files, |path| std::fs::read(path))?;
    if !problems.is_empty() {
        return Ok(fail("프로바이더 정책에 어긋나는 코드가 있습니다", &problems));
    }
    println!("정책 검사 통과: {}개 파일", files.len());
    Ok(0)
}

/// Runs the scanner with its mode argument (default `--all`); returns the process exit code.
pub fn scan_main(args: &[String]) -> i32 {
    let mode = args.first().map(String::as_str).unwrap_or("--all");
    match run(mode) {
        Ok(code) => code,
        Err(error) => {
            // Never pass silently: a scan that could not finish fails the hook or CI step.
            eprintln!("정책 검사를 마치지 못했습니다: {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO: &str = "0000000000000000000000000000000000000000";

    #[test]
    fn pushes_to_protected_branches_are_blocked() {
        let main = format!("refs/heads/younchan abc refs/heads/main {ZERO}");
        let test_1 = format!("refs/heads/test_2 abc refs/heads/test_1 {ZERO}");
        let test_2 = format!("refs/heads/younchan abc refs/heads/test_2 {ZERO}\r");
        let main_hotfix = format!("refs/heads/younchan-fix abc refs/heads/main_hotfix {ZERO}");
        let own = format!("refs/heads/younchan abc refs/heads/younchan {ZERO}");
        let input = format!("{own}\n{main}\n\n{test_1}\n{test_2}\n{main_hotfix}\n");
        assert_eq!(blocked_refs(&input), vec![main, test_1, test_2, main_hotfix]);
    }

    #[test]
    fn personal_and_lookalike_refs_pass() {
        let input = "refs/heads/x a refs/heads/main-old b\nrefs/heads/x a refs/heads/younchan-test b\nrefs/heads/x a refs/heads/test b\nrefs/heads/x a refs/heads/test_10 b\nrefs/tags/v1 a refs/tags/test_1 b\n";
        assert!(blocked_refs(input).is_empty());
        assert!(blocked_refs("").is_empty());
    }

    #[test]
    fn scan_skips_allowed_and_missing_files_but_not_files_with_nul_bytes_or_other_errors() {
        // Split so this file itself does not trip the scanner.
        const BYPASS: &str = concat!("--yo", "lo");
        let files: Vec<String> = ["docs/a.md", "gone.rs", "bin.dat", "wide.txt", "ok.rs"].map(String::from).to_vec();
        let problems = scan_files(&files, |path| match path {
            "gone.rs" => Err(io::Error::from(io::ErrorKind::NotFound)),
            "bin.dat" => Ok(format!("\0{BYPASS}").into_bytes()),
            // UTF-16: a NUL after every letter.
            "wide.txt" => Ok(BYPASS.bytes().flat_map(|b| [b, 0]).collect()),
            _ => Ok(format!("fn main() {{}}\n{BYPASS}\n").into_bytes()),
        })
        .unwrap();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].starts_with("bin.dat:1 [permission-bypass]"), "{}", problems[0]);
        assert!(problems[1].starts_with("wide.txt:1 [permission-bypass]"), "{}", problems[1]);
        assert!(problems[2].starts_with("ok.rs:2 [permission-bypass]"), "{}", problems[2]);

        let denied = scan_files(&files[4..], |_| Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        assert!(denied.is_err());
    }

    /// ADR-0014: the provider files and their test may spell the bypass switches; nothing else
    /// may, and the provider files still may not touch credentials.
    #[test]
    fn provider_files_may_spell_bypass_switches_and_nothing_else() {
        const BYPASS: &str = concat!("--dangerously-", "skip-permissions");
        const TOKEN: &str = concat!("CLAUDE_CODE_", "OAUTH_TOKEN");
        let files: Vec<String> =
            ["crates/o-core/src/providers/claude.rs", "crates/o-core/tests/providers.rs", "AGENTS.md", "crates/o-core/src/lead/mod.rs", "app/src/x.ts", "crates/o-core/src/providers/sub/x.rs"]
                .map(String::from)
                .to_vec();
        let problems = scan_files(&files, |_| Ok(format!("args.push(\"{BYPASS}\");\n").into_bytes())).unwrap();
        let flagged: Vec<&str> = problems.iter().map(|p| p.split(':').next().unwrap()).collect();
        assert_eq!(flagged, ["AGENTS.md", "crates/o-core/src/lead/mod.rs", "app/src/x.ts", "crates/o-core/src/providers/sub/x.rs"]);
        let credential = scan_files(&files[..1], |_| Ok(format!("env::var(\"{TOKEN}\")\n").into_bytes())).unwrap();
        assert_eq!(credential.len(), 1);
        assert!(credential[0].contains("[claude-oauth-token]"), "{}", credential[0]);
    }
}
