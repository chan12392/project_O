//! Normalize declared resources before they are reserved across runs.
use std::io;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plan::{Resource, ResourceKind, ResourceMode};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Key {
    kind: ResourceKind,
    key: String,
    mode: ResourceMode,
}

pub(crate) fn normalize(project: &str, resources: &[Resource]) -> io::Result<Vec<Key>> {
    if resources.is_empty() { return Ok(Vec::new()); }
    let root = if resources.iter().any(|r| r.kind == ResourceKind::ProjectPath) { Some(std::fs::canonicalize(project)?) } else { None };
    resources.iter().map(|resource| {
        let key = match resource.kind {
            ResourceKind::External => resource.key.trim().to_lowercase(),
            ResourceKind::ProjectPath => {
                let root = root.as_ref().expect("project path requires a root");
                let value = resource.key.replace('\\', "/");
                let relative = Path::new(&value);
                if relative.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
                    return Err(io::Error::other("자원 경로는 프로젝트 안의 상대 경로여야 합니다"));
                }
                let mut path = root.join(relative);
                let mut missing = Vec::new();
                if cfg!(windows) && relative.components().any(|c| match c { Component::Normal(s) => { let s = s.to_string_lossy(); s.contains(':') || s.ends_with(['.', ' ']) }, _ => false }) {
                    return Err(io::Error::other("자원 경로 이름이 모호합니다"));
                }
                loop {
                    match std::fs::symlink_metadata(&path) {
                        Ok(_) => break,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            let name = path.file_name().ok_or_else(|| io::Error::other("자원 경로를 확인할 수 없습니다"))?.to_owned();
                            missing.push(name);
                            path = path.parent().ok_or_else(|| io::Error::other("자원 경로를 확인할 수 없습니다"))?.to_path_buf();
                        }
                        Err(e) => return Err(e),
                    }
                }
                let mut resolved = path.canonicalize()?;
                if !resolved.starts_with(root) { return Err(io::Error::other("프로젝트 밖을 가리키는 자원 경로입니다")); }
                for name in missing.iter().rev() { resolved.push(name); }
                let text = resolved.to_string_lossy().replace('\\', "/");
                if cfg!(windows) { text.to_lowercase() } else { text }
            }
        };
        Ok(Key { kind: resource.kind, key, mode: resource.mode })
    }).collect()
}

pub(crate) fn conflicts(a: &[Key], b: &[Key]) -> bool {
    if a.is_empty() || b.is_empty() { return true; }
    a.iter().any(|x| b.iter().any(|y| {
        if x.kind != y.kind || (x.mode == ResourceMode::Read && y.mode == ResourceMode::Read) { return false; }
        match x.kind {
            ResourceKind::External => x.key == y.key,
            ResourceKind::ProjectPath => PathBuf::from(&x.key).starts_with(&y.key) || PathBuf::from(&y.key).starts_with(&x.key),
        }
    }))
}
