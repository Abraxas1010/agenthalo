//! Tests for the dashboard API security and correctness.

use nucleusdb::dashboard::api::api_router;
use nucleusdb::dashboard::DashboardState;
use nucleusdb::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
use nucleusdb::halo::trace::{now_unix_secs, TraceWriter};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use tower::ServiceExt;

fn temp_db_path(tag: &str) -> PathBuf {
    let stamp = format!("{}-{}-{}", tag, std::process::id(), now_unix_secs());
    std::env::temp_dir().join(format!("dashboard_test_{stamp}.ndb"))
}

fn test_state(tag: &str) -> (DashboardState, PathBuf) {
    let db_path = temp_db_path(tag);
    let creds = std::env::temp_dir().join(format!("creds_{tag}_{}.json", std::process::id()));
    let state = DashboardState {
        db_path: db_path.clone(),
        credentials_path: creds,
        db_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
    };
    (state, db_path)
}

fn seed_session(db_path: &std::path::Path, session_id: &str) {
    let mut writer = TraceWriter::new(db_path).expect("writer");
    writer
        .start_session(SessionMetadata {
            session_id: session_id.to_string(),
            agent: "claude".to_string(),
            model: Some("claude-opus-4-6".to_string()),
            started_at: now_unix_secs(),
            ended_at: None,
            prompt: Some("test".to_string()),
            status: SessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })
        .expect("start");

    writer
        .write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Assistant,
            content: json!({"text": "hello from test"}),
            input_tokens: Some(10),
            output_tokens: Some(5),
            cache_read_tokens: Some(0),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
        .expect("event");

    writer.end_session(SessionStatus::Completed).expect("end");
}

async fn api_get(state: DashboardState, path: &str) -> (StatusCode, Value) {
    let app = api_router(state.clone()).with_state(state);
    let req = Request::builder().uri(path).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&body).unwrap_or(json!(null));
    (status, val)
}

async fn api_post(state: DashboardState, path: &str, body: Value) -> (StatusCode, Value) {
    let app = api_router(state.clone()).with_state(state);
    let req = Request::builder()
        .uri(path)
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&body).unwrap_or(json!(null));
    (status, val)
}

// ---------------------------------------------------------------------------
// P1: Agent whitelist — shell injection prevention
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wrap_rejects_invalid_agent_name() {
    let (state, db_path) = test_state("wrap_reject");
    let (status, val) = api_post(
        state,
        "/config/wrap",
        json!({"agent": "'; rm -rf /; echo '", "enable": true}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(val["error"]
        .as_str()
        .unwrap()
        .contains("claude, codex, gemini"));
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn wrap_accepts_valid_agents() {
    for agent in &["claude", "codex", "gemini"] {
        let (state, db_path) = test_state(&format!("wrap_ok_{agent}"));
        // We don't actually test shell RC modification (that needs real files),
        // but the agent whitelist check passes.
        let (status, _val) = api_post(
            state,
            "/config/wrap",
            json!({"agent": agent, "enable": false}),
        )
        .await;
        // Should not return BAD_REQUEST (may fail for other reasons like missing RC)
        assert_ne!(status, StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_file(&db_path);
    }
}

// ---------------------------------------------------------------------------
// P2: SQL execution is functional (not a stub)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sql_executes_real_queries() {
    let (state, db_path) = test_state("sql_exec");
    seed_session(&db_path, "sql-test-session");

    // SHOW STATUS should return real data
    let (status, val) = api_post(
        state.clone(),
        "/nucleusdb/sql",
        json!({"query": "SHOW STATUS"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Should have columns and rows — not "not_implemented"
    assert!(
        val.get("columns").is_some() || val.get("message").is_some(),
        "SQL result should have columns or message, got: {val}"
    );
    assert!(
        val.get("status").is_none(),
        "should not have stub status field"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn sql_insert_select_roundtrip() {
    let (state, db_path) = test_state("sql_rt");

    // INSERT + COMMIT in a single request (each request loads fresh DB from disk)
    let (s1, v1) = api_post(
        state.clone(),
        "/nucleusdb/sql",
        json!({"query": "INSERT INTO data (key, value) VALUES ('test_key', 42); COMMIT"}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK);
    assert!(
        v1.get("error").is_none(),
        "insert+commit should not error: {v1}"
    );

    // Select back (fresh DB load reads persisted data)
    let (s3, v3) = api_post(
        state.clone(),
        "/nucleusdb/sql",
        json!({"query": "SELECT key, value FROM data WHERE key = 'test_key'"}),
    )
    .await;
    assert_eq!(s3, StatusCode::OK);
    let rows = v3["rows"].as_array().expect("should have rows");
    assert!(!rows.is_empty(), "should find inserted key");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn sql_rejects_bad_syntax() {
    let (state, db_path) = test_state("sql_bad");
    let (status, val) = api_post(
        state,
        "/nucleusdb/sql",
        json!({"query": "SELCT * FORM nonexistent"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Should return an error message, not crash
    assert!(
        val.get("error").is_some(),
        "bad SQL should return error field: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// P2: Attestation verification is cryptographic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attestation_verify_is_cryptographic() {
    let (state, db_path) = test_state("attest_verify");
    let session_id = format!("sess-verify-{}", now_unix_secs());
    seed_session(&db_path, &session_id);

    // Create attestation
    let (s1, v1) = api_post(
        state.clone(),
        &format!("/sessions/{session_id}/attest"),
        json!({}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "attest should succeed: {v1}");
    let digest = v1["attestation"]["attestation_digest"]
        .as_str()
        .expect("should have digest")
        .to_string();

    // Verify — should be cryptographically verified, not just "found"
    let (s2, v2) = api_post(
        state.clone(),
        "/attestations/verify",
        json!({"digest": digest}),
    )
    .await;
    assert_eq!(s2, StatusCode::OK);
    assert_eq!(v2["found"], true);
    assert_eq!(
        v2["verified"], true,
        "should be cryptographically verified: {v2}"
    );

    // Verify checks are present
    assert_eq!(v2["checks"]["digest_match"], true);
    assert_eq!(v2["checks"]["merkle_root_match"], true);
    assert_eq!(v2["checks"]["event_count_match"], true);

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn attestation_verify_nonexistent_returns_not_found() {
    let (state, db_path) = test_state("attest_miss");

    let (status, val) = api_post(
        state,
        "/attestations/verify",
        json!({"digest": "00".repeat(32)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(val["found"], false);
    assert_eq!(val["verified"], false);

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// Status endpoint returns expected fields
// ---------------------------------------------------------------------------

#[tokio::test]
async fn status_endpoint_returns_expected_fields() {
    let (state, db_path) = test_state("status_fields");
    seed_session(&db_path, "status-test");

    let (status, val) = api_get(state, "/status").await;
    assert_eq!(status, StatusCode::OK);
    assert!(val.get("version").is_some());
    assert!(val.get("session_count").is_some());
    assert!(val.get("total_cost_usd").is_some());
    assert!(val.get("wrapping").is_some());

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// NucleusDB browse returns real data
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_browse_returns_data() {
    let (state, db_path) = test_state("ndb_browse");
    seed_session(&db_path, "browse-test");

    let (status, val) = api_get(state, "/nucleusdb/browse").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        val.get("total").is_some(),
        "browse should have total: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// NucleusDB browse returns paginated data with metadata
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_browse_paginated_has_metadata() {
    let (state, db_path) = test_state("ndb_browse_pag");
    seed_session(&db_path, "browse-pag-test");

    let (status, val) = api_get(state, "/nucleusdb/browse?page=0&page_size=10").await;
    assert_eq!(status, StatusCode::OK);
    assert!(val.get("rows").is_some(), "should have rows: {val}");
    assert!(val.get("total").is_some(), "should have total: {val}");
    assert!(val.get("page").is_some(), "should have page: {val}");
    assert!(
        val.get("page_size").is_some(),
        "should have page_size: {val}"
    );
    assert!(
        val.get("total_pages").is_some(),
        "should have total_pages: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// NucleusDB stats returns key count and prefixes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_stats_returns_counts() {
    let (state, db_path) = test_state("ndb_stats");
    seed_session(&db_path, "stats-test");

    let (status, val) = api_get(state, "/nucleusdb/stats").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        val.get("key_count").is_some(),
        "should have key_count: {val}"
    );
    assert!(
        val.get("commit_count").is_some(),
        "should have commit_count: {val}"
    );
    assert!(
        val.get("top_prefixes").is_some(),
        "should have top_prefixes: {val}"
    );
    assert!(
        val.get("db_size_bytes").is_some(),
        "should have db_size_bytes: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// NucleusDB export returns JSON content
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_export_returns_json() {
    let (state, db_path) = test_state("ndb_export");
    seed_session(&db_path, "export-test");

    let (status, val) = api_get(state, "/nucleusdb/export?format=json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(val.get("format").is_some(), "should have format: {val}");
    assert!(val.get("content").is_some(), "should have content: {val}");
    assert!(val.get("count").is_some(), "should have count: {val}");
    assert_eq!(val["format"], "json");

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// NucleusDB typed value edit roundtrip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_edit_text_value() {
    let (state, db_path) = test_state("ndb_edit_text");
    seed_session(&db_path, "edit-text-test");

    let (status, val) = api_post(
        state.clone(),
        "/nucleusdb/edit",
        json!({"key": "greeting", "value": "Hello, World!"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "edit text: {val}");
    assert_eq!(val["ok"], true);
    assert_eq!(val["type"], "text");

    // Verify via browse
    let (s2, v2) = api_get(state, "/nucleusdb/browse?prefix=greeting").await;
    assert_eq!(s2, StatusCode::OK);
    let rows = v2["rows"].as_array().expect("should have rows");
    assert!(!rows.is_empty(), "should find the key");
    assert_eq!(rows[0]["type"], "text");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_edit_json_value() {
    let (state, db_path) = test_state("ndb_edit_json");
    seed_session(&db_path, "edit-json-test");

    let (status, val) = api_post(
        state.clone(),
        "/nucleusdb/edit",
        json!({"key": "user:bob", "value": {"name": "Bob", "age": 25}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "edit json: {val}");
    assert_eq!(val["ok"], true);
    assert_eq!(val["type"], "json");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_edit_respects_explicit_text_type() {
    let (state, db_path) = test_state("ndb_edit_explicit_text");
    seed_session(&db_path, "edit-explicit-text-test");

    let (status, val) = api_post(
        state.clone(),
        "/nucleusdb/edit",
        json!({"key": "payload:text", "type": "text", "value": "{\"name\":\"Alice\"}"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "edit explicit text: {val}");
    assert_eq!(val["ok"], true);
    assert_eq!(val["type"], "text");

    let (s2, v2) = api_get(state, "/nucleusdb/browse?prefix=payload:text").await;
    assert_eq!(s2, StatusCode::OK);
    let rows = v2["rows"].as_array().expect("should have rows");
    assert_eq!(rows[0]["type"], "text");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_edit_rejects_explicit_type_mismatch() {
    let (state, db_path) = test_state("ndb_edit_type_mismatch");
    seed_session(&db_path, "edit-type-mismatch-test");

    let (status, val) = api_post(
        state,
        "/nucleusdb/edit",
        json!({"key": "bad:int", "type": "integer", "value": "not-a-number"}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "type mismatch should be 400: {val}"
    );
    assert!(val.get("error").is_some(), "should include error: {val}");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_edit_vector_value() {
    let (state, db_path) = test_state("ndb_edit_vec");
    seed_session(&db_path, "edit-vec-test");

    let (status, val) = api_post(
        state.clone(),
        "/nucleusdb/edit",
        json!({"key": "doc:1:embedding", "value": [0.1, 0.2, 0.3, 0.4]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "edit vector: {val}");
    assert_eq!(val["ok"], true);
    assert_eq!(val["type"], "vector");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_key_history_includes_typed_fields() {
    let (state, db_path) = test_state("ndb_key_history_typed");
    seed_session(&db_path, "key-history-test");

    let (s1, v1) = api_post(
        state.clone(),
        "/nucleusdb/edit",
        json!({"key": "history:text", "type": "text", "value": "hello history"}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "edit should succeed: {v1}");

    let (s2, v2) = api_get(state, "/nucleusdb/key-history/history:text").await;
    assert_eq!(s2, StatusCode::OK, "key-history should succeed: {v2}");
    assert_eq!(v2["found"], true);
    assert_eq!(v2["type"], "text");
    assert_eq!(v2["current_typed_value"], json!("hello history"));
    assert_eq!(v2["current_display"], "hello history");
    assert!(
        v2.get("current_value").and_then(|v| v.as_u64()).is_some(),
        "raw current_value should remain for backward compatibility: {v2}"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_vector_search_endpoint() {
    let (state, db_path) = test_state("ndb_vsearch");
    seed_session(&db_path, "vsearch-test");

    // Insert vectors
    for i in 0..3 {
        let dims: Vec<f64> = (0..4).map(|j| if i == j { 1.0 } else { 0.0 }).collect();
        let (s, _) = api_post(
            state.clone(),
            "/nucleusdb/edit",
            json!({"key": format!("vec:{i}"), "value": dims}),
        )
        .await;
        assert_eq!(s, StatusCode::OK);
    }

    // Search
    let (status, val) = api_post(
        state.clone(),
        "/nucleusdb/vector-search",
        json!({"query": [1.0, 0.0, 0.0, 0.0], "k": 2, "metric": "cosine"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "vector search: {val}");
    let results = val["results"].as_array().expect("should have results");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["key"], "vec:0");

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_stats_includes_type_distribution() {
    let (state, db_path) = test_state("ndb_type_dist");
    seed_session(&db_path, "type-dist-test");

    let (status, val) = api_get(state, "/nucleusdb/stats").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        val.get("type_distribution").is_some(),
        "should have type_distribution: {val}"
    );
    assert!(
        val.get("blob_count").is_some(),
        "should have blob_count: {val}"
    );
    assert!(
        val.get("vector_count").is_some(),
        "should have vector_count: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// NucleusDB history returns commit history
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_history_has_commits_and_sessions() {
    let (state, db_path) = test_state("ndb_hist");
    seed_session(&db_path, "hist-test");

    let (status, val) = api_get(state, "/nucleusdb/history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        val.get("commits").is_some(),
        "should have commits field: {val}"
    );
    assert!(
        val.get("sessions").is_some(),
        "should have sessions field: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}
