//! The project list (ADR-0009): `o projects` and dev `/api/projects/*` print the same JSON.
//! The folder picker is the native dialog, opened from Rust; the screens get no dialog
//! permission of their own.

use o_core::projects::{self, Listing};
use o_core::run::store::{default_home, user_home};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use super::blocking;

#[tauri::command]
pub async fn projects_list() -> Result<Listing, String> {
    blocking(|| projects::list(&default_home(), &user_home())).await?
}

#[tauri::command]
pub async fn projects_add(path: String) -> Result<Listing, String> {
    blocking(move || projects::add(&default_home(), &user_home(), &path)).await?
}

#[tauri::command]
pub async fn project_open(path: String) -> Result<(), String> {
    blocking(move || projects::open(&default_home(), &user_home(), &path)).await?
}

#[tauri::command]
pub async fn projects_remove(path: String) -> Result<Listing, String> {
    blocking(move || projects::remove(&default_home(), &user_home(), &path)).await?
}

#[derive(Serialize)]
pub struct Created {
    created: String,
    #[serde(flatten)]
    listing: Listing,
}

#[tauri::command]
pub async fn projects_new(name: String) -> Result<Created, String> {
    blocking(move || projects::new_project(&default_home(), &user_home(), &name).map(|(created, listing)| Created { created, listing })).await?
}

/// Settings → 프로젝트: another root (`path`), back to the default (`reset`), and/or make it (`create`).
#[tauri::command]
pub async fn projects_root(path: Option<String>, reset: bool, create: bool) -> Result<Listing, String> {
    blocking(move || {
        let (home, user) = (default_home(), user_home());
        let listing = match (path, reset) {
            (_, true) => projects::set_root(&home, &user, None)?,
            (Some(path), false) => projects::set_root(&home, &user, Some(&path))?,
            (None, false) => projects::list(&home, &user)?,
        };
        if create && !listing.root_exists {
            return projects::create_root(&home, &user);
        }
        Ok(listing)
    })
    .await?
}

#[derive(Serialize)]
pub struct Checked {
    path: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

/// Which folders found in the history may be listed ("기록에서 찾은 프로젝트").
#[tauri::command]
pub async fn projects_check(paths: Vec<String>) -> Result<Vec<Checked>, String> {
    blocking(move || {
        let (home, user) = (default_home(), user_home());
        paths
            .into_iter()
            .take(200)
            .map(|path| match projects::check(&home, &user, &path) {
                Ok(()) => Checked { path, ok: true, reason: None },
                Err(reason) => Checked { path, ok: false, reason: Some(reason) },
            })
            .collect()
    })
    .await
}

/// The native folder picker; `None` when the person cancels. Registering is a separate call.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, String> {
    blocking(move || {
        let picked = app.dialog().file().set_title("목록에 넣을 프로젝트 폴더 고르기").blocking_pick_folder();
        picked.and_then(|p| p.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
    })
    .await
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::commands::temp;

    #[test]
    fn a_new_project_answers_like_o_projects_new_json() {
        let base = temp("projects");
        let (home, user) = (base.join("project_O"), base.join("user"));
        projects::create_root(&home, &user).unwrap();
        let (created, listing) = projects::new_project(&home, &user, "blog").unwrap();
        let body: Value = serde_json::to_value(Created { created, listing }).unwrap();
        let root = user.join(".project_o").join("project").to_string_lossy().replace('\\', "/");
        assert_eq!(body["created"], json!(format!("{root}/blog")));
        assert_eq!(body["root"], json!(root));
        assert_eq!(body["projects"][0]["kind"], "root");
    }
}
