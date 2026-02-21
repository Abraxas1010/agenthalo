use crate::transparency::ct6962::hex_encode;
use crate::tui::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let backend = format!("{:?}", app.db.backend);
    let mut lines = vec![
        format!("Database: {}", app.db_file_name()),
        format!("Backend: {backend}"),
        format!("State vector size: {}", app.db.state.values.len()),
        format!("Registered keys: {}", app.db.keymap.len()),
        format!("Commit entries: {}", app.db.entries.len()),
        format!(
            "Witness algorithm (active): {}",
            app.db.witness_cfg.signing_algorithm.as_tag()
        ),
    ];

    if let Some(sth) = app.db.current_sth() {
        lines.push(format!("STH tree size: {}", sth.tree_size));
        lines.push(format!("STH root: {}", hex_encode(&sth.root_hash)));
        lines.push(format!("STH timestamp: {}", sth.timestamp_unix_secs));
        lines.push(format!(
            "STH signature (truncated): {}...",
            sth.sig.chars().take(24).collect::<String>()
        ));
    } else {
        lines.push("STH: not initialized (no commits yet)".to_string());
    }

    lines.push("Lean formal specs: included (18 modules)".to_string());

    let paragraph = Paragraph::new(lines.join("\n"))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Status")
                .title_bottom("Core system snapshot"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
