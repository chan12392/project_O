//! Lead chat attachments: images and documents a person adds to a message.
//!
//! They are saved next to O's state folder, never in it (`<base>/project_O-lead/<threadId>/`),
//! because the lead CLI must read them and O's own folder is no place for an agent.
//! Every file is a new one (`create_new`, which never follows an existing name or link)
//! with a sanitized, numbered name; folders that turn out to be links are refused.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};

use o_guard::rules::{find_hits, CREDENTIAL_RULES};
use serde::{Deserialize, Serialize};

use crate::sys::js::js_head;

/// Ten megabytes per file.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
/// Files per message.
pub const MAX_PER_MESSAGE: usize = 5;
pub const IMAGE_TYPES: [&str; 5] = ["png", "jpg", "jpeg", "gif", "webp"];
pub const DOCUMENT_TYPES: [&str; 6] = ["pdf", "md", "txt", "csv", "json", "log"];
/// Folders of the CLIs' sign-in and of keys: nothing is attached from them.
const PRIVATE_FOLDERS: [&str; 5] = [".claude", ".codex", ".gemini", ".ssh", ".aws"];
const MAX_NAME_CHARS: usize = 80;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// The saved file's name (sanitized and numbered).
    pub name: String,
    /// Where it was saved, for the lead CLI to read.
    pub path: String,
    pub size: u64,
    /// An image the CLI may take natively (Codex `-i`).
    pub image: bool,
}

/// `<base>/project_O-lead` beside O's state folder `<base>/project_O`.
pub fn files_root(home: &Path) -> PathBuf {
    let name = home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project_O".into());
    home.with_file_name(format!("{name}-lead"))
}

pub fn thread_files(home: &Path, thread_id: &str) -> PathBuf {
    files_root(home).join(thread_id)
}

/// The file type O accepts, from the name's extension: `Some(true)` an image, `Some(false)` a document.
pub fn kind_of(name: &str) -> Option<bool> {
    let ext = Path::new(name).extension()?.to_str()?.to_ascii_lowercase();
    if IMAGE_TYPES.contains(&ext.as_str()) {
        Some(true)
    } else if DOCUMENT_TYPES.contains(&ext.as_str()) {
        Some(false)
    } else {
        None
    }
}

/// A file name safe to create: the last path segment, letters, digits, Hangul, `.`, `_`
/// and `-` only, no leading dot, at most 80 characters with the extension kept.
pub fn sanitize(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' })
        .collect::<String>()
        .trim_start_matches('.')
        .to_string();
    if cleaned.chars().count() <= MAX_NAME_CHARS {
        return cleaned;
    }
    let ext = Path::new(&cleaned).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let stem: String = cleaned.chars().take(MAX_NAME_CHARS.saturating_sub(ext.chars().count())).collect();
    format!("{stem}{ext}")
}

/// The checks every attachment passes before a byte is written: type, size and name.
pub fn check(name: &str, size: u64) -> Result<(String, bool), String> {
    let clean = sanitize(name);
    if clean.is_empty() {
        return Err("첨부 파일 이름이 비어 있습니다".into());
    }
    if !find_hits(name, CREDENTIAL_RULES.iter()).is_empty() {
        return Err(format!("로그인 정보 파일로 보이는 이름이라 첨부하지 않습니다: {}", js_head(&clean, 60)));
    }
    let Some(image) = kind_of(&clean) else {
        let allowed = [IMAGE_TYPES.as_slice(), DOCUMENT_TYPES.as_slice()].concat().join(", ");
        return Err(format!("첨부할 수 없는 형식입니다: {clean} (허용: {allowed})"));
    };
    if size > MAX_BYTES {
        return Err(format!("첨부 파일은 하나에 10MB까지입니다: {clean}"));
    }
    if size == 0 {
        return Err(format!("빈 파일은 첨부하지 않습니다: {clean}"));
    }
    Ok((clean, image))
}

/// Makes `dir` (and its parent root) as plain folders; a folder that is a link is refused.
fn plain_dir(root: &Path, dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("첨부 폴더를 만들지 못했습니다: {e}"))?;
    for folder in [root, dir] {
        let meta = fs::symlink_metadata(folder).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(format!("첨부 폴더가 링크라 쓰지 않습니다: {}", folder.to_string_lossy()));
        }
    }
    Ok(())
}

/// Saves bytes the app sent as a new file in the thread's attachment folder.
pub fn save(home: &Path, thread_id: &str, name: &str, bytes: &[u8]) -> Result<Attachment, String> {
    let (clean, image) = check(name, bytes.len() as u64)?;
    let root = files_root(home);
    let dir = thread_files(home, thread_id);
    plain_dir(&root, &dir)?;
    for n in 1..100 {
        let path = dir.join(format!("{n:02}-{clean}"));
        // create_new fails on any existing entry, a link included, so nothing is followed or overwritten.
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                file.write_all(bytes).map_err(|e| format!("첨부 파일을 쓰지 못했습니다: {e}"))?;
                return Ok(Attachment { name: format!("{n:02}-{clean}"), path: path.to_string_lossy().into_owned(), size: bytes.len() as u64, image });
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("첨부 파일을 만들지 못했습니다: {e}")),
        }
    }
    Err("같은 이름의 첨부가 너무 많습니다".into())
}

fn in_private_folder(path: &Path) -> bool {
    path.components().any(|c| PRIVATE_FOLDERS.iter().any(|p| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(p)))
}

/// A file given by path (`o lead send --attach`): one already in the thread's attachment
/// folder is used as it is; any other is copied in. Links, sign-in folders and
/// credential file names are refused, and nothing larger than the limit is read.
pub fn import(home: &Path, thread_id: &str, path: &Path) -> Result<Attachment, String> {
    let shown = path.to_string_lossy();
    if !find_hits(&shown, CREDENTIAL_RULES.iter()).is_empty() || in_private_folder(path) {
        return Err(format!("로그인 정보가 있는 위치의 파일은 첨부하지 않습니다: {shown}"));
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{shown}: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(format!("링크나 폴더는 첨부하지 않습니다: {shown}"));
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (_, image) = check(&name, meta.len())?;
    let dir = thread_files(home, thread_id);
    let inside = match (fs::canonicalize(path), fs::canonicalize(&dir)) {
        (Ok(file), Ok(dir)) => file.parent() == Some(dir.as_path()),
        _ => false,
    };
    if inside {
        return Ok(Attachment { name, path: path.to_string_lossy().into_owned(), size: meta.len(), image });
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("{shown}: {e}"))?;
    save(home, thread_id, &name, &bytes)
}
