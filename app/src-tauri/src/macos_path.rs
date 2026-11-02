//! Finder launches do not inherit a terminal's PATH. Add conventional CLI
//! locations without sourcing shell startup files or reading login data.
use std::path::{Path, PathBuf};

pub fn paths(home: &Path, current: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut paths = current;
    for dir in [home.join(".local/bin"), home.join(".cargo/bin"), PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")] {
        if dir.is_dir() && !paths.contains(&dir) { paths.push(dir); }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finder_path_keeps_existing_order_and_adds_homebrew_once() {
        let current = vec![PathBuf::from("/usr/bin"), PathBuf::from("/opt/homebrew/bin")];
        let result = paths(Path::new("/nonexistent-project-o-test-home"), current.clone());
        assert_eq!(&result[..2], current.as_slice());
        assert_eq!(result.iter().filter(|p| **p == Path::new("/opt/homebrew/bin")).count(), 1);
    }
}
