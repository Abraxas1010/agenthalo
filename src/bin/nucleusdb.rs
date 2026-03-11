use clap::Parser;
use nucleusdb::api::serve_multitenant;
use nucleusdb::cli::repl::{execute_sql_text, run_repl};
use nucleusdb::cli::{default_witness_cfg, parse_backend, print_table, Cli, Commands};
use nucleusdb::mcp::server::remote::{run_remote_mcp_server, RemoteServerConfig};
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

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Commands::Create { db, backend, wal } => cmd_create(&db, &backend, wal.as_deref()),
        Commands::Open { db } => cmd_open(&db),
        Commands::Server { addr, policy } => cmd_server(&addr, &policy),
        Commands::Tui { db } => cmd_tui(&db),
        Commands::Mcp {
            db,
            transport,
            host,
            port,
        } => cmd_mcp(&db, &transport, &host, port),
        Commands::Dashboard { port, no_open } => cmd_dashboard(port, !no_open),
        Commands::Sql { db, file } => cmd_sql(&db, file.as_deref()),
        Commands::Status { db } => cmd_status(&db),
        Commands::Export { db } => cmd_export(&db),
        Commands::VerifyCertificate { path } => cmd_verify_certificate(&path),
        Commands::SignCertificate { path } => cmd_sign_certificate(&path),
        Commands::SubmitCertificate { path } => cmd_submit_certificate(&path),
    }
}

fn cmd_create(db_path: &str, backend: &str, wal_path: Option<&str>) -> Result<(), String> {
    let backend = parse_backend(backend)?;
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
    println!(
        "Created database: {}\nInitialized WAL: {}",
        db_path.display(),
        wal.display()
    );
    Ok(())
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
    run_repl(&mut db, &db_path).map_err(|e| format!("REPL failed: {e}"))
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
            ))
        }
    };
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    rt.block_on(serve_multitenant(addr, policy))
        .map_err(|e| format!("server failed: {e}"))
}

fn cmd_mcp(db_path: &str, transport: &str, host: &str, port: u16) -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    if transport == "http" {
        let listen_addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .map_err(|e| format!("invalid listen addr: {e}"))?;
        rt.block_on(run_remote_mcp_server(RemoteServerConfig {
            db_path: db_path.to_string(),
            listen_addr,
            auth: nucleusdb::mcp::server::auth::AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        }))
    } else {
        rt.block_on(run_mcp_server(db_path))
    }
}

fn cmd_dashboard(port: u16, open_browser: bool) -> Result<(), String> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| format!("failed to start tokio runtime: {e}"))?;
    rt.block_on(nucleusdb::dashboard::serve(port, open_browser))
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
    let summary = execute_sql_text(&mut db, &db_path, &sql_text)
        .map_err(|e| format!("SQL execution failed: {e}"))?;
    if summary.pending_writes > 0 {
        eprintln!(
            "WARNING: {} pending write(s) were not committed.",
            summary.pending_writes
        );
    }
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

fn cmd_verify_certificate(path: &str) -> Result<(), String> {
    let result = nucleusdb::verifier::gate::verify_certificate(PathBuf::from(path).as_path())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&result)
            .map_err(|e| format!("serialize verification result: {e}"))?
    );
    Ok(())
}

fn cmd_sign_certificate(path: &str) -> Result<(), String> {
    nucleusdb::verifier::gate::sign_certificate(PathBuf::from(path).as_path())?;
    Ok(())
}

fn cmd_submit_certificate(path: &str) -> Result<(), String> {
    let dest = nucleusdb::verifier::gate::submit_certificate(PathBuf::from(path).as_path())?;
    println!("{}", dest.display());
    Ok(())
}

fn render_sql_result(out: SqlResult) {
    match out {
        SqlResult::Rows { columns, rows } => print_table(&columns, &rows),
        SqlResult::Ok { message } => println!("{message}"),
        SqlResult::Error { message } => eprintln!("Error: {message}"),
    }
}
