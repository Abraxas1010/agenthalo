fn main() {
    let dry_run = std::env::args().any(|arg| arg == "--dry-run");
    let config = match nucleusdb::discord::config::DiscordConfig::from_env(dry_run) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    if let Err(e) = runtime.block_on(nucleusdb::discord::handler::run(config)) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
