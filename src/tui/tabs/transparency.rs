use crate::transparency::ct6962::hex_encode;
use crate::tui::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(7), Constraint::Min(1)])
        .split(area);

    let sth_lines = if let Some(sth) = app.db.current_sth() {
        vec![
            format!("Tree size: {}", sth.tree_size),
            format!("Root hash: {}", hex_encode(&sth.root_hash)),
            format!("Timestamp: {}", sth.timestamp_unix_secs),
            format!(
                "Signature: {}...",
                sth.sig.chars().take(40).collect::<String>()
            ),
        ]
    } else {
        vec![
            "No Signed Tree Head yet (no commits).".to_string(),
            "After first COMMIT, transparency metadata appears here.".to_string(),
        ]
    };

    let sth_widget = Paragraph::new(sth_lines.join("\n"))
        .block(Block::default().borders(Borders::ALL).title("STH"))
        .wrap(Wrap { trim: false });
    frame.render_widget(sth_widget, split[0]);

    if app.db.ct_leaves.is_empty() {
        let empty = Paragraph::new("No leaves in transparency tree.")
            .block(Block::default().borders(Borders::ALL).title("Leaves"));
        frame.render_widget(empty, split[1]);
        return;
    }

    let selected = app
        .transparency_index
        .min(app.db.ct_leaves.len().saturating_sub(1));
    let visible = split[1].height.saturating_sub(2).max(1) as usize;
    let start = selected.saturating_sub(visible.saturating_sub(1) / 2);
    let end = start.saturating_add(visible).min(app.db.ct_leaves.len());

    let items = app.db.ct_leaves[start..end]
        .iter()
        .enumerate()
        .map(|(offset, leaf)| {
            let global_idx = start + offset;
            let prefix = if global_idx == selected { "> " } else { "  " };
            let line = format!("{prefix}#{global_idx}: {}", hex_encode(leaf));
            ListItem::new(line).style(if global_idx == selected {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            })
        })
        .collect::<Vec<_>>();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title("Leaves")
            .title_bottom(format!(
                "rows {}-{} of {}",
                start + 1,
                end.max(start + 1),
                app.db.ct_leaves.len()
            )),
    );
    frame.render_widget(list, split[1]);
}
