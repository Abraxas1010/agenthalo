use nucleusdb::api::serve_multitenant;
use nucleusdb::multitenant::MultiTenantPolicy;
use std::net::SocketAddr;

fn print_usage() {
    println!(
        "Usage:\n  nucleusdb-server [ADDR] [POLICY]\n\nArguments:\n  ADDR    Socket address (default: 127.0.0.1:8088)\n  POLICY  permissive|production (default: production)\n\nOptions:\n  -h, --help   Show this help"
    );
}

#[tokio::main]
async fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }
    if args.len() > 2 {
        eprintln!("too many arguments: expected at most 2 (ADDR, POLICY)");
        print_usage();
        std::process::exit(2);
    }

    let addr_arg = args
        .first()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:8088".to_string());
    let profile_arg = args
        .get(1)
        .cloned()
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
