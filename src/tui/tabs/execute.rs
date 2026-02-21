use crate::tui::app::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

pub fn render(app: &App, frame: &mut Frame, area: Rect) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);

    let input = Paragraph::new(app.sql_input.as_str())
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("SQL Input")
                .title_bottom("Enter: execute | Esc: clear"),
        )
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(input, sections[0]);

    let output_text = if app.sql_output.is_empty() {
        "(no output)".to_string()
    } else {
        app.sql_output.join("\n")
    };
    let output = Paragraph::new(output_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Output")
                .title_bottom("Results"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(output, sections[1]);

    let max_cursor = sections[0].width.saturating_sub(2);
    let cursor_col = app.sql_input.chars().count().min(max_cursor as usize) as u16;
    frame.set_cursor_position((sections[0].x + 1 + cursor_col, sections[0].y + 1));
}
