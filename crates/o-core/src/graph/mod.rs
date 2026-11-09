//! Code graph indexing, queries and display data (ADR-0033).

pub mod deep;
pub mod git;
pub mod grafly;
pub mod query;
pub mod settings;
pub mod view;
pub mod wiring;

pub use grafly::build;
pub use view::{view, GraphView};

pub(crate) fn check_project(home: &std::path::Path, project: &str) -> Result<(), String> {
    crate::lead::check_folder(home, project)
}
