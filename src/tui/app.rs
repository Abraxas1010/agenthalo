use crate::cli::default_witness_cfg;
use crate::persistence::{default_wal_path, persist_snapshot_and_sync_wal};
use crate::protocol::{NucleusDb, VcBackend};
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::tui::tabs;
use chrono::{TimeZone, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Status,
    Browse,
    Execute,
    History,
    Transparency,
}

impl Tab {
    fn all() -> [Tab; 5] {
        [
            Tab::Status,
            Tab::Browse,
            Tab::Execute,
            Tab::History,
            Tab::Transparency,
        ]
    }

    fn title(self) -> &'static str {
        match self {
            Tab::Status => "Status",
            Tab::Browse => "Browse",
            Tab::Execute => "Execute",
            Tab::History => "History",
            Tab::Transparency => "Transparency",
        }
    }
}

pub struct App {
    pub(crate) db: NucleusDb,
    pub(crate) db_path: PathBuf,
    pub(crate) current_tab_idx: usize,
    pub(crate) sql_input: String,
    pub(crate) sql_output: Vec<String>,
    pub(crate) sql_history: Vec<String>,
    pub(crate) sql_history_cursor: Option<usize>,
    pub(crate) browse_index: usize,
    pub(crate) history_index: usize,
    pub(crate) transparency_index: usize,
    should_quit: bool,
}

impl App {
    pub fn load(db_path: &str) -> io::Result<Self> {
        let db_path = PathBuf::from(db_path);
        let cfg = default_witness_cfg();
        let db = if db_path.exists() {
            NucleusDb::load_persistent(&db_path, cfg)
                .map_err(|e| io::Error::other(format!("failed to load snapshot: {e:?}")))?
        } else {
            let db = NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, cfg);
            db.save_persistent(&db_path)
                .map_err(|e| io::Error::other(format!("failed to save snapshot: {e:?}")))?;
            db
        };

        Ok(Self {
            db,
            db_path,
            current_tab_idx: 0,
            sql_input: String::new(),
            sql_output: vec!["Ready. Type SQL in Execute tab and press Enter.".to_string()],
            sql_history: Vec::new(),
            sql_history_cursor: None,
            browse_index: 0,
            history_index: 0,
            transparency_index: 0,
            should_quit: false,
        })
    }

    pub fn run(&mut self) -> io::Result<()> {
        let mut terminal = ratatui::init();
        let run_result = self.run_loop(&mut terminal);
        ratatui::restore();
        match run_result {
            Err(run_err) => Err(run_err),
            Ok(()) => Ok(()),
        }
    }

    fn run_loop(&mut self, terminal: &mut ratatui::DefaultTerminal) -> io::Result<()> {
        while !self.should_quit {
            terminal.draw(|frame| self.render(frame))?;
            if event::poll(Duration::from_millis(120))? {
                let ev = event::read()?;
                if let Event::Key(key) = ev {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area);

        let tabs_widget = Tabs::new(Tab::all().iter().map(|t| t.title()).collect::<Vec<_>>())
            .select(self.current_tab_idx)
            .block(Block::default().borders(Borders::ALL).title("NucleusDB"))
            .style(Style::default().fg(Color::Gray))
            .divider(" │ ")
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            );
        frame.render_widget(tabs_widget, chunks[0]);

        match self.current_tab() {
            Tab::Status => tabs::status::render(self, frame, chunks[1]),
            Tab::Browse => tabs::browse::render(self, frame, chunks[1]),
            Tab::Execute => tabs::execute::render(self, frame, chunks[1]),
            Tab::History => tabs::history::render(self, frame, chunks[1]),
            Tab::Transparency => tabs::transparency::render(self, frame, chunks[1]),
        }

        let footer = Paragraph::new(self.footer_text()).style(Style::default().fg(Color::Gray));
        frame.render_widget(footer, chunks[2]);
    }

    fn footer_text(&self) -> &'static str {
        match self.current_tab() {
            Tab::Execute => {
                "F1-F5/Tab: switch tabs | Enter: execute SQL | Up/Down: SQL history | Esc: clear"
            }
            _ => "F1-F5/Tab: switch tabs | Up/Down: scroll | q: quit | Ctrl-C: quit",
        }
    }

    fn current_tab(&self) -> Tab {
        Tab::all()
            .get(self.current_tab_idx)
            .copied()
            .unwrap_or(Tab::Status)
    }

    fn next_tab(&mut self) {
        self.current_tab_idx = (self.current_tab_idx + 1) % Tab::all().len();
    }

    fn prev_tab(&mut self) {
        if self.current_tab_idx == 0 {
            self.current_tab_idx = Tab::all().len() - 1;
        } else {
            self.current_tab_idx -= 1;
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> io::Result<()> {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return Ok(());
        }

        match key.code {
            KeyCode::F(1) => self.current_tab_idx = 0,
            KeyCode::F(2) => self.current_tab_idx = 1,
            KeyCode::F(3) => self.current_tab_idx = 2,
            KeyCode::F(4) => self.current_tab_idx = 3,
            KeyCode::F(5) => self.current_tab_idx = 4,
            KeyCode::Tab => self.next_tab(),
            KeyCode::BackTab => self.prev_tab(),
            _ => match self.current_tab() {
                Tab::Execute => self.handle_execute_key(key)?,
                _ => self.handle_non_execute_key(key),
            },
        }
        Ok(())
    }

    fn handle_non_execute_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.scroll_up(),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_down(),
            _ => {}
        }
    }

    fn handle_execute_key(&mut self, key: KeyEvent) -> io::Result<()> {
        match key.code {
            KeyCode::Enter => self.execute_sql()?,
            KeyCode::Esc => {
                self.sql_input.clear();
                self.sql_history_cursor = None;
            }
            KeyCode::Backspace => {
                self.sql_input.pop();
            }
            KeyCode::Up => self.sql_history_prev(),
            KeyCode::Down => self.sql_history_next(),
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.sql_input.push(ch);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn execute_sql(&mut self) -> io::Result<()> {
        let sql = self.sql_input.trim();
        if sql.is_empty() {
            return Ok(());
        }

        let sql_owned = sql.to_string();
        self.sql_history.push(sql_owned.clone());
        self.sql_history_cursor = None;

        let (result, committed) = {
            let mut executor = SqlExecutor::new(&mut self.db);
            let r = executor.execute(&sql_owned);
            (r, executor.committed())
        };

        match result {
            SqlResult::Rows { columns, rows } => {
                let mut lines = Vec::new();
                lines.push(columns.join(" | "));
                lines.push("-".repeat(lines[0].len().max(3)));
                for row in rows {
                    lines.push(row.join(" | "));
                }
                self.sql_output = if lines.is_empty() {
                    vec!["(no rows)".to_string()]
                } else {
                    lines
                };
            }
            SqlResult::Ok { message } => {
                self.sql_output = vec![format!("OK: {message}")];
            }
            SqlResult::Error { message } => {
                self.sql_output = vec![format!("Error: {message}")];
            }
        }

        // Only persist when a COMMIT actually happened (Bug #3 fix).
        if committed {
            self.persist_snapshot()?;
        }

        self.sql_input.clear();
        Ok(())
    }

    fn persist_snapshot(&self) -> io::Result<()> {
        let wal_path = default_wal_path(&self.db_path);
        persist_snapshot_and_sync_wal(&self.db_path, &wal_path, &self.db)
            .map_err(|e| io::Error::other(format!("failed to save snapshot+wal: {e:?}")))
    }

    fn sql_history_prev(&mut self) {
        if self.sql_history.is_empty() {
            return;
        }
        let next = match self.sql_history_cursor {
            None => self.sql_history.len().saturating_sub(1),
            Some(cur) => cur.saturating_sub(1),
        };
        self.sql_history_cursor = Some(next);
        if let Some(sql) = self.sql_history.get(next) {
            self.sql_input = sql.clone();
        }
    }

    fn sql_history_next(&mut self) {
        if self.sql_history.is_empty() {
            return;
        }
        let Some(cur) = self.sql_history_cursor else {
            return;
        };
        if cur + 1 >= self.sql_history.len() {
            self.sql_history_cursor = None;
            self.sql_input.clear();
            return;
        }
        let next = cur + 1;
        self.sql_history_cursor = Some(next);
        if let Some(sql) = self.sql_history.get(next) {
            self.sql_input = sql.clone();
        }
    }

    fn scroll_up(&mut self) {
        match self.current_tab() {
            Tab::Browse => {
                self.browse_index = self.browse_index.saturating_sub(1);
            }
            Tab::History => {
                self.history_index = self.history_index.saturating_sub(1);
            }
            Tab::Transparency => {
                self.transparency_index = self.transparency_index.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn scroll_down(&mut self) {
        match self.current_tab() {
            Tab::Browse => {
                let max = self.db.keymap.len().saturating_sub(1);
                self.browse_index = self.browse_index.saturating_add(1).min(max);
            }
            Tab::History => {
                let max = self.db.entries.len().saturating_sub(1);
                self.history_index = self.history_index.saturating_add(1).min(max);
            }
            Tab::Transparency => {
                let max = self.db.ct_leaves.len().saturating_sub(1);
                self.transparency_index = self.transparency_index.saturating_add(1).min(max);
            }
            _ => {}
        }
    }

    pub(crate) fn indexed_key_rows(&self) -> Vec<(String, u64, usize)> {
        let mut rows = self
            .db
            .keymap
            .all_keys()
            .map(|(key, idx)| {
                let value = self.db.state.values.get(idx).copied().unwrap_or(0);
                (key.to_string(), value, idx)
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|(_, _, idx)| *idx);
        rows
    }

    pub(crate) fn db_file_name(&self) -> String {
        self.db_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("nucleusdb.ndb")
            .to_string()
    }
}

pub fn run_tui(db_path: &str) -> io::Result<()> {
    let mut app = App::load(db_path)?;
    app.run()
}

pub(crate) fn format_unix_utc(ts: u64) -> String {
    Utc.timestamp_opt(ts as i64, 0)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| format!("invalid_unix_ts({ts})"))
}
