//! User annotations are kept outside the append-only conversation event stream.
use std::fs::{self, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::thread_dir;
use crate::run::store::{is_run_id, new_run_id};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: String,
    pub quote: String,
    #[serde(default)]
    pub anchor: String,
    #[serde(default)]
    pub occurrence: usize,
    pub tag: String,
    pub text: String,
    pub sent: bool,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Change { Add { note: Note }, Update { note: Note }, Delete { id: String }, Sent { notes: Vec<Note> } }

pub fn read(home: &Path, thread: &str) -> io::Result<Vec<Note>> {
    if !is_run_id(thread) || !thread_dir(home, thread).is_dir() { return Err(io::Error::other("총괄 대화를 찾지 못했습니다")); }
    match fs::read(thread_dir(home, thread).join("notes.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn change(home: &Path, thread: &str, change: Change) -> io::Result<Vec<Note>> {
    if !is_run_id(thread) || !thread_dir(home, thread).is_dir() { return Err(io::Error::other("총괄 대화를 찾지 못했습니다")); }
    let dir = thread_dir(home, thread);
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("notes.lock"))?;
    lock.lock()?;
    let mut notes = read(home, thread)?;
    match change {
        Change::Add { note } | Change::Update { note } => {
            if !is_run_id(&note.id) || note.sent || note.quote.trim().is_empty() || note.quote.chars().count() > 500 || note.anchor.chars().count() > 2000 || note.occurrence > 10000 || note.text.trim().is_empty() || note.text.chars().count() > 2000 || !["태그", "주석"].contains(&note.tag.as_str()) {
                return Err(io::Error::other("주석 내용이나 개수가 올바르지 않습니다"));
            }
            if let Some(existing) = notes.iter_mut().find(|existing| existing.id == note.id) { *existing = note; }
            else if notes.len() < 100 { notes.push(note); }
            else { return Err(io::Error::other("주석은 100개까지 저장할 수 있습니다")); }
        }
        Change::Delete { id } => notes.retain(|note| note.id != id),
        Change::Sent { notes: sent } => for note in &mut notes { if sent.contains(note) { note.sent = true; } },
    }
    let temp = dir.join(format!("notes-{}.tmp", new_run_id(chrono::Utc::now())));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
    file.write_all(&serde_json::to_vec(&notes).map_err(io::Error::other)?)?;
    file.sync_all()?;
    fs::rename(temp, dir.join("notes.json"))?;
    Ok(notes)
}
