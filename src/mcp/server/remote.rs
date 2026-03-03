//! Remote MCP server over Streamable HTTP transport.
//!
//! Exposes the full NucleusDB MCP tool surface at a single `/mcp` endpoint
//! using the MCP Streamable HTTP specification (2025-03-26).
//!
//! Supports dual authentication: CAB-as-bearer-token and OAuth 2.1 JWT.

use crate::mcp::server::auth::AuthConfig;
use crate::mcp::tools::NucleusDbMcpService;
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

/// Configuration for the remote MCP HTTP server.
#[derive(Debug, Clone)]
pub struct RemoteServerConfig {
    /// Database path for NucleusDB state.
    pub db_path: String,
    /// Listen address (e.g., "0.0.0.0:8443" or "127.0.0.1:3000").
    pub listen_addr: SocketAddr,
    /// Authentication configuration.
    pub auth: AuthConfig,
    /// MCP endpoint path (default: "/mcp").
    pub endpoint_path: String,
}

impl Default for RemoteServerConfig {
    fn default() -> Self {
        Self {
            db_path: "nucleusdb.ndb".to_string(),
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 3000)),
            auth: AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        }
    }
}

/// Run the NucleusDB MCP server over Streamable HTTP.
pub async fn run_remote_mcp_server(config: RemoteServerConfig) -> Result<(), String> {
    let db_path = config.db_path.clone();
    let auth_config = Arc::new(config.auth.clone());

    // StreamableHttpService takes a factory closure that creates a fresh service
    // per MCP session. Each session gets its own NucleusDbMcpService sharing
    // the same DB path (state is file-backed, not in-memory per session).
    let mcp_service = StreamableHttpService::new(
        move || NucleusDbMcpService::new(&db_path).map_err(std::io::Error::other),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    // CORS: restrict to localhost and configured origins. In production,
    // set NUCLEUSDB_CORS_ORIGINS env var to a comma-separated list of allowed origins.
    let cors = {
        let origins_env = std::env::var("NUCLEUSDB_CORS_ORIGINS").unwrap_or_default();
        if origins_env.is_empty() {
            // Default: localhost only.
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:3000".parse().unwrap(),
                    "http://127.0.0.1:3000".parse().unwrap(),
                ])
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::ACCEPT,
                ])
        } else {
            let origins: Vec<axum::http::HeaderValue> = origins_env
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::ACCEPT,
                ])
        }
    };

    // Rate limiting: concurrency limit per server (default 64 concurrent requests).
    let max_concurrent: usize = std::env::var("NUCLEUSDB_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);

    let app = Router::new()
        .nest_service(&config.endpoint_path, mcp_service)
        .layer(axum::middleware::from_fn_with_state(
            auth_config.clone(),
            super::auth::auth_middleware,
        ))
        .layer(tower::limit::ConcurrencyLimitLayer::new(max_concurrent))
        .layer(cors)
        .route("/health", axum::routing::get(health_handler))
        .route(
            "/auth/info",
            axum::routing::get(move || {
                let ac = auth_config.clone();
                async move { auth_info_handler(ac).await }
            }),
        );

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .map_err(|e| format!("failed to bind {}: {e}", config.listen_addr))?;

    eprintln!(
        "NucleusDB MCP server listening on http://{}{}",
        config.listen_addr, config.endpoint_path
    );
    eprintln!(
        "  Auth: {}",
        if config.auth.enabled {
            "enabled (CAB + OAuth)"
        } else {
            "disabled (dev mode)"
        }
    );
    eprintln!("  Health: http://{}/health", config.listen_addr);

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "server": "nucleusdb-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "transport": "streamable-http",
        "protocol": "mcp/2025-03-26"
    }))
}

async fn auth_info_handler(config: Arc<AuthConfig>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "auth_enabled": config.enabled,
        "methods": {
            "cab": {
                "description": "CAB-as-bearer-token: Hardware-anchored agent identity",
                "header": "Authorization: Bearer cab:<base64(json)>",
                "payload_fields": ["agent_address", "contract_address", "rpc_url"],
                "tier_scopes": {
                    "1_consumer": ["read", "trust:verify"],
                    "2_server": ["read", "trust:verify"],
                    "3_server_tpm": ["read", "trust:verify", "write"],
                    "4_dgx": ["read", "trust:verify", "write", "trust:attest", "container"],
                }
            },
            "oauth": {
                "description": "OAuth 2.1 JWT: Standard bearer token",
                "header": "Authorization: Bearer <jwt>",
                "algorithm": "HS256",
                "required_claims": ["sub"],
                "scope_claim": "scope (space-separated)",
                "available_scopes": ["read", "trust:verify", "write", "trust:attest", "container"],
            }
        },
        "tool_scopes": {
            "read": [
                "nucleusdb_help", "nucleusdb_status", "nucleusdb_query",
                "nucleusdb_query_range", "nucleusdb_verify", "nucleusdb_export",
                "nucleusdb_history", "abraxas_query_records", "abraxas_record_status",
                "abraxas_merge_status", "abraxas_workspace_diff", "mesh_peers", "mesh_ping"
            ],
            "trust:verify": [
                "nucleusdb_verify_agent", "verify_agent_multichain", "register_chain"
            ],
            "write": [
                "nucleusdb_execute_sql", "nucleusdb_create_database",
                "nucleusdb_open_database", "nucleusdb_checkpoint",
                "abraxas_submit_record", "abraxas_resolve_conflict",
                "abraxas_export_git", "abraxas_workspace_init", "abraxas_workspace_submit",
                "mesh_call", "mesh_exchange_envelope"
            ],
            "trust:attest": [
                "nucleusdb_agent_register", "submit_composite_attestation", "mesh_grant"
            ],
            "container": ["nucleusdb_container_launch"]
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_sane() {
        let config = RemoteServerConfig::default();
        assert_eq!(config.endpoint_path, "/mcp");
        assert!(config.auth.enabled);
        assert_eq!(config.listen_addr.port(), 3000);
    }
}
