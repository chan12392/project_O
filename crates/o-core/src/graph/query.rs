use super::wiring::{Edge, Node, Wiring};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    In,
    Out,
}

#[derive(Debug, Serialize)]
pub struct Match {
    pub node: Node,
    pub depth: usize,
}

#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub nodes: Vec<Match>,
    pub edges: Vec<Edge>,
}

fn traverse(
    wiring: &Wiring,
    roots: &[&str],
    direction: Direction,
    depth: Option<usize>,
    calls_only: bool,
) -> QueryResult {
    let nodes: HashMap<&str, &Node> = wiring.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let mut adjacent: HashMap<&str, Vec<&Edge>> = HashMap::new();
    for edge in &wiring.edges {
        if edge.relation == "contains" || (calls_only && edge.relation != "calls") {
            continue;
        }
        let key = match direction {
            Direction::In => &edge.target,
            Direction::Out => &edge.source,
        };
        adjacent.entry(key).or_default().push(edge);
    }
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    for root in roots {
        if nodes.contains_key(root) && visited.insert(*root) {
            queue.push_back((*root, 0));
        }
    }
    let mut result = QueryResult {
        nodes: Vec::new(),
        edges: Vec::new(),
    };
    while let Some((id, level)) = queue.pop_front() {
        result.nodes.push(Match {
            node: nodes[id].clone(),
            depth: level,
        });
        if depth.is_some_and(|max| level >= max) {
            continue;
        }
        for edge in adjacent.get(id).into_iter().flatten() {
            let next = match direction {
                Direction::In => edge.source.as_str(),
                Direction::Out => edge.target.as_str(),
            };
            if !nodes.contains_key(next) {
                continue;
            }
            result.edges.push((*edge).clone());
            if visited.insert(next) {
                queue.push_back((next, level + 1));
            }
        }
    }
    result
}

/// Matches an exact node ID first, otherwise all nodes with the given name.
pub fn callers(
    wiring: &Wiring,
    symbol: &str,
    direction: Direction,
    depth: Option<usize>,
) -> Result<QueryResult, String> {
    let exact = wiring.nodes.iter().find(|n| n.id == symbol);
    let roots: Vec<&str> = match exact {
        Some(node) => vec![node.id.as_str()],
        None => wiring
            .nodes
            .iter()
            .filter(|n| n.name == symbol)
            .map(|n| n.id.as_str())
            .collect(),
    };
    if roots.is_empty() {
        return Err(format!("symbol not found: {symbol}"));
    }
    Ok(traverse(wiring, &roots, direction, depth, true))
}

/// Finds the incoming dependency paths of every symbol in the changed files.
pub fn blast(wiring: &Wiring, paths: &[String], depth: usize) -> QueryResult {
    let paths: HashSet<String> = paths.iter().map(|p| p.replace('\\', "/")).collect();
    let roots: Vec<&str> = wiring
        .nodes
        .iter()
        .filter(|n| paths.contains(&n.path.replace('\\', "/")))
        .map(|n| n.id.as_str())
        .collect();
    traverse(wiring, &roots, Direction::In, Some(depth.clamp(1, 8)), false)
}

/// Returns the indexed symbols in a file, including private symbols.
pub fn skeleton(wiring: &Wiring, path: &str) -> Vec<Node> {
    let path = path.replace('\\', "/");
    wiring
        .nodes
        .iter()
        .filter(|n| n.kind != "file" && n.path.replace('\\', "/") == path)
        .cloned()
        .collect()
}

#[derive(Debug, Serialize)]
pub struct GraphMap {
    pub files: usize,
    pub nodes: usize,
    pub edges: usize,
    pub kinds: BTreeMap<String, usize>,
    pub relations: BTreeMap<String, usize>,
    pub directories: BTreeMap<String, usize>,
}

pub fn map(wiring: &Wiring) -> GraphMap {
    let mut result = GraphMap {
        files: 0,
        nodes: wiring.nodes.len(),
        edges: wiring.edges.len(),
        kinds: BTreeMap::new(),
        relations: BTreeMap::new(),
        directories: BTreeMap::new(),
    };
    let mut files = HashSet::new();
    for node in &wiring.nodes {
        *result.kinds.entry(node.kind.clone()).or_default() += 1;
        if !node.path.is_empty() {
            files.insert(node.path.replace('\\', "/"));
        }
    }
    result.files = files.len();
    for file in files {
        let directory = file.rsplit_once('/').map_or(".", |(dir, _)| dir);
        *result.directories.entry(directory.to_string()).or_default() += 1;
    }
    for edge in &wiring.edges {
        *result.relations.entry(edge.relation.clone()).or_default() += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph() -> Wiring {
        serde_json::from_str(
            r##"{
          "nodes":[
            {"id":"a#f","name":"f","kind":"function","path":"a.rs","span":"L1-L3"},
            {"id":"b#f","name":"f","kind":"function","path":"b.rs"},
            {"id":"c#g","name":"g","kind":"function","path":"c.rs"},
            {"id":"d#h","name":"h","kind":"function","path":"sub/d.rs"}],
          "edges":[
            {"source":"c#g","target":"a#f","relation":"calls"},
            {"source":"a#f","target":"c#g","relation":"calls"},
            {"source":"d#h","target":"c#g","relation":"imports"},
            {"source":"b#f","target":"a#f","relation":"contains"},
            {"source":"missing","target":"a#f","relation":"calls"}]
        }"##,
        )
        .unwrap()
    }

    #[test]
    fn callers_handle_duplicates_cycles_direction_and_depth() {
        let graph = graph();
        let result = callers(&graph, "f", Direction::In, Some(1)).unwrap();
        assert_eq!(
            result
                .nodes
                .iter()
                .map(|m| (m.node.id.as_str(), m.depth))
                .collect::<Vec<_>>(),
            vec![("a#f", 0), ("b#f", 0), ("c#g", 1)]
        );
        assert_eq!(result.edges.len(), 1);
        let result = callers(&graph, "a#f", Direction::Out, None).unwrap();
        assert_eq!(result.nodes.len(), 2);
        assert_eq!(result.edges.len(), 2);
        assert!(callers(&graph, "absent", Direction::In, None).is_err());
    }

    #[test]
    fn blast_skeleton_and_map_cover_file_dependencies() {
        let graph = graph();
        let result = blast(&graph, &["a.rs".into()], 2);
        assert_eq!(
            result
                .nodes
                .iter()
                .map(|m| (m.node.id.as_str(), m.depth))
                .collect::<Vec<_>>(),
            vec![("a#f", 0), ("c#g", 1), ("d#h", 2)]
        );
        assert_eq!(skeleton(&graph, "sub\\d.rs")[0].id, "d#h");
        let overview = map(&graph);
        assert_eq!((overview.files, overview.nodes, overview.edges), (4, 4, 5));
        assert_eq!(overview.directories["sub"], 1);
        assert_eq!(overview.relations["calls"], 3);
    }

    #[test]
    fn blast_caps_depth_in_core() {
        let nodes: Vec<_> = (0..11).map(|i| serde_json::json!({"id": i.to_string(), "path": format!("{i}.rs")})).collect();
        let edges: Vec<_> = (1..11).map(|i| serde_json::json!({"source": i.to_string(), "target": (i - 1).to_string(), "relation": "calls"})).collect();
        let wiring: Wiring = serde_json::from_value(serde_json::json!({"nodes": nodes, "edges": edges})).unwrap();
        assert_eq!(blast(&wiring, &["0.rs".into()], usize::MAX).nodes.len(), 9);
        assert_eq!(blast(&wiring, &["0.rs".into()], 0).nodes.len(), 2);
    }
}
