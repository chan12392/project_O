//! Hierarchical graph data for the graph screen.
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
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

pub(crate) fn node(id: String, label: String, kind: &str, project: Option<&str>, path: Option<&str>, size: usize, glow: Option<&str>) -> GraphNode {
    GraphNode { id, label, kind: kind.into(), project: project.map(str::to_owned), path: path.map(str::to_owned), size: size.max(1), glow: glow.map(str::to_owned), summary: None, indexed: None }
}
pub(crate) fn link(source: &str, target: &str, kind: &str, weight: usize, glow: bool) -> GraphLink {
    GraphLink { source: source.into(), target: target.into(), kind: kind.into(), weight, glow: glow.then_some(true) }
}
pub(crate) fn name(path: &str) -> &str { path.trim_end_matches('/').rsplit('/').next().unwrap_or(path) }
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

type Stamp = Option<(SystemTime, u64)>;
type CacheKey = (PathBuf, Stamp, Stamp, String);
type OverallCache = VecDeque<(CacheKey, BTreeMap<String, usize>)>;
static OVERALL_CACHE: OnceLock<Mutex<OverallCache>> = OnceLock::new();

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

fn cache_key(project: &Path) -> CacheKey {
    let git = project.ancestors().map(|p| p.join(".git")).find(|p| p.exists());
    let git = git.map(|p| {
        if p.is_file() {
            let text = std::fs::read_to_string(&p).unwrap_or_default();
            p.parent().unwrap_or(project).join(text.trim().strip_prefix("gitdir: ").unwrap_or(""))
        } else { p }
    });
    let index = git.as_ref().and_then(|p| stamp(&p.join("index")));
    let head = git.as_ref().map(|p| {
        let head = std::fs::read_to_string(p.join("HEAD")).unwrap_or_default();
        let common = std::fs::read_to_string(p.join("commondir")).ok().map(|d| p.join(d.trim())).unwrap_or_else(|| p.clone());
        let value = head.trim().strip_prefix("ref: ").and_then(|r| std::fs::read_to_string(common.join(r)).ok()).unwrap_or_default();
        format!("{head}{value}{:?}", stamp(&common.join("packed-refs")))
    }).unwrap_or_default();
    (project.to_path_buf(), stamp(&project.join(".grafly/.graph/wiring.json")), index, head)
}

fn overall_files(project: &Path) -> Result<Option<BTreeMap<String, usize>>, String> {
    if std::fs::metadata(project.join(".grafly/.graph/wiring.json")).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(None);
    }
    let key = cache_key(project);
    if key.1.is_none() {
        return wiring::load(project).map(|w| w.map(|w| {
            let mut files = BTreeMap::new();
            for n in w.nodes { if !n.path.is_empty() { *files.entry(n.path).or_default() += usize::from(n.kind != "file"); } }
            files
        }));
    }
    let cache = OVERALL_CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    if let Some((_, files)) = cache.lock().unwrap().iter().find(|(k, _)| *k == key) { return Ok(Some(files.clone())); }
    let Some(wiring) = wiring::load(project)? else { return Ok(None) };
    let mut files = BTreeMap::new();
    for n in wiring.nodes { if !n.path.is_empty() { *files.entry(n.path).or_default() += usize::from(n.kind != "file"); } }
    let mut cache = cache.lock().unwrap();
    cache.retain(|(k, _)| k.0 != key.0);
    if cache.len() >= 64 { cache.pop_front(); }
    cache.push_back((key, files.clone()));
    Ok(Some(files))
}

fn make_overall_part(home: &Path, project: &str, label: &str, status: &grafly::Status) -> Result<GraphView, String> {
    let mut result = GraphView { scope: "overall".into(), project: None, grafly: status.clone(), indexed: false, nodes: Vec::new(), links: Vec::new(), truncated: false, deep: None };
    let files = overall_files(Path::new(project))?;
    result.indexed = files.is_some();
    let mut data = Data { files: files.unwrap_or_default(), edges: BTreeMap::new(), glow: HashMap::new() };
    if result.indexed {
        for path in git::changed(Path::new(project))? {
            data.files.entry(path.clone()).or_default();
            data.glow.insert(path, "active");
        }
    }
    let indexed = result.indexed;
    overall_project(&mut result, project, label, &data, indexed);
    result.links.retain(|l| l.source != "o:root");
    apply_deep(home, project, &mut result)?;
    Ok(result)
}

/// One checked project in the overall graph, without the overall root.
pub fn overall_part(home: &Path, project: &str) -> Result<GraphView, String> {
    super::check_project(home, project)?;
    let listing = crate::projects::list(home, &crate::run::store::user_home())?;
    let label = listing.projects.iter().find(|p| crate::projects::same(&p.path, project)).map(|p| p.name.as_str()).unwrap_or_else(|| name(project));
    make_overall_part(home, project, label, &grafly::status())
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
        let ctx = crate::run::store::policy_context(home, chrono::Utc::now()).map_err(|e| e.to_string())?;
        let projects: Vec<_> = listing.projects.into_iter().filter(|item| !item.missing && crate::policy::check_project(Some(&item.path), &ctx).is_empty()).collect();
        for chunk in projects.chunks(4) {
            let parts = std::thread::scope(|scope| {
                let handles: Vec<_> = chunk.iter().map(|item| {
                    let status = &result.grafly;
                    scope.spawn(move || make_overall_part(home, &item.path, &item.name, status))
                }).collect();
                handles.into_iter().map(|h| h.join().map_err(|_| "그래프 작업이 중단되었습니다".to_string())?).collect::<Result<Vec<_>, String>>()
            })?;
            for part in parts {
                result.indexed |= part.indexed;
                if let Some(project) = part.nodes.first() { result.links.push(link("o:root", &project.id, "contains", 1, project.glow.is_some())); }
                result.nodes.extend(part.nodes);
                result.links.extend(part.links);
            }
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
    fn overall_part_tracks_worktree_and_refreshes_wiring_cache() {
        let base = std::env::temp_dir().join(format!("o-graph-part-{}", crate::run::store::new_run_id(chrono::Utc::now())));
        let project = base.join("project");
        let home = base.join("state");
        std::fs::create_dir_all(project.join(".grafly/.graph")).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let git = |args: &[&str]| {
            let mut cmd = std::process::Command::new("git");
            cmd.arg("-C").arg(&project).args(args);
            assert!(crate::sys::proc::hide_window(&mut cmd).output().unwrap().status.success());
        };
        git(&["init", "-q"]);
        let wiring = project.join(".grafly/.graph/wiring.json");
        std::fs::write(&wiring, r#"{"nodes":[{"id":"a","path":"src/a.rs"}]}"#).unwrap();
        let path = project.to_string_lossy();
        let first = overall_part(&home, &path).unwrap();
        assert!(first.indexed);
        assert!(first.nodes.iter().any(|n| n.path.as_deref() == Some("src")));
        assert!(!first.nodes.iter().any(|n| n.id == "o:root"));
        assert!(!first.links.iter().any(|l| l.source == "o:root"));
        std::fs::create_dir_all(project.join("changed")).unwrap();
        std::fs::write(project.join("changed/new.rs"), "new").unwrap();
        let second = overall_part(&home, &path).unwrap();
        assert!(second.nodes.iter().any(|n| n.path.as_deref() == Some("changed") && n.glow.as_deref() == Some("active")));
        std::fs::write(&wiring, r#"{"nodes":[{"id":"b","path":"other/b.rs"},{"id":"c","path":"other/c.rs"}]}"#).unwrap();
        let third = overall_part(&home, &path).unwrap();
        assert!(third.nodes.iter().any(|n| n.path.as_deref() == Some("other")));
        assert!(!third.nodes.iter().any(|n| n.path.as_deref() == Some("src")));
        std::fs::remove_file(wiring).unwrap();
        let empty = overall_part(&home, &path).unwrap();
        assert!(!empty.indexed);
        assert_eq!(empty.nodes.len(), 1);
        assert!(empty.nodes[0].glow.is_none());
        std::fs::remove_dir_all(base).unwrap();
    }
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

    /// Keep this fixture and expected IDs in sync with graphLoad.test.ts: the frontend
    /// caps a merged raw graph using the same order and glow exceptions as this path.
    #[test]
    fn cap_overall_matches_frontend_contract() {
        let nodes = [
            ("o:root", "root", None, None),
            ("a:orphan", "area", Some("/missing"), None),
            ("p:/cold", "project", Some("/cold"), None),
            ("a:/cold|plain", "area", Some("/cold"), None),
            ("a:/hot|active", "area", Some("/hot"), Some("active")),
            ("a:/hot|path", "area", Some("/hot"), Some("path")),
            ("p:/hot", "project", Some("/hot"), None),
            ("a:/hot|plain", "area", Some("/hot"), None),
            ("p:/last", "project", Some("/last"), None),
        ];
        let edges = [
            ("o:root", "p:/cold", "contains"),
            ("o:root", "p:/hot", "contains"),
            ("o:root", "p:/last", "contains"),
            ("p:/cold", "a:/cold|plain", "contains"),
            ("p:/hot", "a:/hot|active", "contains"),
            ("p:/hot", "a:/hot|path", "contains"),
            ("p:/hot", "a:/hot|plain", "contains"),
            ("p:/missing", "a:orphan", "contains"),
            ("a:/cold|plain", "a:/hot|active", "calls"),
        ];
        let original = GraphView {
            scope: "overall".into(), project: None,
            grafly: grafly::Status { installed: true, version: None }, indexed: true,
            nodes: nodes.iter().map(|(id, kind, project, glow)| node((*id).into(), (*id).into(), kind, *project, None, 1, *glow)).collect(),
            links: edges.iter().map(|(source, target, kind)| link(source, target, kind, 1, false)).collect(),
            truncated: false, deep: None,
        };
        let cases: &[(usize, &[&str])] = &[
            (2, &["o:root", "a:/hot|active", "a:/hot|path", "p:/hot"]),
            (5, &["o:root", "p:/cold", "a:/hot|active", "a:/hot|path", "p:/hot"]),
            (6, &["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot"]),
            (7, &["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot", "a:/hot|plain"]),
            (8, &["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot", "a:/hot|plain", "p:/last"]),
            (100, &["o:root", "p:/cold", "a:/cold|plain", "a:/hot|active", "a:/hot|path", "p:/hot", "a:/hot|plain", "p:/last"]),
        ];
        for (max, expected) in cases {
            let mut capped = original.clone();
            cap_overall(&mut capped, *max);
            assert_eq!(capped.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), *expected, "max={max}");
            assert!(capped.truncated, "max={max}");
            let expected_links: Vec<_> = edges.iter().filter(|(source, target, _)| expected.contains(source) && expected.contains(target)).copied().collect();
            assert_eq!(capped.links.iter().map(|l| (l.source.as_str(), l.target.as_str(), l.kind.as_str())).collect::<Vec<_>>(), expected_links, "max={max}");
        }
        let mut already_truncated = original;
        already_truncated.nodes.truncate(1);
        already_truncated.links.clear();
        already_truncated.truncated = true;
        cap_overall(&mut already_truncated, 100);
        assert!(already_truncated.truncated);
        assert_eq!(already_truncated.nodes.len(), 1);
    }

    #[test]
    fn cap_overall_limits_eight_thirteen_node_parts_to_one_hundred() {
        let mut view = GraphView { scope: "overall".into(), project: None, grafly: grafly::Status { installed: true, version: None }, indexed: true, nodes: vec![node("o:root".into(), "root".into(), "root", None, None, 1, None)], links: vec![], truncated: false, deep: None };
        for project in 0..8 {
            let project = format!("/p/{project}");
            let data = Data {
                files: (0..12).map(|area| (format!("area{area:02}/file.rs"), 1)).collect(),
                edges: BTreeMap::new(), glow: HashMap::new(),
            };
            overall_project(&mut view, &project, &project, &data, true);
        }
        assert_eq!(view.nodes.len(), 105);
        let expected: Vec<_> = view.nodes.iter().take(100).map(|n| n.id.clone()).collect();
        cap_overall(&mut view, 100);
        assert_eq!(view.nodes.iter().map(|n| n.id.clone()).collect::<Vec<_>>(), expected);
        assert!(view.truncated);
        let ids: HashSet<_> = view.nodes.iter().map(|n| n.id.as_str()).collect();
        assert!(view.links.iter().all(|l| ids.contains(l.source.as_str()) && ids.contains(l.target.as_str())));
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
