use clap::Parser;
use nucleusdb::api::serve_multitenant;
use nucleusdb::cli::repl::{execute_sql_text, run_repl};
use nucleusdb::cli::{default_witness_cfg, parse_backend, print_table, Cli, Commands};
use nucleusdb::license::{load_and_verify, verification_report, LicenseLevel, ProFeature};
use nucleusdb::mcp::server::run_mcp_server;
use nucleusdb::multitenant::MultiTenantPolicy;
use nucleusdb::persistence::{default_wal_path, init_wal};
use nucleusdb::protocol::NucleusDb;
use nucleusdb::sql::executor::SqlResult;
use nucleusdb::state::State;
use nucleusdb::tui::app::run_tui;
use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

/// Load the license level from the `--license` flag, falling back to Community.
fn resolve_license(license_path: Option<&str>) -> LicenseLevel {
    match license_path {
        Some(p) => {
            let path = PathBuf::from(p);
            match load_and_verify(&path) {
                Ok(level) => {
                    if let LicenseLevel::Pro { licensee, .. } = &level {
                        eprintln!("License: Pro (licensee: {licensee})");
                    }
                    level
                }
                Err(e) => {
                    eprintln!("License warning: {e} — falling back to Community edition");
                    LicenseLevel::Community
                }
            }
        }
        None => LicenseLevel::Community,
    }
}

/// Gate a Pro feature — returns Err with a user-friendly message if not licensed.
fn require_pro(license: &LicenseLevel, feature: &ProFeature, action: &str) -> Result<(), String> {
    if license.has(feature) {
        return Ok(());
    }
    Err(format!(
        "{action} requires a Pro license with the '{}' feature.\n\
         Obtain a license at https://www.agentpmt.com/agentaddress\n\
         Then pass --license <path-to-certificate.json>",
        feature.as_leaf_str()
    ))
}

fn run(cli: Cli) -> Result<(), String> {
    let license = resolve_license(cli.license.as_deref());

    match cli.command {
        Commands::Create { db, backend, wal } => {
            cmd_create(&db, &backend, wal.as_deref(), &license)
        }
        Commands::Open { db } => cmd_open(&db),
        Commands::Server { addr, policy } => {
            require_pro(&license, &ProFeature::MultiTenant, "HTTP API server")?;
            cmd_server(&addr, &policy)
        }
        Commands::Tui { db } => {
            require_pro(&license, &ProFeature::Tui, "Terminal UI")?;
            cmd_tui(&db)
        }
        Commands::Mcp { db } => {
            require_pro(&license, &ProFeature::McpServer, "MCP server")?;
            cmd_mcp(&db)
        }
        Commands::Sql { db, file } => cmd_sql(&db, file.as_deref()),
        Commands::Status { db } => cmd_status(&db),
        Commands::Export { db } => cmd_export(&db),
        Commands::License { cert } => cmd_license(&cert),
    }
}

fn cmd_create(
    db_path: &str,
    backend: &str,
    wal_path: Option<&str>,
    license: &LicenseLevel,
) -> Result<(), String> {
    let backend = parse_backend_gated(backend, license)?;
    let cfg = default_witness_cfg();
    let db = NucleusDb::new(State::new(vec![]), backend, cfg);
    let db_path = PathBuf::from(db_path);
    db.save_persistent(&db_path)
        .map_err(|e| format!("failed to save snapshot {}: {e:?}", db_path.display()))?;

    let wal = wal_path
        .map(PathBuf::from)
        .unwrap_or_else(|| default_wal_path(&db_path));
    init_wal(&wal, &db)
        .map_err(|e| format!("failed to initialize WAL {}: {e:?}", wal.display()))?;

    println!("Created database: {}", db_path.display());
    println!("Initialized WAL: {}", wal.display());
    Ok(())
}

/// Backend parsing with license gate for IPA and KZG.
fn parse_backend_gated(
    backend: &str,
    license: &LicenseLevel,
) -> Result<nucleusdb::protocol::VcBackend, String> {
    let b = parse_backend(backend)?;
    match &b {
        nucleusdb::protocol::VcBackend::Ipa => {
            require_pro(license, &ProFeature::IpaBackend, "IPA backend")?;
        }
        nucleusdb::protocol::VcBackend::Kzg => {
            require_pro(license, &ProFeature::KzgBackend, "KZG backend")?;
        }
        nucleusdb::protocol::VcBackend::BinaryMerkle => {} // always available
    }
    Ok(b)
}

fn cmd_open(db_path: &str) -> Result<(), String> {
    let db_path = PathBuf::from(db_path);
    if !db_path.exists() {
        return Err(format!(
            "database file does not exist: {}",
            db_path.display()
        ));
    }
    let cfg = default_witness_cfg();
    let mut db = NucleusDb::load_persistent(&db_path, cfg)
        .map_err(|e| format!("failed to load snapshot {}: {e:?}", db_path.display()))?;
    run_repl(&mut db, &db_path).map_err(|e| format!("REPL failed: {e}"))?;
    Ok(())
}

fn cmd_server(addr: &str, policy: &str) -> Result<(), String> {
    let addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("invalid socket address '{addr}': {e}"))?;
    let policy = match policy.trim().to_ascii_lowercase().as_str() {
        "permissive" => MultiTenantPolicy::permissive(),
        "production" => MultiTenantPolicy::production(),
        other => {
            return Err(format!(
                "invalid policy profile '{other}', expected production|permissive"
            ));
        }
    };
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    rt.block_on(serve_multitenant(addr, policy))
        .map_err(|e| format!("server failed: {e}"))
}

fn cmd_mcp(db_path: &str) -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    rt.block_on(run_mcp_server(db_path))
}

fn cmd_tui(db_path: &str) -> Result<(), String> {
    run_tui(db_path).map_err(|e| format!("TUI failed: {e}"))
}

fn cmd_sql(db_path: &str, file: Option<&str>) -> Result<(), String> {
    let db_path = PathBuf::from(db_path);
    let cfg = default_witness_cfg();
    let mut db = if db_path.exists() {
        NucleusDb::load_persistent(&db_path, cfg)
            .map_err(|e| format!("failed to load snapshot {}: {e:?}", db_path.display()))?
    } else {
        NucleusDb::new(State::new(vec![]), parse_backend("merkle")?, cfg)
    };

    let sql_text = if let Some(path) = file {
        std::fs::read_to_string(path).map_err(|e| format!("failed to read SQL file {path}: {e}"))?
    } else {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("failed to read stdin: {e}"))?;
        buf
    };
    execute_sql_text(&mut db, &db_path, &sql_text)
        .map_err(|e| format!("SQL execution failed: {e}"))?;
    Ok(())
}

fn cmd_status(db_path: &str) -> Result<(), String> {
    let db_path = PathBuf::from(db_path);
    let cfg = default_witness_cfg();
    let mut db = NucleusDb::load_persistent(&db_path, cfg)
        .map_err(|e| format!("failed to load snapshot {}: {e:?}", db_path.display()))?;
    let mut exec = nucleusdb::sql::executor::SqlExecutor::new(&mut db);
    render_sql_result(exec.execute("SHOW STATUS;"));
    Ok(())
}

fn cmd_export(db_path: &str) -> Result<(), String> {
    let db_path = PathBuf::from(db_path);
    let cfg = default_witness_cfg();
    let mut db = NucleusDb::load_persistent(&db_path, cfg)
        .map_err(|e| format!("failed to load snapshot {}: {e:?}", db_path.display()))?;
    let mut exec = nucleusdb::sql::executor::SqlExecutor::new(&mut db);
    render_sql_result(exec.execute("EXPORT;"));
    Ok(())
}

fn cmd_license(cert_path: &str) -> Result<(), String> {
    let path = PathBuf::from(cert_path);
    if !path.exists() {
        return Err(format!("certificate file not found: {}", path.display()));
    }
    let raw =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read certificate: {e}"))?;
    let cert: nucleusdb::license::LicenseCertificate =
        serde_json::from_str(&raw).map_err(|e| format!("failed to parse certificate JSON: {e}"))?;
    println!("{}", verification_report(&cert));
    Ok(())
}

fn render_sql_result(out: SqlResult) {
    match out {
        SqlResult::Rows { columns, rows } => print_table(&columns, &rows),
        SqlResult::Ok { message } => println!("{message}"),
        SqlResult::Error { message } => eprintln!("Error: {message}"),
    }
}
