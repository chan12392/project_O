//! A plan's life: the plan itself, the normalized events it writes, the runner
//! that starts its cards, the state folder and its SQLite index, the snapshot a
//! screen draws, and the run's team channel, and the project_O 프로필 사용 switch (`mode`).
//! Pure: `plan`, `events`, `snapshot`, `mode`. The rest work on O's state folder
//! and processes.

pub mod events;
pub mod followup;
mod resources;
pub mod index;
pub mod input;
#[cfg(windows)]
pub mod job_watch;
mod leases;
pub mod mode;
pub mod msg_relay;
pub mod plan;
pub mod ram;
pub mod result;
pub mod runner;
pub mod snapshot;
pub mod store;
pub mod team;
pub mod team_command;
pub mod yolo;
pub(crate) mod workspaces;
pub use workspaces::review_changes;
