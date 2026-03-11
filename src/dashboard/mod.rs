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
}

pub fn build_state(db_path: PathBuf) -> DashboardState {
    let discord_db_path = std::env::var("NUCLEUSDB_DISCORD_DB_PATH")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| db_path.clone());
    DashboardState {
        db_path,
        discord_db_path,
        db_lock: Arc::new(Mutex::new(())),
        crypto: Arc::new(StdMutex::new(CryptoSession::default())),
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
