# Windows atomic Job handoff

Comparison snapshot: local `younchan` commit `6284535`; this checkout is an older branch with uncommitted work. The other checkout and its uncommitted changes were not modified or incorporated. Do not merge this branch wholesale into `younchan`.

## Changes to carry

- `crates/o-core/src/sys/{launch,worker,win_launch}.rs`, `sys/mod.rs`, the relevant Windows Job changes in `sys/proc.rs`, and the required `windows-sys` features in `crates/o-core/Cargo.toml` form the launch and owned-process boundary. Preserve the latest branch's unrelated process and Linux recovery work when adapting it.
- `crates/o-cli/src/main.rs` has the pre-initialization `--o-handle-owner` entry. The lead launched from the desktop executable looks for `o.exe` beside that executable; missing or incompatible helpers fail closed. Verify packaged and development layouts before release, and verify callers embedding `o-core` have a trusted compatible helper.
- `crates/o-core/src/run/runner.rs` contains the explicit `Spawner`, card launch and durable reservation transition. Port these around the latest predecessor-result verification and Devin prompt-file handling, not over them. In `younchan` at `6284535`, result verification is around lines 251–263 and the Devin prompt file around lines 560–566 (`be8a6af`, `fa8118e`, `a5065c4`).
- `crates/o-core/src/lead/mod.rs` in this checkout builds a native suspended child inside a kill-on-close Job and preserves the marker when creation or marker writing is uncertain. Carry only this spawn/ownership logic and explicit Spawner environment changes into the newer lead implementation.
- Tests and fixture support are in `crates/o-core/tests/{resource_reservation,lead_recovery,lead_worker,lead_stream,windows_native_launch}.rs`, `tests/support/mod.rs`, `src/bin/o-handle-owner.rs`, and `crates/o-cli/tests/windows_handle_owner.rs`; the CLI test uses a Windows-only test dependency. Adapt existing callers and tests to the new Spawner contract rather than converting `std::process::Command` after construction.

## Conflicts and protections to retain

- The principal lead conflict is `send_turn`: `younchan` around lines 908–925 hands the child, Job and pipes to `lead/turn_process.rs`, whereas the older checkout around lines 876–921 uses its own watcher. Adapt `TurnProcess` to the common owned child and already-created Job; preserve its deadline through blocked stdout, stderr, stdin and process cleanup, and its bounded cancellation/reaping. The older watcher can stop before stderr and child cleanup and must not replace `b924f86`.
- Preserve `younchan`'s `Running.unresolved`, `TurnSlot.keep_marker`, project-wide turn exclusion, recovery checks, and refusal of follow-ups when termination is uncertain (roughly lines 285–423, 1164–1227, 1331, 1450 and 1709). Preserve this prototype's fail-closed child-marker write and ensure a panic around creation cannot clear the turn marker.
- Preserve the latest isolated card-question sessions and default-model/provider verification (`36095cb`, `4b3f8e2`), including `codex_app_server::run_with_default_model`, history checks and read-only permissions. This older checkout does not contain all of that behavior; do not substitute its `lead/mod.rs` or `providers/codex_app_server.rs` for the newer versions.
- Preserve the latest complete-result verification and long-result delivery in `run/result.rs`, `run/runner.rs`, associated CLI paths and tests. The concurrent worker's uncommitted changes in the other checkout were not inspected and may add further requirements.
- Keep the card's `mark_spawning` before native creation, retain unknown reservations after ambiguous failure, refund only a confirmed pre-creation failure, and release only after the owned Job reaches zero and DB exit confirmation succeeds. A DB exit-confirmation failure currently leaves the lease held even if the completed card is displayed as done.

## Evidence and limits

Windows tests in this checkout exercised actual `o.exe` with a fake provider, a fake API-key variable, normal reply and owner termination; the child process handle signaled and the old turn remained unknown. Focused fake lead tests covered live stop, simultaneous turns, bad helper, failed child-marker write, and repeated result reading. Existing card tests covered concurrent independent cards, stop, descendant exit, nested Job quota, spawn failure, and failed DB exit confirmation.

The desktop application's packaged sidecar path, a real provider or subscription, non-Windows runtime, complete token/privilege equivalence, and death inside the `CreateProcessW` syscall were not verified. The old checkout's timeout implementation remains incomplete; its successful focused tests are not evidence that the newer deadline and recovery protections are preserved. No worktree-creating tests or harness-free lead/runner suites were run.
