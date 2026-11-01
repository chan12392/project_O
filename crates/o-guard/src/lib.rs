//! The provider-policy guard: rules every check shares, the agent PreToolUse
//! hook (`o hook`) and the repository scanner (`o scan`).

pub mod hook;
pub mod rules;
pub mod scan;

pub use hook::{hook_main, is_mirror, review, GitProbe, RealGit, PERSONAL_BRANCHES};
pub use scan::scan_main;
