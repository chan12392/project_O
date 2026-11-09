//! The agent CLIs: status, the official install and sign-in (`o agents …`; dev `/api/agents`).

use o_core::agents::{self, catalog, clean, ActionResult, AgentStatus};
use o_core::run::plan::ProviderId;

use super::blocking;

fn provider(id: &str) -> Result<ProviderId, String> {
    ProviderId::parse(id).ok_or_else(|| format!("에이전트는 {} 중 하나입니다: {id}", ProviderId::ALL.map(ProviderId::as_str).join(", ")))
}

#[tauri::command]
pub async fn agents_status(clean: bool) -> Result<Vec<AgentStatus>, String> {
    blocking(move || { o_core::sys::resolve::refresh_paths(); agents::status(&clean::default_root(), clean) }).await
}

#[tauri::command]
pub async fn agents_catalog(refresh: bool) -> Result<Vec<catalog::Listed>, String> {
    blocking(move || catalog::cached_list(refresh)).await
}

#[tauri::command]
pub async fn agent_install(id: String) -> Result<ActionResult, String> {
    let provider = provider(&id)?;
    blocking(move || { o_core::sys::resolve::ensure_paths(); agents::install(provider) }).await
}

#[tauri::command]
pub async fn agent_login(id: String, clean: bool) -> Result<ActionResult, String> {
    let provider = provider(&id)?;
    blocking(move || { o_core::sys::resolve::ensure_paths(); agents::open_login(&clean::default_root(), provider, clean) }).await
}

#[tauri::command]
pub async fn agent_terminal() -> Result<ActionResult, String> {
    blocking(agents::open_terminal).await
}

#[tauri::command]
pub async fn open_external(url: String) -> Result<(), String> {
    blocking(move || agents::open_external(&url)).await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_agent_ids_are_refused() {
        assert!(provider("claude").is_ok());
        assert!(provider("../claude").is_err());
        assert!(provider("").is_err());
    }
}
