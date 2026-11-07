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
    let wiring: Wiring = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if wiring.nodes.iter().any(|node| {
        let name = &node.path;
        name.starts_with(['/', '\\'])
            || (name.as_bytes().get(1) == Some(&b':') && name.as_bytes()[0].is_ascii_alphabetic())
            || name.split(['/', '\\']).any(|part| part == "..")
            || name.contains('\0')
    }) {
        return Err(format!("{}: invalid graph node path", path.display()));
    }
    Ok(Some(wiring))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_outside_project_on_all_platforms() {
        let project = std::env::temp_dir().join(format!("o-graph-wiring-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        let path = project.join(".grafly/.graph/wiring.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for name in ["../secret", "dir/../secret", r"dir\..\secret", "/secret", r"\secret", r"C:\secret", "C:secret", "a\0b"] {
            std::fs::write(&path, serde_json::json!({"nodes": [{"id":"n", "path":name}]}).to_string()).unwrap();
            assert!(load(&project).is_err(), "accepted {name:?}");
        }
        std::fs::write(&path, r#"{"nodes":[{"id":"n","path":"src/main.rs"}]}"#).unwrap();
        assert_eq!(load(&project).unwrap().unwrap().nodes[0].path, "src/main.rs");
        std::fs::remove_dir_all(project).unwrap();
    }
}
