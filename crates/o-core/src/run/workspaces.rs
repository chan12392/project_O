//! Separate checkouts for a lead's work graph. They are kept for review and recovery.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
type SnapshotEntry = Option<(Vec<u8>, fs::Permissions)>;
type Snapshot = BTreeMap<PathBuf, SnapshotEntry>;

fn snapshot(root: &Path, skip_git: bool) -> io::Result<Snapshot> {
    if PathBuf::from(resolved_path(&root.to_string_lossy())) != root {
        return Err(io::Error::other("복사본 경로가 변경되었습니다"));
    }
    if skip_git && !root.join(".git").is_dir() { return Err(io::Error::other("복사본 Git 폴더가 없습니다")); }
    let entries = inventory_copy(root, skip_git)?;
    let mut result = Snapshot::new();
    for (path, (dir, _, _)) in &entries {
        result.insert(path.clone(), if *dir { None } else {
            let file = root.join(path);
            Some((fs::read(&file)?, fs::metadata(file)?.permissions()))
        });
    }
    if entries != inventory_copy(root, skip_git)? {
        return Err(io::Error::other("복사본을 읽는 중 변경되었습니다"));
    }
    Ok(result)
}

/// Do not follow links or copy nested repositories into an agent's independent folder.
fn inventory(root: &Path) -> io::Result<Entries> {
    inventory_copy(root, false)
}

fn inventory_copy(root: &Path, skip_git: bool) -> io::Result<Entries> {
    let mut entries = Entries::new();
    let mut dirs = vec![PathBuf::new()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(root.join(&dir))? {
            let entry = entry?;
            let relative = dir.join(entry.file_name());
            let meta = fs::symlink_metadata(entry.path())?;
            let real = PathBuf::from(resolved_path(&entry.path().to_string_lossy()));
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 { return Err(io::Error::other("연결 지점은 복사할 수 없습니다")); }
            }
            if skip_git && dir.as_os_str().is_empty() && entry.file_name().to_string_lossy().eq_ignore_ascii_case(".git") {
                if !meta.is_dir() || meta.file_type().is_symlink() || !real.starts_with(root) {
                    return Err(io::Error::other("복사본 Git 폴더가 변경되었습니다"));
                }
                continue;
            }
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

pub(super) struct Workspaces {
    paths: HashMap<String, String>,
    project: String,
    git_base: Option<(PathBuf, PathBuf, String)>,
    copy_base: Option<Snapshot>,
    copy_git: HashMap<String, Snapshot>,
}

impl Workspaces {
    pub(super) fn get(&self, id: &str) -> Option<&String> { self.paths.get(id) }

    /// Applies each predecessor's complete project-scoped Git change to a fresh checkout.
    pub(super) fn handoff(&self, home: &Path, run_id: &str, plan: &Plan, card: &crate::run::plan::Card) -> io::Result<()> {
        let Some((root, relative, base_commit)) = self.git_base.as_ref() else {
            let target = Path::new(self.paths.get(&card.id).ok_or_else(|| io::Error::other("후속 작업 폴더가 없습니다"))?);
            let base = target.parent().ok_or_else(|| io::Error::other("복사본 기준 폴더가 없습니다"))?.join("base");
            let original = self.copy_base.as_ref().ok_or_else(|| io::Error::other("복사본 기준이 없습니다"))?;
            let target_git = self.copy_git.get(&card.id).ok_or_else(|| io::Error::other("복사본 Git 기준이 없습니다"))?;
            let unchanged = || -> io::Result<()> {
                if snapshot(Path::new(&self.project), false)? != *original || snapshot(&base, false)? != *original {
                    return Err(io::Error::other("원본 프로젝트 또는 복사본 기준이 변경되었습니다"));
                }
                if snapshot(target, true)? != *original || snapshot(&target.join(".git"), false)? != *target_git {
                    return Err(io::Error::other("후속 복사본에 변경이 있습니다"));
                }
                Ok(())
            };
            unchanged()?;
            let mut changes: BTreeMap<PathBuf, Option<SnapshotEntry>> = BTreeMap::new();
            for id in &card.depends_on {
                let path = Path::new(self.paths.get(id).ok_or_else(|| io::Error::other("선행 작업 폴더가 없습니다"))?);
                let git_base = self.copy_git.get(id).ok_or_else(|| io::Error::other("복사본 Git 기준이 없습니다"))?;
                if snapshot(&path.join(".git"), false)? != *git_base {
                    return Err(io::Error::other("비 Git 복사본의 Git 기록이 변경되어 인계할 수 없습니다"));
                }
                let source = snapshot(path, true)?;
                if source != snapshot(path, true)? || snapshot(&path.join(".git"), false)? != *git_base {
                    return Err(io::Error::other("선행 복사본이 변경되었습니다"));
                }
                let names: HashSet<_> = original.keys().chain(source.keys()).collect();
                for name in names {
                    if original.get(name) == source.get(name) { continue; }
                    if changes.keys().any(|other| other == name || other.starts_with(name) || name.starts_with(other)) {
                        return Err(io::Error::other(format!("선행 변경 충돌: {}", name.display())));
                    }
                }
                for name in original.keys().chain(source.keys()) {
                    if original.get(name) != source.get(name) { changes.insert(name.clone(), source.get(name).cloned()); }
                }
            }
            unchanged()?;
            let card_index = plan.cards.iter().position(|item| item.id == card.id).ok_or_else(|| io::Error::other("후속 작업을 찾지 못했습니다"))?;
            let backup = target.parent().unwrap().join(format!("handoff-{}", card_index + 1));
            fs::create_dir(&backup)?;
            // Move old files aside so even a concurrent user edit is preserved for review.
            for (name, _) in changes.iter().rev() {
                if let Some(old) = original.get(name) {
                    if old.is_none() {
                        fs::remove_dir(target.join(name))?;
                    } else {
                        let saved = backup.join(name);
                        fs::create_dir_all(saved.parent().unwrap())?;
                        fs::rename(target.join(name), &saved)?;
                        let meta = fs::symlink_metadata(&saved)?;
                        if !meta.is_file() || meta.file_type().is_symlink() || (fs::read(&saved)?, meta.permissions()) != *old.as_ref().unwrap() {
                            return Err(io::Error::other("인계 중 후속 파일이 변경되어 보존했습니다"));
                        }
                    }
                }
            }
            for (name, value) in &changes {
                match value {
                    Some(None) => fs::create_dir(target.join(name))?,
                    Some(Some((bytes, permissions))) => {
                        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(target.join(name))?;
                        file.write_all(bytes)?;
                        file.set_permissions(permissions.clone())?;
                    }
                    None => (),
                }
            }
            return Ok(());
        };
        if base(&self.project)?.as_ref() != self.git_base.as_ref() {
            return Err(io::Error::other("인계 전에 원본 프로젝트의 기준이 변경되었습니다"));
        }
        let target = Path::new(self.paths.get(&card.id).ok_or_else(|| io::Error::other("후속 작업 폴더가 없습니다"))?);
        let common = git(root, &["rev-parse", "--path-format=absolute", "--git-common-dir"])?;
        let verify = |path: &Path| -> io::Result<PathBuf> {
            if !path.is_dir() || PathBuf::from(resolved_path(&path.to_string_lossy())) != path {
                return Err(io::Error::other("작업 폴더가 변경되었습니다"));
            }
            if git(path, &["rev-parse", "--path-format=absolute", "--git-common-dir"])? != common {
                return Err(io::Error::other("작업 폴더의 Git 저장소가 변경되었습니다"));
            }
            let checkout = PathBuf::from(resolved_path(&git(path, &["rev-parse", "--show-toplevel"])?));
            if path.strip_prefix(&checkout).ok() != Some(relative.as_path()) {
                return Err(io::Error::other("작업 폴더의 프로젝트 범위가 변경되었습니다"));
            }
            Ok(checkout)
        };
        let target_root = verify(target)?;
        if git(target, &["rev-parse", "--verify", "HEAD^{commit}"])? != *base_commit
            || !git(&target_root, &["status", "--porcelain=v1", "--untracked-files=all", "--ignored=matching"])?.is_empty() {
            return Err(io::Error::other("후속 작업 폴더에 변경이 있어 인계하지 않았습니다"));
        }
        let card_index = plan.cards.iter().position(|item| item.id == card.id).ok_or_else(|| io::Error::other("후속 작업을 찾지 못했습니다"))?;
        let run_dir = crate::run::store::run_dir(home, run_id);
        let run_meta = fs::symlink_metadata(&run_dir)?;
        if !run_meta.is_dir() || run_meta.file_type().is_symlink() { return Err(io::Error::other("실행 기록 폴더가 변경되었습니다")); }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if run_meta.file_attributes() & 0x400 != 0 { return Err(io::Error::other("실행 기록 폴더가 연결 지점입니다")); }
        }
        let run_root = PathBuf::from(resolved_path(&run_dir.to_string_lossy()));
        let handoff_root = run_dir.join("handoff");
        fs::create_dir_all(&handoff_root)?;
        let meta = fs::symlink_metadata(&handoff_root)?;
        if !meta.is_dir() || meta.file_type().is_symlink()
            || resolved_path(&handoff_root.to_string_lossy()) != run_root.join("handoff").to_string_lossy() {
            return Err(io::Error::other("인계 기록 폴더가 변경되었습니다"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 { return Err(io::Error::other("인계 기록 폴더가 연결 지점입니다")); }
        }
        let handoff = handoff_root.join((card_index + 1).to_string());
        fs::create_dir(&handoff)?;
        let original_files = handoff.join("original");
        fs::create_dir(&original_files)?;
        let mut saved = HashSet::new();
        let scope = relative.to_string_lossy().replace('\\', "/");
        for (index, id) in card.depends_on.iter().enumerate() {
            let source = Path::new(self.paths.get(id).ok_or_else(|| io::Error::other("선행 작업 폴더가 없습니다"))?);
            let source_root = verify(source)?;
            let head = git(source, &["rev-parse", "--verify", "HEAD^{commit}"])?;
            git(source, &["merge-base", "--is-ancestor", base_commit, &head])
                .map_err(|_| io::Error::other("선행 작업의 기준 커밋이 바뀌었습니다"))?;
            if !git(&source_root, &["ls-files", "--others", "--ignored", "--exclude-standard", "--directory"])?.is_empty() {
                return Err(io::Error::other("선행 작업에 무시된 파일이 있어 안전하게 인계할 수 없습니다"));
            }
            if !git_output(&source_root, &["ls-files", "--unmerged", "-z"])?.is_empty() {
                return Err(io::Error::other("선행 작업에 해결되지 않은 Git 충돌이 있습니다"));
            }
            let staged = git_output(&source_root, &["diff", "--cached", "--name-only", "-z", "--no-renames"])?;
            let staged_patch = git_output(&source_root, &["diff", "--cached", "--binary", "--full-index", "--no-ext-diff", "--no-renames"])?;
            let unstaged = git_output(&source_root, &["diff", "--name-only", "-z", "--no-renames"])?;
            let untracked = git_output(&source_root, &["ls-files", "--others", "--exclude-standard", "-z"])?;
            let edited: HashSet<&[u8]> = unstaged.split(|b| *b == 0).chain(untracked.split(|b| *b == 0)).filter(|name| !name.is_empty()).collect();
            if staged.split(|b| *b == 0).any(|name| !name.is_empty() && edited.contains(name)) {
                return Err(io::Error::other("같은 파일의 스테이징된 내용과 작업 파일이 달라 인계할 수 없습니다"));
            }
            let snapshot = |suffix: &str| -> io::Result<(Vec<u8>, Vec<String>, Vec<String>)> {
                let index_path = handoff.join(format!("{}-{suffix}.index", index + 1));
                git_index(&source_root, &index_path, &["read-tree", &head])?;
                git_index(&source_root, &index_path, &["add", "--all", "--", "."])?;
                let names = git_index(&source_root, &index_path, &["diff", "--cached", "--name-only", "-z", "--no-renames", base_commit, "--"])?;
                let mut changed = Vec::new();
                for name in names.split(|b| *b == 0).filter(|name| !name.is_empty()) {
                    let name = std::str::from_utf8(name).map_err(io::Error::other)?;
                    if !scope.is_empty() && !name.starts_with(&format!("{scope}/")) {
                        return Err(io::Error::other("선행 변경이 프로젝트 범위 밖에 있습니다"));
                    }
                    changed.push(name.to_string());
                }
                let raw = git_index(&source_root, &index_path, &["diff", "--cached", "--raw", "-z", "--no-renames", base_commit, "--"])?;
                for header in raw.split(|b| *b == 0).step_by(2).filter(|part| !part.is_empty()) {
                    let header = std::str::from_utf8(header).map_err(io::Error::other)?;
                    if header.split_whitespace().take(2).any(|mode| matches!(mode.trim_start_matches(':'), "120000" | "160000")) {
                        return Err(io::Error::other("링크 또는 서브모듈 변경은 인계할 수 없습니다"));
                    }
                }
                let tracked = git_index(&source_root, &index_path, &["ls-files", "-z"])?;
                let tracked = tracked.split(|b| *b == 0).filter(|name| !name.is_empty())
                    .map(|name| std::str::from_utf8(name).map(str::to_string).map_err(io::Error::other))
                    .collect::<io::Result<Vec<_>>>()?;
                let patch = git_index(&source_root, &index_path, &["diff", "--cached", "--binary", "--full-index", "--no-ext-diff", "--no-renames", base_commit, "--"])?;
                Ok((patch, changed, tracked))
            };
            let first = snapshot("first")?;
            if first != snapshot("second")? || git(source, &["rev-parse", "--verify", "HEAD^{commit}"])? != head
                || staged != git_output(&source_root, &["diff", "--cached", "--name-only", "-z", "--no-renames"])?
                || staged_patch != git_output(&source_root, &["diff", "--cached", "--binary", "--full-index", "--no-ext-diff", "--no-renames"])?
                || unstaged != git_output(&source_root, &["diff", "--name-only", "-z", "--no-renames"])?
                || untracked != git_output(&source_root, &["ls-files", "--others", "--exclude-standard", "-z"])?
                || base(&self.project)?.as_ref() != self.git_base.as_ref() {
                return Err(io::Error::other("인계 준비 중 프로젝트가 변경되었습니다"));
            }
            let (patch, changed, tracked) = first;
            for name in tracked.iter().filter(|name| (scope.is_empty() || name.starts_with(&format!("{scope}/"))) && !changed.contains(name)) {
                let baseline = if saved.contains(name) { &original_files } else { &target_root };
                if !same_file(&source_root, baseline, name)? {
                    return Err(io::Error::other(format!("Git이 기록하지 않은 파일 변경이 있습니다: {name}")));
                }
            }
            for name in &changed {
                if saved.insert(name.clone()) {
                    let target_file = target_root.join(name);
                    match fs::symlink_metadata(&target_file) {
                        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                        Err(error) => return Err(error),
                        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() && PathBuf::from(resolved_path(&target_file.to_string_lossy())).starts_with(&target_root) => {
                            let backup = original_files.join(name);
                            fs::create_dir_all(backup.parent().ok_or_else(|| io::Error::other("파일 상위 폴더가 없습니다"))?)?;
                            fs::copy(target_file, backup)?;
                        }
                        Ok(_) => return Err(io::Error::other("기준 파일이 일반 파일이 아닙니다")),
                    }
                }
            }
            fs::write(handoff.join(format!("{}.patch", index + 1)), &patch)?;
            if !patch.is_empty() {
                apply(&target_root, &patch, true)?;
                apply(&target_root, &patch, false)?;
            }
            for name in changed {
                if !same_file(&source_root, &target_root, &name)? {
                    return Err(io::Error::other(format!("인계 후 파일이 원본과 다릅니다: {name}")));
                }
            }
        }
        Ok(())
    }
}

fn same_file(a: &Path, b: &Path, name: &str) -> io::Result<bool> {
    let paths = [a.join(name), b.join(name)];
    let metadata = |path: &Path| match fs::symlink_metadata(path) {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    let (left, right) = (metadata(&paths[0])?, metadata(&paths[1])?);
    let (Some(left), Some(right)) = (left.as_ref(), right.as_ref()) else { return Ok(left.is_none() && right.is_none()); };
    for (root, path) in [(a, &paths[0]), (b, &paths[1])] {
        let parent = path.parent().ok_or_else(|| io::Error::other("파일 상위 폴더가 없습니다"))?;
        if !PathBuf::from(resolved_path(&parent.to_string_lossy())).starts_with(root) {
            return Err(io::Error::other("파일 상위 폴더가 작업 폴더 밖을 가리킵니다"));
        }
    }
    if left.file_type().is_symlink() || right.file_type().is_symlink() {
        return Ok(left.file_type().is_symlink() && right.file_type().is_symlink() && fs::read_link(&paths[0])? == fs::read_link(&paths[1])?);
    }
    if !left.is_file() || !right.is_file() { return Err(io::Error::other("일반 파일이 아닙니다")); }
    if left.len() != right.len() { return Ok(false); }
    let (mut left, mut right) = (fs::File::open(&paths[0])?, fs::File::open(&paths[1])?);
    let (mut a, mut b) = ([0u8; 8192], [0u8; 8192]);
    loop {
        let count = left.read(&mut a)?;
        right.read_exact(&mut b[..count])?;
        if a[..count] != b[..count] { return Ok(false); }
        if count == 0 { return Ok(right.read(&mut b[..1])? == 0); }
    }
}

fn git_index(dir: &Path, index: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut command = Command::new("git");
    command.current_dir(dir).args(args).env("LC_ALL", "C").env("GIT_INDEX_FILE", index);
    let out = proc::hide_window(&mut command).output()?;
    if !out.status.success() { return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string())); }
    Ok(out.stdout)
}

fn git_output(dir: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let mut command = Command::new("git");
    command.current_dir(dir).args(args).env("LC_ALL", "C");
    let out = proc::hide_window(&mut command).output()?;
    if !out.status.success() { return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string())); }
    Ok(out.stdout)
}

fn apply(dir: &Path, patch: &[u8], check: bool) -> io::Result<()> {
    let mut command = Command::new("git");
    command.current_dir(dir).arg("apply");
    if check { command.arg("--check"); }
    command.arg("-").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = proc::hide_window(&mut command).spawn()?;
    let write = child.stdin.take().ok_or_else(|| io::Error::other("Git 입력을 열지 못했습니다"))?.write_all(patch);
    let out = child.wait_with_output()?;
    if !out.status.success() { return Err(io::Error::other(format!("선행 변경 충돌: {}", String::from_utf8_lossy(&out.stderr).trim()))); }
    write
}

pub(super) fn create(home: &Path, run_id: &str, project: &str, plan: &Plan) -> io::Result<Workspaces> {
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
    let copy_snapshot = if git_base.is_none() {
        let entries = inventory(&root)?;
        let source = run.join("base");
        copy_entries(&root, &source, &entries)?;
        if inventory(&root)? != entries { return Err(io::Error::other("복사 중 원본 프로젝트가 변경되어 시작하지 않았습니다")); }
        Some((source, entries))
    } else { None };
    let mut result = HashMap::new();
    let mut copy_git = HashMap::new();
    for (i, card) in plan.cards.iter().enumerate() {
        // Numeric folder names are independent of model-generated card IDs.
        let path = run.join((i + 1).to_string());
        let path_text = path.to_string_lossy();
        let cwd = if let Some((_, relative, base_commit)) = &git_base {
            git(&root, &["worktree", "add", "--detach", "--", &path_text, base_commit])
                .map_err(|e| io::Error::other(format!("워크트리를 만들지 못했습니다: {e}. 만든 작업 폴더는 {}에 보존했습니다", run.display())))?;
            path.join(relative)
        } else {
            let (source, entries) = copy_snapshot.as_ref().expect("non-Git snapshot");
            copy_entries(source, &path, entries)?;
            git(&path, &["init", "--quiet"])?;
            copy_git.insert(card.id.clone(), snapshot(&path.join(".git"), false)?);
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
    if current_base != git_base || copy_snapshot.as_ref().is_some_and(|(_, entries)| inventory(&root).map_or(true, |current| current != *entries)) {
        return Err(io::Error::other(format!("워크트리 준비 중 프로젝트가 바뀌었습니다. 만든 폴더는 {}에 보존했습니다", run.display())));
    }
    let copy_base = if let Some((source, _)) = &copy_snapshot {
        let original = snapshot_copy_base(&root, source)?;
        Some(original)
    } else { None };
    let baseline = match &git_base {
        Some((_, _, commit)) => serde_json::json!({"gitCommit": commit}),
        None => serde_json::json!({"copyPath": run.join("base")}),
    };
    let mut baseline_file = fs::OpenOptions::new().write(true).create_new(true)
        .open(crate::run::store::run_dir(home, run_id).join("workspace-baseline.json"))?;
    serde_json::to_writer(&mut baseline_file, &baseline).map_err(io::Error::other)?;
    baseline_file.sync_all()?;
    Ok(Workspaces { paths: result, project: project.to_string(), git_base, copy_base, copy_git })
}

fn snapshot_copy_base(root: &Path, source: &Path) -> io::Result<Snapshot> {
    let original = snapshot(root, false)?;
    if original != snapshot(source, false)? || original != snapshot(root, false)? {
        return Err(io::Error::other("복사본 기준이 변경되었습니다"));
    }
    Ok(original)
}

/// A read-only inventory for review; it does not establish that applying changes is safe.
pub fn review_changes(home: &Path, run_id: &str, card_id: &str) -> io::Result<serde_json::Value> {
    use crate::run::{events::OEvent, store};
    if !store::is_run_id(run_id) { return Err(io::Error::other("올바르지 않은 실행 ID")); }
    let dir = store::run_dir(home, run_id);
    if !dir.join("isolated-worktrees").is_file() { return Err(io::Error::other("분리된 실행이 아닙니다")); }
    let plan = store::read_plan(home, run_id)?;
    let ids = store::plan_card_ids(&plan)?;
    let index = ids.iter().position(|id| id == card_id).ok_or_else(|| io::Error::other("카드를 찾지 못했습니다"))?;
    let name = home.file_name().ok_or_else(|| io::Error::other("O 상태 폴더가 올바르지 않습니다"))?.to_string_lossy();
    let parent = PathBuf::from(resolved_path(&home.with_file_name(format!("{name}-worktrees")).to_string_lossy()));
    let run = parent.join(run_id);
    let checkout = run.join((index + 1).to_string());
    if Path::new(&resolved_path(&run.to_string_lossy())) != run || Path::new(&resolved_path(&checkout.to_string_lossy())) != checkout {
        return Err(io::Error::other("작업 폴더 경로가 변경되었습니다"));
    }
    let workspace = store::read_events(home, run_id).into_iter().rev().find_map(|event| match event {
        OEvent::CardTool { card_id: id, name, summary, .. } if id == card_id && name == "O 작업 폴더" => Some(PathBuf::from(summary)),
        _ => None,
    }).ok_or_else(|| io::Error::other("카드 작업 폴더 기록이 없습니다"))?;
    if !workspace.is_dir() || Path::new(&resolved_path(&workspace.to_string_lossy())) != workspace || !workspace.starts_with(&checkout) {
        return Err(io::Error::other("카드 작업 폴더가 변경되었습니다"));
    }
    let baseline: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("workspace-baseline.json"))?).map_err(io::Error::other)?;
    if let Some(commit) = baseline.get("gitCommit").and_then(|v| v.as_str()) {
        if !matches!(commit.len(), 40 | 64) || !commit.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(io::Error::other("Git 기준 커밋이 올바르지 않습니다"));
        }
        let output = |args: &[&str]| -> io::Result<Vec<u8>> {
            let mut command = Command::new("git");
            command.current_dir(&workspace).args(["-c", "core.fsmonitor=false"]).args(args).env("GIT_OPTIONAL_LOCKS", "0")
                .env_remove("GIT_DIR").env_remove("GIT_WORK_TREE").env_remove("GIT_INDEX_FILE");
            proc::hide_window(&mut command);
            let out = command.output()?;
            if !out.status.success() { return Err(io::Error::other(String::from_utf8_lossy(&out.stderr).trim().to_string())); }
            Ok(out.stdout)
        };
        let top = output(&["rev-parse", "--show-toplevel"])?;
        let top = std::str::from_utf8(&top).map_err(io::Error::other)?.trim_end_matches(['\r', '\n']);
        if Path::new(&resolved_path(top)) != checkout { return Err(io::Error::other("Git 작업 폴더가 변경되었습니다")); }
        let list = |args: &[&str]| -> io::Result<Vec<String>> {
            output(args)?.split(|b| *b == 0).filter(|v| !v.is_empty())
                .map(|v| std::str::from_utf8(v).map(str::to_string).map_err(io::Error::other)).collect()
        };
        let collect = || -> io::Result<serde_json::Value> {
            let changed = list(&["diff", "--relative", "--name-only", "-z", "--no-ext-diff", "--no-textconv", "--no-renames", commit, "--", "."])?;
            let staged = list(&["diff", "--relative", "--cached", "--name-only", "-z", "--no-ext-diff", "--no-textconv", "--no-renames", "--", "."])?;
            let unstaged = list(&["diff", "--relative", "--name-only", "-z", "--no-ext-diff", "--no-textconv", "--no-renames", "--", "."])?;
            let unmerged = list(&["diff", "--relative", "--name-only", "-z", "--no-ext-diff", "--no-textconv", "--diff-filter=U", "--", "."])?;
            let untracked = list(&["ls-files", "--others", "--exclude-standard", "-z", "--", "."])?;
            let ignored = list(&["ls-files", "--others", "--ignored", "--exclude-standard", "-z", "--", "."])?;
            Ok(serde_json::json!({"changed":changed,"staged":staged,"unstaged":unstaged,"unmerged":unmerged,"untracked":untracked,"ignored":ignored}))
        };
        let changes = collect()?;
        if changes != collect()? { return Err(io::Error::other("변경 목록을 읽는 중 작업 폴더가 바뀌었습니다")); }
        Ok(serde_json::json!({"workspace":workspace,"gitCommit":commit,"changes":changes,"note":"Git의 인덱스 표시 설정에 따라 숨겨진 변경이 있을 수 있습니다. 원본 적용의 안전성은 별도로 확인해야 합니다."}))
    } else {
        let base = run.join("base");
        if Path::new(&resolved_path(&base.to_string_lossy())) != base { return Err(io::Error::other("기준 복사본 경로가 변경되었습니다")); }
        let recorded = baseline.get("copyPath").and_then(|v| v.as_str()).ok_or_else(|| io::Error::other("복사본 기준 정보가 없습니다"))?;
        if Path::new(recorded) != base || workspace != checkout { return Err(io::Error::other("복사본 경로가 변경되었습니다")); }
        let before = snapshot(&base, false)?;
        let after = snapshot(&workspace, true)?;
        if before != snapshot(&base, false)? || after != snapshot(&workspace, true)? {
            return Err(io::Error::other("변경 목록을 읽는 중 복사본이 바뀌었습니다"));
        }
        let names: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).collect();
        let mut changed = Vec::new();
        for path in names {
            if before.get(path) != after.get(path) {
                changed.push(serde_json::json!({"path": path.to_str().ok_or_else(|| io::Error::other("UTF-8이 아닌 경로가 있습니다"))?, "kind": if !before.contains_key(path) { "added" } else if !after.contains_key(path) { "deleted" } else { "modified" }}));
            }
        }
        Ok(serde_json::json!({"workspace":workspace,"copyBase":base,"changes":changed,"note":"기준 복사본은 수정될 수 있습니다. 원본 적용의 안전성은 별도로 확인해야 합니다."}))
    }
}
