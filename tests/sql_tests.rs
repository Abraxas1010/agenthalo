use nucleusdb::protocol::{NucleusDb, VcBackend};
use nucleusdb::sql::executor::{SqlExecutor, SqlResult};
use nucleusdb::state::State;
use nucleusdb::witness::WitnessConfig;
use std::collections::BTreeMap;

fn mk_cfg() -> WitnessConfig {
    WitnessConfig::with_generated_keys(2, vec!["w1".into(), "w2".into(), "w3".into()])
}

fn mk_db() -> NucleusDb {
    NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, mk_cfg())
}

fn expect_ok(res: SqlResult) {
    match res {
        SqlResult::Ok { .. } => {}
        SqlResult::Error { message } => panic!("expected Ok, got Error: {message}"),
        SqlResult::Rows { .. } => panic!("expected Ok, got Rows"),
    }
}

fn expect_rows(res: SqlResult) -> (Vec<String>, Vec<Vec<String>>) {
    match res {
        SqlResult::Rows { columns, rows } => (columns, rows),
        SqlResult::Error { message } => panic!("expected Rows, got Error: {message}"),
        SqlResult::Ok { message } => panic!("expected Rows, got Ok: {message}"),
    }
}

#[test]
fn sql_insert_select_commit_roundtrip() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('temperature', 42);"));
    expect_ok(exec.execute("COMMIT;"));

    let (cols, rows) =
        expect_rows(exec.execute("SELECT key, value FROM data WHERE key = 'temperature';"));
    assert_eq!(cols, vec!["key".to_string(), "value".to_string()]);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0], vec!["temperature".to_string(), "42".to_string()]);
}

#[test]
fn sql_show_status_tracks_pending_and_entries() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('x', 7);"));
    let (_, rows_before) = expect_rows(exec.execute("SHOW STATUS;"));
    let map_before: BTreeMap<String, String> = rows_before
        .into_iter()
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    assert_eq!(
        map_before.get("pending_writes").map(String::as_str),
        Some("1")
    );
    assert_eq!(map_before.get("entries").map(String::as_str), Some("0"));

    expect_ok(exec.execute("COMMIT;"));
    let (_, rows_after) = expect_rows(exec.execute("SHOW STATUS;"));
    let map_after: BTreeMap<String, String> = rows_after
        .into_iter()
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    assert_eq!(
        map_after.get("pending_writes").map(String::as_str),
        Some("0")
    );
    assert_eq!(map_after.get("entries").map(String::as_str), Some("1"));
    assert_eq!(map_after.get("key_count").map(String::as_str), Some("1"));
}

#[test]
fn sql_where_like_prefix_filtering() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('temp_a', 1);"));
    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('temp_b', 2);"));
    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('other', 9);"));
    expect_ok(exec.execute("COMMIT;"));

    let (_, rows) =
        expect_rows(exec.execute("SELECT key, value FROM data WHERE key LIKE 'temp%';"));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "temp_a");
    assert_eq!(rows[1][0], "temp_b");
}

#[test]
fn sql_update_and_delete_apply_on_commit() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('k', 10);"));
    expect_ok(exec.execute("COMMIT;"));

    expect_ok(exec.execute("UPDATE data SET value = 99 WHERE key = 'k';"));
    expect_ok(exec.execute("COMMIT;"));
    let (_, rows_after_update) =
        expect_rows(exec.execute("SELECT key, value FROM data WHERE key = 'k';"));
    assert_eq!(
        rows_after_update[0],
        vec!["k".to_string(), "99".to_string()]
    );

    expect_ok(exec.execute("DELETE FROM data WHERE key = 'k';"));
    expect_ok(exec.execute("COMMIT;"));
    let (_, rows_after_delete) =
        expect_rows(exec.execute("SELECT key, value FROM data WHERE key = 'k';"));
    assert_eq!(rows_after_delete[0], vec!["k".to_string(), "0".to_string()]);
}
