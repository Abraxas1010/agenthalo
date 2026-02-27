//! Tests for the dashboard API security and correctness.

use nucleusdb::dashboard::api::api_router;
use nucleusdb::dashboard::{build_state, DashboardState};
use nucleusdb::halo::agentpmt;
use nucleusdb::halo::auth::{save_credentials, Credentials};
use nucleusdb::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
use nucleusdb::halo::trace::{list_sessions as list_trace_sessions, now_unix_secs, TraceWriter};
use nucleusdb::halo::vault::Vault;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

fn temp_db_path(tag: &str) -> PathBuf {
    let stamp = format!("{}-{}-{}", tag, std::process::id(), now_unix_secs());
    std::env::temp_dir().join(format!("dashboard_test_{stamp}.ndb"))
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let prev = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        Self { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(v) = self.prev.as_ref() {
            std::env::set_var(self.key, v);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn test_state(tag: &str) -> (DashboardState, PathBuf) {
    let db_path = temp_db_path(tag);
    let creds = std::env::temp_dir().join(format!("creds_{tag}_{}.json", std::process::id()));
    let _ = save_credentials(
        &creds,
        &Credentials {
            api_key: Some("test-local-api-key".to_string()),
            oauth_token: None,
            oauth_provider: None,
            user_id: Some("dashboard-tests".to_string()),
            created_at: now_unix_secs(),
        },
    );
    let state = build_state(db_path.clone(), creds);
    (state, db_path)
}

fn test_state_unauth(tag: &str) -> (DashboardState, PathBuf, PathBuf) {
    let db_path = temp_db_path(tag);
    let creds = std::env::temp_dir().join(format!(
        "creds_unauth_{tag}_{}_{}.json",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_file(&creds);
    let state = build_state(db_path.clone(), creds.clone());
    (state, db_path, creds)
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

async fn api_delete(state: DashboardState, path: &str) -> (StatusCode, Value) {
    let app = api_router(state.clone()).with_state(state);
    let req = Request::builder()
        .uri(path)
        .method("DELETE")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    let val: Value = serde_json::from_slice(&body).unwrap_or(json!(null));
    (status, val)
}

fn write_wallet_json(path: &std::path::Path, key_id: &str, seed_hex: &str) {
    let wallet = json!({
        "version": 1,
        "algorithm": "ml_dsa65",
        "key_id": key_id,
        "public_key_hex": "00",
        "secret_seed_hex": seed_hex,
        "created_at": now_unix_secs(),
    });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, serde_json::to_vec_pretty(&wallet).unwrap()).unwrap();
}

fn test_vault(tag: &str) -> (Arc<Vault>, PathBuf, PathBuf) {
    let wallet_path = std::env::temp_dir().join(format!(
        "wallet_{}_{}_{}.json",
        tag,
        std::process::id(),
        now_unix_secs()
    ));
    let vault_path = std::env::temp_dir().join(format!(
        "vault_{}_{}_{}.enc",
        tag,
        std::process::id(),
        now_unix_secs()
    ));
    write_wallet_json(
        &wallet_path,
        "test-key-id",
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    );
    let vault = Arc::new(Vault::open(&wallet_path, &vault_path).expect("open vault"));
    (vault, wallet_path, vault_path)
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

// ---------------------------------------------------------------------------
// NucleusDB grant management routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn nucleusdb_grants_create_list_revoke_roundtrip() {
    let (state, db_path) = test_state("ndb_grants_roundtrip");
    let grantor = "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let grantee = "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let (s1, v1) = api_post(
        state.clone(),
        "/nucleusdb/grants",
        json!({
            "grantor_puf_hex": grantor,
            "grantee_puf_hex": grantee,
            "key_pattern": "docs/*",
            "permissions": {"read": true, "write": false, "append": false},
            "expires_at": null
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "create grant should succeed: {v1}");
    assert_eq!(v1["ok"], true);
    let grant_id = v1["grant"]["grant_id_hex"]
        .as_str()
        .expect("grant_id_hex should be present")
        .to_string();

    let (s2, v2) = api_get(state.clone(), "/nucleusdb/grants?active=true").await;
    assert_eq!(s2, StatusCode::OK, "list active should succeed: {v2}");
    let grants = v2["grants"].as_array().expect("grants should be array");
    assert_eq!(grants.len(), 1, "exactly one active grant expected");
    assert_eq!(grants[0]["key_pattern"], "docs/*");
    assert_eq!(grants[0]["active"], true);

    let (s3, v3) = api_post(
        state.clone(),
        &format!("/nucleusdb/grants/{grant_id}/revoke"),
        json!({}),
    )
    .await;
    assert_eq!(s3, StatusCode::OK, "revoke should succeed: {v3}");
    assert_eq!(v3["ok"], true);
    assert_eq!(v3["grant"]["revoked"], true);

    let (s4, v4) = api_get(state.clone(), "/nucleusdb/grants?active=true").await;
    assert_eq!(s4, StatusCode::OK);
    assert_eq!(
        v4["grants"].as_array().expect("grants array").len(),
        0,
        "active list should be empty after revoke"
    );

    let (s5, v5) = api_get(state, "/nucleusdb/grants?include_revoked=true").await;
    assert_eq!(s5, StatusCode::OK);
    let all = v5["grants"].as_array().expect("grants array");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0]["revoked"], true);

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_grants_reject_invalid_hex_input() {
    let (state, db_path) = test_state("ndb_grants_badhex");
    let (status, val) = api_post(
        state,
        "/nucleusdb/grants",
        json!({
            "grantor_puf_hex": "0x1234",
            "grantee_puf_hex": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "key_pattern": "docs/*",
            "permissions": {"read": true, "write": false, "append": false},
            "expires_at": null
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "invalid hex should be rejected"
    );
    assert!(
        val["error"].is_string(),
        "error field should be present: {val}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_grants_persist_across_state_restart() {
    let (state1, db_path) = test_state("ndb_grants_restart");
    let creds2 = std::env::temp_dir().join(format!(
        "creds_ndb_grants_restart_reload_{}_{}.json",
        std::process::id(),
        now_unix_secs()
    ));

    let (s1, v1) = api_post(
        state1.clone(),
        "/nucleusdb/grants",
        json!({
            "grantor_puf_hex": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "grantee_puf_hex": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "key_pattern": "persist/*",
            "permissions": {"read": true, "write": false, "append": false},
            "expires_at": null
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "create grant should succeed: {v1}");

    let state2 = build_state(db_path.clone(), creds2);
    let (s2, v2) = api_get(state2, "/nucleusdb/grants?active=true").await;
    assert_eq!(s2, StatusCode::OK, "list after reload should succeed: {v2}");
    let grants = v2["grants"].as_array().expect("grants array");
    assert_eq!(grants.len(), 1, "grant should survive state restart");
    assert_eq!(grants[0]["key_pattern"], "persist/*");

    let grants_path = db_path.with_extension("pod_grants.json");
    let _ = std::fs::remove_file(&grants_path);
    let _ = std::fs::remove_file(&db_path);
}

// ---------------------------------------------------------------------------
// Cockpit + Deploy + Vault + Proxy routes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cockpit_session_create_list_destroy_roundtrip() {
    let (state, db_path) = test_state("cockpit_roundtrip");

    let (s1, v1) = api_post(
        state.clone(),
        "/cockpit/sessions",
        json!({"command": "/bin/bash", "args": [], "cols": 80, "rows": 24}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "create session should succeed: {v1}");
    let id = v1["id"].as_str().expect("session id").to_string();

    let (s2, v2) = api_get(state.clone(), "/cockpit/sessions").await;
    assert_eq!(s2, StatusCode::OK, "list sessions should succeed: {v2}");
    let sessions = v2["sessions"].as_array().expect("sessions array");
    assert!(sessions.iter().any(|s| s["id"] == id));

    let (s3, v3) = api_delete(state.clone(), &format!("/cockpit/sessions/{id}")).await;
    assert_eq!(s3, StatusCode::OK, "destroy session should succeed: {v3}");

    let traced = list_trace_sessions(&db_path).expect("list trace sessions");
    assert!(
        traced.iter().any(|s| s.session_id == id),
        "destroyed cockpit session should be flushed to trace DB"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn cockpit_rejects_shell_dash_c_commands() {
    let (state, db_path) = test_state("cockpit_reject_shell_c");
    let (status, val) = api_post(
        state,
        "/cockpit/sessions",
        json!({"command": "/bin/sh", "args": ["-c", "whoami"], "cols": 80, "rows": 24}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "must reject shell -c: {val}"
    );
    assert!(
        val["error"]
            .as_str()
            .unwrap_or_default()
            .contains("-c/--command"),
        "error should explain shell execution flags are disallowed: {val}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn deploy_catalog_and_preflight_shell() {
    let (state, db_path) = test_state("deploy_catalog");

    let (s1, v1) = api_get(state.clone(), "/deploy/catalog").await;
    assert_eq!(s1, StatusCode::OK);
    let agents = v1["agents"].as_array().expect("agents list");
    assert!(
        agents.iter().any(|a| a["id"] == "shell"),
        "shell agent should be present: {v1}"
    );

    let (s2, v2) = api_post(state, "/deploy/preflight", json!({"agent_id": "shell"})).await;
    assert_eq!(s2, StatusCode::OK, "shell preflight should pass: {v2}");
    assert_eq!(v2["ready"], true);

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn vault_set_list_delete_via_api() {
    let (mut state, db_path) = test_state("vault_api");
    let (vault, wallet_path, vault_path) = test_vault("vault_api");
    state.vault = Some(vault);

    let (s1, v1) = api_post(
        state.clone(),
        "/vault/keys/openai",
        json!({"key": "sk-test-123", "env_var": "OPENAI_API_KEY"}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "set key should succeed: {v1}");

    let (s2, v2) = api_get(state.clone(), "/vault/keys").await;
    assert_eq!(s2, StatusCode::OK, "list keys should succeed: {v2}");
    let keys = v2["keys"].as_array().expect("keys array");
    let openai = keys.iter().find(|k| k["provider"] == "openai").unwrap();
    assert_eq!(openai["configured"], true);

    let (s3, v3) = api_delete(state, "/vault/keys/openai").await;
    assert_eq!(s3, StatusCode::OK, "delete key should succeed: {v3}");

    let _ = std::fs::remove_file(&wallet_path);
    let _ = std::fs::remove_file(&vault_path);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn proxy_models_empty_without_vault() {
    let (state, db_path) = test_state("proxy_models");
    let has_vault = state.vault.is_some();
    let (status, val) = api_get(state, "/proxy/v1/models").await;
    assert_eq!(status, StatusCode::OK, "models route should succeed: {val}");
    assert_eq!(val["object"], "list");
    let data = val["data"].as_array().expect("data array");
    if has_vault {
        // When a vault is configured (e.g. dev machine with OpenRouter key),
        // models will be returned.  Just verify the structure.
        assert!(!data.is_empty(), "vault present: should return models");
    } else {
        assert!(data.is_empty(), "no vault: data should be empty");
    }
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn config_includes_agentpmt_endpoint_auth_and_tool_count() {
    let (state, db_path) = test_state("cfg_agentpmt_fields");
    let (status, val) = api_get(state, "/config").await;
    assert_eq!(status, StatusCode::OK, "config route should succeed: {val}");

    let pmt = &val["agentpmt"];
    assert!(
        pmt.get("endpoint").and_then(|v| v.as_str()).is_some(),
        "config.agentpmt.endpoint should be present: {val}"
    );
    assert!(
        pmt.get("auth_configured")
            .and_then(|v| v.as_bool())
            .is_some(),
        "config.agentpmt.auth_configured should be present: {val}"
    );
    assert!(
        pmt.get("tool_count").and_then(|v| v.as_u64()).is_some(),
        "config.agentpmt.tool_count should be present: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn capabilities_mcp_tool_counts_are_consistent() {
    let (state, db_path) = test_state("caps_agentpmt_counts");
    let (status, val) = api_get(state, "/capabilities").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "capabilities route should succeed: {val}"
    );

    let total = val["mcp_tools"].as_u64().expect("mcp_tools should be u64");
    let native = val["mcp_native_tools"]
        .as_u64()
        .expect("mcp_native_tools should be u64");
    let proxied = val["mcp_proxied_tools"]
        .as_u64()
        .expect("mcp_proxied_tools should be u64");
    assert_eq!(native, 18, "native MCP tool count should stay 18");
    assert_eq!(
        total,
        native + proxied,
        "mcp_tools should equal native + proxied: {val}"
    );
    assert!(
        val.get("tool_proxy_endpoint")
            .and_then(|v| v.as_str())
            .is_some(),
        "tool_proxy_endpoint should be present: {val}"
    );
    assert!(
        val.get("tool_proxy_auth_configured")
            .and_then(|v| v.as_bool())
            .is_some(),
        "tool_proxy_auth_configured should be present: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn agentpmt_tools_endpoint_returns_catalog_shape() {
    let (state, db_path) = test_state("agentpmt_tools_endpoint");
    let (status, val) = api_get(state, "/agentpmt/tools").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "agentpmt tools route should succeed: {val}"
    );

    assert!(
        val.get("enabled").and_then(|v| v.as_bool()).is_some(),
        "enabled should be present: {val}"
    );
    assert!(
        val.get("endpoint").and_then(|v| v.as_str()).is_some(),
        "endpoint should be present: {val}"
    );
    assert!(
        val.get("auth_configured")
            .and_then(|v| v.as_bool())
            .is_some(),
        "auth_configured should be present: {val}"
    );
    assert!(
        val.get("count").and_then(|v| v.as_u64()).is_some(),
        "count should be present: {val}"
    );
    assert!(
        val.get("source").and_then(|v| v.as_str()).is_some(),
        "source should be present: {val}"
    );
    assert!(
        val.get("stale").and_then(|v| v.as_bool()).is_some(),
        "stale should be present: {val}"
    );
    assert!(
        val.get("refresh_attempted")
            .and_then(|v| v.as_bool())
            .is_some(),
        "refresh_attempted should be present: {val}"
    );

    let tools = val["tools"]
        .as_array()
        .expect("tools array should be present");
    let count = val["count"].as_u64().expect("count should be numeric");
    assert_eq!(count as usize, tools.len(), "count must match tools length");
    for tool in tools {
        let name = tool["name"].as_str().expect("tool name should be string");
        assert!(
            name.starts_with("agentpmt/"),
            "proxied tool names should be prefixed: {name}"
        );
    }

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn config_includes_setup_complete_fields() {
    let (state, db_path) = test_state("config_setup_complete");
    let (status, val) = api_get(state, "/config").await;
    assert_eq!(status, StatusCode::OK, "config route should succeed: {val}");

    let sc = val
        .get("setup_complete")
        .expect("setup_complete should be present");
    assert!(
        sc.get("identity").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.identity should be a boolean: {sc}"
    );
    assert!(
        sc.get("agentpmt").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.agentpmt should be a boolean: {sc}"
    );
    assert!(
        sc.get("llm").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.llm should be a boolean: {sc}"
    );
    assert!(
        sc.get("complete").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.complete should be a boolean: {sc}"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn profile_get_and_save_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_profile_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("profile_roundtrip");
    let (s1, v1) = api_get(state.clone(), "/profile").await;
    assert_eq!(s1, StatusCode::OK, "get profile should succeed: {v1}");

    let (s2, v2) = api_post(
        state.clone(),
        "/profile",
        json!({"display_name":"Alice Test","avatar_type":"initials"}),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "save profile should succeed: {v2}");
    assert_eq!(v2["display_name"], "Alice Test");

    let (s3, v3) = api_get(state, "/profile").await;
    assert_eq!(
        s3,
        StatusCode::OK,
        "get profile after save should succeed: {v3}"
    );
    assert_eq!(v3["display_name"], "Alice Test");

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_anonymous_status_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("identity_roundtrip");
    let (s1, v1) = api_post(
        state.clone(),
        "/identity/anonymous",
        json!({"enabled":true}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "enable anonymous should succeed: {v1}");
    assert_eq!(v1["anonymous_mode"], true);

    let (s2, v2) = api_get(state.clone(), "/identity/status").await;
    assert_eq!(s2, StatusCode::OK, "identity status should succeed: {v2}");
    assert_eq!(v2["anonymous_mode"], true);
    assert_eq!(v2["identity_done"], true);

    let (s3, v3) = api_post(state, "/identity/anonymous", json!({"enabled":false})).await;
    assert_eq!(s3, StatusCode::OK, "disable anonymous should succeed: {v3}");
    assert_eq!(v3["anonymous_mode"], false);

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_device_scan_and_save_roundtrip() {
    let (state, db_path) = test_state("identity_device_roundtrip");

    let (s1, v1) = api_get(state.clone(), "/identity/device").await;
    assert_eq!(s1, StatusCode::OK, "device scan should succeed: {v1}");
    assert!(
        v1.get("components").and_then(|v| v.as_array()).is_some(),
        "components should be an array: {v1}"
    );
    assert!(
        v1.get("tier").and_then(|v| v.as_str()).is_some(),
        "tier should be present: {v1}"
    );

    let (s2, v2) = api_post(
        state.clone(),
        "/identity/device",
        json!({
            "browser_fingerprint":"browser-fp-test",
            "selected_components":[]
        }),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "device save should succeed: {v2}");
    assert!(
        v2.get("fingerprint_hex").and_then(|v| v.as_str()).is_some(),
        "fingerprint_hex should be present: {v2}"
    );

    let (s3, v3) = api_get(state, "/identity/status").await;
    assert_eq!(s3, StatusCode::OK, "identity status should succeed: {v3}");
    assert_eq!(v3["device_configured"], true);

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn config_agentpmt_setup_false_when_token_unverified() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_agentpmt_unverified_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");

    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _token_guard = EnvVarGuard::set("AGENTPMT_API_KEY", Some("fake-token-for-test"));
    let _bearer_guard = EnvVarGuard::set("AGENTPMT_BEARER_TOKEN", None);

    let cfg = agentpmt::AgentPmtConfig {
        enabled: true,
        budget_tag: None,
        mcp_endpoint: Some("http://127.0.0.1:1/mcp".to_string()),
        auth_token: None,
        updated_at: now_unix_secs(),
    };
    let cfg_path = agentpmt::agentpmt_config_path();
    agentpmt::save_config(&cfg_path, &cfg).expect("save agentpmt config");
    let _ = std::fs::remove_file(agentpmt::tool_catalog_path());

    let (state, db_path) = test_state("config_agentpmt_unverified");
    let (status, val) = api_get(state, "/config").await;
    assert_eq!(status, StatusCode::OK, "config should succeed: {val}");
    assert_eq!(
        val["setup_complete"]["agentpmt"], false,
        "agentpmt setup should fail closed when token cannot be verified: {val}"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn agentpmt_refresh_requires_auth() {
    let (state, db_path, creds_path) = test_state_unauth("agentpmt_refresh_auth");
    let (status, val) = api_post(state, "/agentpmt/refresh", json!({})).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "refresh should require auth"
    );
    assert_eq!(val["code"], "auth_required");
    assert_eq!(val["setup_route"], "#/setup");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn agentpmt_enable_requires_auth() {
    let (state, db_path, creds_path) = test_state_unauth("agentpmt_enable_auth");
    let (status, val) = api_post(state, "/agentpmt/enable", json!({})).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "enable should require auth"
    );
    assert_eq!(val["code"], "auth_required");
    assert_eq!(val["setup_route"], "#/setup");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn agentpmt_enable_sets_enabled_in_config() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_agentpmt_enable_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");

    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _token_guard = EnvVarGuard::set("AGENTPMT_API_KEY", None);
    let _bearer_guard = EnvVarGuard::set("AGENTPMT_BEARER_TOKEN", None);

    let (state, db_path) = test_state("agentpmt_enable_sets_enabled");
    let (s1, v1) = api_post(state.clone(), "/agentpmt/enable", json!({})).await;
    assert_eq!(s1, StatusCode::OK, "enable should succeed: {v1}");
    assert_eq!(v1["ok"], true);
    assert_eq!(v1["enabled"], true);

    let (s2, v2) = api_get(state, "/config").await;
    assert_eq!(s2, StatusCode::OK, "config should succeed: {v2}");
    assert_eq!(v2["agentpmt"]["enabled"], true);

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn cockpit_create_requires_auth_and_returns_setup_payload() {
    let (state, db_path, creds_path) = test_state_unauth("cockpit_auth_required");
    let (status, val) = api_post(
        state,
        "/cockpit/sessions",
        json!({"command":"/bin/bash","args":[],"agent_type":"shell"}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(val["code"], "auth_required");
    assert_eq!(val["setup_route"], "#/setup");
    assert!(val["error"]
        .as_str()
        .unwrap_or_default()
        .contains("agenthalo login"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}
