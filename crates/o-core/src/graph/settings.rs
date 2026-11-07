//! Persistent graph preferences (ADR-0033).
use std::{fs, io, path::Path};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GraphSettings {
    pub effects: bool,
    pub particles: bool,
    pub deep_provider: String,
    pub codex_model: String,
    pub claude_model: String,
    pub warn_deep: bool,
    pub max_nodes: usize,
    pub depth: usize,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct Change {
    pub effects: Option<bool>,
    pub particles: Option<bool>,
    pub deep_provider: Option<String>,
    pub codex_model: Option<String>,
    pub claude_model: Option<String>,
    pub warn_deep: Option<bool>,
    pub max_nodes: Option<usize>,
    pub depth: Option<usize>,
}

impl Default for GraphSettings {
    fn default() -> Self {
        Self { effects: true, particles: true, deep_provider: "codex".into(), codex_model: "gpt-6.1-terra".into(), claude_model: "sonnet".into(), warn_deep: true, max_nodes: 1500, depth: 2 }
    }
}

impl GraphSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.deep_provider.as_str(), "codex" | "claude") { return Err("deepProvider must be codex or claude".into()); }
        if !crate::policy::is_model_id(&self.codex_model) || !crate::policy::is_model_id(&self.claude_model) { return Err("invalid model id".into()); }
        if !(100..=5000).contains(&self.max_nodes) { return Err("maxNodes must be between 100 and 5000".into()); }
        if !(1..=4).contains(&self.depth) { return Err("depth must be between 1 and 4".into()); }
        Ok(())
    }
}

fn read_map(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn load(home: &Path) -> Result<GraphSettings, String> {
    let path = home.join("graph/settings.json");
    let settings: GraphSettings = serde_json::from_value(Value::Object(read_map(&path)?)).map_err(|e| format!("{}: {e}", path.display()))?;
    settings.validate()?;
    Ok(settings)
}

pub fn read(home: &Path) -> GraphSettings { load(home).unwrap_or_default() }

/// Keep unknown fields; malformed or invalid existing files are never overwritten.
pub fn save(home: &Path, settings: &GraphSettings) -> Result<(), String> {
    settings.validate()?;
    let dir = home.join("graph");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("settings.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    write_locked(&dir, settings)
}

fn write_locked(dir: &Path, settings: &GraphSettings) -> Result<(), String> {
    let path = dir.join("settings.json");
    let mut map = read_map(&path)?;
    let old: GraphSettings = serde_json::from_value(Value::Object(map.clone())).map_err(|e| e.to_string())?;
    old.validate()?;
    let Value::Object(updated) = serde_json::to_value(settings).map_err(|e| e.to_string())? else { unreachable!() };
    map.extend(updated);
    let temp = dir.join(format!("settings.{}.tmp", crate::run::store::new_run_id(chrono::Utc::now())));
    let bytes = serde_json::to_vec_pretty(&map).map_err(|e| e.to_string())?;
    fs::write(&temp, bytes).and_then(|()| fs::rename(&temp, &path)).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn update(home: &Path, change: Change) -> Result<GraphSettings, String> {
    let dir = home.join("graph");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lock = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(dir.join("settings.lock")).map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    let mut settings = load(home)?;
    if let Some(v) = change.effects { settings.effects = v; }
    if let Some(v) = change.particles { settings.particles = v; }
    if let Some(v) = change.deep_provider { settings.deep_provider = v; }
    if let Some(v) = change.codex_model { settings.codex_model = v; }
    if let Some(v) = change.claude_model { settings.claude_model = v; }
    if let Some(v) = change.warn_deep { settings.warn_deep = v; }
    if let Some(v) = change.max_nodes { settings.max_nodes = v; }
    if let Some(v) = change.depth { settings.depth = v; }
    settings.validate()?;
    write_locked(&dir, &settings)?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn home() -> std::path::PathBuf { std::env::temp_dir().join(format!("o-graph-settings-{}", crate::run::store::new_run_id(chrono::Utc::now()))) }
    #[test]
    fn defaults_validation_and_preservation() {
        let home = home();
        let defaults = GraphSettings::default();
        assert_eq!(load(&home).unwrap(), defaults);
        let mut invalid = defaults.clone();
        invalid.codex_model = "--bad model".into();
        assert!(save(&home, &invalid).is_err());
        invalid = defaults.clone(); invalid.depth = 0;
        assert!(save(&home, &invalid).is_err());
        invalid = defaults.clone(); invalid.max_nodes = 5001;
        assert!(save(&home, &invalid).is_err());
        fs::create_dir_all(home.join("graph")).unwrap();
        let path = home.join("graph/settings.json");
        fs::write(&path, r#"{"unknown":42}"#).unwrap();
        save(&home, &defaults).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["unknown"], 42);
        let changed = update(&home, Change { effects: Some(false), ..Change::default() }).unwrap();
        assert!(!changed.effects);
        assert_eq!(load(&home).unwrap(), changed);
        assert_eq!(serde_json::from_slice::<Value>(&fs::read(&path).unwrap()).unwrap()["unknown"], 42);
        fs::write(&path, b"{broken").unwrap();
        assert!(load(&home).is_err());
        assert!(save(&home, &defaults).is_err());
        assert!(update(&home, Change { effects: Some(false), ..Change::default() }).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"{broken");
        let _ = fs::remove_dir_all(home);
    }
}
