use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_nucleusdb")
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
