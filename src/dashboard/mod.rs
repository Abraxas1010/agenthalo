//! Web dashboard for AgentHALO — embedded in the single binary.
//!
//! Serves a localhost web UI at `http://localhost:3100` (configurable)
//! using axum + rust-embed. All web assets are compiled into the binary.

pub mod api;
pub mod assets;

use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Shared state for all dashboard API handlers.
///
/// The `db_lock` serializes all database access. Redb uses file-level
/// exclusive locking — concurrent opens from parallel HTTP requests
/// cause "Database already open" errors. This mutex ensures at most
/// one handler accesses the trace store at a time.
#[derive(Clone)]
pub struct DashboardState {
    pub db_path: std::path::PathBuf,
    pub credentials_path: std::path::PathBuf,
    pub db_lock: Arc<Mutex<()>>,
}

/// Build the full axum Router with embedded assets + API routes.
pub fn build_router(state: DashboardState) -> Router {
    let api_router = api::api_router(state.clone());

    Router::new()
        .nest("/api", api_router)
        .route("/events", get(api::sse_handler))
        .fallback(get(assets::static_handler))
        .with_state(state)
}

/// Start the dashboard server and optionally open the browser.
pub async fn serve(port: u16, open_browser: bool) -> Result<(), String> {
    let state = DashboardState {
        db_path: crate::halo::config::db_path(),
        credentials_path: crate::halo::config::credentials_path(),
        db_lock: Arc::new(Mutex::new(())),
    };

    let app = build_router(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let url = format!("http://localhost:{port}");

    println!("Agent H.A.L.O. Dashboard");
    println!("  URL: {url}");
    println!("  Press Ctrl+C to stop");
    println!();

    if open_browser {
        let _ = webbrowser::open(&url);
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve dashboard: {e}"))
}
