//! Plumbing with no O policy in it: processes and job objects, finding a CLI
//! on PATH without a shell, the JavaScript-compatible formats O's files use,
//! and the mirror checkout sync.

pub mod agent_job;
pub mod js;
#[cfg(any(target_os = "macos", test))]
mod macos_terminal;
pub mod launch;
pub mod memory;
pub mod mirror;
pub mod proc;
pub mod pty;
pub mod resolve;
pub mod worker;
#[cfg(windows)]
pub mod win_launch;

#[cfg(target_os = "macos")]
pub mod macos_worker;

#[cfg(target_os = "macos")]
pub(crate) mod macos_paths;

#[cfg(target_os = "macos")]
mod macos_native;
#[cfg(target_os = "macos")]
mod macos_coalition;
