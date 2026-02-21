use nucleusdb::tui::app::run_tui;

fn main() {
    let db_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "nucleusdb.ndb".to_string());
    if let Err(e) = run_tui(&db_path) {
        eprintln!("TUI error: {e}");
        std::process::exit(1);
    }
}
