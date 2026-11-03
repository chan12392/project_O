//! Separate checkouts for a lead's work graph. They are kept for review and recovery.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::run::plan::Plan;
use crate::run::store::resolved_path;
use crate::sys::proc;

fn git(dir: &Path, args: &[&str]) -> io::Result<String> {
    let mut command = Command::new("git");
    command.current_dir(dir).args(args);
    proc::hide_window(&mut command);
    let out = command.output()?;
    if !out.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// A clean committed base is required: never silently omit another session's edits.
fn base(project: &str) -> io::Result<(PathBuf, PathBuf, String)> {
    let project = Path::new(project);
    let root = git(project, &["rev-parse", "--show-toplevel"])
        .map_err(|e| io::Error::other(format!("워크트리 분리에는 Git 저장소가 필요합니다: {e}")))?;
    let root = PathBuf::from(resolved_path(&root));
    let project = PathBuf::from(resolved_path(&project.to_string_lossy()));
    let relative = project.strip_prefix(&root).map_err(|_| io::Error::other("프로젝트가 Git 저장소 안에 있지 않습니다"))?;
    let status = git(&root, &["status", "--porcelain=v1", "--untracked-files=all"])?;
    if !status.is_empty() {
        return Err(io::Error::other("프로젝트에 커밋하지 않은 변경이 있어 분리하지 않았습니다. 변경을 보존한 뒤 다시 시도하세요"));
    }
    let base = git(&root, &["rev-parse", "--verify", "HEAD^{commit}"])?;
    Ok((root, relative.to_path_buf(), base))
}

pub(crate) fn preflight(project: &str) -> io::Result<()> {
    base(project).map(|_| ())
}

pub(super) fn create(home: &Path, run_id: &str, project: &str, plan: &Plan) -> io::Result<HashMap<String, String>> {
    let (root, relative, base_commit) = base(project)?;
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
    let mut result = HashMap::new();
    for (i, card) in plan.cards.iter().enumerate() {
        // Numeric folder names are independent of model-generated card IDs.
        let path = run.join((i + 1).to_string());
        let path_text = path.to_string_lossy();
        git(&root, &["worktree", "add", "--detach", "--", &path_text, &base_commit])
            .map_err(|e| io::Error::other(format!("워크트리를 만들지 못했습니다: {e}. 만든 작업 폴더는 {}에 보존했습니다", run.display())))?;
        let cwd = path.join(&relative);
        if !cwd.is_dir() { return Err(io::Error::other(format!("작업 폴더가 없습니다: {}", cwd.display()))); }
        let checkout = PathBuf::from(resolved_path(&path_text));
        let cwd = PathBuf::from(resolved_path(&cwd.to_string_lossy()));
        if !cwd.starts_with(&checkout) {
            return Err(io::Error::other(format!("작업 폴더가 워크트리 밖을 가리킵니다. 만든 폴더는 {}에 보존했습니다", run.display())));
        }
        result.insert(card.id.clone(), cwd.to_string_lossy().into_owned());
    }
    let (current_root, current_relative, current_base) = base(project)
        .map_err(|e| io::Error::other(format!("워크트리 준비 중 프로젝트가 바뀌었습니다: {e}. 만든 폴더는 {}에 보존했습니다", run.display())))?;
    if current_root != root || current_relative != relative || current_base != base_commit {
        return Err(io::Error::other(format!("워크트리 준비 중 프로젝트가 바뀌었습니다. 만든 폴더는 {}에 보존했습니다", run.display())));
    }
    Ok(result)
}
