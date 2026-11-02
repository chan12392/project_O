//! Fixed-repository release checks. Only the explicit install command executes an installer.
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use o_core::sys::{proc, resolve};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::installer_paths::{fresh_dir, no_links};

pub const REPOSITORY: &str = "chan12392/project_O";
pub const BUILD_SHA: &str = match option_env!("PROJECT_O_BUILD_SHA") { Some(v) => v, None => "" };
pub const BUILD_TIME: &str = match option_env!("PROJECT_O_BUILD_TIME") { Some(v) => v, None => "" };
const HELP: &str = "GitHub CLI(gh)를 설치하고 gh auth login --hostname github.com으로 로그인해 주세요. https://github.com/chan12392/project_O/releases 에서 직접 받을 수도 있습니다.";
const MAX_SETUP_SIZE: u64 = 1024 * 1024 * 1024;
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\Atest_1-[0-9]{8}-[0-9a-f]{7}\z").unwrap());
static ASSET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\Aproject_O_[0-9]+\.[0-9]+\.[0-9]+_x64-setup\.exe\z").unwrap());
static COMMIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[0-9a-f]{40}\z").unwrap());
static SHA256: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A[0-9a-f]{64}\z").unwrap());
/// Who publishes releases: the release workflow's token, never a person.
const RELEASE_AUTHOR: &str = "github-actions[bot]";
/// The approved channel: a release is offered only when its commit is already in this branch.
const APPROVED_BRANCH: &str = "test_1";
static INSTALLING: Mutex<()> = Mutex::new(());
static CANCELLED: AtomicBool = AtomicBool::new(false);
const CANCELLED_MESSAGE: &str = "업데이트를 취소했습니다";
static PENDING: Mutex<Option<Update>> = Mutex::new(None);

pub fn start_checks() {
    if BUILD_SHA.is_empty() { return; }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            let result = latest();
            *LAST_CHECK.lock().unwrap() = Some(last_check(&result, chrono::Utc::now().to_rfc3339()));
            if let Ok(release) = result { *PENDING.lock().unwrap() = release; }
            std::thread::sleep(Duration::from_secs(6 * 60 * 60));
        }
    });
}

#[tauri::command]
pub fn update_pending() -> Option<Update> {
    PENDING.lock().unwrap().take()
}

/// The last background check, kept so Settings → 정보 can say why updates are not arriving
/// (no gh, not signed in) instead of the check failing silently.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCheck {
    at: String,
    /// The release it found, if any.
    tag: Option<String>,
    error: Option<String>,
}

static LAST_CHECK: Mutex<Option<LastCheck>> = Mutex::new(None);

fn last_check(result: &Result<Option<Update>, String>, at: String) -> LastCheck {
    match result {
        Ok(release) => LastCheck { at, tag: release.as_ref().map(|u| u.tag.clone()), error: None },
        Err(error) => LastCheck { at, tag: None, error: Some(error.clone()) },
    }
}

/// What Settings → 정보 shows before anyone presses "업데이트 확인".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// Release builds check by themselves; local builds only on the button.
    automatic: bool,
    /// Whether a usable `gh` is on PATH (updates need it, signed in).
    gh: bool,
    last_check: Option<LastCheck>,
}

#[tauri::command]
pub fn update_status() -> UpdateStatus {
    UpdateStatus { automatic: !BUILD_SHA.is_empty(), gh: gh_command(&[]).is_ok(), last_check: LAST_CHECK.lock().unwrap().clone() }
}

#[tauri::command]
pub fn update_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    tag: String,
    published_at: String,
    notes: String,
    asset: String,
    size: u64,
    /// The commit the release was built from; checked against test_1 before it is offered.
    #[serde(skip)]
    target: String,
    /// Lowercase hex SHA-256 of the installer, from the asset's `digest` (`sha256:<hex>`).
    #[serde(skip)]
    sha256: String,
}

#[derive(Serialize)]
pub struct Check {
    release: Option<Update>,
    message: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    target_commitish: Option<String>,
    author: Option<Author>,
    prerelease: bool,
    draft: bool,
    published_at: Option<String>,
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Author {
    login: String,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    state: String,
    digest: Option<String>,
}

/// `sha256:<64 lowercase hex>` → the hex; anything else (missing, another algorithm) is `None`.
fn sha256_of(digest: Option<&str>) -> Option<String> {
    digest?.strip_prefix("sha256:").filter(|hex| SHA256.is_match(hex)).map(str::to_string)
}

fn published(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

fn valid_tag(tag: &str) -> bool {
    TAG.is_match(tag) && chrono::NaiveDate::parse_from_str(&tag[7..15], "%Y%m%d").is_ok()
}

fn select_release(json: &str, sha: &str, built_at: &str) -> Result<Option<Update>, String> {
    let releases: Vec<Release> = serde_json::from_str(json).map_err(|_| "릴리스 응답을 읽지 못했습니다")?;
    let newest = releases.into_iter().filter_map(|r| {
        if !r.prerelease || r.draft || !valid_tag(&r.tag_name) { return None; }
        // Only the release workflow publishes, and it pins the release to the commit named in the tag.
        if r.author.as_ref().is_none_or(|a| a.login != RELEASE_AUTHOR) { return None; }
        let target = r.target_commitish.filter(|t| COMMIT.is_match(t) && t.starts_with(&r.tag_name[16..]))?;
        let date = r.published_at.as_deref().and_then(published)?;
        let (asset, sha256) = r.assets.into_iter().find_map(|a| {
            let ok = ASSET.is_match(&a.name) && a.state == "uploaded" && (1..=MAX_SETUP_SIZE).contains(&a.size);
            // Without GitHub's digest the download could not be checked, so the release is not offered.
            let sha256 = sha256_of(a.digest.as_deref()).filter(|_| ok)?;
            Some((a, sha256))
        })?;
        Some((date, Update { tag: r.tag_name, published_at: r.published_at?, notes: r.body.unwrap_or_default(), asset: asset.name, size: asset.size, target, sha256 }))
    }).max_by_key(|(date, _)| *date);
    Ok(newest.and_then(|(date, update)| {
        // A local build can only reach this path through the manual button.
        if sha.is_empty() || (!sha.starts_with(&update.tag[16..]) && published(built_at).is_some_and(|built| date > built)) {
            Some(update)
        } else { None }
    }))
}

fn gh_command(args: &[&str]) -> Result<Command, String> {
    let executable = resolve::which("gh").ok_or_else(|| HELP.to_string())?;
    if cfg!(windows) && executable.extension().is_none_or(|e| !e.eq_ignore_ascii_case("exe")) {
        return Err(HELP.into());
    }
    let mut command = Command::new(executable);
    command.args(args).env("GH_HOST", "github.com").env("GH_PROMPT_DISABLED", "1");
    Ok(command)
}

fn gh(args: &[&str], timeout: Duration) -> Result<String, String> {
    let (status, out, _) = proc::capture(&mut gh_command(args)?, timeout).map_err(|_| format!("GitHub 확인이 시간 안에 끝나지 않았습니다. {HELP}"))?;
    if !status.success() { return Err(format!("릴리스에 접근하지 못했습니다. 네트워크와 저장소 접근 권한을 확인해 주세요. {HELP}")); }
    Ok(out)
}

/// Runs a download that "나중에" stops: the moment `cancelled` is set, the command and what it
/// started are ended (a job object), instead of finishing a 15-minute download first.
fn run_cancellable(command: &mut Command, timeout: Duration, cancelled: &AtomicBool) -> Result<(), String> {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut run = proc::Contained::spawn(command).map_err(|e| format!("다운로드를 시작하지 못했습니다: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled.load(Ordering::SeqCst) { return Err(CANCELLED_MESSAGE.into()); }
        match run.child.try_wait().map_err(|e| e.to_string())? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => return Err(format!("설치 파일을 받지 못했습니다. 네트워크와 저장소 접근 권한을 확인해 주세요. {HELP}")),
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(200)),
            None => return Err(format!("설치 파일 다운로드가 시간 안에 끝나지 않았습니다. {HELP}")),
        }
    }
}

/// One install at a time. The cancel flag is cleared only by the install that got the lock, so
/// a second click cannot undo "나중에" of the one already downloading.
fn begin_install() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    let guard = INSTALLING.try_lock().map_err(|_| "업데이트가 이미 진행 중입니다".to_string())?;
    CANCELLED.store(false, Ordering::SeqCst);
    Ok(guard)
}

fn latest() -> Result<Option<Update>, String> {
    let endpoint = format!("repos/{REPOSITORY}/releases?per_page=20");
    let body = gh(&["api", "--hostname", "github.com", &endpoint], Duration::from_secs(30))?;
    approved(select_release(&body, BUILD_SHA, BUILD_TIME)?, |target| {
        let compare = format!("repos/{REPOSITORY}/compare/{APPROVED_BRANCH}...{target}");
        gh(&["api", "--hostname", "github.com", &compare, "--jq", ".status"], Duration::from_secs(30))
    })
}

/// Offers a release only when its commit is already in test_1 (`compare/test_1...<sha>` is
/// identical or behind). A failed lookup or any other answer refuses it.
fn approved(update: Option<Update>, compare_status: impl FnOnce(&str) -> Result<String, String>) -> Result<Option<Update>, String> {
    let Some(update) = update else { return Ok(None) };
    if !COMMIT.is_match(&update.target) { return Err("릴리스의 커밋을 확인하지 못했습니다".into()); }
    match compare_status(&update.target)?.trim() {
        "identical" | "behind" => Ok(Some(update)),
        _ => Err(format!("릴리스 {}의 커밋이 {APPROVED_BRANCH}에 없어 설치를 제안하지 않습니다", update.tag)),
    }
}

#[tauri::command]
pub async fn update_check() -> Result<Check, String> {
    tauri::async_runtime::spawn_blocking(|| latest().map(|release| Check {
        message: if release.is_some() { "새 버전이 있습니다" } else { "설치할 새 버전이 없습니다" }.into(), release,
    })).await.map_err(|e| e.to_string())?
}

fn verify_download(dir: &Path, update: &Update) -> Result<std::path::PathBuf, String> {
    if !ASSET.is_match(&update.asset) || !(1..=MAX_SETUP_SIZE).contains(&update.size) { return Err("잘못된 설치 파일 정보입니다".into()); }
    let path = dir.join(&update.asset);
    no_links(&path)?;
    let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() != update.size { return Err("설치 파일 크기가 일치하지 않습니다. 다시 시도해 주세요".into()); }
    if !SHA256.is_match(&update.sha256) || file_sha256(&path)? != update.sha256 {
        return Err("설치 파일의 SHA-256이 릴리스와 일치하지 않습니다. 설치하지 않았습니다".into());
    }
    Ok(path)
}

fn file_sha256(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, tag: String) -> Result<(), String> {
    if !valid_tag(&tag) { return Err("잘못된 릴리스 태그입니다".into()); }
    tauri::async_runtime::spawn_blocking(move || {
        let _installing = begin_install()?;
        let update = latest()?.filter(|u| u.tag == tag).ok_or("릴리스가 바뀌었습니다. 다시 확인해 주세요")?;
        let dir = fresh_dir(&std::env::temp_dir(), "project_O-update")?;
        let result: Result<(), String> = (|| {
            let folder = dir.to_str().ok_or("임시 폴더 경로를 읽지 못했습니다")?;
            let mut download = gh_command(&["release", "download", &update.tag, "--repo", REPOSITORY, "--pattern", &update.asset, "--dir", folder])?;
            run_cancellable(&mut download, Duration::from_secs(15 * 60), &CANCELLED)?;
            let setup = verify_download(&dir, &update)?;
            if CANCELLED.load(Ordering::SeqCst) { return Err(CANCELLED_MESSAGE.into()); }
            // Interactive NSIS owns errors, cancellation and the final 'run app' checkbox.
            proc::spawn_detached(&mut Command::new(setup)).map_err(|e| format!("설치 프로그램을 열지 못했습니다: {e}"))?;
            Ok(())
        })();
        if result.is_err() { let _ = fs::remove_dir_all(&dir); }
        result
    }).await.map_err(|e| e.to_string())??;
    app.exit(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("../../tests/releases.json");

    #[test]
    fn newest_uploaded_prerelease_only_and_commit_and_time_must_both_change() {
        let next = select_release(FIXTURE, "abcdef012345678901234567890123456789012345", "2026-11-01T10:00:00Z").unwrap().unwrap();
        assert_eq!(next.tag, "test_1-20261102-1234567");
        assert!(select_release(FIXTURE, "1234567890123456789012345678901234567890123", "2026-11-01T00:00:00Z").unwrap().is_none());
        assert!(select_release(FIXTURE, "abcdef0", "2026-11-03T00:00:00Z").unwrap().is_none());
        assert!(select_release(FIXTURE, "abcdef0", "2026-11-02T12:00:00Z").unwrap().is_none());
        assert!(select_release(FIXTURE, "abcdef0", "bad date").unwrap().is_none());
        assert!(select_release(FIXTURE, "", "").unwrap().is_some());
        assert!(select_release("[]", "", "").unwrap().is_none());
        assert!(select_release("{}", "", "").is_err());
    }

    #[test]
    fn argv_names_are_strict_and_repository_is_fixed() {
        assert_eq!(REPOSITORY, "chan12392/project_O");
        assert!(valid_tag("test_1-20261102-123abcd"));
        for tag in ["--help", "test_1-20261302-123abcd", "test_1-20261102-123abcd\n", "test_1-20261102-123ABCD", "../test_1-20261102-123abcd", "test_1-20261102-123abcd --repo evil/x"] { assert!(!valid_tag(tag), "{tag}"); }
        assert!(ASSET.is_match("project_O_0.3.0_x64-setup.exe"));
        for asset in ["../project_O_0.3.0_x64-setup.exe", "project_O_0.3.0_x64-setup.exe\n", "project_O_*_x64-setup.exe", "--help", "project_O_0.3.0_x64-setup.cmd"] { assert!(!ASSET.is_match(asset), "{asset}"); }
    }

    #[test]
    fn only_workflow_releases_of_the_tagged_commit_are_selected() {
        // Newer releases by a person, with a branch name as target, with a commit that is not the
        // tag's, or with no author/target at all are all passed over.
        let next = select_release(FIXTURE, "", "").unwrap().unwrap();
        assert_eq!(next.tag, "test_1-20261102-1234567");
        assert_eq!(next.target, "1234567890abcdef1234567890abcdef12345678");
    }

    #[test]
    fn a_release_is_offered_only_when_its_commit_is_in_test_1() {
        let update = || select_release(FIXTURE, "", "").unwrap();
        let mut asked = String::new();
        let offered = approved(update(), |sha| { asked = sha.to_string(); Ok("behind\n".into()) }).unwrap();
        assert_eq!(asked, "1234567890abcdef1234567890abcdef12345678");
        assert!(offered.is_some());
        assert!(approved(update(), |_| Ok("identical".into())).unwrap().is_some());
        for status in ["ahead", "diverged", ""] {
            assert!(approved(update(), |_| Ok(status.into())).is_err(), "{status}");
        }
        assert!(approved(update(), |_| Err("gh 실패".into())).is_err());
        assert!(approved(None, |_| -> Result<String, String> { panic!("조회하지 않아야 합니다") }).unwrap().is_none());
    }

    /// The updater offers test_1-* pre-releases, so the workflow publishes them only from test_1,
    /// even when someone runs it by hand on another branch.
    #[test]
    fn release_workflow_publishes_only_from_test_1() {
        let workflow = include_str!("../../../../.github/workflows/release.yml").replace("\r\n", "\n");
        let jobs = workflow.matches("\n    runs-on:").count();
        assert!(jobs > 0);
        assert_eq!(workflow.matches("\n    if: github.ref == 'refs/heads/test_1'\n").count(), jobs, "모든 job이 test_1에서만 돌아야 합니다");
    }

    #[test]
    fn download_size_and_unique_directory_are_checked() {
        let first = fresh_dir(&std::env::temp_dir(), "project_O-update-test").unwrap();
        let second = fresh_dir(&std::env::temp_dir(), "project_O-update-test").unwrap();
        assert_ne!(first, second);
        let update = select_release(FIXTURE, "", "").unwrap().unwrap();
        assert!(verify_download(&first, &update).is_err());
        fs::write(first.join(&update.asset), b"bad").unwrap();
        assert!(verify_download(&first, &update).is_err());
        fs::write(first.join(&update.asset), vec![0; update.size as usize]).unwrap();
        assert!(verify_download(&first, &update).is_ok());
    }

    #[test]
    fn a_failed_background_check_is_kept_with_its_reason() {
        let found = select_release(FIXTURE, "", "").unwrap();
        assert_eq!(last_check(&Ok(found), "t".into()), LastCheck { at: "t".into(), tag: Some("test_1-20261102-1234567".into()), error: None });
        assert_eq!(last_check(&Ok(None), "t".into()).tag, None);
        let failed = last_check(&Err(HELP.into()), "t".into());
        assert_eq!(failed.error.as_deref(), Some(HELP));
        assert!(HELP.contains("gh auth login"));
    }

    /// "나중에" during a download ends it at once, and a second install click while the first is
    /// downloading neither starts another nor clears the first one's cancel.
    #[cfg(windows)]
    #[test]
    fn later_stops_the_download_and_a_second_click_keeps_the_cancel() {
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        let error = std::thread::scope(|s| {
            s.spawn(|| { std::thread::sleep(Duration::from_millis(500)); cancelled.store(true, Ordering::SeqCst); });
            run_cancellable(Command::new("ping").args(["-n", "30", "127.0.0.1"]), Duration::from_secs(60), &cancelled)
        }).unwrap_err();
        assert_eq!(error, CANCELLED_MESSAGE);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(run_cancellable(&mut Command::new("cmd").args(["/d", "/c", "exit /b 0"]), Duration::from_secs(60), &AtomicBool::new(false)).is_ok());
        assert!(run_cancellable(&mut Command::new("cmd").args(["/d", "/c", "exit /b 1"]), Duration::from_secs(60), &AtomicBool::new(false)).is_err());

        let first = begin_install().unwrap();
        CANCELLED.store(true, Ordering::SeqCst);
        assert!(begin_install().is_err());
        assert!(CANCELLED.load(Ordering::SeqCst), "두 번째 클릭이 취소를 되돌리면 안 됩니다");
        drop(first);
        let _second = begin_install().unwrap();
        assert!(!CANCELLED.load(Ordering::SeqCst));
    }

    /// A file of the right size but other bytes (a swapped or tampered asset) is refused:
    /// the download must match the SHA-256 GitHub reports for the release asset.
    #[test]
    fn download_must_match_the_release_asset_digest() {
        let dir = fresh_dir(&std::env::temp_dir(), "project_O-update-test").unwrap();
        let update = select_release(FIXTURE, "", "").unwrap().unwrap();
        fs::write(dir.join(&update.asset), vec![1; update.size as usize]).unwrap();
        assert!(verify_download(&dir, &update).is_err());
        fs::write(dir.join(&update.asset), vec![0; update.size as usize]).unwrap();
        assert!(verify_download(&dir, &update).is_ok());
        let _ = fs::remove_dir_all(&dir);
    }
}
