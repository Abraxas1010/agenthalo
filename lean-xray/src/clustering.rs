use crate::types::{ClusterInfo, FileReport};
use std::collections::HashMap;

/// Cluster files by their top-level module path prefix.
/// e.g., "HeytingLean.PTS.BaseExtension.Main" clusters under "HeytingLean.PTS"
pub fn cluster_files(files: &[FileReport], depth: usize) -> Vec<ClusterInfo> {
    let mut clusters: HashMap<String, Vec<&FileReport>> = HashMap::new();

    for file in files {
        let module = file.path.replace('/', ".").replace('\\', ".").trim_end_matches(".lean").to_string();
        let parts: Vec<&str> = module.split('.').collect();
        let prefix = if parts.len() > depth {
            parts[..depth].join(".")
        } else {
            module.clone()
        };
        clusters.entry(prefix).or_default().push(file);
    }

    let mut result: Vec<ClusterInfo> = clusters
        .into_iter()
        .map(|(name, files)| {
            let total_lines: usize = files.iter().map(|f| f.lines).sum();
            let total_sorrys: usize = files.iter().map(|f| f.sorry_count).sum();
            let health_score = if total_lines == 0 {
                1.0
            } else {
                files
                    .iter()
                    .map(|f| f.health.score * f.lines.max(1) as f64)
                    .sum::<f64>()
                    / total_lines as f64
            };
            ClusterInfo {
                name,
                files: files.iter().map(|f| f.path.clone()).collect(),
                total_lines,
                total_sorrys,
                health_score,
            }
        })
        .collect();

    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}
