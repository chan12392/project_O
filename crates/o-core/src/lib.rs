//! O core: plans, policy, provider launch and stream reading, the run store,
//! the runner and the usage probes. Pure modules (`plan`, `events`, `policy`,
//! `providers`, `usage`, `snapshot`) touch nothing; `store`, `runner`,
//! `probe`, `team` and `mirror` work on O's state folder and processes. The
//! `o` binary and a desktop shell both sit on top of this.

pub mod events;
pub mod index;
pub mod js;
pub mod mirror;
pub mod plan;
pub mod policy;
pub mod probe;
pub mod proc;
pub mod providers;
pub mod resolve;
pub mod runner;
pub mod snapshot;
pub mod store;
pub mod team;
pub mod usage;
