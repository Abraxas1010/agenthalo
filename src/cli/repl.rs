use crate::persistence::{default_wal_path, persist_snapshot_and_sync_wal};
use crate::protocol::NucleusDb;
use crate::sql::executor::{split_sql_statements, SqlExecutor, SqlResult};
use rustyline::DefaultEditor;
use std::path::Path;

use super::print_table;

pub fn run_repl(db: &mut NucleusDb, db_path: &Path) -> Result<(), String> {
    let mut rl = DefaultEditor::new().map_err(|e| format!("failed to start REPL editor: {e}"))?;
    let mut executor = SqlExecutor::new(db);

    println!(
        "NucleusDB v{} — Type SQL or .help for commands",
        env!("CARGO_PKG_VERSION")
    );
    println!("Type .quit to exit.");

    loop {
        let readline = rl.readline("nucleusdb> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed.eq_ignore_ascii_case(".quit") || trimmed.eq_ignore_ascii_case(".exit") {
                    break;
                }
                if trimmed.eq_ignore_ascii_case(".help") {
                    print_help();
                    continue;
                }
                rl.add_history_entry(trimmed)
                    .map_err(|e| format!("failed to record history: {e}"))?;

                let out = executor.execute(trimmed);
                match out {
                    SqlResult::Rows { columns, rows } => print_table(&columns, &rows),
                    SqlResult::Ok { message } => println!("{message}"),
                    SqlResult::Error { message } => eprintln!("Error: {message}"),
                }

                // Persist after successful COMMIT so interactive sessions are durable.
                if trimmed.trim_end_matches(';').eq_ignore_ascii_case("commit") {
                    persist_current(db_path, executor.db())?;
                    println!("Snapshot persisted to {}", db_path.display());
                }
            }
            Err(_) => break,
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlBatchSummary {
    pub statements_executed: usize,
    pub committed: bool,
    pub pending_writes: usize,
}

pub fn execute_sql_text(
    db: &mut NucleusDb,
    db_path: &Path,
    sql_text: &str,
) -> Result<SqlBatchSummary, String> {
    let statements = split_sql_statements(sql_text.trim());
    if statements.is_empty() {
        println!("No statements");
        return Ok(SqlBatchSummary {
            statements_executed: 0,
            committed: false,
            pending_writes: 0,
        });
    }

    let total = statements.len();
    let (committed, pending_writes) = {
        let mut executor = SqlExecutor::new(db);
        for (i, stmt) in statements.iter().enumerate() {
            if total > 1 {
                println!("-- [{}/{}] {}", i + 1, total, statement_preview(stmt));
            }
            match executor.execute(stmt) {
                SqlResult::Rows { columns, rows } => print_table(&columns, &rows),
                SqlResult::Ok { message } => println!("{message}"),
                SqlResult::Error { message } => {
                    eprintln!("Error: {message}");
                    return Err(format!("statement {}/{} failed: {}", i + 1, total, message));
                }
            }
        }
        (executor.committed(), executor.pending_writes_len())
    };

    // Only persist when a COMMIT actually happened — prevents uncommitted
    // keymap pollution from being saved to disk (Bug #3).
    if committed {
        persist_current(db_path, db)?;
    }

    Ok(SqlBatchSummary {
        statements_executed: total,
        committed,
        pending_writes,
    })
}

fn persist_current(path: &Path, db: &NucleusDb) -> Result<(), String> {
    let wal_path = default_wal_path(path);
    persist_snapshot_and_sync_wal(path, &wal_path, db)
        .map_err(|e| format!("failed to persist snapshot+wal: {e:?}"))
}

fn statement_preview(stmt: &str) -> String {
    let compact = stmt.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let preview: String = chars.by_ref().take(96).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

fn print_help() {
    println!("REPL commands:");
    println!("  .help                  Show this help");
    println!("  .quit | .exit          Exit REPL");
    println!("SQL:");
    println!("  INSERT INTO data (key, value) VALUES ('k', 1);");
    println!("  SELECT key, value FROM data WHERE key LIKE 'k%';");
    println!("  UPDATE data SET value = 2 WHERE key = 'k';");
    println!("  DELETE FROM data WHERE key = 'k';");
    println!("  SHOW STATUS;");
    println!("  SHOW HISTORY;");
    println!("  SHOW HISTORY 'k';");
    println!("  VERIFY 'k';");
    println!("  COMMIT;");
}
