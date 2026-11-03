use o_core::run::store::{default_home, user_home};
use o_core::schedule::{self, Job, Request};
use super::{blocking, o_binary};

#[tauri::command]
pub async fn schedule_list() -> Result<Vec<Job>, String> { blocking(|| schedule::list(&default_home())).await? }

#[tauri::command]
pub async fn schedule_create(request: Request) -> Result<Job, String> {
    blocking(move || schedule::create(&default_home(), &user_home(), &o_binary().ok_or("예약 실행용 O CLI가 없습니다")?, request)).await?
}

#[tauri::command]
pub async fn schedule_delete(id: String) -> Result<(), String> { blocking(move || schedule::delete(&default_home(), &id)).await? }

#[tauri::command]
pub async fn schedule_set_enabled(id: String, enabled: bool) -> Result<Job, String> { blocking(move || schedule::set_enabled(&default_home(), &id, enabled)).await? }
