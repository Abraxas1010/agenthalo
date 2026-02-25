//! Web dashboard for AgentHALO — embedded in the single binary.
//!
//! Serves a localhost web UI at `http://localhost:3100` (configurable)
//! using axum + rust-embed. All web assets are compiled into the binary.

pub mod api;
pub mod assets;

use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;


/// Shared state for all dashboard API handlers.
#[derive(Clone)]
pub struct DashboardState {
    pub db_path: std::path::PathBuf,
    pub credentials_path: std::path::PathBuf,
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
