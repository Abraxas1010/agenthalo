use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_nucleusdb")
}

fn resolve_bin(env_key: &str, fallback_name: &str) -> PathBuf {
    if let Ok(path) = std::env::var(env_key) {
        return PathBuf::from(path);
    }
    let exe = std::env::current_exe().expect("current_exe");
    let debug_dir = exe
        .parent()
        .and_then(|p| p.parent())
        .expect("test binary should live under target/*/deps");
    let candidate = debug_dir.join(fallback_name);
    #[cfg(windows)]
    {
        candidate.set_extension("exe");
    }
    candidate
}

fn server_bin() -> PathBuf {
    resolve_bin("CARGO_BIN_EXE_nucleusdb_server", "nucleusdb-server")
}

#[test]
fn cli_help_smoke() {
    let out = Command::new(bin())
        .arg("--help")
        .output()
        .expect("run --help");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("nucleusdb"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("open"));
}

#[test]
fn nucleusdb_dashboard_help_smoke() {
    let out = Command::new(bin())
        .args(["dashboard", "--help"])
        .output()
        .expect("run nucleusdb dashboard --help");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("dashboard"));
    assert!(stdout.contains("--port"));
}

#[test]
fn nucleusdb_server_help_smoke() {
    let out = Command::new(server_bin())
        .arg("--help")
        .output()
        .expect("run nucleusdb-server --help");
    assert!(out.status.success(), "{out:?}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("nucleusdb-server"));
    assert!(stdout.contains("production"));
}

#[test]
fn cli_tui_non_tty_exits_with_actionable_error() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("nucleusdb_cli_tui_{stamp}.ndb"));

    let out = Command::new(bin())
        .args(["tui", "--db"])
        .arg(&db_path)
        .output()
        .expect("run nucleusdb tui");

    assert!(!out.status.success(), "{out:?}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("interactive terminal (TTY)"),
        "stderr must include actionable TTY guidance: {stderr}"
    );
    assert!(
        !stderr.to_ascii_lowercase().contains("panicked"),
        "command should fail cleanly without panic: {stderr}"
    );

    let _ = std::fs::remove_file(db_path);
}

#[test]
fn cli_create_sql_status_export_smoke() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("nucleusdb_cli_{stamp}.ndb"));
    let sql_path = std::env::temp_dir().join(format!("nucleusdb_cli_{stamp}.sql"));
    std::fs::write(
        &sql_path,
        "INSERT INTO data (key, value) VALUES ('alpha', 7); COMMIT;",
    )
    .expect("write sql");

    let create = Command::new(bin())
        .args(["create", "--db"])
        .arg(&db_path)
        .args(["--backend", "merkle"])
        .output()
        .expect("create");
    assert!(create.status.success(), "{create:?}");

    let sql = Command::new(bin())
        .args(["sql", "--db"])
        .arg(&db_path)
        .arg(&sql_path)
        .output()
        .expect("sql");
    assert!(sql.status.success(), "{sql:?}");

    let status = Command::new(bin())
        .args(["status", "--db"])
        .arg(&db_path)
        .output()
        .expect("status");
    assert!(status.status.success(), "{status:?}");
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(status_out.contains("entries"));

    let export = Command::new(bin())
        .args(["export", "--db"])
        .arg(&db_path)
        .output()
        .expect("export");
    assert!(export.status.success(), "{export:?}");
    let export_out = String::from_utf8_lossy(&export.stdout);
    assert!(export_out.contains("alpha"));
}

#[test]
fn cli_sql_warns_when_writes_uncommitted() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("nucleusdb_cli_warn_{stamp}.ndb"));
    let sql_path = std::env::temp_dir().join(format!("nucleusdb_cli_warn_{stamp}.sql"));
    std::fs::write(&sql_path, "INSERT INTO data (key, value) VALUES ('k1', 1);")
        .expect("write sql");

    let create = Command::new(bin())
        .args(["create", "--db"])
        .arg(&db_path)
        .args(["--backend", "merkle"])
        .output()
        .expect("create");
    assert!(create.status.success(), "{create:?}");

    let sql = Command::new(bin())
        .args(["sql", "--db"])
        .arg(&db_path)
        .arg(&sql_path)
        .output()
        .expect("sql");
    assert!(sql.status.success(), "{sql:?}");
    let sql_stderr = String::from_utf8_lossy(&sql.stderr);
    assert!(sql_stderr.contains("WARNING:"), "{sql_stderr}");
    assert!(sql_stderr.contains("not committed"), "{sql_stderr}");

    let export = Command::new(bin())
        .args(["export", "--db"])
        .arg(&db_path)
        .output()
        .expect("export");
    assert!(export.status.success(), "{export:?}");
    let export_out = String::from_utf8_lossy(&export.stdout);
    assert!(
        !export_out.contains("k1"),
        "uncommitted write should not be persisted: {export_out}"
    );
}

#[test]
fn cli_sql_streams_multi_statement_results() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let db_path = std::env::temp_dir().join(format!("nucleusdb_cli_stream_{stamp}.ndb"));
    let sql_path = std::env::temp_dir().join(format!("nucleusdb_cli_stream_{stamp}.sql"));
    std::fs::write(
        &sql_path,
        "INSERT INTO data (key, value) VALUES ('a', 1);\n\
         INSERT INTO data (key, value) VALUES ('b', 2);\n\
         COMMIT;\n\
         SELECT key, value FROM data WHERE key = 'a';\n\
         SELECT key, value FROM data WHERE key = 'b';\n",
    )
    .expect("write sql");

    let create = Command::new(bin())
        .args(["create", "--db"])
        .arg(&db_path)
        .args(["--backend", "merkle"])
        .output()
        .expect("create");
    assert!(create.status.success(), "{create:?}");

    let sql = Command::new(bin())
        .args(["sql", "--db"])
        .arg(&db_path)
        .arg(&sql_path)
        .output()
        .expect("sql");
    assert!(sql.status.success(), "{sql:?}");

    let out = String::from_utf8_lossy(&sql.stdout);
    assert!(out.contains("-- [4/5]"), "{out}");
    assert!(out.contains("-- [5/5]"), "{out}");
    let table_header_count = out.matches(" key | value ").count();
    assert!(
        table_header_count >= 2,
        "expected at least two SELECT result tables, got {table_header_count}: {out}"
    );
    assert!(out.contains(" a   | 1"), "{out}");
    assert!(out.contains(" b   | 2"), "{out}");
}
