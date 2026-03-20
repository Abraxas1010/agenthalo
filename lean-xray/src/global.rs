use crate::clustering::cluster_files;
use crate::dependency::{build_dependency_graph, path_to_module};
use crate::metrics::{compute_health, project_health_score};
use crate::parser::{count_sorrys, extract_declarations, extract_imports, strip_comments};
use crate::scanner::scan_lean_files;
use crate::types::{FileReport, HealthMetrics, HealthStatus, ProjectReport};
use anyhow::Result;
use std::path::Path;
use std::time::Instant;

/// Perform a full project scan and return the aggregate report.
pub fn scan_project(dir: &Path) -> Result<ProjectReport> {
    let start = Instant::now();
    let files = scan_lean_files(dir)?;

    let mut file_reports = Vec::with_capacity(files.len());
    let mut file_imports: Vec<(String, Vec<String>)> = Vec::new();

    for file_path in &files {
        let rel = file_path
            .strip_prefix(dir)
            .unwrap_or(file_path)
            .to_string_lossy()
            .to_string();

        let src = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines = src.lines().count();
        let stripped = strip_comments(&src);
        let sorry_count = count_sorrys(&stripped);
        let imports = extract_imports(&src);
        let declarations = extract_declarations(&stripped);
        let decl_count = declarations.len();

        let module_name = path_to_module(&rel);
        file_imports.push((module_name, imports.clone()));

        let mut report = FileReport {
            path: rel,
            lines,
            decl_count,
            sorry_count,
            import_count: imports.len(),
            health: HealthMetrics {
                score: 1.0,
                sorry_ratio: 0.0,
                avg_decl_length: 0.0,
                max_decl_length: 0,
                has_sorry: false,
                status: HealthStatus::Clean,
            },
            declarations,
            imports,
        };

        compute_health(&mut report);
        file_reports.push(report);
    }

    let dependency_graph = build_dependency_graph(&file_imports);
    let clusters = cluster_files(&file_reports, 2);
    let health_score = project_health_score(&file_reports);

    let total_files = file_reports.len();
    let total_lines: usize = file_reports.iter().map(|f| f.lines).sum();
    let total_sorrys: usize = file_reports.iter().map(|f| f.sorry_count).sum();
    let total_decls: usize = file_reports.iter().map(|f| f.decl_count).sum();
    let scan_time_ms = start.elapsed().as_millis() as u64;

    Ok(ProjectReport {
        dir: dir.to_string_lossy().to_string(),
        scan_time_ms,
        total_files,
        total_lines,
        total_sorrys,
        total_decls,
        health_score,
        files: file_reports,
        dependency_graph,
        clusters,
    })
}
