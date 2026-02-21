use crate::tui::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let rows = app.indexed_key_rows();
    if rows.is_empty() {
        let empty = Paragraph::new("No keys registered yet. Use Execute tab to INSERT and COMMIT.")
            .block(Block::default().borders(Borders::ALL).title("Browse"));
        frame.render_widget(empty, area);
        return;
    }

    let selected = app.browse_index.min(rows.len().saturating_sub(1));
    let body_height = area.height.saturating_sub(4).max(1) as usize;
    let start = selected.saturating_sub(body_height.saturating_sub(1) / 2);
    let end = start.saturating_add(body_height).min(rows.len());

    let table_rows = rows[start..end]
        .iter()
        .enumerate()
        .map(|(offset, (key, value, idx))| {
            let global_idx = start + offset;
            let row = Row::new(vec![
                Cell::from(key.clone()),
                Cell::from(value.to_string()),
                Cell::from(idx.to_string()),
            ]);
            if global_idx == selected {
                row.style(Style::default().fg(Color::Yellow))
            } else {
                row
            }
        })
        .collect::<Vec<_>>();

    let header = Row::new(vec!["Key", "Value", "Index"]).style(
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );

    let table = Table::new(
        table_rows,
        [
            Constraint::Percentage(55),
            Constraint::Percentage(25),
            Constraint::Percentage(20),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title("Browse")
            .title_bottom(format!(
                "rows {}-{} of {}",
                start + 1,
                end.max(start + 1),
                rows.len()
            )),
    );

    frame.render_widget(table, area);
}
