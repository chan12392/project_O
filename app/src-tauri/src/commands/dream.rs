//! The Wiki and Dreaming tabs (ADR-0041): `o dream list|read|graph|add|remove`, dev `/api/dream/*`.
//! Materials a person put in and one record per finished card, read locally.

use o_core::{dream, tuning};
use o_core::run::store;
use serde_json::{json, Value};

use super::blocking;

/// Every document with its title (`o dream list`).
#[tauri::command]
pub async fn dream_list() -> Result<Value, String> {
    blocking(|| dream::list(&store::default_home()).map(|list| json!(list))).await?
}

/// One listed document's text and what it is wired to (`o dream read`).
#[tauri::command]
pub async fn dream_read(path: String) -> Result<Value, String> {
    blocking(move || dream::read(&store::default_home(), &path).map(|page| json!(page))).await?
}

/// The documents as one graph (`o dream graph`).
#[tauri::command]
pub async fn dream_graph() -> Result<Value, String> {
    blocking(|| dream::graph(&store::default_home()).map(|graph| json!(graph))).await?
}

/// Puts a material in: pasted text, or a web address fetched once (`o dream add -`).
#[tauri::command]
pub async fn dream_add(input: Value) -> Result<Value, String> {
    blocking(move || dream::add(&store::default_home(), &input).map(|doc| json!(doc))).await?
}

/// What Dreaming changed in card prompts from repeated failures (`o dream changes`).
#[tauri::command]
pub async fn dream_changes() -> Result<Value, String> {
    blocking(|| tuning::refresh(&store::default_home()).map(|changes| json!(changes))).await?
}

/// Takes one change back, or lets it apply again (`o dream revert|restore`).
#[tauri::command]
pub async fn dream_revert(id: String, reverted: bool) -> Result<Value, String> {
    blocking(move || tuning::set_reverted(&store::default_home(), &id, reverted).map(|change| json!(change))).await?
}

/// Takes one material out (`o dream remove`).
#[tauri::command]
pub async fn dream_remove(path: String) -> Result<(), String> {
    blocking(move || dream::remove(&store::default_home(), &path)).await?
}
