//! O core: plans, policy, provider launch and stream reading, the run store,
//! the runner and the usage probes. The `o` binary and the desktop shell both
//! sit on top of this. Module map: docs/ARCHITECTURE.md.
//!
//! - `run`: a plan's life — plan, events, runner, store, index, snapshot, team channel
//! - `policy`: whether a plan or a team message may go (shared boundary)
//! - `providers`: each official agent CLI behind one `Provider` table
//! - `agents`: CLI install/sign-in status and clean test profiles
//! - `usage`: subscription windows, their probes and the daily budget
//! - `context`: shared session context and token report (ADR-0005)
//! - `review`, `sessions`: `o review` and Claude Code sessions as runs
//! - `sys`: processes, CLI lookup, JS-compatible formats, mirror sync

pub mod agents;
pub mod context;
pub mod policy;
pub mod providers;
pub mod review;
pub mod run;
pub mod sessions;
pub mod sys;
pub mod usage;
