//! `o sync-mirror <dir>`: brings a test/main mirror checkout up to its remote
//! branch. Fast-forward only. A mirror with local changes is left alone and
//! logged: mirrors should never have any, and discarding someone's edits
//! silently would be worse than a stale mirror.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::js::now_iso;
use crate::proc;

const MIRROR_BRANCHES: [&str; 2] = ["test", "main"];

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
    // test would otherwise run here unseen. Pointing hooksPath at a missing folder disables them.
    let no_hooks = home.join("no-hooks");
    let git = |args: &[&str]| -> Result<String, String> {
        let mut command = Command::new("git");
        command
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

    let branch = git(&["branch", "--show-current"])?;
    if !MIRROR_BRANCHES.contains(&branch.as_str()) {
        return Err(format!("거울 브랜치({})가 아닙니다: {branch}", MIRROR_BRANCHES.join(", ")));
    }
    git(&["fetch", "--prune", "origin", &branch])?;
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
