//! Helpers for unit-test binaries; Cargo may place them under deps/ or build/<id>/out/.
use std::path::{Path, PathBuf};

fn handle_owner_near(executable: &Path) -> Option<PathBuf> {
    executable.parent()?.ancestors()
        .map(|dir| dir.join("o-handle-owner.exe"))
        .find(|candidate| candidate.is_file())
}

#[cfg(windows)]
pub(crate) fn handle_owner() -> PathBuf {
    let executable = std::env::current_exe().unwrap();
    handle_owner_near(&executable).unwrap_or_else(|| panic!("Cargo handle-owner helper missing near {}", executable.display()))
}

#[test]
fn helper_is_found_in_both_cargo_unit_test_layouts() {
    let root = std::env::temp_dir().join(format!("o-unit-helper-layout-{}", std::process::id()));
    let profile = root.join("target/debug");
    std::fs::create_dir_all(&profile).unwrap();
    let helper = profile.join("o-handle-owner.exe");
    std::fs::write(&helper, b"fixture").unwrap();
    for relative in ["deps/o_core-test.exe", "build/o-core/hash/out/o_core-test.exe"] {
        let executable = profile.join(relative);
        std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
        assert_eq!(handle_owner_near(&executable), Some(helper.clone()));
    }
    std::fs::remove_file(helper).unwrap();
    assert_eq!(handle_owner_near(&profile.join("deps/o_core-test.exe")), None);
    std::fs::remove_dir_all(root).unwrap();
}
