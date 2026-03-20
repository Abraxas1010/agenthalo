use crate::types::DependencyGraph;
use std::collections::{HashMap, HashSet};

/// Build a dependency graph from file paths and their import lists.
/// Maps module names (e.g., "HeytingLean.Core") to the list of modules they import.
pub fn build_dependency_graph(
    file_imports: &[(String, Vec<String>)],
) -> DependencyGraph {
    let known_modules: HashSet<&str> = file_imports.iter().map(|(m, _)| m.as_str()).collect();
    let mut edges = Vec::new();

    for (module, imports) in file_imports {
        for imp in imports {
            // Only include edges to modules we know about (within the project)
            if known_modules.contains(imp.as_str()) {
                edges.push((module.clone(), imp.clone()));
            }
        }
    }

    let nodes: Vec<String> = file_imports.iter().map(|(m, _)| m.clone()).collect();
    DependencyGraph { nodes, edges }
}

/// Compute reverse dependencies: for each module, which modules import it.
pub fn reverse_deps(graph: &DependencyGraph) -> HashMap<String, Vec<String>> {
    let mut rev: HashMap<String, Vec<String>> = HashMap::new();
    for node in &graph.nodes {
        rev.entry(node.clone()).or_default();
    }
    for (from, to) in &graph.edges {
        rev.entry(to.clone()).or_default().push(from.clone());
    }
    rev
}

/// Convert a file path relative to project root into a Lean module name.
/// e.g., "HeytingLean/PTS/BaseExtension/Main.lean" → "HeytingLean.PTS.BaseExtension.Main"
pub fn path_to_module(rel_path: &str) -> String {
    rel_path
        .trim_end_matches(".lean")
        .replace('/', ".")
        .replace('\\', ".")
}
