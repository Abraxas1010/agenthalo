use crate::transparency::ct6962::hex_encode;
use crate::tui::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    if app.db.entries.is_empty() {
        let empty = Paragraph::new("No commits yet. Use Execute tab and COMMIT to create history.")
            .block(Block::default().borders(Borders::ALL).title("History"));
        frame.render_widget(empty, area);
        return;
    }

    let split = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(area);

    let mut entries = app.db.entries.iter().collect::<Vec<_>>();
    entries.reverse();
    let selected = app.history_index.min(entries.len().saturating_sub(1));
    let body_height = split[0].height.saturating_sub(4).max(1) as usize;
    let start = selected.saturating_sub(body_height.saturating_sub(1) / 2);
    let end = start.saturating_add(body_height).min(entries.len());

    let table_rows = entries[start..end]
        .iter()
        .enumerate()
        .map(|(offset, entry)| {
            let global_idx = start + offset;
            let root_hex = hex_encode(&entry.state_root);
            let short_root = format!("{}...", root_hex.chars().take(12).collect::<String>());
            let row = Row::new(vec![
                Cell::from(entry.height.to_string()),
                Cell::from(entry.sth.timestamp_unix_secs.to_string()),
                Cell::from(short_root),
                Cell::from(entry.delta_digest.to_string()),
            ]);
            if global_idx == selected {
                row.style(Style::default().fg(Color::Yellow))
            } else {
                row
            }
        })
        .collect::<Vec<_>>();

    let header = Row::new(vec!["Height", "Timestamp", "State Root", "Delta"]).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(
        table_rows,
        [
            Constraint::Length(8),
            Constraint::Length(14),
            Constraint::Length(18),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("History")
            .title_bottom(format!(
                "rows {}-{} of {}",
                start + 1,
                end.max(start + 1),
                entries.len()
            )),
    );
    frame.render_widget(table, split[0]);

    let selected_entry = entries[selected];
    let detail_lines = vec![
        format!("Height: {}", selected_entry.height),
        format!("Prev root: {}", hex_encode(&selected_entry.prev_state_root)),
        format!("State root: {}", hex_encode(&selected_entry.state_root)),
        format!("Tree size: {}", selected_entry.sth.tree_size),
        format!("Timestamp: {}", selected_entry.sth.timestamp_unix_secs),
        format!("Backend: {}", selected_entry.vc_backend_id),
        format!(
            "Witness alg: {}",
            selected_entry.witness_signature_algorithm
        ),
        format!("VC scheme: {}", selected_entry.vc_scheme_id),
        format!("VC max degree: {}", selected_entry.vc_max_degree),
    ];
    let details = Paragraph::new(detail_lines.join("\n"))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Entry Details"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(details, split[1]);
}
