//! Local image previews are confined to the selected project or its recorded worktree.
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use crate::run::store;

const MAX_IMAGE: u64 = 10 * 1024 * 1024;

#[derive(Serialize)]
pub struct Image {
    pub mime: &'static str,
    pub bytes: Vec<u8>,
}

fn no_links(root: &Path, candidate: &Path) -> bool {
    let root_parts: Vec<_> = root.components().collect();
    let parts: Vec<_> = candidate.components().collect();
    if parts.len() <= root_parts.len() || !root_parts.iter().zip(&parts).all(|(a, b)| {
        if cfg!(windows) { a.as_os_str().to_string_lossy().eq_ignore_ascii_case(&b.as_os_str().to_string_lossy()) } else { a == b }
    }) { return false; }
    let mut current = root.to_path_buf();
    for part in &parts[root_parts.len()..] {
        current.push(part.as_os_str());
        let Ok(meta) = fs::symlink_metadata(&current) else { return false };
        if meta.file_type().is_symlink() { return false; }
        #[cfg(windows)]
        { use std::os::windows::fs::MetadataExt; if meta.file_attributes() & 0x400 != 0 { return false; } }
    }
    true
}

fn image_path(home: &Path, user: &Path, project: &str, run: Option<&str>, card: Option<&str>, path: &str) -> Result<PathBuf, String> {
    if project.starts_with(r"\\") || project.starts_with("//") { return Err("네트워크 프로젝트의 이미지는 열 수 없습니다".into()); }
    crate::projects::check(home, user, project)?;
    let root = PathBuf::from(store::resolved_path(project));
    let mut roots = vec![root.clone()];
    if let (Some(run), Some(card)) = (run, card) {
        if !store::is_run_id(run) { return Err("올바르지 않은 실행 ID입니다".into()); }
        if let Ok(plan) = store::read_plan(home, run) {
            let plan_project = plan["project"].as_str().ok_or("프로젝트 정보가 없습니다")?;
            if !crate::projects::same(&store::resolved_path(plan_project), &root.to_string_lossy()) { return Err("다른 프로젝트의 실행입니다".into()); }
            let cards = store::plan_card_ids(&plan).map_err(|e| e.to_string())?;
            let index = cards.iter().position(|id| id == card).ok_or("작업을 찾지 못했습니다")?;
            let name = home.file_name().ok_or("상태 폴더가 올바르지 않습니다")?.to_string_lossy();
            let base = home.with_file_name(format!("{name}-worktrees"));
            let worktree = base.join(run).join((index + 1).to_string());
            if base.parent().is_some_and(|parent| no_links(parent, &base)) && no_links(&base, &worktree) && worktree.is_dir() { roots.push(PathBuf::from(store::resolved_path(&worktree.to_string_lossy()))); }
        }
    }
    let path = Path::new(path);
    if path.components().any(|part| matches!(part, std::path::Component::ParentDir)) || path.to_string_lossy().starts_with(r"\\") || path.to_string_lossy().starts_with("//") {
        return Err("네트워크 또는 상위 폴더 경로는 열 수 없습니다".into());
    }
    let candidates: Vec<PathBuf> = if path.is_absolute() { vec![path.to_path_buf()] } else { roots.iter().rev().map(|root| root.join(path)).collect() };
    for candidate in candidates {
        let norm = |path: &Path| { let text = path.to_string_lossy().replace('\\', "/"); if cfg!(windows) { text.to_lowercase() } else { text } };
        let text = norm(&candidate);
        if !roots.iter().any(|root| { let prefix = norm(root); text.starts_with(&format!("{prefix}/")) && no_links(root, &candidate) }) { continue; }
        if let Ok(candidate) = fs::canonicalize(candidate) {
            let candidate = PathBuf::from(store::resolved_path(&candidate.to_string_lossy()));
            let text = norm(&candidate);
            if roots.iter().any(|root| { let root = norm(root); text.starts_with(&format!("{root}/")) }) && candidate.is_file() { return Ok(candidate); }
        }
    }
    Err("프로젝트 또는 해당 작업 폴더의 이미지만 열 수 있습니다".into())
}

pub fn read(home: &Path, user: &Path, project: &str, run: Option<&str>, card: Option<&str>, path: &str) -> Result<Image, String> {
    let path = image_path(home, user, project, run, card, path)?;
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_IMAGE { return Err("이미지는 10MB까지 표시할 수 있습니다".into()); }
    let mut bytes = Vec::new();
    file.take(MAX_IMAGE + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_IMAGE { return Err("이미지는 10MB까지 표시할 수 있습니다".into()); }
    let mime = match ext.as_str() {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => "image/png",
        "jpg" | "jpeg" if bytes.starts_with(b"\xff\xd8\xff") => "image/jpeg",
        "gif" if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") => "image/gif",
        "webp" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => "image/webp",
        _ => return Err("지원하지 않는 이미지 형식입니다".into()),
    };
    Ok(Image { mime, bytes })
}

pub fn open(home: &Path, user: &Path, project: &str, run: Option<&str>, card: Option<&str>, path: &str, folder: bool) -> Result<(), String> {
    // Check the bytes as well as the extension before asking the OS to open the file.
    read(home, user, project, run, card, path)?;
    let path = image_path(home, user, project, run, card, path)?;
    #[cfg(windows)]
    let mut command = { let mut c = Command::new("explorer.exe"); if folder { c.arg(format!("/select,{}", path.display())); } else { c.arg(&path); } c };
    #[cfg(not(windows))]
    let mut command = { let mut c = Command::new("xdg-open"); c.arg(if folder { path.parent().unwrap_or(&path) } else { &path }); c };
    crate::sys::proc::hide_window(&mut command);
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn previews_stay_in_project_or_recorded_card_worktree() {
        let root = std::env::temp_dir().join(format!("o-image-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let user = root.join("user");
        let home = root.join("state");
        let project = root.join("project");
        let other = root.join("other");
        let worktree = root.join("state-worktrees").join("run1").join("1");
        for dir in [&user, &home, &project, &other, &worktree] { fs::create_dir_all(dir).unwrap(); }
        let png = b"\x89PNG\r\n\x1a\nexample";
        fs::write(project.join("a.png"), png).unwrap();
        fs::write(worktree.join("b.png"), png).unwrap();
        fs::write(other.join("secret.png"), png).unwrap();
        fs::write(project.join("bad.png"), b"not an image").unwrap();
        fs::create_dir_all(home.join("runs/run1")).unwrap();
        fs::write(home.join("runs/run1/plan.json"), serde_json::json!({"project": project.to_string_lossy(), "cards": [{"id": "a"}]}).to_string()).unwrap();
        let p = project.to_str().unwrap();
        assert_eq!(read(&home, &user, p, None, None, "a.png").unwrap().bytes, png);
        assert_eq!(read(&home, &user, p, Some("run1"), Some("a"), "b.png").unwrap().bytes, png);
        assert!(read(&home, &user, p, None, None, other.join("secret.png").to_str().unwrap()).is_err());
        assert!(read(&home, &user, p, None, None, "../other/secret.png").is_err());
        assert!(read(&home, &user, p, None, None, "//example.invalid/image.png").is_err());
        assert!(read(&home, &user, p, None, None, "bad.png").is_err());
        assert!(read(&home, &user, p, Some("run1"), Some("wrong"), "b.png").is_err());
    }
}
