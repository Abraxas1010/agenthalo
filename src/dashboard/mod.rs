//! Web dashboard for AgentHALO — embedded in the single binary.
//!
//! Serves a localhost web UI at `http://localhost:3100` (configurable)
//! using axum + rust-embed. All web assets are compiled into the binary.

pub mod api;
pub mod assets;

use axum::routing::get;
use axum::Router;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
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
    pub grant_store: crate::pod::acl::SharedGrantStore,
    pub grant_store_path: PathBuf,
    pub vault: Option<Arc<crate::halo::vault::Vault>>,
    pub pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
    /// Customer API key store for metered proxy.
    pub key_store: Arc<crate::halo::api_keys::CustomerKeyStore>,
    /// WDK sidecar lifecycle + encrypted seed proxy.
    pub wdk_manager: Arc<StdMutex<crate::halo::wdk_proxy::WdkManager>>,
    /// Unlock throttling state for WDK passphrase attempts.
    pub wdk_unlock_state: Arc<StdMutex<WdkUnlockState>>,
    /// Proxy resale configuration (markup, rate limits).
    pub proxy_config: crate::halo::pricing::ProxyConfig,
    /// Pricing table for cost calculation.
    pub pricing_table: std::collections::HashMap<String, crate::halo::pricing::ModelPricing>,
}

#[derive(Debug, Default)]
pub struct WdkUnlockState {
    pub failed_attempts: u32,
    pub locked_until_unix: u64,
}

fn default_grant_store_path(db_path: &Path) -> PathBuf {
    db_path.with_extension("pod_grants.json")
}

fn load_grants_into_store(
    store: &crate::pod::acl::SharedGrantStore,
    path: &Path,
) -> Result<usize, String> {
    if !path.exists() {
        return Ok(0);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("read grants {}: {e}", path.display()))?;
    let grants: Vec<crate::pod::acl::AccessGrant> = serde_json::from_slice(&bytes)
        .map_err(|e| format!("parse grants {}: {e}", path.display()))?;
    let count = grants.len();
    let mut guard = store
        .write()
        .map_err(|e| format!("grant store write lock poisoned: {e}"))?;
    guard.replace_all(grants);
    Ok(count)
}

pub fn build_state(db_path: PathBuf, credentials_path: PathBuf) -> DashboardState {
    let grant_store = crate::pod::acl::GrantStore::shared();
    let grant_store_path = default_grant_store_path(&db_path);
    if let Err(e) = load_grants_into_store(&grant_store, &grant_store_path) {
        eprintln!("warning: failed to load POD grants: {e}");
    }

    let vault = if crate::halo::config::pq_wallet_path().exists() {
        match crate::halo::vault::Vault::open(
            &crate::halo::config::pq_wallet_path(),
            &crate::halo::config::vault_path(),
        ) {
            Ok(v) => Some(Arc::new(v)),
            Err(e) => {
                eprintln!("warning: failed to initialize vault: {e}");
                None
            }
        }
    } else {
        None
    };

    let key_store = Arc::new(crate::halo::api_keys::CustomerKeyStore::open(
        crate::halo::api_keys::CustomerKeyStore::default_path(),
    ));
    let proxy_config = crate::halo::pricing::load_proxy_config();
    let pricing_table = crate::halo::pricing::default_pricing();

    DashboardState {
        db_path,
        credentials_path,
        db_lock: Arc::new(Mutex::new(())),
        grant_store,
        grant_store_path,
        vault,
        pty_manager: Arc::new(crate::cockpit::pty_manager::PtyManager::new(10)),
        key_store,
        wdk_manager: Arc::new(StdMutex::new(crate::halo::wdk_proxy::WdkManager::new())),
        wdk_unlock_state: Arc::new(StdMutex::new(WdkUnlockState::default())),
        proxy_config,
        pricing_table,
    }
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
    let state = build_state(
        crate::halo::config::db_path(),
        crate::halo::config::credentials_path(),
    );

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
