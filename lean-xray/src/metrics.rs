use crate::types::{FileReport, HealthMetrics, HealthStatus};

/// Compute health metrics for a file report.
pub fn compute_health(report: &mut FileReport) {
    let sorry_ratio = if report.decl_count > 0 {
        report.sorry_count as f64 / report.decl_count as f64
    } else if report.sorry_count > 0 {
        1.0
    } else {
        0.0
    };

    let decl_lengths: Vec<usize> = report
        .declarations
        .iter()
        .map(|d| d.line_end.saturating_sub(d.line_start).max(1))
        .collect();

    let avg_decl_length = if decl_lengths.is_empty() {
        0.0
    } else {
        decl_lengths.iter().sum::<usize>() as f64 / decl_lengths.len() as f64
    };
    let max_decl_length = decl_lengths.iter().copied().max().unwrap_or(0);

    // Health score: 1.0 = perfect, 0.0 = worst
    // Penalize: sorry ratio (heavy), very large declarations, low declaration density
    let sorry_penalty = sorry_ratio * 0.6;
    let size_penalty = if max_decl_length > 200 { 0.1 } else { 0.0 };
    let score = (1.0 - sorry_penalty - size_penalty).max(0.0).min(1.0);

    let status = if score > 0.8 {
        HealthStatus::Clean
    } else if score > 0.4 {
        HealthStatus::Warning
    } else {
        HealthStatus::Critical
    };

    report.health = HealthMetrics {
        score,
        sorry_ratio,
        avg_decl_length,
        max_decl_length,
        has_sorry: report.sorry_count > 0,
        status,
    };
}

/// Compute project-wide health score from file reports.
pub fn project_health_score(files: &[FileReport]) -> f64 {
    if files.is_empty() {
        return 1.0;
    }
    // Weighted average by line count
    let total_lines: usize = files.iter().map(|f| f.lines.max(1)).sum();
    let weighted_sum: f64 = files
        .iter()
        .map(|f| f.health.score * f.lines.max(1) as f64)
        .sum();
    weighted_sum / total_lines as f64
}
