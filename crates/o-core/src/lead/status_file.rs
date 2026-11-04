//! A read-only snapshot kept stable while the lead is running.

#[cfg(windows)]
use std::{io, path::Path};
use std::path::PathBuf;

pub struct StatusFile {
    pub path: PathBuf,
    #[cfg(windows)]
    _handles: Vec<std::fs::File>,
    #[cfg(all(test, windows))]
    saved: std::collections::BTreeMap<String, Vec<(Vec<u8>, PathBuf)>>,
}

#[cfg(windows)]
fn protect_dirs(dir: &Path, create: &[PathBuf]) -> io::Result<Vec<std::fs::File>> {
    use std::fs::{self, OpenOptions};
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE};
    if dir.components().any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::CurDir)) {
        return Err(io::Error::other("결과 폴더 경로가 올바르지 않습니다"));
    }
    let mut handles = Vec::new();
    for part in dir.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if create.iter().any(|path| part == path) {
            match fs::create_dir(part) { Ok(()) => {}, Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}, Err(e) => return Err(e) }
        }
        let file = OpenOptions::new().read(true).access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(part)?;
        let meta = file.metadata()?;
        if !meta.is_dir() || meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(io::Error::other("결과 폴더에 링크가 있어 저장하지 않았습니다"));
        }
        handles.push(file);
    }
    Ok(handles)
}

#[cfg(windows)]
fn plain_file(file: &std::fs::File) -> io::Result<bool> {
    use std::os::windows::{fs::MetadataExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, GetFileInformationByHandle};
    let meta = file.metadata()?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    Ok(meta.is_file() && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
        && unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) } != 0 && info.nNumberOfLinks == 1)
}

#[cfg(windows)]
pub(super) fn open_events(home: &Path, run_id: &str) -> io::Result<Option<(std::fs::File, Vec<std::fs::File>)>> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE};
    if !crate::run::store::is_run_id(run_id) { return Err(io::Error::other("실행 ID가 올바르지 않습니다")); }
    let dir = std::path::absolute(crate::run::store::run_dir(home, run_id))?;
    let mut parents = match protect_dirs(&dir, &[]) {
        Ok(parents) => parents,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    // Writers take this same lock before opening events.jsonl; never make their append fail.
    let lock = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(dir.join("events.lock"))?;
    if !plain_file(&lock)? { return Err(io::Error::other("실행 기록 잠금 파일이 올바르지 않습니다")); }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(std::fs::TryLockError::WouldBlock) => return Err(io::Error::other("실행 기록이 사용 중입니다")),
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    let file = match std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(dir.join("events.jsonl")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !plain_file(&file)? {
        return Err(io::Error::other("실행 기록이 링크이거나 파일 소유를 확인하지 못했습니다"));
    }
    parents.push(lock);
    Ok(Some((file, parents)))
}


#[cfg(windows)]
pub fn create(home: &Path, thread: &str) -> io::Result<StatusFile> {
    if !crate::run::store::is_run_id(thread) { return Err(io::Error::other("총괄 대화 ID가 올바르지 않습니다")); }
    let root = std::path::absolute(super::files::files_root(home))?;
    let dir = root.join(thread);
    let handles = protect_dirs(&dir, &[root, dir.clone()])?;
    Ok(StatusFile { path: dir, _handles: handles, #[cfg(test)] saved: Default::default() })
}

#[cfg(windows)]
impl StatusFile {
pub(super) fn temporary(&self) -> io::Result<(PathBuf, std::fs::File)> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
    use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_READ};
    for _ in 0..100 {
        let path = self.path.join(format!("run-pages-{}.txt", crate::run::store::new_run_id(chrono::Utc::now())));
        match std::fs::OpenOptions::new().read(true).write(true).create_new(true)
            .access_mode(GENERIC_READ | GENERIC_WRITE | DELETE).share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_DELETE_ON_CLOSE).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("결과 페이지 파일 이름을 확보하지 못했습니다"))
}

pub(super) fn retain(&mut self, file: std::fs::File) { self._handles.push(file); }

#[cfg(test)]
pub fn put(&mut self, bytes: &[u8]) -> io::Result<PathBuf> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    let hash = format!("{:x}", Sha256::digest(bytes));
    if let Some((_, path)) = self.saved.get(&hash).and_then(|items| items.iter().find(|(stored, _)| stored == bytes)) { return Ok(path.clone()); }
    for n in 0..1000 {
        let path = self.path.join(format!("run-status-{hash}-{n}.txt"));
        match OpenOptions::new().read(true).write(true).create_new(true).share_mode(FILE_SHARE_READ).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.flush()?;
                self._handles.push(file);
                self.saved.entry(hash.clone()).or_default().push((bytes.to_vec(), path.clone()));
                return Ok(path);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let opened = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path);
                if let Ok(mut file) = opened {
                    let meta = file.metadata()?;
                    if plain_file(&file)? && meta.len() == bytes.len() as u64 {
                        let mut existing = Vec::new();
                        file.read_to_end(&mut existing)?;
                        if existing == bytes { self._handles.push(file); self.saved.entry(hash.clone()).or_default().push((bytes.to_vec(), path.clone())); return Ok(path); }
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("결과 파일 이름을 확보하지 못했습니다"))
}
}

#[cfg(all(test, windows))]
pub fn save(home: &Path, thread: &str, bytes: &[u8]) -> io::Result<StatusFile> {
    let mut file = create(home, thread)?;
    let path = file.put(bytes)?;
    file.path = path;
    Ok(file)
}


#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn home() -> PathBuf {
        let n = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let base = std::env::temp_dir().join(format!("o-status-file-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        base.join("state")
    }

    #[test]
    fn reading_a_run_waits_writers_instead_of_failing_them() {
        use std::sync::mpsc;
        let home = home();
        let dir = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("events.jsonl"), b"{}\n").unwrap();
        let (file, guards) = open_events(&home, "run").unwrap().unwrap();
        let (started, wait) = mpsc::channel();
        let (done, result) = mpsc::channel();
        let writer_home = home.clone();
        let writer = std::thread::spawn(move || {
            started.send(()).unwrap();
            let event = crate::run::events::OEvent::CardStart { at: "now".into(), card_id: "card".into(), pid: None };
            done.send(crate::run::store::append_event(&writer_home, "run", &event)).unwrap();
        });
        wait.recv().unwrap();
        assert!(result.recv_timeout(std::time::Duration::from_millis(150)).is_err());
        drop(file);
        drop(guards);
        result.recv_timeout(std::time::Duration::from_secs(3)).unwrap().unwrap();
        writer.join().unwrap();
        assert!(std::fs::read_to_string(dir.join("events.jsonl")).unwrap().contains("card.start"));
    }

    #[test]
    fn immutable_while_in_use_and_reused_afterwards() {
        let home = home();
        let status = save(&home, "thread", b"complete result").unwrap();
        assert_eq!(std::fs::read(&status.path).unwrap(), b"complete result");
        assert!(std::fs::write(&status.path, b"bad").is_err());
        let dir = status.path.parent().unwrap();
        assert!(std::fs::rename(dir, dir.with_extension("moved")).is_err());
        let path = status.path.clone();
        drop(status);
        let next = save(&home, "thread", b"complete result").unwrap();
        assert_eq!(path, next.path);
        assert_eq!(std::fs::read(&next.path).unwrap(), b"complete result");
    }

    #[test]
    fn existing_conflicting_file_is_never_overwritten() {
        let home = home();
        let first = save(&home, "thread", b"original").unwrap();
        let path = first.path.clone();
        drop(first);
        std::fs::write(&path, b"other").unwrap();
        let next = save(&home, "thread", b"original").unwrap();
        assert_ne!(path, next.path);
        assert_eq!(std::fs::read(path).unwrap(), b"other");
        assert_eq!(std::fs::read(&next.path).unwrap(), b"original");
    }

    #[test]
    fn hardlinked_snapshot_is_not_reused() {
        let home = home();
        let original = save(&home, "thread", b"result").unwrap();
        let path = original.path.clone();
        drop(original);
        let other = home.parent().unwrap().join("other-file");
        std::fs::hard_link(&path, &other).unwrap();
        let next = save(&home, "thread", b"result").unwrap();
        assert_ne!(path, next.path);
        assert_eq!(std::fs::read(other).unwrap(), b"result");
    }

    #[test]
    fn hardlink_created_during_use_cannot_change_the_snapshot() {
        let home = home();
        let status = save(&home, "thread", b"result").unwrap();
        let other = home.parent().unwrap().join("linked-during-read");
        std::fs::hard_link(&status.path, &other).unwrap();
        assert!(std::fs::write(&other, b"changed").is_err());
        assert_eq!(std::fs::read(&status.path).unwrap(), b"result");
    }

    #[test]
    fn linked_result_root_is_refused() {
        let home = home();
        let base = home.parent().unwrap();
        let target = base.join("outside");
        std::fs::create_dir(&target).unwrap();
        let root = super::super::files::files_root(&home);
        let mut command = std::process::Command::new("cmd");
        command.arg("/C").arg("mklink").arg("/J").arg(&root).arg(&target).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        assert!(crate::sys::proc::hide_window(&mut command).status().unwrap().success());
        assert!(save(&home, "thread", b"private").is_err());
        assert_eq!(std::fs::read_dir(target).unwrap().count(), 0);
    }

    #[test]
    fn linked_source_record_is_refused() {
        let home = home();
        let run = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&run).unwrap();
        let target = home.parent().unwrap().join("outside.jsonl");
        std::fs::write(&target, "{}\n").unwrap();
        std::fs::hard_link(&target, run.join("events.jsonl")).unwrap();
        assert!(open_events(&home, "run").is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "{}\n");
    }

    #[test]
    fn record_with_a_live_writer_is_not_read_partially() {
        let home = home();
        let run = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&run).unwrap();
        let writer = std::fs::OpenOptions::new().create_new(true).write(true).open(run.join("events.jsonl")).unwrap();
        assert!(open_events(&home, "run").is_err());
        drop(writer);
        assert_eq!(open_events(&home, "run").unwrap().unwrap().0.metadata().unwrap().len(), 0);
    }


    #[test]
    fn source_handle_blocks_changes_and_replacement() {
        let home = home();
        let run = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&run).unwrap();
        let original = run.join("events.jsonl");
        std::fs::write(&original, "original\n").unwrap();
        let parent = std::fs::canonicalize(&run).unwrap();
        assert!(parent.starts_with(std::fs::canonicalize(std::env::temp_dir()).unwrap()));
        let moved = parent.join("moved.jsonl");
        assert_eq!(moved.parent(), Some(parent.as_path()));
        let (_file, _parents) = open_events(&home, "run").unwrap().unwrap();
        assert!(std::fs::write(&original, "changed").is_err());
        assert!(std::fs::rename(&original, &moved).is_err());
        assert_eq!(std::fs::read_to_string(original).unwrap(), "original\n");
    }

    #[test]
    fn protected_ancestors_cannot_be_replaced_during_use() {
        let home = home();
        let status = save(&home, "thread", b"result").unwrap();
        let root = super::super::files::files_root(&home);
        let base = std::fs::canonicalize(home.parent().unwrap()).unwrap();
        let temp = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        assert!(base.starts_with(&temp));
        assert_eq!(std::fs::canonicalize(&root).unwrap().parent(), Some(base.as_path()));
        let moved = base.join("moved-root");
        assert_eq!(moved.parent(), Some(base.as_path()));
        assert!(std::fs::rename(&root, &moved).is_err());
        let moved_base = temp.join(format!("{}-moved", base.file_name().unwrap().to_string_lossy()));
        assert_eq!(moved_base.parent(), Some(temp.as_path()));
        assert!(std::fs::rename(&base, moved_base).is_err());
        drop(status);
    }
}
