//! Settings → 정보: version and folders (`o about --json`, dev `/api/about`).

use std::path::Path;

use o_core::agents::clean;
use o_core::run::store;
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct About {
    pub version: String,
    pub commit: String,
    pub built_at: String,
    pub home: String,
    pub agents_home: String,
}

pub fn about_info(home: &Path, agents_home: &Path) -> About {
    About { version: env!("CARGO_PKG_VERSION").into(), commit: super::update::BUILD_SHA.into(), built_at: super::update::BUILD_TIME.into(), home: home.to_string_lossy().into_owned(), agents_home: agents_home.to_string_lossy().into_owned() }
}

#[tauri::command]
pub fn about() -> About {
    about_info(&store::default_home(), &clean::default_root())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn about_names_version_and_folders() {
        let about = about_info(Path::new("C:/state/project_O"), Path::new("C:/state/project_O-agents"));
        let value = json!(about);
        assert_eq!(value["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(value["home"], "C:/state/project_O");
        assert_eq!(value["agentsHome"], "C:/state/project_O-agents");
    }
}
