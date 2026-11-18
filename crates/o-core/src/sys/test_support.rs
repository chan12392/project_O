//! Where cargo put the helper binary, for unit tests that start the real handle owner.
//!
//! Cargo keeps a unit's final artifacts next to the profile folder (`target/debug/`) or, in the
//! newer layout, in a per-unit `target/debug/build/<crate>/<hash>/out/` folder. Unit tests get
//! no `CARGO_BIN_EXE_*` variable, so the folders above the test binary are searched instead:
//! the helper sits beside the test binary's `deps` folder or in one of those `out` folders.

use std::path::PathBuf;

const NAME: &str = "o-handle-owner.exe";

/// The `o-handle-owner` binary of this build.
pub(crate) fn handle_owner() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable");
    let mut folder = exe.parent();
    for _ in 0..6 {
        let Some(current) = folder else { break };
        let beside = current.join(NAME);
        if beside.is_file() {
            return beside;
        }
        if current.file_name().is_some_and(|name| name == "build") {
            if let Some(found) = in_build_folder(current) {
                return found;
            }
        }
        folder = current.parent();
    }
    panic!("{NAME} was not found above {}", exe.display());
}

/// `build/<crate>/<hash>/out/<helper>`, the first one in a stable order.
fn in_build_folder(build: &std::path::Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for unit in std::fs::read_dir(build).ok()?.flatten() {
        let Ok(hashes) = std::fs::read_dir(unit.path()) else { continue };
        for hash in hashes.flatten() {
            let candidate = hash.path().join("out").join(NAME);
            if candidate.is_file() {
                found.push(candidate);
            }
        }
    }
    found.sort();
    found.into_iter().next()
}
