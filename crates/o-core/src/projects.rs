//! The project list the app's sidebar shows (ADR-0009).
//!
//! Every direct subfolder of the projects root (default `<home>/.project_o/project`)
//! is a project; the root's `test` subfolder is a group whose subfolders are
//! projects; folders outside the root are registered by hand, like Orca's
//! "프로젝트 추가". O keeps only the list, never the folders:
//!
//! ```text
//! projects.json   { "root": null | "<folder>", "registered": ["<folder>", …] }
//! ```
//!
//! Nothing here deletes a folder. Unregistering forgets a path; the only folder
//! ever made is the root itself or a new empty project inside it.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::context::normalize_project;
use crate::policy::{check_project, empty_counts, PolicyContext};
use crate::run::store::{resolved_path, to_pretty};

pub const FILE: &str = "projects.json";
/// The root's subfolder shown as a group of test projects.
pub const TEST_GROUP: &str = "test";

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    root: Option<String>,
    #[serde(default)]
    registered: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A direct subfolder of the root.
    Root,
    /// A subfolder of the root's `test` folder.
    Test,
    /// A folder registered by hand.
    Registered,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub path: String,
    pub name: String,
    pub kind: Kind,
    /// A registered folder that is gone; it stays listed so it can be taken off.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    pub root: String,
    /// Whether the root is the default one (no folder chosen in Settings).
    pub default_root: bool,
    pub root_exists: bool,
    pub projects: Vec<Listed>,
}

/// `<home>/.project_o/project`: user data inside the person's home, never in O's state folder.
pub fn default_root(user_home: &Path) -> PathBuf {
    user_home.join(".project_o").join("project")
}

/// Folders compare as the run index compares them: any slash, no trailing one, and on Windows any case.
pub fn same(a: &str, b: &str) -> bool {
    let (a, b) = (normalize_project(a), normalize_project(b));
    if cfg!(windows) {
        a.eq_ignore_ascii_case(&b)
    } else {
        a == b
    }
}

fn file(home: &Path) -> PathBuf {
    home.join(FILE)
}

/// The saved list; a file that cannot be read is an error, so a save never drops what it held.
fn load(home: &Path) -> Result<Saved, String> {
    match fs::read_to_string(file(home)) {
        Ok(text) => serde_json::from_str(&text).map_err(|e| format!("{}을(를) 읽지 못했습니다: {e}", file(home).display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Saved::default()),
        Err(e) => Err(format!("{}을(를) 읽지 못했습니다: {e}", file(home).display())),
    }
}

/// Written whole to a side file and renamed over, so a reader never sees half a list.
fn save(home: &Path, saved: &Saved) -> Result<(), String> {
    let write = || -> io::Result<()> {
        fs::create_dir_all(home)?;
        let side = home.join(format!("{FILE}.{}.tmp", std::process::id()));
        fs::write(&side, to_pretty(saved)?)?;
        fs::rename(&side, file(home))
    };
    write().map_err(|e| format!("프로젝트 목록을 저장하지 못했습니다: {e}"))
}

/// The list is read, changed and written whole by the app, the preview window and `o`, which
/// share one state folder: whoever changes it holds this lock from the read to the write, so a
/// change another process saved in between is not written over. The lock is a file of its own
/// (the list file is replaced by every save).
fn locked(home: &Path) -> Result<fs::File, String> {
    let open = || -> io::Result<fs::File> {
        fs::create_dir_all(home)?;
        let lock = fs::OpenOptions::new().create(true).truncate(false).write(true).open(home.join(format!("{FILE}.lock")))?;
        lock.lock()?;
        Ok(lock)
    };
    open().map_err(|e| format!("프로젝트 목록을 잠그지 못했습니다: {e}"))
}

fn root_of(saved: &Saved, user_home: &Path) -> String {
    saved.root.clone().unwrap_or_else(|| normalize_project(&default_root(user_home).to_string_lossy()))
}

/// The checks a card's project gets (`policy::check_project`), without O's run counts:
/// not O's state folder (nor around it), not the home folder itself or above it, no mirror.
fn refusal(home: &Path, user_home: &Path, path: &str, must_exist: bool) -> Option<String> {
    let folder = normalize_project(&resolved_path(path));
    let inside = |dir: &Path| {
        let base = normalize_project(&resolved_path(&dir.to_string_lossy()));
        let (folder, base) = if cfg!(windows) { (folder.to_lowercase(), base.to_lowercase()) } else { (folder.clone(), base) };
        folder == base || folder.starts_with(&format!("{base}/"))
    };
    if home.parent().is_some_and(|parent| inside(&parent.join("project_O-agents"))) {
        return Some("에이전트 프로필 폴더는 프로젝트로 등록할 수 없습니다".into());
    }
    #[cfg(windows)]
    if std::env::var_os("SystemRoot").is_some_and(|root| inside(Path::new(&root))) {
        return Some("Windows 시스템 폴더는 프로젝트로 등록할 수 없습니다".into());
    }
    let ctx = PolicyContext {
        now: Utc::now(),
        active_by_provider: empty_counts(),
        cooldowns: HashMap::new(),
        over_budget: HashMap::new(),
        project_exists: if must_exist { Box::new(|p| Path::new(p).is_dir()) } else { Box::new(|_| true) },
        is_mirror: Box::new(|p| Path::new(p).is_dir() && o_guard::is_mirror(p)),
        o_home: home.to_string_lossy().into_owned(),
        user_home: user_home.to_string_lossy().into_owned(),
        resolve: Box::new(resolved_path),
        profile_not_ready: Box::new(|_| None),
        agy_workers_allowed: false,
    };
    check_project(Some(path), &ctx).into_iter().next().map(|v| v.message)
}

#[cfg(windows)]
fn hidden(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn hidden(_meta: &fs::Metadata) -> bool {
    false
}

/// A folder's real subfolders: no files, no dot or hidden folders, and no links or
/// junctions (one could point at the home folder).
fn subfolders(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut found: Vec<(String, String)> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir() && !t.is_symlink()))
        .filter(|e| !e.metadata().is_ok_and(|m| hidden(&m)))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            (!name.starts_with('.')).then(|| (normalize_project(&e.path().to_string_lossy()), name))
        })
        .collect();
    found.sort_by_key(|(_, name)| name.to_lowercase());
    found
}

fn listing(saved: &Saved, user_home: &Path) -> Listing {
    let root = root_of(saved, user_home);
    let root_dir = PathBuf::from(&root);
    let mut projects = Vec::new();
    for (path, name) in subfolders(&root_dir) {
        if name.eq_ignore_ascii_case(TEST_GROUP) {
            projects.extend(subfolders(Path::new(&path)).into_iter().map(|(path, name)| Listed { path, name, kind: Kind::Test, missing: false }));
        } else {
            projects.push(Listed { path, name, kind: Kind::Root, missing: false });
        }
    }
    for path in saved.registered.iter().map(|p| normalize_project(p)) {
        if projects.iter().any(|p| same(&p.path, &path)) {
            continue;
        }
        let name = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(&path).to_string();
        let missing = !Path::new(&path).is_dir();
        projects.push(Listed { path, name, kind: Kind::Registered, missing });
    }
    Listing { root, default_root: saved.root.is_none(), root_exists: root_dir.is_dir(), projects }
}

/// The root, its projects and the registered folders.
pub fn list(home: &Path, user_home: &Path) -> Result<Listing, String> {
    Ok(listing(&load(home)?, user_home))
}

/// Whether a folder may be listed: the same rules as a card's project folder.
pub fn check(home: &Path, user_home: &Path, path: &str) -> Result<(), String> {
    match refusal(home, user_home, path.trim(), true) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// Open an explicitly selected, allowed project directory in the system file manager.
pub fn open(home: &Path, user_home: &Path, path: &str) -> Result<(), String> {
    check(home, user_home, path)?;
    #[cfg(windows)]
    let program = "explorer.exe";
    #[cfg(not(windows))]
    let program = "xdg-open";
    let mut command = std::process::Command::new(program);
    command.arg(path);
    crate::sys::proc::hide_window(&mut command);
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Registers a folder outside the root. A folder the root already lists is left as it is.
pub fn add(home: &Path, user_home: &Path, path: &str) -> Result<Listing, String> {
    let path = normalize_project(path.trim());
    check(home, user_home, &path)?;
    let _lock = locked(home)?;
    let mut saved = load(home)?;
    let root = root_of(&saved, user_home);
    if same(&path, &root) || same(&path, &format!("{root}/{TEST_GROUP}")) {
        return Err(format!("{path}는 프로젝트 루트(또는 그 test 묶음) 폴더 자체라 등록할 수 없습니다. 그 안의 폴더가 프로젝트입니다"));
    }
    if listing(&saved, user_home).projects.iter().any(|p| same(&p.path, &path)) {
        return Err(format!("이미 목록에 있는 폴더입니다: {path}"));
    }
    saved.registered.push(path);
    save(home, &saved)?;
    Ok(listing(&saved, user_home))
}

/// Takes a registered folder off the list. The folder and its history stay.
pub fn remove(home: &Path, user_home: &Path, path: &str) -> Result<Listing, String> {
    let _lock = locked(home)?;
    let mut saved = load(home)?;
    let before = saved.registered.len();
    saved.registered.retain(|p| !same(p, path.trim()));
    if saved.registered.len() == before {
        let inside = listing(&saved, user_home).projects.iter().any(|p| same(&p.path, path.trim()));
        return Err(if inside {
            "프로젝트 루트 안의 폴더는 목록에서 뺄 수 없습니다. 루트 밖으로 옮기거나 루트를 바꾸세요".to_string()
        } else {
            format!("등록된 폴더가 아닙니다: {}", path.trim())
        });
    }
    save(home, &saved)?;
    Ok(listing(&saved, user_home))
}

/// Settings → 프로젝트: another root (`None`: back to the default). It need not exist yet.
pub fn set_root(home: &Path, user_home: &Path, path: Option<&str>) -> Result<Listing, String> {
    let _lock = locked(home)?;
    let mut saved = load(home)?;
    saved.root = match path.map(|p| normalize_project(p.trim())) {
        None => None,
        Some(p) if same(&p, &default_root(user_home).to_string_lossy()) => None,
        Some(p) => {
            if let Some(reason) = refusal(home, user_home, &p, false) {
                return Err(reason);
            }
            Some(p)
        }
    };
    save(home, &saved)?;
    Ok(listing(&saved, user_home))
}

/// Makes the root folder (the app asks first).
pub fn create_root(home: &Path, user_home: &Path) -> Result<Listing, String> {
    let saved = load(home)?;
    let root = root_of(&saved, user_home);
    if let Some(reason) = refusal(home, user_home, &root, false) {
        return Err(reason);
    }
    fs::create_dir_all(&root).map_err(|e| format!("프로젝트 루트를 만들지 못했습니다({root}): {e}"))?;
    Ok(listing(&saved, user_home))
}

/// Names Windows keeps for devices; a folder may not take them.
const DEVICE_NAMES: &[&str] = &["con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9"];

/// A new project's folder name: letters (any language), digits, spaces, `.`, `_` and `-`,
/// at most 64, not starting with a dot, not ending in a dot or space, not a device name.
fn check_name(name: &str) -> Result<(), String> {
    let count = name.chars().count();
    if count == 0 || count > 64 {
        return Err("프로젝트 이름은 1~64자입니다".into());
    }
    if !name.chars().all(|c| c.is_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-')) {
        return Err("프로젝트 이름에는 글자·숫자·공백·. _ -만 쓸 수 있습니다".into());
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        return Err("프로젝트 이름은 점으로 시작하거나 점·공백으로 끝날 수 없습니다".into());
    }
    let stem = name.split('.').next().unwrap_or(name).to_lowercase();
    if DEVICE_NAMES.contains(&stem.as_str()) {
        return Err(format!("{name}은(는) Windows가 쓰는 이름이라 폴더 이름으로 쓸 수 없습니다"));
    }
    if name.eq_ignore_ascii_case(TEST_GROUP) {
        return Err("test는 시험용 프로젝트를 모으는 폴더 이름이라 쓸 수 없습니다".into());
    }
    Ok(())
}

/// "새 프로젝트": a new empty folder in the root. Returns its path and the new list.
pub fn new_project(home: &Path, user_home: &Path, name: &str) -> Result<(String, Listing), String> {
    let name = name.trim();
    check_name(name)?;
    let saved = load(home)?;
    let root = root_of(&saved, user_home);
    if !Path::new(&root).is_dir() {
        return Err(format!("프로젝트 루트 폴더가 없습니다: {root}. 먼저 만드세요"));
    }
    let path = format!("{root}/{name}");
    fs::create_dir(&path).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => format!("같은 이름의 폴더가 이미 있습니다: {path}"),
        _ => format!("폴더를 만들지 못했습니다({path}): {e}"),
    })?;
    Ok((path, listing(&saved, user_home)))
}
