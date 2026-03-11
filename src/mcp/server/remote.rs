use crate::mcp::tools::NucleusDbMcpService;
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

#[derive(Debug, Clone)]
pub struct RemoteServerConfig {
    pub db_path: String,
    pub listen_addr: SocketAddr,
    pub endpoint_path: String,
}

impl Default for RemoteServerConfig {
    fn default() -> Self {
        Self {
            db_path: "nucleusdb.ndb".to_string(),
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 3000)),
            endpoint_path: "/mcp".to_string(),
        }
    }
}

pub async fn run_remote_mcp_server(config: RemoteServerConfig) -> Result<(), String> {
    let db_path = config.db_path.clone();
    let mcp_service = StreamableHttpService::new(
        move || NucleusDbMcpService::new(&db_path).map_err(std::io::Error::other),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    let app = Router::new()
        .nest_service(&config.endpoint_path, mcp_service)
        .route(
            "/health",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({"ok": true, "service": "nucleusdb-mcp"}))
            }),
        )
        .layer(CorsLayer::permissive());

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .map_err(|e| format!("failed to bind {}: {e}", config.listen_addr))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}
