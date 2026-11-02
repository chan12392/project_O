//! "에이전트 검토 요청" (ADR-0006): `o review <pr> --post --detach`, dev `/api/review`.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use o_core::review;
use o_core::sys::proc;

use super::{blocking, o_binary};

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

/// As long as the dev server's `/api/review` waits.
const REVIEW_TIMEOUT: Duration = Duration::from_secs(120);

/// `o review <pr> --post --detach` in the project folder, as the dev server's `/api/review`
/// does. `o` reads the PR with gh, checks the review plan like any dispatch and starts it
/// in the background; the review is posted as a PR comment, never an approval or a merge.
#[tauri::command]
pub async fn review(project: String, pr: String, clean: bool) -> Result<serde_json::Value, String> {
    review_request(&project, &pr)?;
    blocking(move || {
        let o = o_binary().ok_or("o CLI를 찾지 못했습니다 (cargo install --path crates/o-cli)")?;
        let mut command = Command::new(o);
        command.args(review::request_args(&pr, clean)).current_dir(&project);
        let stdout = finish(command, REVIEW_TIMEOUT)?;
        serde_json::from_slice(stdout.as_bytes()).map_err(|e| format!("o review 출력을 읽지 못했습니다: {e}"))
    })
    .await?
}

/// Waits for a command at most `timeout` and gives its stdout, or the reason it failed: its
/// stderr, else its exit code. A command still running at the deadline is ended with what it
/// started (a gh call that hangs). Not a job object: the review card `o review --detach`
/// starts must outlive it.
fn finish(mut command: Command, timeout: Duration) -> Result<String, String> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = proc::hide_window(&mut command).spawn().map_err(|e| format!("o review를 실행하지 못했습니다: {e}"))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut out);
            }
            let _ = send.send(String::from_utf8_lossy(&out).into_owned());
        });
        receive
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            None => {
                proc::kill_tree(child.id());
                let _ = child.wait();
                return Err(format!("PR 검토 요청이 {}초 안에 끝나지 않아 멈췄습니다. 네트워크와 gh 로그인 상태를 확인한 뒤 다시 시도해 주세요.", timeout.as_secs()));
            }
        }
    };
    // Whatever `o` left running in the background may still hold a pipe; don't wait for it long.
    let read = |pipe: mpsc::Receiver<String>| pipe.recv_timeout(Duration::from_secs(5)).unwrap_or_default();
    let (out, err) = (read(stdout), read(stderr));
    if status.success() {
        return Ok(out);
    }
    let err = err.trim();
    if !err.is_empty() {
        return Err(err.to_string());
    }
    let code = proc::exit_code(status).map(|c| c.to_string()).unwrap_or_else(|| "?".into());
    Err(format!("PR 검토 요청이 실패했습니다 (o review 종료 코드 {code}). 이유가 출력되지 않았습니다."))
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

    #[cfg(windows)]
    fn cmd(script: &str) -> Command {
        let mut command = Command::new("cmd");
        command.args(["/d", "/c", script]);
        command
    }

    #[cfg(windows)]
    #[test]
    fn a_failure_always_says_why() {
        assert_eq!(finish(cmd("echo started"), REVIEW_TIMEOUT).unwrap().trim(), "started");
        assert_eq!(finish(cmd("echo gh: not logged in 1>&2 & exit /b 1"), REVIEW_TIMEOUT).unwrap_err(), "gh: not logged in");
        // Nothing on stderr: the exit code instead of an empty message.
        let silent = finish(cmd("exit /b 3"), REVIEW_TIMEOUT).unwrap_err();
        assert!(silent.contains("종료 코드 3"), "{silent}");
    }

    #[cfg(windows)]
    #[test]
    fn a_review_request_that_hangs_is_ended_at_the_deadline() {
        let started = Instant::now();
        let error = finish(cmd("ping -n 30 127.0.0.1 >nul"), Duration::from_secs(1)).unwrap_err();
        assert!(error.contains("1초 안에 끝나지 않아"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(15));
    }
}
