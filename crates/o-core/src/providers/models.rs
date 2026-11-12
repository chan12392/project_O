//! One catalog for lead, workers and shells, keyed by the actual launch environment.
use std::fs::{self, OpenOptions};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::clean;
use crate::run::plan::ProviderId;
use crate::sys::{js::now_iso, proc, resolve::resolve_cli};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub efforts: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub provider: ProviderId,
    pub profile: String,
    pub models: Vec<Model>,
    pub checked_at: Option<String>,
    pub stale: bool,
    pub error: Option<String>,
    pub worker_available: bool,
    pub unavailable_reason: Option<String>,
}

fn cache_path(home: &Path, provider: ProviderId, root: Option<&Path>, cwd: &Path) -> std::path::PathBuf {
    let profile = crate::usage::accounting::profile(provider, root);
    let stamp = resolve_cli(provider.as_str()).map(|c| {
        let scripts: Vec<_> = c.prefix.iter().map(|arg| fs::metadata(arg).and_then(|m| m.modified()).ok()).collect();
        format!("{}:{:?}:{:?}:{scripts:?}", c.command.display(), c.prefix, fs::metadata(&c.command).and_then(|m| m.modified()).ok())
    }).unwrap_or_default();
    let key = crate::run::result::digest(&format!("{provider}|{}|{}|{stamp}", profile.display(), crate::run::store::resolved_path(&cwd.to_string_lossy())));
    home.join("model-catalog").join(format!("{key}.json"))
}

pub fn cached(home: &Path, provider: ProviderId, root: Option<&Path>, cwd: &Path) -> Option<Catalog> {
    let mut value: Catalog = serde_json::from_slice(&fs::read(cache_path(home, provider, root, cwd)).ok()?).ok()?;
    value.stale |= value.checked_at.as_deref().and_then(crate::sys::js::parse_time).is_none_or(|at| chrono::Utc::now().signed_duration_since(at).num_seconds() >= 600);
    Some(value)
}

pub fn parse(provider: ProviderId, text: &str) -> Vec<Model> {
    if provider == ProviderId::OpenCode {
        return super::opencode::parse_models(text).into_iter().map(|m| Model { id: m.id, label: m.label, hint: m.hint, efforts: Vec::new() }).collect();
    }
    let value = if provider == ProviderId::Claude {
        text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).find_map(|v| {
            (v["type"] == "control_response" && v["response"]["subtype"] == "success").then(|| v["response"]["response"].clone())
        }).unwrap_or(Value::Null)
    } else { serde_json::from_str(text).unwrap_or(Value::Null) };
    value["models"].as_array().into_iter().flatten().filter_map(|m| {
        if provider == ProviderId::Codex && m["visibility"] != "list" { return None; }
        let id = if provider == ProviderId::Claude { m["value"].as_str() } else { m["slug"].as_str() }?;
        if !crate::policy::is_model_id(id) { return None; }
        let label = if provider == ProviderId::Claude { m["displayName"].as_str() } else { m["display_name"].as_str() }.unwrap_or(id);
        let levels = if provider == ProviderId::Claude { &m["supportedEffortLevels"] } else { &m["supported_reasoning_levels"] };
        let efforts = levels.as_array().into_iter().flatten().filter_map(|level| level.as_str().or_else(|| level["effort"].as_str()).or_else(|| level["reasoning_effort"].as_str())).map(str::to_owned).collect();
        Some(Model { id: id.into(), label: label.into(), hint: m["resolvedModel"].as_str().map(str::to_owned), efforts })
    }).collect()
}

pub fn catalog(home: &Path, provider: ProviderId, profile_root: Option<&Path>, cwd: &Path, refresh: bool) -> Result<Catalog, String> {
    let profile = crate::usage::accounting::profile(provider, profile_root).to_string_lossy().into_owned();
    let cli = resolve_cli(provider.as_str());
    let path = cache_path(home, provider, profile_root, cwd);
    let dir = home.join("model-catalog");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path.with_extension("lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let old: Option<Catalog> = fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if !refresh {
        if let Some(catalog) = old.as_ref().filter(|c| !c.stale && c.checked_at.as_deref().and_then(crate::sys::js::parse_time).is_some_and(|at| chrono::Utc::now().signed_duration_since(at).num_seconds() < 600)) { return Ok(catalog.clone()); }
    }
    let mut catalog = old.unwrap_or(Catalog { provider, profile, models: Vec::new(), checked_at: None, stale: true, error: None, worker_available: false, unavailable_reason: None });
    let result = (|| {
        let cli = cli.ok_or_else(|| format!("{provider} CLI를 찾지 못했습니다"))?;
        if matches!(provider, ProviderId::Agy | ProviderId::Devin) { return Err("이 에이전트는 자동 모델 목록 조회를 지원하지 않습니다".into()); }
        let mut command = Command::new(&cli.command);
        command.args(&cli.prefix).current_dir(cwd);
        if let Some(root) = profile_root { command.env_clear().envs(clean::environment(root, provider).map_err(|e| e.to_string())?); }
        command.envs(super::get(provider).check_env().iter().copied());
        let input = match provider {
            ProviderId::Codex => { command.args(["debug", "models"]); None }
            ProviderId::OpenCode => { command.arg("models"); None }
            ProviderId::Claude => {
                command.args(["--print", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose", "--setting-sources", "user", "--settings", "{\"disableAllHooks\":true}", "--strict-mcp-config", "--mcp-config", "{\"mcpServers\":{}}"]);
                Some("{\"type\":\"control_request\",\"request_id\":\"o-models\",\"request\":{\"subtype\":\"initialize\"}}\n")
            }
            _ => unreachable!(),
        };
        let (status, out, _) = proc::capture_input(&mut command, input, Duration::from_secs(20))?;
        let models = parse(provider, &out);
        if !status.success() || models.is_empty() { return Err("공식 CLI에서 모델 메타데이터를 확인하지 못했습니다".into()); }
        Ok(models)
    })();
    match result {
        Ok(models) => { catalog.models = models; catalog.checked_at = Some(now_iso()); catalog.stale = false; catalog.error = None; }
        Err(error) => {
            catalog.stale = true;
            catalog.error = Some(error);
            if catalog.models.is_empty() && provider == ProviderId::Claude {
                catalog.models = ["fable", "opus", "sonnet", "haiku"].into_iter().map(|id| Model { id: id.into(), label: format!("{id} (버전 미확인)"), hint: None, efforts: super::get(provider).lead_efforts().iter().map(|s| (*s).into()).collect() }).collect();
            }
        }
    }
    catalog.unavailable_reason = if matches!(provider, ProviderId::Agy | ProviderId::Devin) { Some("자동 모델·effort 조회를 지원하지 않습니다".into()) }
        else {
            let status = crate::agents::status_of(profile_root.unwrap_or(&clean::default_root()), provider, profile_root.is_some());
            if !status.installed { Some("CLI 미설치".into()) } else if status.logged_in != Some(true) { Some("로그인 상태 미확인".into()) } else { status.profile_not_ready.or(status.profile_unsupported) }
        };
    catalog.worker_available = catalog.unavailable_reason.is_none() && !super::get(provider).terminal_card();
    let temp = path.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec(&catalog).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())?;
    Ok(catalog)
}

pub fn validate(catalog: &Catalog, model: &str, effort: Option<&str>) -> Result<(), String> {
    if !crate::policy::is_model_id(model) { return Err("모델 ID 형식이 올바르지 않습니다".into()); }
    if let Some(found) = catalog.models.iter().find(|m| m.id == model) {
        if let Some(effort) = effort {
            if !found.efforts.is_empty() && !found.efforts.iter().any(|e| e == effort) { return Err(format!("{model}은 effort {effort}를 지원하지 않습니다")); }
        }
    }
    Ok(())
}

pub fn worker_prompt(home: &Path, root: Option<&Path>, cwd: &Path) -> String {
    let models: Vec<Value> = ProviderId::ALL.into_iter().filter(|p| !matches!(p, ProviderId::Agy | ProviderId::Devin)).map(|provider| {
        match cached(home, provider, root, cwd) {
            Some(c) => json!({"provider":provider,"available":c.worker_available,"reason":c.unavailable_reason,"stale":c.stale,"models":c.models}),
            None => json!({"provider":provider,"available":null,"reason":"모델 목록 미확인: 사용자 지정 모델만 선택 가능"}),
        }
    }).collect();
    format!("작업 모델 후보(실행 프로필 기준, 미확인 값은 추측하지 않음): {}", json!(models))
}

pub fn refresh_workers(home: &Path, root: Option<&Path>, cwd: &Path) {
    std::thread::scope(|scope| {
        for provider in [ProviderId::Claude, ProviderId::Codex, ProviderId::OpenCode] {
            scope.spawn(move || { let _ = catalog(home, provider, root, cwd, false); });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_preserves_resolved_model_and_effort() {
        let text = json!({"type":"control_response","response":{"subtype":"success","response":{"models":[{"value":"fable","resolvedModel":"claude-fable-5-2","displayName":"Fable 5.2","supportedEffortLevels":["low","medium"]}]}}}).to_string();
        let found = parse(ProviderId::Claude, &text);
        assert_eq!(found[0].hint.as_deref(), Some("claude-fable-5-2"));
        assert_eq!(found[0].efforts, ["low", "medium"]);
        assert!(parse(ProviderId::Claude, "{\"models\":[]}").is_empty());
    }

    #[test]
    fn cache_separates_profiles_and_projects_and_marks_old_metadata() {
        let dir = std::env::temp_dir().join(format!("o-model-cache-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        let profile = dir.join("profile-a");
        let cwd = dir.join("project-a");
        let path = cache_path(&dir, ProviderId::Claude, Some(&profile), &cwd);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let model = Model { id: "custom-model".into(), label: "Fixture model".into(), hint: None, efforts: vec!["medium".into()] };
        let catalog = Catalog { provider: ProviderId::Claude, profile: profile.to_string_lossy().into_owned(), models: vec![model], checked_at: Some((chrono::Utc::now() - chrono::Duration::minutes(11)).to_rfc3339()), stale: false, error: None, worker_available: true, unavailable_reason: None };
        fs::write(path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        assert!(cached(&dir, ProviderId::Claude, Some(&profile), &cwd).unwrap().stale);
        assert!(cached(&dir, ProviderId::Claude, Some(&dir.join("profile-b")), &cwd).is_none());
        assert!(cached(&dir, ProviderId::Claude, Some(&profile), &dir.join("project-b")).is_none());
        assert!(validate(&catalog, "custom-model", Some("high")).is_err());
        assert!(validate(&catalog, "explicit-new-model", Some("medium")).is_ok());
    }
}
