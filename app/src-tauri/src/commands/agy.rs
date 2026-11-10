use o_core::agents::agy::{self, AgyReport, Change};
use o_core::run::store;
use super::blocking;

#[tauri::command]
pub async fn agy_get() -> Result<AgyReport, String> {
    blocking(|| agy::report(&store::default_home())).await
}

#[tauri::command]
pub async fn agy_set(workers: Option<bool>, ack: Option<bool>, confirm_paste: Option<bool>) -> Result<AgyReport, String> {
    blocking(move || agy::set(&store::default_home(), Change { workers, ack, confirm_paste })).await?
}
