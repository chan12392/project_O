//! A read-only snapshot kept stable while the lead is running.

use std::io;
use std::path::{Path, PathBuf};

pub struct StatusFile {
    pub path: PathBuf,
    #[cfg(windows)]
    _handles: Vec<std::fs::File>,
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
pub fn read_events(home: &Path, run_id: &str) -> io::Result<Option<String>> {
    use std::io::Read;
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    if !crate::run::store::is_run_id(run_id) { return Err(io::Error::other("실행 ID가 올바르지 않습니다")); }
    let dir = std::path::absolute(crate::run::store::run_dir(home, run_id))?;
    let _parents = match protect_dirs(&dir, &[]) {
        Ok(parents) => parents,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut file = match std::fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(dir.join("events.jsonl")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if !plain_file(&file)? {
        return Err(io::Error::other("실행 기록이 링크이거나 파일 소유를 확인하지 못했습니다"));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(Some(text))
}

#[cfg(not(windows))]
pub fn read_events(_home: &Path, _run_id: &str) -> io::Result<Option<String>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 실행 기록의 안전한 읽기를 지원하지 않습니다"))
}

#[cfg(windows)]
pub fn save(home: &Path, thread: &str, bytes: &[u8]) -> io::Result<StatusFile> {
    use std::fs::OpenOptions;
    use std::io::{Read, Write};
    use std::os::windows::fs::OpenOptionsExt;
    use sha2::{Digest, Sha256};
    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};
    if !crate::run::store::is_run_id(thread) { return Err(io::Error::other("총괄 대화 ID가 올바르지 않습니다")); }
    let root = std::path::absolute(super::files::files_root(home))?;
    let dir = root.join(thread);
    let mut handles = protect_dirs(&dir, &[root, dir.clone()])?;
    let hash = format!("{:x}", Sha256::digest(bytes));
    for n in 0..1000 {
        let path = dir.join(format!("run-status-{hash}-{n}.json"));
        match OpenOptions::new().read(true).write(true).create_new(true).share_mode(FILE_SHARE_READ).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.flush()?;
                handles.push(file);
                return Ok(StatusFile { path, _handles: handles });
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                let opened = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).custom_flags(FILE_FLAG_OPEN_REPARSE_POINT).open(&path);
                if let Ok(mut file) = opened {
                    let meta = file.metadata()?;
                    if plain_file(&file)? && meta.len() == bytes.len() as u64 {
                        let mut existing = Vec::new();
                        file.read_to_end(&mut existing)?;
                        if existing == bytes { handles.push(file); return Ok(StatusFile { path, _handles: handles }); }
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("결과 파일 이름을 확보하지 못했습니다"))
}

#[cfg(not(windows))]
pub fn save(_home: &Path, _thread: &str, _bytes: &[u8]) -> io::Result<StatusFile> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "이 운영체제에서는 결과 파일의 안전한 생성을 지원하지 않습니다"))
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
        assert!(read_events(&home, "run").is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "{}\n");
    }

    #[test]
    fn record_with_a_live_writer_is_not_read_partially() {
        let home = home();
        let run = crate::run::store::run_dir(&home, "run");
        std::fs::create_dir_all(&run).unwrap();
        let writer = std::fs::OpenOptions::new().create_new(true).write(true).open(run.join("events.jsonl")).unwrap();
        assert!(read_events(&home, "run").is_err());
        drop(writer);
        assert_eq!(read_events(&home, "run").unwrap().as_deref(), Some(""));
    }
}
