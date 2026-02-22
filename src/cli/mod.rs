pub mod repl;

use crate::protocol::VcBackend;
use crate::witness::{WitnessConfig, WitnessSignatureAlgorithm};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "nucleusdb",
    version,
    about = "Verifiable database with vector commitments"
)]
pub struct Cli {
    /// Path to CAB license certificate (enables Pro features).
    #[arg(long, global = true)]
    pub license: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Create a new database
    Create {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
        #[arg(
            long,
            default_value = "merkle",
            help = "VC backend: merkle|binary_merkle|binary-merkle|ipa|kzg"
        )]
        backend: String,
        #[arg(long)]
        wal: Option<String>,
    },
    /// Open database and enter REPL
    Open {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
    },
    /// Start HTTP API server
    Server {
        #[arg(long, default_value = "127.0.0.1:8088")]
        addr: String,
        #[arg(
            long,
            default_value = "permissive",
            help = "Policy profile: permissive|production"
        )]
        policy: String,
    },
    /// Start terminal UI
    Tui {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
    },
    /// Start MCP server (stdio transport)
    Mcp {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
    },
    /// Execute SQL from file or stdin
    Sql {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
        /// SQL file to execute (reads from stdin if not specified)
        file: Option<String>,
    },
    /// Show database status
    Status {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
    },
    /// Export state as JSON
    Export {
        #[arg(long, default_value = "nucleusdb.ndb")]
        db: String,
    },
    /// Verify a CAB license certificate
    License {
        /// Path to the certificate JSON file
        cert: String,
    },
}

pub fn parse_backend(backend: &str) -> Result<VcBackend, String> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "ipa" => Ok(VcBackend::Ipa),
        "kzg" => Ok(VcBackend::Kzg),
        "binary_merkle" | "binary-merkle" | "merkle" => Ok(VcBackend::BinaryMerkle),
        other => Err(format!(
            "invalid backend '{other}', expected one of: merkle|binary_merkle|binary-merkle|ipa|kzg"
        )),
    }
}

pub fn default_witness_cfg() -> WitnessConfig {
    let seed = std::env::var("NUCLEUSDB_WITNESS_SEED")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "nucleusdb-cli-default-seed-v1".to_string());
    let mut cfg = WitnessConfig::with_seed(2, vec!["w1".into(), "w2".into(), "w3".into()], &seed);
    cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
    cfg
}

pub fn print_table(columns: &[String], rows: &[Vec<String>]) {
    if columns.is_empty() {
        return;
    }
    let mut widths: Vec<usize> = columns.iter().map(|c| c.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let sep = widths
        .iter()
        .map(|w| "-".repeat(*w + 2))
        .collect::<Vec<_>>()
        .join("+");

    let header = columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!(" {:width$} ", c, width = widths[i]))
        .collect::<Vec<_>>()
        .join("|");
    println!("{header}");
    println!("{sep}");
    for row in rows {
        let line = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let cell = row.get(i).cloned().unwrap_or_default();
                format!(" {:width$} ", cell, width = *w)
            })
            .collect::<Vec<_>>()
            .join("|");
        println!("{line}");
    }
}
