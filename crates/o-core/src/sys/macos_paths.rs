//! Resource comparison, including missing paths and reservations from old versions.
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use core_foundation_sys::{base::CFRelease, string::*};

pub(crate) fn key(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() { return Err(io::Error::other("자원 경로는 절대 경로여야 합니다")); }
    let mut prefix = PathBuf::new();
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => { prefix.push(component); result.push(component); }
            Component::Normal(name) => {
                // A mount point name belongs to its parent volume; names below
                // it belong to the mounted volume. Preserve that distinction.
                let sensitive = case_sensitive(&prefix)?;
                let text = name.to_str().ok_or_else(|| io::Error::other("자원 경로가 올바른 UTF-8이 아닙니다"))?;
                result.push(fold(text, !sensitive)?);
                prefix.push(name);
            }
            _ => return Err(io::Error::other("자원 경로를 확인할 수 없습니다")),
        }
    }
    Ok(result)
}

fn case_sensitive(path: &Path) -> io::Result<bool> {
    let mut ancestor = path;
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or_else(|| io::Error::other("자원 경로 볼륨을 확인할 수 없습니다"))?;
            }
            Err(e) => return Err(e),
        }
    }
    let name = CString::new(ancestor.as_os_str().as_bytes())?;
    match unsafe { libc::pathconf(name.as_ptr(), libc::_PC_CASE_SENSITIVE) } {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(io::Error::other("자원 볼륨의 이름 비교 방식을 확인할 수 없습니다")),
    }
}

fn fold(text: &str, case_insensitive: bool) -> io::Result<String> {
    unsafe {
        let source = CFStringCreateWithBytes(std::ptr::null(), text.as_ptr(), text.len() as isize, kCFStringEncodingUTF8, 0);
        if source.is_null() { return Err(io::Error::other("자원 경로 문자 변환 실패")); }
        let value = CFStringCreateMutableCopy(std::ptr::null(), 0, source);
        CFRelease(source.cast());
        if value.is_null() { return Err(io::Error::other("자원 경로 문자 변환 실패")); }
        CFStringNormalize(value, kCFStringNormalizationFormD);
        if case_insensitive { CFStringFold(value, kCFCompareCaseInsensitive, std::ptr::null()); }
        CFStringNormalize(value, kCFStringNormalizationFormD);
        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), kCFStringEncodingUTF8);
        let mut bytes = vec![0u8; usize::try_from(size).ok().and_then(|n| n.checked_add(1)).unwrap_or(0)];
        let ok = CFStringGetCString(value, bytes.as_mut_ptr().cast(), bytes.len() as isize, kCFStringEncodingUTF8);
        CFRelease(value.cast());
        if ok == 0 { return Err(io::Error::other("자원 경로 문자 변환 실패")); }
        bytes.truncate(bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len()));
        String::from_utf8(bytes).map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_forms_and_case_policy() {
        assert_eq!(fold("한글/É", true).unwrap(), fold("한글/e\u{301}", true).unwrap());
        assert_ne!(fold("A", false).unwrap(), fold("a", false).unwrap());
        assert_ne!(fold("é", true).unwrap(), fold("e", true).unwrap());
    }
}
