//! `o hook` and `o scan`: the policy guard (`o_guard`) for agent hooks, git hooks and CI.

use super::Command;

pub const HOOK: Command = Command {
    name: "hook",
    usage: "  o hook                     에이전트 PreToolUse 훅 (도구 호출 JSON을 stdin으로 받음)",
    run: |_, _| o_guard::hook::hook_main(),
};

pub const SCAN: Command = Command {
    name: "scan",
    usage: "  o scan [--all|--staged|--pre-push]   저장소 정책 검사 (git 훅·CI용)",
    run: |_, args| o_guard::scan::scan_main(args),
};
