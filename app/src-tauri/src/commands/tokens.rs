//! Tokens per card for the dashboard (`o tokens --json`, dev `/api/tokens`, ADR-0005).

use std::path::Path;

use o_core::context::tokens::{report, TokenQuery};
use o_core::run::store;
use serde_json::json;

use super::blocking;

/// The token report `o tokens --json [--project <폴더>]` prints, as JSON text.
pub fn tokens_json(home: &Path, project: Option<&str>) -> Result<String, String> {
    let report = report(home, &TokenQuery { project, since: None }).map_err(|e| e.to_string())?;
    Ok(json!(report).to_string())
}

#[tauri::command]
pub async fn tokens(project: Option<String>) -> Result<String, String> {
    blocking(move || tokens_json(&store::default_home(), project.as_deref())).await?
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::commands::temp;

    #[test]
    fn tokens_without_runs_is_an_empty_report() {
        let home = temp("tokens");
        let body: Value = serde_json::from_str(&tokens_json(&home, None).unwrap()).unwrap();
        let lead = json!({ "turns": 0, "input": 0, "cached": 0, "output": 0, "total": 0 });
        assert_eq!(body, json!({ "groups": [], "compare": { "baseline": null, "shared": null }, "runs": [], "lead": lead }));
    }
}
