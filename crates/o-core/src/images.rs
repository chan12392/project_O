//! Local image previews are confined to the selected project or its recorded worktree.
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use crate::run::events::OEvent;
use crate::run::store;

const MAX_IMAGE: u64 = 10 * 1024 * 1024;
/// The tool name under which the runner records a card's separate work folder.
const WORK_FOLDER: &str = "O 작업 폴더";

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
    // `roots` bound what may be opened; `bases` resolve relative paths, the card's own folder first.
    let mut roots = vec![root.clone()];
    let mut bases = vec![root.clone()];
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
            if base.parent().is_some_and(|parent| no_links(parent, &base)) && no_links(&base, &worktree) && worktree.is_dir() {
                let checkout = PathBuf::from(store::resolved_path(&worktree.to_string_lossy()));
                // The folder the card ran in as recorded at its start (`<checkout>/<project's place in the repository>`),
                // not one guessed again from the repository as it is now.
                let recorded = store::read_events(home, run).into_iter().find_map(|event| match event {
                    OEvent::CardTool { card_id, name, summary, .. } if card_id == card && name == WORK_FOLDER => Some(PathBuf::from(store::resolved_path(&summary))),
                    _ => None,
                });
                let card_base = match recorded {
                    Some(cwd) if cwd == checkout || (no_links(&checkout, &cwd) && cwd.is_dir()) => Some(cwd),
                    Some(_) => None,
                    None => Some(checkout.clone()),
                };
                if let Some(cwd) = card_base {
                    roots.push(checkout);
                    bases.insert(0, cwd);
                }
            }
        }
    }
    let path = Path::new(path);
    if path.components().any(|part| matches!(part, std::path::Component::ParentDir)) || path.to_string_lossy().starts_with(r"\\") || path.to_string_lossy().starts_with("//") {
        return Err("네트워크 또는 상위 폴더 경로는 열 수 없습니다".into());
    }
    let candidates: Vec<PathBuf> = if path.is_absolute() { vec![path.to_path_buf()] } else { bases.iter().map(|base| base.join(path)).collect() };
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

    #[test]
    fn relative_previews_use_the_folder_the_card_ran_in() {
        let root = std::env::temp_dir().join(format!("o-image-sub-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap()));
        let user = root.join("user");
        let home = root.join("state");
        // The project is `repo/app`; each card ran in `<checkout>/app`.
        let project = root.join("repo").join("app");
        let runs = root.join("state-worktrees").join("run1");
        let (first, second) = (runs.join("1"), runs.join("2"));
        let outside = root.join("outside");
        for dir in [&user, &home, &project.join("assets"), &first.join("app/assets"), &second.join("app"), &outside] { fs::create_dir_all(dir).unwrap(); }
        let image = |tag: &str| [b"\x89PNG\r\n\x1a\n".as_slice(), tag.as_bytes()].concat();
        fs::write(project.join("assets/output.png"), image("original")).unwrap();
        fs::write(project.join("assets/only.png"), image("only original")).unwrap();
        fs::write(first.join("app/assets/output.png"), image("card a")).unwrap();
        fs::write(first.join("top.png"), image("checkout top")).unwrap();
        fs::write(outside.join("x.png"), image("outside")).unwrap();
        fs::create_dir_all(home.join("runs/run1")).unwrap();
        fs::write(home.join("runs/run1/plan.json"), serde_json::json!({"project": project.to_string_lossy(), "cards": [{"id": "a"}, {"id": "b"}, {"id": "c"}]}).to_string()).unwrap();
        let folder = |card: &str, path: &Path| store::append_event(&home, "run1", &OEvent::CardTool { at: "2026-01-01T00:00:00Z".into(), card_id: card.into(), name: WORK_FOLDER.into(), summary: store::resolved_path(&path.to_string_lossy()) }).unwrap();
        folder("a", &first.join("app"));
        folder("b", &second.join("app"));
        // A forged record pointing out of the card's checkout gives no card folder at all.
        fs::create_dir_all(runs.join("3")).unwrap();
        folder("c", &outside);
        let p = project.to_str().unwrap();
        let read_as = |card: Option<&str>, path: &str| read(&home, &user, p, card.map(|_| "run1"), card, path).map(|image| image.bytes);
        // The new work result wins over the original image of the same name.
        assert_eq!(read_as(Some("a"), "assets/output.png").unwrap(), image("card a"));
        assert_eq!(read_as(None, "assets/output.png").unwrap(), image("original"));
        // A file the card did not make still comes from the original project.
        assert_eq!(read_as(Some("a"), "assets/only.png").unwrap(), image("only original"));
        // Relative paths start at the card's folder, not the checkout root; an absolute path in the checkout still opens.
        assert!(read_as(Some("a"), "top.png").is_err());
        assert_eq!(read_as(Some("a"), first.join("top.png").to_str().unwrap()).unwrap(), image("checkout top"));
        // Another card's folder and outside paths stay closed.
        assert_eq!(read_as(Some("b"), "assets/output.png").unwrap(), image("original"));
        assert!(read_as(Some("b"), first.join("app/assets/output.png").to_str().unwrap()).is_err());
        assert!(read_as(Some("c"), "x.png").is_err());
        assert!(read_as(Some("c"), outside.join("x.png").to_str().unwrap()).is_err());

        // A project at the repository root ran in the checkout itself.
        let project = root.join("whole");
        let checkout = root.join("state-worktrees").join("run2").join("1");
        for dir in [&project.join("assets"), &checkout.join("assets")] { fs::create_dir_all(dir).unwrap(); }
        fs::write(project.join("assets/output.png"), image("original")).unwrap();
        fs::write(checkout.join("assets/output.png"), image("card")).unwrap();
        fs::create_dir_all(home.join("runs/run2")).unwrap();
        fs::write(home.join("runs/run2/plan.json"), serde_json::json!({"project": project.to_string_lossy(), "cards": [{"id": "a"}]}).to_string()).unwrap();
        store::append_event(&home, "run2", &OEvent::CardTool { at: "2026-01-01T00:00:00Z".into(), card_id: "a".into(), name: WORK_FOLDER.into(), summary: store::resolved_path(&checkout.to_string_lossy()) }).unwrap();
        assert_eq!(read(&home, &user, project.to_str().unwrap(), Some("run2"), Some("a"), "assets/output.png").unwrap().bytes, image("card"));
    }
}
