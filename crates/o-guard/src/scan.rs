//! Static policy scan, run by git hooks and CI.
//!
//!   --all       every tracked file (CI)
//!   --staged    staged content, and no commits on test_1/test_2/main (pre-commit)
//!   --pre-push  refuses pushes that target test_1/test_2/main (pre-push; refs on stdin)

use std::io::{self, Read};
use std::process::Stdio;

use crate::hook::git_command;
use crate::rules::{code_rules, find_hits, is_allowed_path, READ_ONLY_BRANCHES};

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
        if bytes.contains(&0) {
            continue; // binary
        }
        let text = String::from_utf8_lossy(&bytes);
        for hit in find_hits(&text, code_rules()) {
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

/// The pre-push lines (`<local ref> <local sha> <remote ref> <remote sha>`) whose remote ref is a protected branch (test_1, test_2, main).
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
            return Ok(fail("보호 브랜치(test_1·test_2·main)에는 직접 push할 수 없습니다. GitHub PR 병합으로만 바뀝니다", &blocked));
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
        let own = format!("refs/heads/younchan abc refs/heads/younchan {ZERO}");
        let input = format!("{own}\n{main}\n\n{test_1}\n{test_2}\n");
        assert_eq!(blocked_refs(&input), vec![main, test_1, test_2]);
    }

    #[test]
    fn personal_and_lookalike_refs_pass() {
        let input = "refs/heads/x a refs/heads/main-old b\nrefs/heads/x a refs/heads/younchan-test b\nrefs/heads/x a refs/heads/test b\nrefs/heads/x a refs/heads/test_10 b\nrefs/tags/v1 a refs/tags/test_1 b\n";
        assert!(blocked_refs(input).is_empty());
        assert!(blocked_refs("").is_empty());
    }

    #[test]
    fn scan_skips_allowed_missing_and_binary_files_but_not_other_errors() {
        // Split so this file itself does not trip the scanner.
        const BYPASS: &str = concat!("--yo", "lo");
        let files: Vec<String> = ["docs/a.md", "gone.rs", "bin.dat", "ok.rs"].map(String::from).to_vec();
        let problems = scan_files(&files, |path| match path {
            "gone.rs" => Err(io::Error::from(io::ErrorKind::NotFound)),
            "bin.dat" => Ok(format!("\0{BYPASS}").into_bytes()),
            _ => Ok(format!("fn main() {{}}\n{BYPASS}\n").into_bytes()),
        })
        .unwrap();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].starts_with("ok.rs:2 [permission-bypass]"), "{}", problems[0]);

        let denied = scan_files(&files[3..], |_| Err(io::Error::from(io::ErrorKind::PermissionDenied)));
        assert!(denied.is_err());
    }
}
