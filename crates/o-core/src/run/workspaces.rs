//! Separate checkouts for a lead's work graph. They are kept for review and recovery.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::run::plan::Plan;
use crate::run::store::resolved_path;
use crate::sys::proc;

fn git(dir: &Path, args: &[&str]) -> io::Result<String> {
    let mut command = Command::new("git");
    command.current_dir(dir).args(args).env("LC_ALL", "C");
    proc::hide_window(&mut command);
    let out = command.output()?;
    if !out.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A clean committed base is required: never silently omit another session's edits.
fn base(project: &str) -> io::Result<Option<(PathBuf, PathBuf, String)>> {
    let project = Path::new(project);
    let root = match git(project, &["rev-parse", "--show-toplevel"]) {
        Ok(root) => root,
        Err(e) if e.to_string().contains("not a git repository") && !project.ancestors().any(|p| fs::symlink_metadata(p.join(".git")).is_ok()) => return Ok(None),
        Err(e) => return Err(io::Error::other(format!("Git 저장소를 확인하지 못했습니다: {e}"))),
    };
    let root = PathBuf::from(resolved_path(&root));
    let project = PathBuf::from(resolved_path(&project.to_string_lossy()));
    let relative = project.strip_prefix(&root).map_err(|_| io::Error::other("프로젝트가 Git 저장소 안에 있지 않습니다"))?;
    let status = git(&root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    if !status.is_empty() {
        return Err(io::Error::other("프로젝트에 커밋하지 않은 변경이 있어 분리하지 않았습니다. 변경을 보존한 뒤 다시 시도하세요"));
    }
    // `git worktree add` does not check out submodules, so a worker would start without their files.
    // Only the project's own subtree matters: a submodule elsewhere in the repository is never read.
    let scope = if relative.as_os_str().is_empty() { ".".to_string() } else { relative.to_string_lossy().replace('\\', "/") };
    let index = git(&root, &["ls-files", "--stage", "--", &scope])?;
    if let Some(entry) = index.lines().find(|line| line.starts_with("160000 ")) {
        let path = entry.split_once('\t').map_or(entry, |(_, path)| path);
        return Err(io::Error::other(format!("Git 서브모듈이 있는 저장소는 작업 폴더 분리를 아직 지원하지 않습니다: {path}")));
    }
    let base = git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    Ok(Some((root, relative.to_path_buf(), base)))
}

type Entries = BTreeMap<PathBuf, (bool, u64, SystemTime)>;

/// Do not follow links or copy nested repositories into an agent's independent folder.
fn inventory(root: &Path) -> io::Result<Entries> {
    let mut entries = Entries::new();
    let mut dirs = vec![PathBuf::new()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(root.join(&dir))? {
            let entry = entry?;
            let relative = dir.join(entry.file_name());
            let meta = fs::symlink_metadata(entry.path())?;
            let real = PathBuf::from(resolved_path(&entry.path().to_string_lossy()));
            if entry.file_name().to_string_lossy().eq_ignore_ascii_case(".git") || meta.file_type().is_symlink() || !real.starts_with(root) {
                return Err(io::Error::other(format!("링크 또는 중첩 Git 저장소는 복사할 수 없습니다: {}", relative.display())));
            }
            let is_dir = meta.is_dir();
            if !is_dir && !meta.is_file() { return Err(io::Error::other(format!("일반 파일이 아닙니다: {}", relative.display()))); }
            entries.insert(relative.clone(), (is_dir, meta.len(), meta.modified()?));
            if is_dir { dirs.push(relative); }
        }
    }
    Ok(entries)
}

fn copy_entries(source: &Path, target: &Path, entries: &Entries) -> io::Result<()> {
    fs::create_dir(target)?;
    for (path, (is_dir, _, _)) in entries {
        let src = source.join(path);
        let meta = fs::symlink_metadata(&src)?;
        if meta.file_type().is_symlink() || !PathBuf::from(resolved_path(&src.to_string_lossy())).starts_with(source) {
            return Err(io::Error::other(format!("복사 중 링크가 변경됐습니다: {}", path.display())));
        }
        if *is_dir { fs::create_dir(target.join(path))?; } else { fs::copy(src, target.join(path))?; }
    }
    Ok(())
}

pub(crate) fn preflight(project: &str) -> io::Result<()> {
    if base(project)?.is_none() { inventory(Path::new(&resolved_path(project)))?; }
    Ok(())
}

pub(super) fn create(home: &Path, run_id: &str, project: &str, plan: &Plan) -> io::Result<HashMap<String, String>> {
    let git_base = base(project)?;
    let root = git_base.as_ref().map(|(root, _, _)| root.clone()).unwrap_or_else(|| PathBuf::from(resolved_path(project)));
    if run_id.is_empty() || !run_id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(io::Error::other("올바르지 않은 실행 ID"));
    }
    let name = home.file_name().ok_or_else(|| io::Error::other("O 상태 폴더가 올바르지 않습니다"))?.to_string_lossy();
    let parent = home.with_file_name(format!("{name}-worktrees"));
    fs::create_dir_all(&parent)?;
    let metadata = fs::symlink_metadata(&parent)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("워크트리 보관 폴더가 일반 폴더가 아닙니다"));
    }
    let parent = PathBuf::from(resolved_path(&parent.to_string_lossy()));
    if parent.starts_with(&root) || parent.starts_with(home) || root.starts_with(&parent) {
        return Err(io::Error::other("워크트리 보관 폴더가 프로젝트 또는 O 상태 폴더와 겹칩니다"));
    }
    let run = parent.join(run_id);
    fs::create_dir(&run)?;
    let snapshot = if git_base.is_none() {
        let entries = inventory(&root)?;
        let source = run.join("base");
        copy_entries(&root, &source, &entries)?;
        if inventory(&root)? != entries { return Err(io::Error::other("복사 중 원본 프로젝트가 변경되어 시작하지 않았습니다")); }
        Some((source, entries))
    } else { None };
    let mut result = HashMap::new();
    for (i, card) in plan.cards.iter().enumerate() {
        // Numeric folder names are independent of model-generated card IDs.
        let path = run.join((i + 1).to_string());
        let path_text = path.to_string_lossy();
        let cwd = if let Some((_, relative, base_commit)) = &git_base {
            git(&root, &["worktree", "add", "--detach", "--", &path_text, base_commit])
                .map_err(|e| io::Error::other(format!("워크트리를 만들지 못했습니다: {e}. 만든 작업 폴더는 {}에 보존했습니다", run.display())))?;
            path.join(relative)
        } else {
            let (source, entries) = snapshot.as_ref().expect("non-Git snapshot");
            copy_entries(source, &path, entries)?;
            git(&path, &["init", "--quiet"])?;
            path.clone()
        };
        if !cwd.is_dir() { return Err(io::Error::other(format!("작업 폴더가 없습니다: {}", cwd.display()))); }
        let checkout = PathBuf::from(resolved_path(&path_text));
        let cwd = PathBuf::from(resolved_path(&cwd.to_string_lossy()));
        if !cwd.starts_with(&checkout) {
            return Err(io::Error::other(format!("작업 폴더가 워크트리 밖을 가리킵니다. 만든 폴더는 {}에 보존했습니다", run.display())));
        }
        result.insert(card.id.clone(), cwd.to_string_lossy().into_owned());
    }
    let current_base = base(project)
        .map_err(|e| io::Error::other(format!("워크트리 준비 중 프로젝트가 바뀌었습니다: {e}. 만든 폴더는 {}에 보존했습니다", run.display())))?;
    if current_base != git_base || snapshot.as_ref().is_some_and(|(_, entries)| inventory(&root).map_or(true, |current| current != *entries)) {
        return Err(io::Error::other(format!("워크트리 준비 중 프로젝트가 바뀌었습니다. 만든 폴더는 {}에 보존했습니다", run.display())));
    }
    Ok(result)
}
