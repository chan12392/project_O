//! Fixed-repository release checks. Only the explicit install command executes an installer.
use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{LazyLock, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
static INSTALLING: Mutex<()> = Mutex::new(());
static CANCELLED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Update>> = Mutex::new(None);

pub fn start_checks() {
    if BUILD_SHA.is_empty() { return; }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(10));
        loop {
            if let Ok(release) = latest() { *PENDING.lock().unwrap() = release; }
            std::thread::sleep(Duration::from_secs(6 * 60 * 60));
        }
    });
}

#[tauri::command]
pub fn update_pending() -> Option<Update> {
    PENDING.lock().unwrap().take()
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
}

#[derive(Serialize)]
pub struct Check {
    release: Option<Update>,
    message: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    prerelease: bool,
    draft: bool,
    published_at: Option<String>,
    body: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    state: String,
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
        let date = r.published_at.as_deref().and_then(published)?;
        let asset = r.assets.into_iter().find(|a| ASSET.is_match(&a.name) && a.state == "uploaded" && (1..=MAX_SETUP_SIZE).contains(&a.size))?;
        Some((date, Update { tag: r.tag_name, published_at: r.published_at?, notes: r.body.unwrap_or_default(), asset: asset.name, size: asset.size }))
    }).max_by_key(|(date, _)| *date);
    Ok(newest.and_then(|(date, update)| {
        // A local build can only reach this path through the manual button.
        if sha.is_empty() || (!sha.starts_with(&update.tag[16..]) && published(built_at).is_some_and(|built| date > built)) {
            Some(update)
        } else { None }
    }))
}

fn gh(args: &[&str], timeout: Duration) -> Result<String, String> {
    let executable = resolve::which("gh").ok_or_else(|| HELP.to_string())?;
    if cfg!(windows) && executable.extension().is_none_or(|e| !e.eq_ignore_ascii_case("exe")) {
        return Err(HELP.into());
    }
    let mut command = Command::new(executable);
    command.args(args).env("GH_HOST", "github.com").env("GH_PROMPT_DISABLED", "1");
    let (status, out, _) = proc::capture(&mut command, timeout).map_err(|_| format!("GitHub 확인이 시간 안에 끝나지 않았습니다. {HELP}"))?;
    if !status.success() { return Err(format!("릴리스에 접근하지 못했습니다. 네트워크와 저장소 접근 권한을 확인해 주세요. {HELP}")); }
    Ok(out)
}

fn latest() -> Result<Option<Update>, String> {
    let endpoint = format!("repos/{REPOSITORY}/releases?per_page=20");
    let body = gh(&["api", "--hostname", "github.com", &endpoint], Duration::from_secs(30))?;
    select_release(&body, BUILD_SHA, BUILD_TIME)
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
    Ok(path)
}

#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, tag: String) -> Result<(), String> {
    if !valid_tag(&tag) { return Err("잘못된 릴리스 태그입니다".into()); }
    CANCELLED.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || {
        let _installing = INSTALLING.try_lock().map_err(|_| "업데이트가 이미 진행 중입니다")?;
        let update = latest()?.filter(|u| u.tag == tag).ok_or("릴리스가 바뀌었습니다. 다시 확인해 주세요")?;
        let dir = fresh_dir(&std::env::temp_dir(), "project_O-update")?;
        let result: Result<(), String> = (|| {
            let folder = dir.to_str().ok_or("임시 폴더 경로를 읽지 못했습니다")?;
            gh(&["release", "download", &update.tag, "--repo", REPOSITORY, "--pattern", &update.asset, "--dir", folder], Duration::from_secs(15 * 60))?;
            let setup = verify_download(&dir, &update)?;
            if CANCELLED.load(Ordering::SeqCst) { return Err("업데이트를 취소했습니다".into()); }
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
}
