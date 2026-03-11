use nucleusdb::mcp::server::remote::{run_remote_mcp_server, RemoteServerConfig};
use nucleusdb::mcp::server::run_mcp_server;
use std::net::SocketAddr;

fn print_usage() {
    eprintln!("Usage: nucleusdb-mcp [OPTIONS] [DB_PATH]\n\nTransport modes:\n  --transport stdio\n  --transport http --host 127.0.0.1 --port 3000");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio");
    runtime.block_on(async_main(args));
}

async fn async_main(args: Vec<String>) {
    let transport = find_flag_value(&args, "--transport").unwrap_or_else(|| "stdio".to_string());
    let db_path = find_positional(&args).unwrap_or_else(|| "nucleusdb.ndb".to_string());
    let result = if transport == "http" {
        let port: u16 = find_flag_value(&args, "--port")
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let host = find_flag_value(&args, "--host").unwrap_or_else(|| "127.0.0.1".to_string());
        let listen_addr: SocketAddr = format!("{host}:{port}").parse().unwrap();
        run_remote_mcp_server(RemoteServerConfig {
            db_path,
            listen_addr,
            auth: nucleusdb::mcp::server::auth::AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        })
        .await
    } else {
        run_mcp_server(&db_path).await
    };
    if let Err(e) = result {
        eprintln!("MCP server error: {e}");
        std::process::exit(1);
    }
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}
fn find_positional(args: &[String]) -> Option<String> {
    let mut i = 1;
    while i < args.len() {
        if args[i].starts_with("--") {
            if matches!(args[i].as_str(), "--transport" | "--port" | "--host") {
                i += 2;
            } else {
                i += 1;
            }
        } else {
            return Some(args[i].clone());
        }
    }
    None
}
