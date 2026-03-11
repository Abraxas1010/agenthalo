//! Standalone NucleusDB dashboard server.

pub mod api;
pub mod assets;

use axum::routing::get;
use axum::Router;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::Mutex;

#[derive(Clone, Default)]
pub struct CryptoSession {
    pub password_unlocked: bool,
    pub master_key: Option<[u8; 32]>,
}

#[derive(Clone)]
pub struct DashboardState {
    pub db_path: PathBuf,
    pub discord_db_path: PathBuf,
    pub db_lock: Arc<Mutex<()>>,
    pub crypto: Arc<StdMutex<CryptoSession>>,
    pub pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
    pub orchestrator: Arc<crate::orchestrator::Orchestrator>,
    pub mcp_service: Arc<crate::mcp::tools::NucleusDbMcpService>,
}

pub fn build_state(db_path: PathBuf) -> DashboardState {
    let discord_db_path = std::env::var("NUCLEUSDB_DISCORD_DB_PATH")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| db_path.clone());
    let vault = crate::halo::vault::Vault::open(
        &crate::halo::config::pq_wallet_path(),
        &crate::halo::config::vault_path(),
    )
    .ok()
    .map(Arc::new);
    let governor_registry = crate::halo::governor_registry::build_default_registry();
    crate::halo::governor_registry::install_global_registry(governor_registry.clone());
    let pty_manager = Arc::new(
        crate::cockpit::pty_manager::PtyManager::with_governor_registry(
            24,
            Some(governor_registry.clone()),
        ),
    );
    let orchestrator = Arc::new(crate::orchestrator::Orchestrator::new(
        pty_manager.clone(),
        vault.clone(),
        db_path.clone(),
    ));
    let mcp_service = Arc::new(
        crate::mcp::tools::NucleusDbMcpService::new_with_runtime(
            &db_path,
            vault,
            pty_manager.clone(),
            governor_registry,
            (*orchestrator).clone(),
        )
        .expect("dashboard-local MCP service should initialize"),
    );
    DashboardState {
        db_path,
        discord_db_path,
        db_lock: Arc::new(Mutex::new(())),
        crypto: Arc::new(StdMutex::new(CryptoSession::default())),
        pty_manager,
        orchestrator,
        mcp_service,
    }
}

pub fn build_router(state: DashboardState) -> Router {
    Router::new()
        .nest("/api", api::api_router(state.clone()))
        .fallback(get(assets::static_handler))
        .with_state(state)
}

pub async fn serve(port: u16, open_browser: bool) -> Result<(), String> {
    let state = build_state(crate::config::db_path());
    let app = build_router(state);
    let host = std::env::var("NUCLEUSDB_DASHBOARD_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let ip: IpAddr = host
        .parse()
        .map_err(|e| format!("invalid NUCLEUSDB_DASHBOARD_HOST `{host}`: {e}"))?;
    let addr = SocketAddr::new(ip, port);
    let url = if ip.is_unspecified() {
        format!("http://localhost:{port}")
    } else {
        format!("http://{host}:{port}")
    };
    println!("NucleusDB Dashboard\n  URL: {url}");
    if open_browser {
        let _ = webbrowser::open(&url);
    }
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind dashboard: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve dashboard: {e}"))
}
