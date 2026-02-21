use nucleusdb::mcp::server::run_mcp_server;

#[tokio::main]
async fn main() {
    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "nucleusdb.ndb".to_string());
    if let Err(e) = run_mcp_server(&db_path).await {
        eprintln!("MCP server error: {e}");
        std::process::exit(1);
    }
}
