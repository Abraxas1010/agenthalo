use crate::mcp::tools::NucleusDbMcpService;
use rmcp::{transport::stdio, ServiceExt};

pub mod auth;
pub mod remote;

pub async fn run_mcp_server(db_path: &str) -> Result<(), String> {
    let server = NucleusDbMcpService::new(db_path)?;
    let service = server
        .serve(stdio())
        .await
        .map_err(|e| format!("failed to start MCP service: {e}"))?;
    service
        .waiting()
        .await
        .map_err(|e| format!("MCP service terminated with error: {e}"))?;
    Ok(())
}
