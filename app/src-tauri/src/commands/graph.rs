//! The Graph tab (ADR-0033): `o graph view|build|deep|settings`, dev `/api/graph/*`.
//! The view and the settings are read from O's state; build runs grafly ($0) and Deep runs
//! the official CLI the person chose, once per click.

use std::path::Path;

use o_core::agents::clean;
use o_core::graph::{self, settings};
use o_core::run::plan::ProviderId;
use o_core::run::store;
use serde_json::{json, Value};

use super::blocking;

/// An absolute folder that exists; o-core checks again whether it may be a project.
fn project_arg(project: &str) -> Result<(), String> {
    if !Path::new(project).is_absolute() || !Path::new(project).is_dir() {
        return Err(format!("프로젝트 폴더가 없습니다: {project}"));
    }
    Ok(())
}

/// The whole graph (no project) or one project's (`o graph view --json`).
#[tauri::command]
pub async fn graph_view(project: Option<String>) -> Result<Value, String> {
    if let Some(project) = &project { project_arg(project)?; }
    blocking(move || graph::view(&store::default_home(), project.as_deref()).map(|view| json!(view))).await?
}

/// One project's slice of the overall graph (`o graph part`): the screen draws the overall
/// graph a project at a time instead of waiting for all of them.
#[tauri::command]
pub async fn graph_overall_part(project: String) -> Result<Value, String> {
    project_arg(&project)?;
    blocking(move || graph::view::overall_part(&store::default_home(), &project).map(|view| json!(view))).await?
}

/// Builds the project's grafly index, never its LLM pass (`o graph build`).
#[tauri::command]
pub async fn graph_build(project: String) -> Result<Value, String> {
    project_arg(&project)?;
    blocking(move || graph::build(&store::default_home(), &project)).await?
}

/// Names and summarizes the project's areas with the person's signed-in CLI (`o graph deep`).
#[tauri::command]
pub async fn graph_deep(project: String, provider: Option<String>, model: Option<String>, clean: bool) -> Result<Value, String> {
    project_arg(&project)?;
    blocking(move || {
        let home = store::default_home();
        let provider = provider.as_deref().map(|p| ProviderId::parse(p).ok_or_else(|| format!("알 수 없는 에이전트: {p}"))).transpose()?;
        let model = model.filter(|m| !m.is_empty());
        let root = clean.then(clean::default_root);
        graph::deep::run(&home, &project, provider, model.as_deref(), root.as_deref()).map(|done| json!(done))
    })
    .await?
}

/// The Graph settings, changed first when `change` is given (`o graph settings`).
#[tauri::command]
pub async fn graph_settings(change: Option<Value>) -> Result<Value, String> {
    blocking(move || {
        let home = store::default_home();
        match change {
            None => Ok(json!(settings::read(&home))),
            Some(change) => {
                let change: settings::Change = serde_json::from_value(change).map_err(|e| format!("설정 형식이 올바르지 않습니다: {e}"))?;
                settings::update(&home, change).map(|saved| json!(saved))
            }
        }
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_commands_take_only_existing_absolute_folders() {
        assert!(project_arg("relative/path").is_err());
        assert!(project_arg(&std::env::temp_dir().join("project-o-graph-missing-folder").to_string_lossy()).is_err());
        assert!(project_arg(&std::env::temp_dir().to_string_lossy()).is_ok());
    }
}
