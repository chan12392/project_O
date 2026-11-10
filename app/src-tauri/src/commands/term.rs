//! Terminal sessions shared with `o term`.
use o_core::{run::store, term::{self, Chunk, OpenRequest, Session}};
use super::blocking;

#[tauri::command]
pub async fn term_open(provider: String, project: String, thread: Option<String>, cols: Option<u16>, rows: Option<u16>) -> Result<Session, String> {
    blocking(move || term::open(&store::default_home(), OpenRequest { provider, project, thread, cols, rows }).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn term_list() -> Result<Vec<Session>, String> {
    blocking(move || term::list(&store::default_home()).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn term_read(id: String, offset: u64) -> Result<Chunk, String> {
    blocking(move || term::read(&store::default_home(), &id, offset).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn term_write(id: String, data: String) -> Result<(), String> {
    blocking(move || term::decode_data(&data).and_then(|data| term::write(&store::default_home(), &id, &data)).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn term_resize(id: String, cols: u16, rows: u16) -> Result<(), String> {
    blocking(move || term::resize(&store::default_home(), &id, cols, rows).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn term_paste(id: String) -> Result<(), String> {
    blocking(move || term::paste(&store::default_home(), &id).map_err(|e| e.to_string())).await?
}

#[tauri::command]
pub async fn term_close(id: String) -> Result<(), String> {
    blocking(move || term::close(&store::default_home(), &id).map_err(|e| e.to_string())).await?
}
