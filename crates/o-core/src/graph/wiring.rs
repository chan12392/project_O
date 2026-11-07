use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Wiring {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Node {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub span: Option<String>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub exported: bool,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(default)]
    pub relation: String,
    #[serde(default)]
    pub confidence: Option<String>,
}

pub fn load(project: &Path) -> Result<Option<Wiring>, String> {
    let path = project.join(".grafly/.graph/wiring.json");
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(format!("{}: graph exceeds 64 MiB", path.display()));
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("{}: {e}", path.display()))
}
