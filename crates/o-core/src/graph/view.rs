//! Hierarchical graph data for the graph screen.
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::Path;
use serde::Serialize;
use super::{git, grafly, settings, wiring::{self, Wiring}};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView {
    pub scope: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub grafly: grafly::Status,
    pub indexed: bool,
    pub nodes: Vec<GraphNode>,
    pub links: Vec<GraphLink>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deep: Option<DeepInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DeepInfo {
    pub at: String,
    pub provider: String,
    pub model: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub size: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glow: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphLink {
    pub source: String,
    pub target: String,
    pub kind: String,
    pub weight: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glow: Option<bool>,
}

fn node(id: String, label: String, kind: &str, project: Option<&str>, path: Option<&str>, size: usize, glow: Option<&str>) -> GraphNode {
    GraphNode { id, label, kind: kind.into(), project: project.map(str::to_owned), path: path.map(str::to_owned), size: size.max(1), glow: glow.map(str::to_owned), summary: None, indexed: None }
}
fn link(source: &str, target: &str, kind: &str, weight: usize, glow: bool) -> GraphLink {
    GraphLink { source: source.into(), target: target.into(), kind: kind.into(), weight, glow: glow.then_some(true) }
}
fn name(path: &str) -> &str { path.trim_end_matches('/').rsplit('/').next().unwrap_or(path) }
fn dirs(path: &str) -> Vec<String> {
    let parts: Vec<_> = path.split('/').collect();
    (1..parts.len().min(3)).map(|n| parts[..n].join("/")).collect()
}

struct Data {
    files: BTreeMap<String, usize>,
    edges: BTreeMap<(String, String, String), (usize, bool)>,
    glow: HashMap<String, &'static str>,
}

fn data(wiring: Option<&Wiring>, changed: &HashSet<String>, depth: usize) -> Data {
    let mut files = BTreeMap::new();
    let mut glow = HashMap::new();
    for path in changed { files.insert(path.clone(), 0); glow.insert(path.clone(), "active"); }
    let mut edges = BTreeMap::new();
    let Some(wiring) = wiring else { return Data { files, edges, glow } };
    let nodes: HashMap<_, _> = wiring.nodes.iter().filter(|n| !n.path.is_empty()).map(|n| (n.id.as_str(), n.path.as_str())).collect();
    for n in &wiring.nodes {
        if !n.path.is_empty() { *files.entry(n.path.clone()).or_default() += usize::from(n.kind != "file"); }
    }
    let mut incoming: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in &wiring.edges {
        if matches!(e.relation.as_str(), "calls" | "imports" | "references") && nodes.contains_key(e.source.as_str()) && nodes.contains_key(e.target.as_str()) {
            incoming.entry(&e.target).or_default().push(&e.source);
        }
    }
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    for (id, path) in &nodes {
        if changed.contains(*path) { queue.push_back((*id, 0)); visited.insert(*id); }
    }
    let mut glow_edges = HashSet::new();
    while let Some((target, level)) = queue.pop_front() {
        if level >= depth { continue; }
        for &source in incoming.get(target).into_iter().flatten() {
            let from = nodes[source];
            glow.entry(from.to_string()).or_insert("path");
            glow_edges.insert((source, target));
            if visited.insert(source) { queue.push_back((source, level + 1)); }
        }
    }
    for e in &wiring.edges {
        if !matches!(e.relation.as_str(), "calls" | "imports" | "references") { continue; }
        let (Some(&from), Some(&to)) = (nodes.get(e.source.as_str()), nodes.get(e.target.as_str())) else { continue };
        if from == to { continue; }
        let value = edges.entry((from.to_string(), to.to_string(), e.relation.clone())).or_insert((0, false));
        value.0 += 1;
        value.1 |= glow_edges.contains(&(e.source.as_str(), e.target.as_str()));
    }
    Data { files, edges, glow }
}

/// Build the overall graph, or the graph for a checked project.
pub fn view(home: &Path, project: Option<&str>) -> Result<GraphView, String> {
    let settings = settings::read(home);
    let mut result = GraphView { scope: if project.is_some() { "project" } else { "overall" }.into(), project: project.map(str::to_owned), grafly: grafly::status(), indexed: false, nodes: Vec::new(), links: Vec::new(), truncated: false, deep: None };
    if let Some(project) = project {
        super::check_project(home, project)?;
        let wiring = wiring::load(Path::new(project))?;
        result.indexed = wiring.is_some();
        let changed = git::changed(Path::new(project))?;
        project_graph(&mut result, project, &data(wiring.as_ref(), &changed, settings.depth), settings.max_nodes);
        apply_deep(home, project, &mut result)?;
    } else {
        result.nodes.push(node("o:root".into(), "Project O".into(), "root", None, None, 1, None));
        let listing = crate::projects::list(home, &crate::run::store::user_home())?;
        for item in listing.projects {
            if item.missing || super::check_project(home, &item.path).is_err() { continue; }
            let wiring = wiring::load(Path::new(&item.path))?;
            result.indexed |= wiring.is_some();
            let changed = git::changed(Path::new(&item.path))?;
            overall_project(&mut result, &item.path, &item.name, &data(wiring.as_ref(), &changed, settings.depth), wiring.is_some());
            apply_deep(home, &item.path, &mut result)?;
        }
        cap_overall(&mut result, settings.max_nodes);
    }
    Ok(result)
}

fn project_graph(view: &mut GraphView, project: &str, data: &Data, max: usize) {
    let root = format!("p:{project}");
    let mut root_node = node(root.clone(), name(project).into(), "project", Some(project), None, data.files.len(), (!data.glow.is_empty()).then_some("active"));
    root_node.indexed = Some(view.indexed);
    view.nodes.push(root_node);
    let mut degree: HashMap<&str, usize> = HashMap::new();
    for ((a, b, _), (weight, _)) in &data.edges { *degree.entry(a).or_default() += weight; *degree.entry(b).or_default() += weight; }
    let mut files: Vec<_> = data.files.iter().collect();
    files.sort_by(|(a, _), (b, _)| data.glow.contains_key(*b).cmp(&data.glow.contains_key(*a)).then_with(|| degree.get(b.as_str()).unwrap_or(&0).cmp(degree.get(a.as_str()).unwrap_or(&0))).then_with(|| a.cmp(b)));
    let mut dir_info: HashMap<String, (usize, bool, bool)> = HashMap::new();
    for path in data.files.keys() {
        for dir in dirs(path) {
            let info = dir_info.entry(dir).or_default();
            info.0 += 1;
            info.1 |= data.glow.get(path) == Some(&"active");
            info.2 |= data.glow.contains_key(path);
        }
    }
    let mut inserted = HashSet::new();
    for (path, size) in files {
        let directories = dirs(path);
        let needed = 1 + directories.iter().filter(|d| !inserted.contains(*d)).count();
        if !data.glow.contains_key(path) && view.nodes.len() + needed > max { view.truncated = true; continue; }
        let mut parent = root.clone();
        for dir in directories {
            let id = format!("d:{dir}");
            let (count, active, path_glow) = dir_info[&dir];
            if inserted.insert(dir.clone()) {
                view.nodes.push(node(id.clone(), name(&dir).into(), "area", Some(project), Some(&dir), count, if active { Some("active") } else if path_glow { Some("path") } else { None }));
                view.links.push(link(&parent, &id, "contains", 1, path_glow));
            }
            parent = id;
        }
        let id = format!("f:{path}");
        view.nodes.push(node(id.clone(), name(path).into(), "file", Some(project), Some(path), *size, data.glow.get(path).copied()));
        view.links.push(link(&parent, &id, "contains", 1, data.glow.contains_key(path)));
    }
    let ids: HashSet<_> = view.nodes.iter().map(|n| n.id.as_str()).collect();
    for ((a, b, kind), (weight, glow)) in &data.edges {
        let (a, b) = (format!("f:{a}"), format!("f:{b}"));
        if ids.contains(a.as_str()) && ids.contains(b.as_str()) { view.links.push(link(&a, &b, kind, *weight, *glow)); }
    }
}

fn overall_project(view: &mut GraphView, project: &str, label: &str, data: &Data, indexed: bool) {
    let root = format!("p:{project}");
    let active = data.glow.values().any(|g| *g == "active");
    let mut project_node = node(root.clone(), label.into(), "project", Some(project), None, data.files.len(), active.then_some("active"));
    project_node.indexed = Some(indexed);
    view.nodes.push(project_node);
    view.links.push(link("o:root", &root, "contains", 1, active));
    let mut areas: BTreeMap<String, (usize, bool)> = BTreeMap::new();
    for path in data.files.keys() {
        let Some((dir, _)) = path.split_once('/') else { continue };
        let area = areas.entry(dir.into()).or_default();
        area.0 += 1;
        area.1 |= data.glow.get(path) == Some(&"active");
    }
    let mut areas: Vec<_> = areas.into_iter().collect();
    areas.sort_by(|a, b| b.1.1.cmp(&a.1.1).then_with(|| b.1.0.cmp(&a.1.0)).then_with(|| a.0.cmp(&b.0)));
    for (dir, (count, active)) in areas.into_iter().take(12) {
        let id = format!("a:{project}|{dir}");
        view.nodes.push(node(id.clone(), dir.clone(), "area", Some(project), Some(&dir), count, active.then_some("active")));
        view.links.push(link(&root, &id, "contains", 1, active));
    }
}

fn apply_deep(home: &Path, project: &str, view: &mut GraphView) -> Result<(), String> {
    let path = super::deep::path(home, Path::new(project));
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    const MAX_DEEP_BYTES: u64 = 2 * 1024 * 1024;
    let mut bytes = Vec::new();
    file.take(MAX_DEEP_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_DEEP_BYTES { return Err(format!("{}: Deep exceeds 2 MiB", path.display())); }
    let deep: super::deep::Deep = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut areas = HashMap::new();
    for area in &deep.areas {
        areas.insert(area.id.clone(), area);
        if let Some(dir) = area.id.strip_prefix("d:") { areas.insert(format!("a:{project}|{dir}"), area); }
    }
    for n in &mut view.nodes {
        if let Some(area) = (n.project.as_deref() == Some(project)).then(|| areas.get(&n.id)).flatten() {
            n.label.clone_from(&area.name);
            n.summary = Some(area.summary.clone());
        }
    }
    if view.scope == "project" { view.deep = Some(DeepInfo { at: deep.at, provider: deep.provider, model: deep.model }); }
    Ok(())
}

fn cap_overall(view: &mut GraphView, max: usize) {
    let mut keep = HashSet::new();
    for n in &view.nodes { if n.kind == "root" || n.glow.is_some() { keep.insert(n.id.clone()); } }
    let active_projects: HashSet<_> = view.nodes.iter().filter(|n| n.glow.is_some()).filter_map(|n| n.project.as_ref()).collect();
    for n in &view.nodes { if n.kind == "project" && n.project.as_ref().is_some_and(|p| active_projects.contains(p)) { keep.insert(n.id.clone()); } }
    for n in &view.nodes {
        if keep.len() >= max { break; }
        if n.kind != "area" || n.project.as_ref().is_some_and(|p| keep.contains(&format!("p:{p}"))) { keep.insert(n.id.clone()); }
    }
    view.truncated |= keep.len() < view.nodes.len();
    view.nodes.retain(|n| keep.contains(&n.id));
    view.links.retain(|l| keep.contains(&l.source) && keep.contains(&l.target));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incoming_depth_and_cap_keep_active_paths() {
        let wiring: Wiring = serde_json::from_value(serde_json::json!({
            "nodes": [
                {"id":"a", "path":"src/a.rs"},
                {"id":"b", "path":"src/b.rs"},
                {"id":"c", "path":"src/c.rs"},
                {"id":"d", "path":"other/d.rs"}
            ],
            "edges": [
                {"source":"a", "target":"b", "relation":"calls"},
                {"source":"b", "target":"c", "relation":"calls"},
                {"source":"d", "target":"a", "relation":"calls"}
            ]
        })).unwrap();
        let changed = HashSet::from(["src/c.rs".to_string()]);
        let data = data(Some(&wiring), &changed, 2);
        assert_eq!(data.glow.get("src/c.rs"), Some(&"active"));
        assert_eq!(data.glow.get("src/a.rs"), Some(&"path"));
        assert!(!data.glow.contains_key("other/d.rs"));
        let mut view = GraphView { scope: "project".into(), project: Some("/p".into()), grafly: grafly::Status { installed: false, version: None }, indexed: true, nodes: vec![], links: vec![], truncated: false, deep: None };
        project_graph(&mut view, "/p", &data, 2);
        assert!(view.truncated);
        assert!(view.nodes.iter().any(|n| n.id == "f:src/a.rs"));
        assert!(view.nodes.iter().any(|n| n.id == "f:src/c.rs"));
        assert!(!view.nodes.iter().any(|n| n.id == "f:other/d.rs"));
        assert!(view.links.iter().any(|l| l.source == "f:src/a.rs" && l.target == "f:src/b.rs" && l.glow == Some(true)));
    }

    #[test]
    fn overall_keeps_active_area_when_capped() {
        let mut view = GraphView { scope: "overall".into(), project: None, grafly: grafly::Status { installed: false, version: None }, indexed: true, nodes: vec![node("o:root".into(), "root".into(), "root", None, None, 1, None)], links: vec![], truncated: false, deep: None };
        let changed = HashSet::from(["changed/file.rs".to_string()]);
        let mut data = data(None, &changed, 2);
        data.files.insert("other/file.rs".into(), 1);
        overall_project(&mut view, "/p", "p", &data, true);
        cap_overall(&mut view, 2);
        assert!(view.truncated);
        assert!(view.nodes.iter().any(|n| n.id == "a:/p|changed" && n.glow.as_deref() == Some("active")));
        assert!(view.links.iter().any(|l| l.target == "a:/p|changed" && l.glow == Some(true)));
        assert!(!view.nodes.iter().any(|n| n.id == "a:/p|other"));
    }

    #[test]
    fn deep_metadata_keeps_json_shape_and_rejects_oversized_file() {
        let home = std::env::temp_dir().join(format!("o-graph-view-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        let path = super::super::deep::path(&home, Path::new("/p"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, r#"{"at":"now","provider":"codex","model":"m","areas":[{"id":"d:src","name":"Source","summary":"summary"}]}"#).unwrap();
        let mut view = GraphView { scope: "project".into(), project: Some("/p".into()), grafly: grafly::Status { installed: false, version: None }, indexed: true, nodes: vec![node("d:src".into(), "src".into(), "area", Some("/p"), Some("src"), 1, None)], links: vec![], truncated: false, deep: None };
        apply_deep(&home, "/p", &mut view).unwrap();
        assert_eq!(serde_json::to_value(&view.deep).unwrap(), serde_json::json!({"at":"now","provider":"codex","model":"m"}));
        assert_eq!(view.nodes[0].summary.as_deref(), Some("summary"));
        std::fs::File::create(&path).unwrap().set_len(2 * 1024 * 1024 + 1).unwrap();
        assert!(apply_deep(&home, "/p", &mut view).unwrap_err().contains("exceeds 2 MiB"));
        std::fs::remove_dir_all(home).unwrap();
    }
}
