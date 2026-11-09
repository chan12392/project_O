//! `o review <pr>` (ADR-0006) and its background half `o _review`.

use std::path::Path;
use std::process::Command as Process;

use chrono::Utc;
use o_core::policy::check_plan_in;
use o_core::review;
use o_core::run::runner;
use o_core::run::store::{self, run_dir};
use o_core::sys::proc;
use serde_json::json;

use super::{fail, help, print_violations, run_id_arg, Command};

pub const REVIEW: Command = Command {
    name: "review",
    usage: "  o review <PR 번호|주소> [--provider claude|codex|agy|devin] [--model M] [--effort E] [--profile project-o|own] [--post] [--dry-run] [--detach]
                             지금 폴더 저장소의 PR을 gh로 읽어 에이전트 카드 하나로 검토 (파일 변경 없음)
                             --post: 끝나면 검토문을 PR 댓글로 게시 (승인·병합은 하지 않음)
                             --dry-run: 실행 없이 프롬프트 크기·고른 에이전트·정책 결과만 출력
                             --detach: 백그라운드로 시작하고 JSON 출력 (앱용)",
    run: review,
};

/// `o _review <runId> <pr> <number> --head <sha> [--post]`: the background half of `o review --detach`.
pub const REVIEW_BACKGROUND: Command = Command { name: "_review", usage: "", run: review_background };

/// `o review <pr> …`: reads the PR with gh in this folder, checks a one-card
/// review plan like `o dispatch`, then runs it here (or in the background with
/// `--detach`) and, with `--post`, comments the review on the PR.
fn review(home: &Path, args: &[String]) -> i32 {
    let args = match review::parse_args(args) {
        Ok(args) => args,
        Err(error) => return fail(&format!("{error}\n\n{}", help())),
    };
    let project = match std::env::current_dir() {
        Ok(dir) => dir,
        Err(error) => return fail(&error.to_string()),
    };
    let gh = review::OfficialGh;
    let (pr, diff) = match review::fetch(&gh, &project, &args.selector) {
        Ok(found) => found,
        Err(error) => return fail(&format!("PR을 읽지 못했습니다: {error}")),
    };
    let author = review::authorship(home, &pr);
    let provider = args.provider.unwrap_or_else(|| review::choose_provider(author.provider));
    // Local files may fill in a cut diff only when this folder is the PR's head commit, unchanged.
    let local = review::local_tree(&project, &pr.head_ref_oid);
    let local_is_head = local == review::LocalTree::Head;
    let task = review::build_task(&pr, &diff, &review::new_nonce(), local_is_head);
    let project_text = project.to_string_lossy().into_owned();
    let input = review::build_plan(&project_text, pr.number, &task.text, provider, args.model.as_deref(), args.effort.as_deref());

    let now = Utc::now();
    let ctx = match store::policy_context(home, now) {
        Ok(ctx) => ctx,
        Err(error) => return fail(&error.to_string()),
    };
    let checked = check_plan_in(&input, &ctx, args.mode);
    let summary = || {
        let cut = if task.cut_files.is_empty() { String::new() } else { format!(", 잘린 파일 {}개", task.cut_files.len()) };
        let redacted = if task.redacted_lines > 0 { format!(", 정책 규칙으로 뺀 줄 {}개", task.redacted_lines) } else { String::new() };
        eprintln!("PR #{} {} — {}", pr.number, pr.title, pr.url);
        eprintln!(
            "검토할 커밋: head {} (base {}){}",
            review::short_sha(&pr.head_ref_oid),
            review::short_sha(&pr.base_ref_oid),
            match local {
                review::LocalTree::Head => " · 이 폴더와 같음",
                review::LocalTree::Dirty => " · 이 폴더에 커밋하지 않은 변경이 있어 로컬 파일로 보충하지 않음",
                review::LocalTree::Other => " · 이 폴더는 다른 커밋이라 로컬 파일로 보충하지 않음",
            }
        );
        eprintln!("작성: {} → 검토: {provider}{}", author.describe(), if args.provider.is_some() { " (지정)" } else { "" });
        eprintln!("프롬프트 {}자 (상한 {}) · diff {}자 중 {}자 포함{cut}{redacted}", o_core::sys::js::js_len(&task.text), o_core::policy::LIMITS.max_task_chars, task.diff_chars, task.diff_included);
    };
    if !args.detach || args.dry_run {
        summary();
    }
    let Some(plan) = checked.plan else {
        if !args.dry_run {
            if let Err(error) = store::record_blocked(home, now, input.get("title"), &checked.violations) {
                eprintln!("{error}");
            }
        }
        print_violations(&checked.violations);
        return 2;
    };
    if args.dry_run {
        println!("정책 통과 — 실행하지 않음 (--dry-run). 검토: {provider}, 프롬프트 {}자", o_core::sys::js::js_len(&task.text));
        return 0;
    }

    let run_id = store::new_run_id(now);
    if let Err(error) = review::create_run(home, &run_id, &plan, args.mode) {
        return fail(&error.to_string());
    }
    let events = run_dir(home, &run_id).join("events.jsonl");
    if args.detach {
        // The PR, its head commit and the post flag go to the background runner as arguments, out of the cards' reach.
        let number = pr.number.to_string();
        let mut background = vec!["_review", &run_id, &args.selector, &number, "--head", &pr.head_ref_oid];
        if args.post {
            background.push("--post");
        }
        let started = std::env::current_exe().and_then(|exe| proc::spawn_detached(Process::new(exe).args(&background).current_dir(&project)));
        if let Err(error) = started {
            return fail(&format!("실행기를 시작하지 못했습니다: {error}"));
        }
        let message = format!(
            "PR #{} 검토를 시작했습니다 · {provider}{} · 커밋 {}{}",
            pr.number,
            if args.mode.clean() { " (project_O 프로필)" } else { " (내 설정)" },
            review::short_sha(&pr.head_ref_oid),
            if args.post { " · 끝나면 PR 댓글로 게시 (그사이 새 커밋이 오면 게시하지 않음)" } else { "" }
        );
        println!(
            "{}",
            json!({ "ok": true, "message": message, "runId": run_id, "pr": pr.number, "head": pr.head_ref_oid, "provider": provider, "post": args.post, "profile": args.mode.profile, "clean": args.mode.clean(), "events": events.to_string_lossy() })
        );
        return 0;
    }
    eprintln!("실행 {run_id} — 검토 중… (다른 창에서 o events {run_id} --follow)");
    review_run(home, &run_id, &args.selector, pr.number, Some(&pr.head_ref_oid), args.post, &project)
}

/// Runs the review card (policy checked again, like `o _run`) and finishes: saves, and posts when asked.
fn review_run(home: &Path, run_id: &str, selector: &str, number: u64, head: Option<&str>, post: bool, project: &Path) -> i32 {
    if let Err(message) = runner::run_detached(home, run_id) {
        return fail(&message);
    }
    match review::finish(home, run_id, selector, number, head, post, project, &review::OfficialGh) {
        Ok(done) => {
            if let Some(head) = &done.head {
                println!("검토한 커밋: {head}");
            }
            match (&done.review_file, &done.comment_url) {
                (_, Some(url)) => println!("검토 댓글을 게시했습니다: {url}"),
                (Some(file), None) => println!("검토문: {}\n게시하려면 --post를 붙여 다시 실행하거나 o status {run_id}로 확인하세요", file.to_string_lossy()),
                (None, None) => println!("검토 카드가 {}로 끝나 검토문이 없습니다. o status {run_id}", done.status.as_str()),
            }
            i32::from(done.review_file.is_none())
        }
        Err(error) => fail(&error),
    }
}

fn review_background(home: &Path, args: &[String]) -> i32 {
    let (Some(run_id), Some(selector), Some(number)) = (run_id_arg(args.first().map(String::as_str)), args.get(1), args.get(2).and_then(|n| n.parse().ok())) else {
        return fail(help());
    };
    if !review::is_selector(selector) {
        return fail(&format!("PR 형식이 아닙니다: {selector}"));
    }
    let head = args.iter().position(|a| a == "--head").and_then(|i| args.get(i + 1)).filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()));
    let project = std::env::current_dir().unwrap_or_default();
    review_run(home, run_id, selector, number, head.map(String::as_str), args.iter().any(|a| a == "--post"), &project)
}
