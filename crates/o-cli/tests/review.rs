//! `o review` refuses malformed arguments before it asks gh anything.

use std::process::{Command, Output};

fn o(args: &[&str]) -> Output {
    let base = std::env::temp_dir().join(format!("o-review-cli-{}", std::process::id()));
    Command::new(env!("CARGO_BIN_EXE_o")).args(args).env("LOCALAPPDATA", &base).env("O_RUN_ID", "").env("O_CARD_ID", "").output().unwrap()
}

#[test]
fn bad_review_arguments_stop_before_gh() {
    for args in [&["review"][..], &["review", "--post"][..], &["review", "-R", "x/y"][..], &["review", "7", "--approve"][..], &["review", "7", "--provider", "gpt"][..]] {
        let out = o(args);
        assert_eq!(out.status.code(), Some(1), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("PR을 읽지 못했습니다"), "{args:?}: gh was asked: {err}");
    }
    // The background half takes only a run id, a PR and its number.
    assert_eq!(o(&["_review", "../x", "7", "7"]).status.code(), Some(1));
    assert_eq!(o(&["_review", "20261102-100000-aaaa", "--repo", "7"]).status.code(), Some(1));
}

#[test]
fn help_lists_review() {
    let out = o(&["help"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("o review <PR 번호|주소>"));
}
