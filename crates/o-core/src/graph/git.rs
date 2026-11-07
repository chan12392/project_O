//! Changed files in a project, including untracked files.

use std::collections::HashSet;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::grafly::capture;

pub fn changed(project: &Path) -> Result<HashSet<String>, String> {
    let mut prefix_cmd = Command::new("git");
    prefix_cmd.arg("-C").arg(project).args(["rev-parse", "--show-prefix"]).env("LC_ALL", "C");
    let (status, prefix, stderr) = capture(&mut prefix_cmd, Duration::from_secs(20))?;
    if !status.success() {
        return if stderr.contains("not a git repository") { Ok(HashSet::new()) } else { Err(stderr.trim().to_string()) };
    }
    let prefix = prefix.trim_end_matches(['\r', '\n']).to_string();
    let mut command = Command::new("git");
    command.arg("-C").arg(project).args(["-c", "core.fsmonitor=false", "status", "--porcelain=v1", "-z", "--untracked-files=all", "--", "."]);
    let (status, output, stderr) = capture(&mut command, Duration::from_secs(20))?;
    if !status.success() {
        return Err(stderr.trim().to_string());
    }
    let mut entries = output.as_bytes().split(|b| *b == 0);
    let mut files = HashSet::new();
    while let Some(entry) = entries.next() {
        if entry.len() < 4 { continue; }
        let path = String::from_utf8_lossy(&entry[3..]);
        if let Some(path) = path.strip_prefix(&prefix) {
            files.insert(path.to_string());
        }
        if entry[..2].iter().any(|c| *c == b'R' || *c == b'C') {
            if let Some(original) = entries.next() {
                let original = String::from_utf8_lossy(original);
                if let Some(original) = original.strip_prefix(&prefix) {
                    files.insert(original.to_string());
                }
            }
        }
    }
    Ok(files)
}

/// Returns tracked paths changed relative to a git revision, scoped to the project.
pub fn changed_since(project: &Path, base: &str) -> Result<HashSet<String>, String> {
    if base.starts_with('-') { return Err("base는 git 참조여야 합니다".into()); }
    let mut command = Command::new("git");
    command.arg("-C").arg(project).args(["-c", "core.fsmonitor=false", "diff", "--relative", "--name-only", "--no-renames", "--no-ext-diff", "--no-textconv", "-z", base, "--", "."]);
    let (status, output, stderr) = capture(&mut command, Duration::from_secs(20))?;
    if !status.success() { return Err(format!("git diff: {}", stderr.trim())); }
    Ok(output.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_project_includes_untracked_and_renamed_files() {
        let root = std::env::temp_dir().join(format!("o-graph-git-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let run = |args: &[&str]| {
            let mut cmd = Command::new("git");
            cmd.arg("-C").arg(&root).args(args);
            assert!(capture(&mut cmd, Duration::from_secs(20)).unwrap().0.success());
        };
        run(&["init", "-q"]);
        std::fs::write(root.join("sub/old.txt"), "content").unwrap();
        run(&["add", "sub/old.txt"]);
        run(&["-c", "user.name=test", "-c", "user.email=test@example.invalid", "commit", "-qm", "test"]);
        run(&["mv", "sub/old.txt", "sub/new.txt"]);
        std::fs::write(root.join("sub/new file.txt"), "new").unwrap();
        std::fs::write(root.join("outside.txt"), "outside").unwrap();
        let hook = root.join(".git/hooks/fsmonitor-test");
        std::fs::write(&hook, "#!/bin/sh\ntouch fsmonitor-ran\nprintf 'token\\0'\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        run(&["config", "core.fsmonitor", ".git/hooks/fsmonitor-test"]);
        run(&["status", "--porcelain"]);
        assert!(root.join("fsmonitor-ran").exists());
        std::fs::remove_file(root.join("fsmonitor-ran")).unwrap();
        let found = changed(&root.join("sub")).unwrap();
        assert_eq!(found, HashSet::from(["old.txt".into(), "new.txt".into(), "new file.txt".into()]));
        assert_eq!(changed_since(&root.join("sub"), "HEAD").unwrap(), HashSet::from(["old.txt".into(), "new.txt".into()]));
        assert!(changed_since(&root, "--output=unexpected").is_err());
        assert!(changed_since(&root, "missing-revision").is_err());
        assert!(!root.join("fsmonitor-ran").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
