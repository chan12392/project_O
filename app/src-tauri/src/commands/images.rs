use o_core::run::store::{default_home, user_home};
use super::blocking;

#[tauri::command]
pub async fn image_read(project: String, run: Option<String>, card: Option<String>, path: String) -> Result<o_core::images::Image, String> {
    blocking(move || o_core::images::read(&default_home(), &user_home(), &project, run.as_deref(), card.as_deref(), &path)).await?
}

#[tauri::command]
pub async fn image_open(project: String, run: Option<String>, card: Option<String>, path: String, folder: bool) -> Result<(), String> {
    blocking(move || o_core::images::open(&default_home(), &user_home(), &project, run.as_deref(), card.as_deref(), &path, folder)).await?
}
