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
//! - `lead`: the lead chat — a read-only lead CLI proposes plans, the person dispatches (ADR-0008)
//! - `review`, `sessions`: `o review` and Claude Code sessions as runs
//! - `projects`: the projects root and registered folders the app lists (ADR-0009)
//! - `sys`: processes, CLI lookup, JS-compatible formats, mirror sync

/// This build's O version (`o about`); the desktop app shows it beside its own.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod agents;
pub mod context;
pub mod graph;
pub mod images;
pub mod issues;
pub mod lead;
pub mod policy;
pub mod projects;
pub mod providers;
pub mod review;
pub mod run;
pub mod schedule;
pub mod sessions;
pub mod skills;
pub mod sys;
pub mod term;
pub mod usage;
pub mod dream;
pub mod tuning;
