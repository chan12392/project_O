//! `o sync-mirror <dir>`: brings a mirror checkout (the team runs one on test_2;
//! the other protected branches are allowed too) up to its remote
//! branch. Fast-forward only. A mirror with local changes is left alone and
//! logged: mirrors should never have any, and discarding someone's edits
//! silently would be worse than a stale mirror.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::sys::js::now_iso;
use crate::sys::proc;

/// The protected branches; sync refuses any other checkout.
pub const MIRROR_BRANCHES: [&str; 5] = o_guard::rules::READ_ONLY_BRANCHES;

/// Runs one sync and logs to `<home>/mirror-sync.log`; returns the exit code.
pub fn sync_mirror(home: &Path, mirror: Option<&str>) -> i32 {
    let shown = mirror.unwrap_or("undefined");
    let log = |message: &str| {
        let _ = fs::create_dir_all(home);
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(home.join("mirror-sync.log")) {
            let _ = writeln!(file, "{} {shown} {message}", now_iso());
        }
    };
    match sync(home, mirror) {
        Ok(Some(update)) => {
            log(&update);
            0
        }
        Ok(None) => 0,
        Err(error) => {
            log(&format!("error {}", error.lines().next().unwrap_or_default()));
            1
        }
    }
}

fn sync(home: &Path, mirror: Option<&str>) -> Result<Option<String>, String> {
    let mirror = mirror.ok_or("사용법: o sync-mirror <mirror 폴더>")?;
    // No repository hooks: this runs unattended every few minutes, and a hook merged into
    // test_2 would otherwise run here unseen. Pointing hooksPath at a missing folder disables them.
    let no_hooks = home.join("no-hooks");
    let git = |args: &[&str]| -> Result<String, String> {
        let executable = o_guard::hook::installed_program("git").ok_or("지원하는 설치 경로에서 Git을 찾지 못했습니다")?;
        let mut command = Command::new(executable);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().to_ascii_uppercase().starts_with("GIT_") { command.env_remove(key); }
        }
        let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
        command
            .env("GIT_CONFIG_GLOBAL", empty).env("GIT_CONFIG_SYSTEM", empty).env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("-c")
            .arg(format!("core.hooksPath={}", no_hooks.to_string_lossy()))
            .args(["-C", mirror])
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let out = proc::hide_window(&mut command).output().map_err(|e| format!("git {}: {e}", args.join(" ")))?;
        if !out.status.success() {
            return Err(format!("Command failed: git {}\n{}", args.join(" "), String::from_utf8_lossy(&out.stderr)));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };

    // Config is data here; reject options that make fetch/status/checkout execute a helper or
    // rewrite the validated origin. Global settings and injected GIT_* settings are not loaded.
    let keys = git(&["config", "--name-only", "--list"])?;
    if let Some(key) = keys.lines().find(|key| unsafe_config(key)) {
        return Err(format!("동기화 중 외부 명령·원격 변경을 일으킬 수 있는 Git 설정입니다: {key}"));
    }
    let branch = git(&["branch", "--show-current"])?;
    if !MIRROR_BRANCHES.contains(&branch.as_str()) {
        return Err(format!("거울 브랜치({})가 아닙니다: {branch}", MIRROR_BRANCHES.join(", ")));
    }
    // The mirror follows project_O only: an `origin` pointed elsewhere would be pulled in unseen.
    let origin = git(&["remote", "get-url", "origin"])?;
    if !o_guard::rules::PROJECT_REMOTE.is_match(&origin) {
        return Err(format!("origin이 project_O 저장소가 아니어서 동기화하지 않았습니다: {origin}"));
    }
    git(&["fetch", "--no-recurse-submodules", "--no-tags", "origin", &format!("+refs/heads/{branch}:refs/remotes/origin/{branch}")])?;
    let before = git(&["rev-parse", "--short", "HEAD"])?;
    let target = git(&["rev-parse", "--short", &format!("origin/{branch}")])?;
    if before == target {
        return Ok(None);
    }
    if !git(&["status", "--porcelain"])?.is_empty() {
        return Err(format!("로컬 변경이 있어 동기화하지 않았습니다 ({before} → {target} 대기)"));
    }
    git(&["merge", "--ff-only", &format!("origin/{branch}")])?;
    Ok(Some(format!("updated {before} → {target}")))
}

fn unsafe_config(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("url.") || key.starts_with("filter.") || key.starts_with("credential.")
        || key.starts_with("include.") || key.starts_with("includeif.") || key.starts_with("http.")
        || key.starts_with("protocol.") || key.starts_with("submodule.")
        || ["core.sshcommand", "core.gitproxy", "core.fsmonitor"].contains(&key.as_str())
        || (key.starts_with("remote.") && [".uploadpack", ".receivepack", ".vcs", ".proxy"].iter().any(|suffix| key.ends_with(suffix)))
        || ((key.starts_with("diff.") || key.starts_with("merge.")) && (key.ends_with(".command") || key.ends_with(".driver")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh repository with no remote, checked out on `branch`.
    fn repo_on(branch: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("o-mirror-{branch}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let status = Command::new("git").args(["init", "-q", "-b", branch]).current_dir(&dir).status().unwrap();
        assert!(status.success());
        dir
    }

    #[test]
    fn only_protected_branch_checkouts_are_synced() {
        let home = std::env::temp_dir().join(format!("o-mirror-home-{}", std::process::id()));
        for branch in ["younchan", "test"] {
            let dir = repo_on(branch);
            let error = sync(&home, Some(&dir.to_string_lossy())).unwrap_err();
            assert_eq!(error, format!("거울 브랜치(test_1, test_1_hotfix, test_2, main_hotfix, main)가 아닙니다: {branch}"));
        }
        // A protected branch passes the check and goes on to read its origin (none here).
        for branch in MIRROR_BRANCHES {
            let dir = repo_on(branch);
            let error = sync(&home, Some(&dir.to_string_lossy())).unwrap_err();
            assert!(error.starts_with("Command failed: git remote get-url origin"), "{branch}: {error}");
        }
        // An origin that is not project_O is never fetched.
        let dir = repo_on("test_2");
        assert!(Command::new("git").args(["remote", "add", "origin", "https://example.invalid/someone/other.git"]).current_dir(&dir).status().unwrap().success());
        let error = sync(&home, Some(&dir.to_string_lossy())).unwrap_err();
        assert_eq!(error, "origin이 project_O 저장소가 아니어서 동기화하지 않았습니다: https://example.invalid/someone/other.git");
        let git = o_guard::hook::installed_program("git").unwrap();
        for (key, value) in [("url.https://example.invalid/.insteadOf", "https://github.com/"), ("core.sshCommand", "must-never-run"), ("remote.origin.uploadpack", "must-never-run"), ("filter.bad.smudge", "must-never-run")] {
            let status = crate::sys::proc::hide_window(Command::new(&git).args(["config", key, value]).current_dir(&dir)).status().unwrap();
            assert!(status.success());
            let error = sync(&home, Some(&dir.to_string_lossy())).unwrap_err();
            assert!(error.contains("Git 설정"), "{key}: {error}");
            assert!(crate::sys::proc::hide_window(Command::new(&git).args(["config", "--unset", key]).current_dir(&dir)).status().unwrap().success());
        }
    }

}
