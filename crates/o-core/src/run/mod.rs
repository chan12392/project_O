//! A plan's life: the plan itself, the normalized events it writes, the runner
//! that starts its cards, the state folder and its SQLite index, the snapshot a
//! screen draws, and the run's team channel, and the project_O 프로필 사용 switch (`mode`).
//! Pure: `plan`, `events`, `snapshot`, `mode`. The rest work on O's state folder
//! and processes.

pub mod events;
pub mod index;
pub mod mode;
pub mod plan;
pub mod runner;
pub mod snapshot;
pub mod store;
pub mod team;
pub mod yolo;
