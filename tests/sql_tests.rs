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

#[test]
fn sql_script_mixes_custom_and_standard_statements() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);
    let script = r#"
        INSERT INTO data (key, value) VALUES ('acct:1', 7);
        COMMIT;
        VERIFY 'acct:1';
        SHOW STATUS;
    "#;
    let (cols, rows) = expect_rows(exec.execute(script));
    assert_eq!(cols, vec!["field".to_string(), "value".to_string()]);
    let fields: BTreeMap<String, String> = rows
        .into_iter()
        .map(|r| (r[0].clone(), r[1].clone()))
        .collect();
    assert_eq!(fields.get("entries").map(String::as_str), Some("1"));
    assert_eq!(fields.get("key_count").map(String::as_str), Some("1"));
}

// --- Bug #2 regression tests: multi-statement batch with custom commands ---

#[test]
fn sql_multi_statement_insert_commit_verify_batch() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    let result = exec
        .execute("INSERT INTO data (key, value) VALUES ('sensor', 42); COMMIT; VERIFY 'sensor';");
    let (cols, rows) = expect_rows(result);
    assert!(cols.contains(&"verified".to_string()));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][3], "true");
}

#[test]
fn sql_multi_statement_select_then_show_status() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('a', 1);"));
    expect_ok(exec.execute("COMMIT;"));

    let result = exec.execute("SELECT * FROM data; SHOW STATUS;");
    let (cols, _rows) = expect_rows(result);
    assert!(cols.contains(&"field".to_string()));
}

#[test]
fn sql_multi_statement_two_custom_commands() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('x', 7);"));
    expect_ok(exec.execute("COMMIT;"));

    let result = exec.execute("SHOW STATUS; SHOW HISTORY;");
    let (cols, _rows) = expect_rows(result);
    assert!(cols.contains(&"height".to_string()));
}

#[test]
fn sql_multi_statement_respects_quoted_semicolons() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    let result =
        exec.execute("INSERT INTO data (key, value) VALUES ('key;with;semi', 99); COMMIT;");
    expect_ok(result);

    let (_, rows) = expect_rows(exec.execute("SELECT * FROM data WHERE key = 'key;with;semi';"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], "99");
}

// --- Bug #3 regression test: committed() flag ---

#[test]
fn sql_committed_flag_tracks_commit() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    assert!(!exec.committed());
    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('x', 1);"));
    assert!(!exec.committed(), "INSERT alone must not set committed");
    expect_ok(exec.execute("COMMIT;"));
    assert!(exec.committed(), "COMMIT must set committed flag");
}

#[test]
fn sql_uncommitted_insert_does_not_mark_committed() {
    let mut db = mk_db();

    {
        let mut exec = SqlExecutor::new(&mut db);
        expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('phantom', 999);"));
        assert!(!exec.committed());
    }

    {
        let mut exec = SqlExecutor::new(&mut db);
        let (_, rows) = expect_rows(exec.execute("SELECT * FROM data WHERE key = 'phantom';"));
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1], "0", "uncommitted key must show default value 0");
    }
}

// --- Immutable Agentic Records Mode ---

fn expect_error(res: SqlResult) -> String {
    match res {
        SqlResult::Error { message } => message,
        SqlResult::Ok { message } => panic!("expected Error, got Ok: {message}"),
        SqlResult::Rows { .. } => panic!("expected Error, got Rows"),
    }
}

#[test]
fn sql_set_mode_append_only_via_sql() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    let res = exec.execute("SHOW MODE;");
    match res {
        SqlResult::Ok { message } => assert!(
            message.contains("Normal"),
            "default should be Normal, got: {message}"
        ),
        other => panic!("expected Ok from SHOW MODE, got: {other:?}"),
    }

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));

    let res = exec.execute("SHOW MODE;");
    match res {
        SqlResult::Ok { message } => assert!(
            message.contains("AppendOnly"),
            "should be AppendOnly, got: {message}"
        ),
        other => panic!("expected Ok from SHOW MODE, got: {other:?}"),
    }
}

#[test]
fn sql_append_only_allows_insert_and_commit() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));
    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('sensor_1', 100);"));
    expect_ok(exec.execute("COMMIT;"));

    let (_, rows) = expect_rows(exec.execute("SELECT * FROM data WHERE key = 'sensor_1';"));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], "100");
}

#[test]
fn sql_append_only_rejects_update() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('record', 42);"));
    expect_ok(exec.execute("COMMIT;"));

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));

    let msg = expect_error(exec.execute("UPDATE data SET value = 99 WHERE key = 'record';"));
    assert!(
        msg.contains("AppendOnly"),
        "rejection message should mention AppendOnly: {msg}"
    );
    assert!(
        msg.contains("UPDATE rejected"),
        "should say UPDATE rejected: {msg}"
    );
}

#[test]
fn sql_append_only_rejects_delete() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('immutable_key', 7);"));
    expect_ok(exec.execute("COMMIT;"));

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));

    let msg = expect_error(exec.execute("DELETE FROM data WHERE key = 'immutable_key';"));
    assert!(
        msg.contains("AppendOnly"),
        "rejection message should mention AppendOnly: {msg}"
    );
    assert!(
        msg.contains("DELETE rejected"),
        "should say DELETE rejected: {msg}"
    );
}

#[test]
fn sql_append_only_multiple_inserts_commit_produces_seals() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));

    for i in 0..3 {
        let sql = format!(
            "INSERT INTO data (key, value) VALUES ('agent_log_{}', {}); COMMIT;",
            i,
            i * 10 + 1
        );
        expect_ok(exec.execute(&sql));
    }

    assert_eq!(
        db.monotone_seals().len(),
        3,
        "each commit should produce a seal"
    );

    let seals: Vec<_> = db.monotone_seals().to_vec();
    for i in 0..seals.len() {
        for j in (i + 1)..seals.len() {
            assert_ne!(seals[i], seals[j], "seals at {i} and {j} must differ");
        }
    }
}

#[test]
fn sql_append_only_data_readable_after_lock() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('pre_lock', 55);"));
    expect_ok(exec.execute("COMMIT;"));

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));

    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('post_lock', 77);"));
    expect_ok(exec.execute("COMMIT;"));

    let (_, rows) = expect_rows(exec.execute("SELECT * FROM data;"));
    assert_eq!(rows.len(), 2);

    let (_, pre) = expect_rows(exec.execute("SELECT * FROM data WHERE key = 'pre_lock';"));
    assert_eq!(pre[0][1], "55");

    let (_, post) = expect_rows(exec.execute("SELECT * FROM data WHERE key = 'post_lock';"));
    assert_eq!(post[0][1], "77");
}

#[test]
fn sql_append_only_verify_still_works() {
    let mut db = mk_db();
    let mut exec = SqlExecutor::new(&mut db);

    expect_ok(exec.execute("SET MODE APPEND_ONLY;"));
    expect_ok(exec.execute("INSERT INTO data (key, value) VALUES ('verified_record', 123);"));
    expect_ok(exec.execute("COMMIT;"));

    let result = exec.execute("VERIFY 'verified_record';");
    let (cols, rows) = expect_rows(result);
    assert!(cols.contains(&"verified".to_string()));
    assert_eq!(rows[0][3], "true");
}
