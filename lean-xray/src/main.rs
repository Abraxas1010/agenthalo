mod clustering;
mod dependency;
mod global;
mod metrics;
mod parser;
mod scanner;
mod types;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "lean-xray", about = "Fast Lean 4 project scanner")]
struct Cli {
    /// Project directory to scan
    #[arg(long, default_value = ".")]
    dir: PathBuf,

    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let report = global::scan_project(&cli.dir)?;

    if cli.json || cli.pretty {
        let output = if cli.pretty {
            serde_json::to_string_pretty(&report)?
        } else {
            serde_json::to_string(&report)?
        };
        println!("{output}");
    } else {
        println!("lean-xray scan: {}", report.dir);
        println!("  Files:   {}", report.total_files);
        println!("  Lines:   {}", report.total_lines);
        println!("  Decls:   {}", report.total_decls);
        println!("  Sorrys:  {}", report.total_sorrys);
        println!("  Health:  {:.2}", report.health_score);
        println!("  Time:    {}ms", report.scan_time_ms);
        println!("  Clusters: {}", report.clusters.len());

        if report.total_sorrys > 0 {
            println!("\n  Files with sorrys:");
            let mut sorry_files: Vec<_> = report
                .files
                .iter()
                .filter(|f| f.sorry_count > 0)
                .collect();
            sorry_files.sort_by(|a, b| b.sorry_count.cmp(&a.sorry_count));
            for f in sorry_files.iter().take(20) {
                println!("    {} ({} sorrys, health {:.2})", f.path, f.sorry_count, f.health.score);
            }
        }
    }

    Ok(())
}
