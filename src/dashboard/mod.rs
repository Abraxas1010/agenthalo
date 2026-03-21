//! Web dashboard for AgentHALO — embedded in the single binary.
//!
//! Serves a localhost web UI at `http://localhost:3100` (configurable)
//! using axum + rust-embed. All web assets are compiled into the binary.

pub mod api;
pub mod assets;
pub mod codeguard_api;
pub mod editor_api;
pub mod explorer_api;
pub mod gate_check;
pub mod forge_api;
pub mod gates_api;
pub mod mcp_bridge;

use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::SystemTime;
use tokio::sync::Mutex;
use tokio::time::Duration;

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
    pub oauth_state_secret: Arc<String>,
    pub oauth_issued_states: Arc<StdMutex<HashMap<String, u64>>>,
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
    /// Unified cryptographic lock/session state for password + scoped unlocks.
    pub crypto_state: Arc<StdMutex<CryptoState>>,
    /// Proxy resale configuration (markup, rate limits).
    pub proxy_config: crate::halo::pricing::ProxyConfig,
    /// Pricing table for cost calculation.
    pub pricing_table: std::collections::HashMap<String, crate::halo::pricing::ModelPricing>,
    /// Shared memory store/embedding runtime for memory recall APIs.
    pub memory_store: Arc<crate::memory::MemoryStore>,
    /// Cached NucleusDB snapshot for memory endpoints to avoid per-request
    /// deserialize cost. Refreshed on-disk fingerprint changes.
    pub memory_db_cache: Arc<StdMutex<MemoryDbCache>>,
    /// Local fallback orchestrator when MCP-proxy mode is disabled.
    pub orchestrator: Option<Arc<crate::orchestrator::Orchestrator>>,
    /// In-process MCP service for self-container lifecycle and dashboard-local calls.
    pub mcp_service: Arc<crate::mcp::tools::NucleusDbMcpService>,
    /// Shared AETHER governor registry across proxy/comms/compute/pty lanes.
    pub governor_registry: Arc<crate::halo::governor_registry::GovernorRegistry>,
    /// Live proxy admission/runtime telemetry wrapper.
    pub proxy_governor: Arc<crate::halo::proxy::ProxyGovernorRuntime>,
    /// First-run password/bootstrap behavior for this dashboard process.
    pub bootstrap_mode: DashboardBootstrapMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DashboardBootstrapMode {
    Required,
    Optional,
    Disabled,
}

impl DashboardBootstrapMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Optional => "optional",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbFileFingerprint {
    pub modified: SystemTime,
    pub len: u64,
}

#[derive(Debug, Default)]
pub struct MemoryDbCache {
    pub db: Option<crate::protocol::NucleusDb>,
    pub file_fingerprint: Option<DbFileFingerprint>,
}

#[derive(Debug, Default)]
pub struct WdkUnlockState {
    pub failed_attempts: u32,
    pub locked_until_unix: u64,
}

#[derive(Debug)]
pub struct CryptoState {
    pub session: crate::halo::session_manager::SessionManager,
    pub migration_status: crate::halo::migration::MigrationStatus,
}

impl CryptoState {
    pub fn new() -> Self {
        Self {
            session: crate::halo::session_manager::SessionManager::new(),
            migration_status: crate::halo::migration::detect_migration_status(),
        }
    }
}

impl Default for CryptoState {
    fn default() -> Self {
        Self::new()
    }
}

fn default_grant_store_path(db_path: &Path) -> PathBuf {
    db_path.with_extension("pod_grants.json")
}

fn random_hex_secret() -> String {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .expect("OS entropy source unavailable for dashboard OAuth secret");
    crate::halo::util::hex_encode(&bytes)
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

fn bootstrap_mode_from_env() -> DashboardBootstrapMode {
    match std::env::var("AGENTHALO_DASHBOARD_BOOTSTRAP_MODE")
        .unwrap_or_else(|_| "disabled".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "required" => DashboardBootstrapMode::Required,
        "optional" => DashboardBootstrapMode::Optional,
        _ => DashboardBootstrapMode::Disabled,
    }
}

pub fn build_state_with_bootstrap(
    db_path: PathBuf,
    credentials_path: PathBuf,
    bootstrap_mode: DashboardBootstrapMode,
) -> DashboardState {
    let grant_store = crate::pod::acl::GrantStore::shared();
    let grant_store_path = default_grant_store_path(&db_path);
    if let Err(e) = load_grants_into_store(&grant_store, &grant_store_path) {
        eprintln!("warning: failed to load POD grants: {e}");
    }

    let vault = {
        let wallet_path = crate::halo::config::pq_wallet_path();
        if wallet_path.exists() {
            match crate::halo::vault::Vault::open(
                &wallet_path,
                &crate::halo::config::vault_path(),
            ) {
                Ok(v) => Some(Arc::new(v)),
                Err(e) => {
                    eprintln!("warning: failed to initialize vault: {e}");
                    None
                }
            }
        } else {
            // No PQ wallet yet — vault unavailable until first `agenthalo setup`.
            None
        }
    };

    let key_store = Arc::new(crate::halo::api_keys::CustomerKeyStore::open(
        crate::halo::api_keys::CustomerKeyStore::default_path(),
    ));
    let proxy_config = crate::halo::pricing::load_proxy_config();
    let pricing_table = crate::halo::pricing::default_pricing();
    let governor_registry = crate::halo::governor_registry::build_default_registry();
    crate::halo::governor_registry::install_global_registry(governor_registry.clone());
    let proxy_governor = Arc::new(crate::halo::proxy::ProxyGovernorRuntime::new(
        governor_registry.clone(),
    ));

    let pty_manager = Arc::new(
        crate::cockpit::pty_manager::PtyManager::with_governor_registry(
            10,
            Some(governor_registry.clone()),
        ),
    );
    let orchestrator = if crate::halo::orchestrator_proxy::orchestrator_proxy_enabled() {
        None
    } else {
        // Use a separate trace DB for orchestrator tasks to avoid redb
        // file-level lock contention with the dashboard's main traces.ndb.
        let orch_trace_db = db_path.with_file_name("orch_traces.ndb");
        Some(Arc::new(crate::orchestrator::Orchestrator::new(
            pty_manager.clone(),
            vault.clone(),
            orch_trace_db,
        )))
    };
    let mcp_service = Arc::new(
        if let Some(shared_orchestrator) = orchestrator.as_ref() {
            crate::mcp::tools::NucleusDbMcpService::new_with_runtime(
                &db_path,
                vault.clone(),
                pty_manager.clone(),
                governor_registry.clone(),
                (**shared_orchestrator).clone(),
            )
        } else {
            crate::mcp::tools::NucleusDbMcpService::new(&db_path)
        }
        .expect("dashboard-local MCP service should initialize"),
    );
    DashboardState {
        db_path,
        credentials_path,
        oauth_state_secret: Arc::new(random_hex_secret()),
        oauth_issued_states: Arc::new(StdMutex::new(HashMap::new())),
        db_lock: Arc::new(Mutex::new(())),
        grant_store,
        grant_store_path,
        vault,
        pty_manager,
        key_store,
        wdk_manager: Arc::new(StdMutex::new(crate::halo::wdk_proxy::WdkManager::new())),
        wdk_unlock_state: Arc::new(StdMutex::new(WdkUnlockState::default())),
        crypto_state: Arc::new(StdMutex::new(CryptoState::new())),
        proxy_config,
        pricing_table,
        memory_store: Arc::new(crate::memory::MemoryStore::default()),
        memory_db_cache: Arc::new(StdMutex::new(MemoryDbCache::default())),
        orchestrator,
        mcp_service,
        governor_registry,
        proxy_governor,
        bootstrap_mode,
    }
}

pub fn build_state(db_path: PathBuf, credentials_path: PathBuf) -> DashboardState {
    build_state_with_bootstrap(db_path, credentials_path, bootstrap_mode_from_env())
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
    serve_with_bootstrap(port, open_browser, bootstrap_mode_from_env()).await
}

pub async fn serve_with_bootstrap(
    port: u16,
    open_browser: bool,
    bootstrap_mode: DashboardBootstrapMode,
) -> Result<(), String> {
    let state = build_state_with_bootstrap(
        crate::halo::config::db_path(),
        crate::halo::config::credentials_path(),
        bootstrap_mode,
    );

    // Reap expired scoped keys in the background.
    let reaper_state = state.crypto_state.clone();
    let maintenance_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Ok(mut guard) = reaper_state.lock() {
                guard.session.reap_expired();
            }
            maintenance_state
                .pty_manager
                .soft_reset_quiescent_governors();
            if let Err(error) = maintenance_state
                .proxy_governor
                .soft_reset_if_quiescent(Duration::from_secs(30))
            {
                eprintln!("warning: proxy governor quiescent reset failed: {error}");
            }
            crate::halo::governor_telemetry::soft_reset_comms_if_quiescent(Duration::from_secs(30));
        }
    });

    let app = build_router(state);
    let host = std::env::var("AGENTHALO_DASHBOARD_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let ip: IpAddr = host
        .parse()
        .map_err(|e| format!("invalid AGENTHALO_DASHBOARD_HOST `{host}`: {e}"))?;
    let addr = SocketAddr::new(ip, port);
    let url = if ip.is_unspecified() {
        format!("http://localhost:{port}")
    } else {
        format!("http://{host}:{port}")
    };

    // Ensure Claude/Codex/Gemini have global MCP configs for auto-discovery.
    crate::container::agent_hookup::ensure_global_agent_mcp_configs();

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
    let mesh_registered = if crate::container::mesh_enabled() {
        match crate::container::register_self_in_mesh() {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[mesh] dashboard registration failed: {e}");
                false
            }
        }
    } else {
        false
    };
    let serve_result = axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve dashboard: {e}"));
    if mesh_registered {
        crate::container::deregister_self_from_mesh();
    }
    serve_result
}
