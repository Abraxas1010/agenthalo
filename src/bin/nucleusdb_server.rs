use nucleusdb::api::serve_multitenant;
use nucleusdb::multitenant::MultiTenantPolicy;
use std::net::SocketAddr;

#[tokio::main]
async fn main() {
    let addr_arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8088".to_string());
    let profile_arg = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "production".to_string());

    let addr: SocketAddr = match addr_arg.parse() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("invalid socket address '{}': {e}", addr_arg);
            std::process::exit(2);
        }
    };
    let policy = match profile_arg.trim().to_ascii_lowercase().as_str() {
        "permissive" => MultiTenantPolicy::permissive(),
        "production" => MultiTenantPolicy::production(),
        other => {
            eprintln!(
                "invalid policy profile '{}', expected production|permissive",
                other
            );
            std::process::exit(2);
        }
    };

    if let Err(e) = serve_multitenant(addr, policy).await {
        eprintln!("nucleusdb_server failed: {e}");
        std::process::exit(1);
    }
}
