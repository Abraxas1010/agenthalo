use nucleusdb::container::{deregister_self_from_mesh, mesh_enabled, register_self_in_mesh};
use nucleusdb::mcp::server::auth::AuthConfig;
use nucleusdb::mcp::server::remote::{run_remote_mcp_server, RemoteServerConfig};
use nucleusdb::mcp::server::run_mcp_server;
use std::net::SocketAddr;

fn print_usage() {
    eprintln!(
        "Usage: nucleusdb-mcp [OPTIONS] [DB_PATH]

NucleusDB MCP server — exposes verifiable database tools via Model Context Protocol.

Transport modes:
  --transport stdio     (default) Standard I/O for local MCP clients
  --transport http      Streamable HTTP for remote MCP clients

HTTP options (only with --transport http):
  --port PORT           Listen port (default: 3000)
  --host HOST           Listen host (default: 127.0.0.1)
  --no-auth             Disable authentication (dev mode only!)
  --jwt-secret SECRET   Shared secret for OAuth JWT validation (HS256)
  --trusted-rpc URL     Add a trusted RPC URL for CAB verification (repeatable)

Examples:
  nucleusdb-mcp                                  # stdio, default DB
  nucleusdb-mcp /tmp/app.ndb                     # stdio, custom DB path
  nucleusdb-mcp --transport http                  # HTTP on 127.0.0.1:3000, auth enabled
  nucleusdb-mcp --transport http --no-auth        # HTTP, auth disabled (dev only)
  nucleusdb-mcp --transport http --host 0.0.0.0 --port 8443 --jwt-secret mysecret --trusted-rpc https://mainnet.base.org"
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    // ToolRouter with 25+ tools creates deep generic nesting in rmcp;
    // the default 2MB tokio worker stack overflows during MCP message
    // serialization. Build runtime with 8MB worker stacks.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .thread_stack_size(8 * 1024 * 1024)
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(async_main(args));
}

async fn async_main(args: Vec<String>) {
    let transport = find_flag_value(&args, "--transport").unwrap_or_else(|| "stdio".to_string());
    let db_path = find_positional(&args).unwrap_or_else(|| "nucleusdb.ndb".to_string());

    match transport.as_str() {
        "stdio" => {
            if let Err(e) = run_mcp_server(&db_path).await {
                eprintln!("MCP server error: {e}");
                std::process::exit(1);
            }
        }
        "http" => {
            let port: u16 = find_flag_value(&args, "--port")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000);
            let host = find_flag_value(&args, "--host").unwrap_or_else(|| "127.0.0.1".to_string());
            let auth_disabled = args.iter().any(|a| a == "--no-auth");
            let jwt_secret = find_flag_value(&args, "--jwt-secret")
                .or_else(|| std::env::var("NUCLEUSDB_JWT_SECRET").ok())
                .unwrap_or_default();
            let trusted_rpc_urls = collect_flag_values(&args, "--trusted-rpc");

            let listen_addr: SocketAddr = format!("{host}:{port}").parse().unwrap_or_else(|e| {
                eprintln!("invalid listen address {host}:{port}: {e}");
                std::process::exit(1);
            });

            if auth_disabled {
                eprintln!(
                    "WARNING: authentication disabled (--no-auth). Do NOT use in production."
                );
            }

            let config = RemoteServerConfig {
                db_path,
                listen_addr,
                auth: AuthConfig {
                    enabled: !auth_disabled,
                    jwt_secret,
                    trusted_rpc_urls,
                    ..Default::default()
                },
                endpoint_path: "/mcp".to_string(),
            };

            let mesh_registered = if mesh_enabled() {
                match register_self_in_mesh() {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("[mesh] registration failed: {e}");
                        false
                    }
                }
            } else {
                false
            };
            let run_result = run_remote_mcp_server(config).await;
            if mesh_registered {
                deregister_self_from_mesh();
            }
            if let Err(e) = run_result {
                eprintln!("Remote MCP server error: {e}");
                std::process::exit(1);
            }
        }
        other => {
            eprintln!("unknown transport: {other} (expected: stdio, http)");
            print_usage();
            std::process::exit(1);
        }
    }
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn find_positional(args: &[String]) -> Option<String> {
    // Skip binary name, skip all --flag and --flag value pairs.
    let mut i = 1;
    while i < args.len() {
        if args[i].starts_with("--") {
            // Flags that take a value.
            if matches!(
                args[i].as_str(),
                "--transport" | "--port" | "--host" | "--jwt-secret" | "--trusted-rpc"
            ) {
                i += 2;
            } else {
                // Boolean flags like --no-auth.
                i += 1;
            }
        } else {
            return Some(args[i].clone());
        }
    }
    None
}

fn collect_flag_values(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == flag {
            if let Some(v) = args.get(i + 1) {
                values.push(v.clone());
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    values
}
