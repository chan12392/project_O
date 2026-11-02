//! "에이전트 검토 요청" (ADR-0006): `o review <pr> --post --detach`, dev `/api/review`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use o_core::review;
use o_core::sys::{proc, resolve};

use super::blocking;

/// The `o` built beside this app (target/release), else the one on PATH.
fn o_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "o.exe" } else { "o" };
    let beside = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.join(name))).filter(|p| p.is_file());
    beside.or_else(|| resolve::which("o"))
}

/// Checks the request before anything runs: a PR number or github.com pull address, and an existing absolute folder.
pub fn review_request(project: &str, pr: &str) -> Result<(), String> {
    if !review::is_selector(pr) {
        return Err(format!("PR 번호나 github.com PR 주소가 아닙니다: {pr}"));
    }
    if !Path::new(project).is_absolute() || !Path::new(project).is_dir() {
        return Err(format!("프로젝트 폴더가 없습니다: {project}"));
    }
    Ok(())
}

/// `o review <pr> --post --detach` in the project folder, as the dev server's `/api/review`
/// does. `o` reads the PR with gh, checks the review plan like any dispatch and starts it
/// in the background; the review is posted as a PR comment, never an approval or a merge.
#[tauri::command]
pub async fn review(project: String, pr: String, clean: bool) -> Result<serde_json::Value, String> {
    review_request(&project, &pr)?;
    blocking(move || {
        let o = o_binary().ok_or("o CLI를 찾지 못했습니다 (cargo install --path crates/o-cli)")?;
        let mut command = Command::new(o);
        command.args(review::request_args(&pr, clean)).current_dir(&project).stdin(Stdio::null());
        let out = proc::hide_window(&mut command).output().map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        serde_json::from_slice(&out.stdout).map_err(|e| format!("o review 출력을 읽지 못했습니다: {e}"))
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::temp;

    #[test]
    fn review_requests_take_only_a_pr_and_an_existing_folder() {
        let dir = temp("review");
        let folder = dir.to_string_lossy();
        assert!(review_request(&folder, "7").is_ok());
        assert!(review_request(&folder, "https://github.com/acme/app/pull/7").is_ok());
        for pr in ["", "0", "--repo", "7 --approve", "https://evil.example/a/b/pull/7"] {
            assert!(review_request(&folder, pr).is_err(), "{pr}");
        }
        assert!(review_request("relative/dir", "7").is_err());
        assert!(review_request(&dir.join("missing").to_string_lossy(), "7").is_err());
    }
}
