//! Tests for the dashboard API security and correctness.

use nucleusdb::dashboard::api::api_router;
use nucleusdb::dashboard::{build_state, DashboardState};
use nucleusdb::halo::agentpmt;
use nucleusdb::halo::auth::{save_credentials, Credentials};
use nucleusdb::halo::config;
use nucleusdb::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
use nucleusdb::halo::trace::{
    list_sessions as list_trace_sessions, now_unix_secs, session_events, TraceWriter,
};
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
            api_key: None,
            oauth_token: Some("test-oauth-token".to_string()),
            oauth_provider: Some("github".to_string()),
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

async fn api_get_raw(state: DashboardState, path: &str) -> (StatusCode, String) {
    let app = api_router(state.clone()).with_state(state);
    let req = Request::builder().uri(path).body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 1_000_000)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&body).to_string())
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

async fn api_post_with_headers(
    state: DashboardState,
    path: &str,
    body: Value,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    let app = api_router(state.clone()).with_state(state);
    let mut builder = Request::builder()
        .uri(path)
        .method("POST")
        .header("content-type", "application/json");
    for (k, v) in headers {
        builder = builder.header(*k, *v);
    }
    let req = builder
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

#[tokio::test]
async fn api_unknown_route_returns_json_404_payload() {
    let (state, db_path) = test_state("api_not_found_json");
    let (status, val) = api_get(state, "/this-route-does-not-exist").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(val["error"], "endpoint not found");
    assert_eq!(val["path"], "/this-route-does-not-exist");
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn api_alias_and_summary_routes_return_json() {
    let (state, db_path) = test_state("api_alias_routes");
    // Routes that should return 200 OK with JSON.
    for route in ["/trust", "/nucleusdb/commits", "/nucleusdb/sharing"] {
        let (status, val) = api_get(state.clone(), route).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "unexpected status for {route}: {val}"
        );
        assert!(val.is_object(), "route {route} must return JSON object");
    }
    // /trust is an alias for /attestations — verify backward-compat keys.
    {
        let (status, val) = api_get(state.clone(), "/trust").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(val["status"], "ok", "/trust must include status");
        assert!(
            val["attestation_count"].is_number(),
            "/trust must include attestation_count"
        );
        assert!(val["count"].is_number(), "/trust must include count");
        assert!(
            val["attestations"].is_array(),
            "/trust must include attestations"
        );
    }
    // /attestations shares the same handler — verify its contract directly.
    {
        let (status, val) = api_get(state.clone(), "/attestations").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(val["status"], "ok", "/attestations must include status");
        assert!(
            val["attestation_count"].is_number(),
            "/attestations must include attestation_count"
        );
        assert!(val["count"].is_number(), "/attestations must include count");
        assert!(
            val["attestations"].is_array(),
            "/attestations must include attestations"
        );
    }
    // Vector/proof summary routes should be available for operator introspection.
    for route in ["/nucleusdb/vectors", "/nucleusdb/proofs"] {
        let (status, val) = api_get(state.clone(), route).await;
        assert_eq!(status, StatusCode::OK, "route {route} should be 200: {val}");
        assert_eq!(val["status"], "ok");
        assert_eq!(val["endpoint"], format!("/api{route}"));
    }
    let _ = std::fs::remove_file(&db_path);
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
// P2: SQL execution is functional (not a mock)
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
        "should not have mock status field"
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
    // Hold env_lock: save_attestation and api_attestation_verify both read
    // AGENTHALO_HOME via config::attestations_dir().  Without the lock a
    // concurrent genesis test can change AGENTHALO_HOME between the attest
    // and verify steps, making the verify scan a different directory.
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_attest_verify_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _api_key_guard = EnvVarGuard::set("AGENTHALO_API_KEY", Some("test-key-attest-verify"));

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
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn attestation_verify_nonexistent_returns_not_found() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_attest_missing_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("0"));
    let _api_key_guard = EnvVarGuard::set("AGENTHALO_API_KEY", Some("test-key-attest-miss"));

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
    let _ = std::fs::remove_dir_all(&halo_home);
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

#[tokio::test]
async fn nucleusdb_memory_store_and_recall_roundtrip() {
    let (state, db_path) = test_state("ndb_memory_roundtrip");
    seed_session(&db_path, "memory-roundtrip-test");

    let (s1, v1) = api_post(
        state.clone(),
        "/nucleusdb/memory/store",
        json!({
            "text": "Vector search is cosine-based and scoped by mem:chunk prefix.",
            "source": "session:memory-roundtrip-test"
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "store memory should succeed: {v1}");
    assert_eq!(v1["ok"], true);
    assert!(v1["key"].as_str().unwrap_or("").starts_with("mem:chunk:"));
    assert_eq!(v1["sealed"], true);

    let (s2, v2) = api_post(
        state.clone(),
        "/nucleusdb/memory/recall",
        json!({
            "query": "How does memory vector similarity work?",
            "k": 5
        }),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "recall should succeed: {v2}");
    assert_eq!(v2["ok"], true);
    let results = v2["results"].as_array().expect("results array");
    assert!(!results.is_empty(), "expected at least one recall result");
    assert!(results[0]["key"]
        .as_str()
        .unwrap_or("")
        .starts_with("mem:chunk:"));

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn nucleusdb_memory_ingest_and_stats() {
    let (state, db_path) = test_state("ndb_memory_ingest_stats");
    seed_session(&db_path, "memory-ingest-test");

    let doc = "## Vector Search\nCosine distance ranks nearest embeddings.\n\n## Seal Chain\nAll writes commit, seal, and witness.";
    let (s1, v1) = api_post(
        state.clone(),
        "/nucleusdb/memory/ingest",
        json!({
            "document": doc,
            "source": "user:manual"
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "ingest should succeed: {v1}");
    assert_eq!(v1["ok"], true);
    assert!(v1["chunks"].as_u64().unwrap_or(0) >= 1);

    let (s2, v2) = api_get(state, "/nucleusdb/memory/stats").await;
    assert_eq!(s2, StatusCode::OK, "stats should succeed: {v2}");
    assert_eq!(v2["ok"], true);
    assert!(v2["total_memories"].as_u64().unwrap_or(0) >= 1);
    assert_eq!(v2["total_dims"], 768);
    assert!(v2["model"]
        .as_str()
        .unwrap_or("")
        .contains("nomic-embed-text"));

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
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_vault_set_list_delete_via_api_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

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
    let _ = std::fs::remove_dir_all(&halo_home);
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
        // A vault may exist without any configured proxy provider key.
        // If models are returned, validate shape; empty is also valid.
        for model in data {
            assert!(
                model.get("id").and_then(|v| v.as_str()).is_some(),
                "model entries should include id: {val}"
            );
            assert_eq!(model.get("object"), Some(&json!("model")));
        }
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
    assert!(
        val.get("authentication")
            .and_then(|a| a.get("required"))
            .and_then(|v| v.as_bool())
            .is_some(),
        "authentication.required should be present and boolean: {val}"
    );

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
    assert!(
        native >= 18,
        "native MCP tool count should be at least baseline 18: {val}"
    );
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
        sc.get("wallet").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.wallet should be a boolean: {sc}"
    );
    assert!(
        sc.get("agentpmt").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.agentpmt should be a boolean: {sc}"
    );
    assert_eq!(
        sc.get("wallet").and_then(|v| v.as_bool()),
        sc.get("agentpmt").and_then(|v| v.as_bool()),
        "legacy setup_complete.agentpmt should mirror wallet completion: {sc}"
    );
    assert!(
        sc.get("llm").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.llm should be a boolean: {sc}"
    );
    assert!(
        sc.get("complete").and_then(|v| v.as_bool()).is_some(),
        "setup_complete.complete should be a boolean: {sc}"
    );
    let ws = val
        .get("wallet_status")
        .expect("wallet_status should be present");
    assert!(
        ws.get("agentpmt_connected")
            .and_then(|v| v.as_bool())
            .is_some(),
        "wallet_status.agentpmt_connected should be a boolean: {ws}"
    );
    assert!(
        ws.get("agentpmt_auth_configured")
            .and_then(|v| v.as_bool())
            .is_some(),
        "wallet_status.agentpmt_auth_configured should be a boolean: {ws}"
    );
    assert!(
        ws.get("anonymous_wallet_connected")
            .and_then(|v| v.as_bool())
            .is_some(),
        "wallet_status.anonymous_wallet_connected should be a boolean: {ws}"
    );
    assert!(
        ws.get("agentaddress_connected")
            .and_then(|v| v.as_bool())
            .is_some(),
        "wallet_status.agentaddress_connected should be a boolean: {ws}"
    );
    assert!(
        ws.get("agentaddress_address").is_some(),
        "wallet_status.agentaddress_address should be present: {ws}"
    );
    assert!(
        ws.get("wdk_available").is_none(),
        "wallet_status should not expose WDK state in active setup flow: {ws}"
    );
    assert!(
        ws.get("wdk_wallet_exists").is_none(),
        "wallet_status should not expose WDK state in active setup flow: {ws}"
    );
    assert!(
        ws.get("wdk_unlocked").is_none(),
        "wallet_status should not expose WDK state in active setup flow: {ws}"
    );
    assert!(
        ws.get("wallet_complete")
            .and_then(|v| v.as_bool())
            .is_some(),
        "wallet_status.wallet_complete should be a boolean: {ws}"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn genesis_status_incomplete_when_missing() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_genesis_status_missing_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", None);

    let (state, db_path) = test_state("genesis_status_missing");
    let (status, val) = api_get(state, "/genesis/status").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "genesis status should succeed: {val}"
    );
    assert_eq!(val["completed"], false, "fresh state should be incomplete");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn genesis_harvest_success_writes_ledger_and_trace() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_genesis_success_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", Some("success"));

    let (state, db_path) = test_state("genesis_success");
    let (s1, v1) = api_post(state.clone(), "/genesis/harvest", json!({})).await;
    assert_eq!(s1, StatusCode::OK, "genesis harvest should succeed: {v1}");
    assert_eq!(v1["success"], true);
    assert_eq!(v1["completed"], true);

    let (s2, v2) = api_get(state.clone(), "/genesis/status").await;
    assert_eq!(s2, StatusCode::OK, "genesis status should succeed: {v2}");
    assert_eq!(v2["completed"], true);
    assert_eq!(
        v2["signed"].as_bool(),
        Some(true),
        "genesis ledger entries should now be signed: {v2}"
    );
    assert_eq!(
        v2["seed_stored"].as_bool(),
        Some(true),
        "genesis seed should be sealed to local encrypted storage: {v2}"
    );
    assert!(
        v2["curby_pulse_id"].as_u64().is_some(),
        "genesis status should expose CURBy pulse id: {v2}"
    );
    assert_eq!(
        v2["sources_count"].as_u64(),
        Some(4),
        "fixture success should persist 4 sources in status: {v2}"
    );
    assert!(
        v2["combined_entropy_sha256"]
            .as_str()
            .map(|s| s.starts_with("sha256:"))
            .unwrap_or(false),
        "genesis status should expose digest hash: {v2}"
    );

    let entries = nucleusdb::halo::identity_ledger::load_entries().expect("load ledger entries");
    assert!(
        entries.iter().any(|e| {
            matches!(
                e.kind,
                nucleusdb::halo::identity_ledger::IdentityLedgerKind::GenesisEntropyHarvested
            ) && e.status == "completed"
        }),
        "identity ledger should contain completed genesis entry"
    );

    let sessions = list_trace_sessions(&db_path).expect("list trace sessions");
    let genesis_session = sessions
        .iter()
        .find(|s| s.agent == "genesis")
        .expect("genesis trace session missing");
    let events = session_events(&db_path, &genesis_session.session_id).expect("session events");
    assert!(
        events
            .iter()
            .any(|e| matches!(e.event_type, EventType::GenesisHarvest)),
        "genesis trace session should include GenesisHarvest event"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn genesis_harvest_failure_records_trace_and_stays_incomplete() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_genesis_failure_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", Some("all_remote_failed"));

    let (state, db_path) = test_state("genesis_failure");
    let (s1, v1) = api_post(state.clone(), "/genesis/harvest", json!({})).await;
    assert_eq!(
        s1,
        StatusCode::BAD_GATEWAY,
        "genesis harvest should fail with bad gateway: {v1}"
    );
    assert_eq!(v1["error_code"], "ALL_REMOTE_FAILED");

    let (s2, v2) = api_get(state.clone(), "/genesis/status").await;
    assert_eq!(s2, StatusCode::OK, "genesis status should succeed: {v2}");
    assert_eq!(v2["completed"], false);

    let sessions = list_trace_sessions(&db_path).expect("list trace sessions");
    let genesis_session = sessions
        .iter()
        .find(|s| s.agent == "genesis")
        .expect("genesis trace session missing");
    let events = session_events(&db_path, &genesis_session.session_id).expect("session events");
    assert!(
        events
            .iter()
            .any(|e| matches!(e.event_type, EventType::GenesisHarvest)),
        "failed harvest should still be written to trace"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn genesis_harvest_reports_seed_read_failure_with_structured_code() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_genesis_seed_read_failure_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", Some("success"));

    let corrupted_seed_path = halo_home.join("genesis_seed.enc");
    std::fs::write(&corrupted_seed_path, vec![0xA5; 64]).expect("write corrupted seed");

    let (state, db_path) = test_state("genesis_seed_read_failure");
    let (status, val) = api_post(state, "/genesis/harvest", json!({})).await;
    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "corrupted sealed seed should return service unavailable: {val}"
    );
    assert_eq!(
        val["error_code"], "SEED_READ_FAILURE",
        "seed read/decrypt failure must be surfaced with explicit code: {val}"
    );
    assert!(
        val["message"]
            .as_str()
            .unwrap_or_default()
            .contains("sealed genesis seed"),
        "error message should explain seed read failure path: {val}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn genesis_reset_is_forbidden_by_default() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_genesis_reset_forbidden_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _reset_guard = EnvVarGuard::set("AGENTHALO_ENABLE_GENESIS_RESET", None);

    let (state, db_path) = test_state("genesis_reset_forbidden");
    let (s, v) = api_post(state, "/genesis/reset", json!({"reason":"test"})).await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "reset should be blocked by policy: {v}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn genesis_reset_is_blocked_after_completed_commit() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_genesis_reset_after_completed_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", Some("success"));
    let _reset_guard = EnvVarGuard::set("AGENTHALO_ENABLE_GENESIS_RESET", Some("1"));

    let (state, db_path) = test_state("genesis_reset_after_completed");
    let (harvest_status, harvest_val) =
        api_post(state.clone(), "/genesis/harvest", json!({})).await;
    assert_eq!(
        harvest_status,
        StatusCode::OK,
        "harvest should complete before reset policy check: {harvest_val}"
    );

    let (reset_status, reset_val) =
        api_post(state, "/genesis/reset", json!({"reason":"test"})).await;
    assert_eq!(
        reset_status,
        StatusCode::CONFLICT,
        "reset must be blocked after completed genesis commit: {reset_val}"
    );
    assert!(
        reset_val["error"]
            .as_str()
            .unwrap_or_default()
            .contains("blocked after a completed genesis commit"),
        "reset conflict should return an explicit policy reason: {reset_val}"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
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
async fn profile_rename_requires_explicit_rename_flag() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_profile_rename_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("profile_rename_requires_flag");
    let (s1, v1) = api_post(
        state.clone(),
        "/profile",
        json!({"display_name":"First Name","avatar_type":"initials"}),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "initial save should succeed: {v1}");

    let (s2, v2) = api_post(
        state.clone(),
        "/profile",
        json!({"display_name":"Second Name","avatar_type":"initials"}),
    )
    .await;
    assert_eq!(
        s2,
        StatusCode::CONFLICT,
        "rename without explicit flag should be rejected: {v2}"
    );

    let (s3, v3) = api_post(
        state,
        "/profile",
        json!({"display_name":"Second Name","avatar_type":"initials","rename":true}),
    )
    .await;
    assert_eq!(s3, StatusCode::OK, "rename with flag should succeed: {v3}");
    assert_eq!(v3["display_name"], "Second Name");
    assert_eq!(v3["name_locked"], true);
    assert_eq!(v3["name_revision"], 1);

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
async fn identity_tier_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_tier_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("identity_tier_roundtrip");
    let (s1, v1) = api_get(state.clone(), "/identity/tier").await;
    assert_eq!(s1, StatusCode::OK, "tier status should succeed: {v1}");
    assert_eq!(
        v1["default_tier"],
        nucleusdb::halo::identity::default_security_tier_str(),
        "tier endpoint should surface backend default tier: {v1}"
    );

    let (s2, v2) = api_post(
        state.clone(),
        "/identity/tier",
        json!({"tier":"max-safe","applied_by":"test"}),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "tier update should succeed: {v2}");
    assert_eq!(v2["tier"], "max-safe");

    let (s3, v3) = api_get(state.clone(), "/identity/status").await;
    assert_eq!(s3, StatusCode::OK, "identity status should succeed: {v3}");
    assert_eq!(v3["security_tier"], "max-safe");

    let (s4, v4) = api_get(state, "/identity/social").await;
    assert_eq!(s4, StatusCode::OK, "social status should succeed: {v4}");
    assert!(
        v4["ledger"]["total_entries"].as_u64().unwrap_or(0) >= 1,
        "tier update should append a ledger entry: {v4}"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_tier_rolls_back_when_ledger_append_fails() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_tier_rollback_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let bad_ledger_path = nucleusdb::halo::config::identity_social_ledger_path();
    std::fs::write(&bad_ledger_path, "{not-json}\n").expect("write corrupt ledger");

    let (state, db_path) = test_state("identity_tier_rollback");
    let (s1, v1) = api_post(state.clone(), "/identity/tier", json!({"tier":"max-safe"})).await;
    assert_eq!(
        s1,
        StatusCode::INTERNAL_SERVER_ERROR,
        "tier update should fail when ledger is corrupt: {v1}"
    );

    let (s2, v2) = api_get(state, "/identity/tier").await;
    assert_eq!(s2, StatusCode::OK, "tier status should still succeed: {v2}");
    assert_eq!(
        v2["configured"], false,
        "tier should remain unconfigured after rollback: {v2}"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_device_scan_and_save_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_device_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

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

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_network_configured_semantics_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_network_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("identity_network_roundtrip");

    let (s1, v1) = api_get(state.clone(), "/identity/status").await;
    assert_eq!(s1, StatusCode::OK, "identity status should succeed: {v1}");
    assert_eq!(
        v1["network_configured"], false,
        "network should start unconfigured"
    );

    // Saving an all-false network payload should not count as configured.
    let (s2, v2) = api_post(
        state.clone(),
        "/identity/network",
        json!({
            "share_local_ip": false,
            "share_public_ip": false,
            "share_mac": false
        }),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "network save should succeed: {v2}");

    let (s3, v3) = api_get(state.clone(), "/identity/status").await;
    assert_eq!(s3, StatusCode::OK, "identity status should succeed: {v3}");
    assert_eq!(
        v3["network_configured"], false,
        "all-false network config should remain unconfigured"
    );

    // Enabling meaningful sharing should flip configured=true.
    let (s4, v4) = api_post(
        state.clone(),
        "/identity/network",
        json!({
            "share_local_ip": true,
            "share_public_ip": false,
            "share_mac": false,
            "local_ip": "10.0.0.7"
        }),
    )
    .await;
    assert_eq!(s4, StatusCode::OK, "network save should succeed: {v4}");

    let (s5, v5) = api_get(state, "/identity/status").await;
    assert_eq!(s5, StatusCode::OK, "identity status should succeed: {v5}");
    assert_eq!(
        v5["network_configured"], true,
        "enabled network sharing should mark configured"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_social_connect_and_revoke_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_social_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("identity_social_roundtrip");
    let (s1, v1) = api_post(
        state.clone(),
        "/identity/social/connect",
        json!({
            "provider": "google",
            "token": "tok-social-test",
            "selected": true,
            "expires_in_days": 30,
            "source": "dashboard_test",
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "social connect should succeed: {v1}");
    assert_eq!(v1["ok"], true);

    let (s2, v2) = api_get(state.clone(), "/identity/social").await;
    assert_eq!(s2, StatusCode::OK, "social status should succeed: {v2}");
    assert_eq!(v2["ledger"]["chain_valid"], true);
    let providers = v2["providers"]
        .as_array()
        .expect("providers should be an array");
    assert!(
        providers.iter().any(|p| p["provider"] == "google"),
        "google provider should be present: {v2}"
    );

    let (s3, v3) = api_post(
        state,
        "/identity/social/revoke",
        json!({"provider":"google","reason":"test_revoke"}),
    )
    .await;
    assert_eq!(s3, StatusCode::OK, "social revoke should succeed: {v3}");
    assert_eq!(v3["ok"], true);

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_super_secure_update_roundtrip() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_super_secure_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("identity_super_secure_roundtrip");
    let (s1, v1) = api_post(
        state.clone(),
        "/identity/super-secure",
        json!({
            "option": "totp",
            "enabled": true,
            "metadata": {"label":"Test Authenticator"}
        }),
    )
    .await;
    assert_eq!(
        s1,
        StatusCode::OK,
        "super secure update should succeed: {v1}"
    );
    assert_eq!(v1["ok"], true);
    assert_eq!(v1["state"]["totp_enabled"], true);

    let (s2, v2) = api_get(state.clone(), "/identity/super-secure").await;
    assert_eq!(
        s2,
        StatusCode::OK,
        "super secure status should succeed: {v2}"
    );
    assert_eq!(v2["totp_enabled"], true);
    assert_eq!(v2["totp_label"], "Test Authenticator");

    let (s3, v3) = api_get(state, "/identity/social").await;
    assert_eq!(
        s3,
        StatusCode::OK,
        "social status should still succeed after super secure update: {v3}"
    );
    assert!(
        v3["ledger"]["total_entries"].as_u64().unwrap_or(0) >= 1,
        "super-secure update should append a ledger entry: {v3}"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn identity_pod_share_filters_by_pattern() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_pod_share_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("identity_pod_share_pattern");
    let (_sp, _sv) = api_post(
        state.clone(),
        "/profile",
        json!({"display_name":"PodUser","avatar_type":"initials"}),
    )
    .await;
    let (s1, v1) = api_post(
        state,
        "/identity/pod-share",
        json!({
            "key_patterns": ["identity/profile/*"],
            "include_ledger": false
        }),
    )
    .await;
    assert_eq!(
        s1,
        StatusCode::OK,
        "identity pod share should succeed: {v1}"
    );
    assert!(v1["ok"].as_bool().unwrap_or(false));
    let records = v1["records"]
        .as_array()
        .expect("records array should exist");
    assert!(!records.is_empty(), "expected at least one profile record");
    assert!(
        records.iter().all(|r| {
            r.get("key")
                .and_then(|k| k.as_str())
                .map(|k| k.starts_with("identity/profile/"))
                .unwrap_or(false)
        }),
        "records should be filtered to identity/profile/*: {v1}"
    );
    assert!(
        v1.get("proof_envelope").is_some(),
        "identity pod share should include proof envelope: {v1}"
    );
    assert_eq!(
        v1["proof_verification"]["accepted"].as_bool(),
        Some(true),
        "proof envelope should verify: {v1}"
    );
    assert_eq!(
        v1["proof_verification"]["signature_present"].as_bool(),
        Some(v1["proof_envelope"]["signature"].is_object()),
        "signature presence should match proof envelope signature material: {v1}"
    );
    if !v1["proof_envelope"]["signature"].is_object() {
        assert!(
            v1["proof_verification"]["signature_valid"].is_null(),
            "unsigned envelope should report signature_valid=null: {v1}"
        );
    }

    let _ = std::fs::remove_dir_all(&halo_home);
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

#[cfg(unix)]
#[tokio::test]
async fn ensure_halo_dir_enforces_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_halo_dir_perms_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    nucleusdb::halo::config::ensure_halo_dir().expect("ensure halo dir");
    let mode = std::fs::metadata(&halo_home)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700, "halo dir should be owner-only");

    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn agentpmt_refresh_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
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
async fn identity_ledger_migrate_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("identity_ledger_migrate_auth");
    let (s, _v) = api_post(
        state,
        "/identity/ledger/migrate-legacy-signatures",
        json!({}),
    )
    .await;
    assert!(
        s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN,
        "unauthenticated migration should be denied"
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn identity_ledger_migrate_returns_ok_for_authenticated_operator() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_identity_ledger_migrate_ok_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    nucleusdb::halo::pq::keygen_pq(false).expect("create pq wallet");

    let (state, db_path) = test_state("identity_ledger_migrate_ok");
    let (s, v) = api_post(
        state,
        "/identity/ledger/migrate-legacy-signatures",
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "migration endpoint should succeed: {v}");
    assert_eq!(v["ok"], true);
    assert_eq!(v["updated_entries"], 0);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&halo_home);
}

#[tokio::test]
async fn agentpmt_enable_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
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
async fn wdk_status_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("wdk_status_auth");
    let (status, val) = api_get(state, "/wdk/status").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(val["code"], "auth_required");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn wdk_available_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("wdk_available_auth");
    let (status, val) = api_get(state, "/wdk/available").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(val["code"], "auth_required");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn agents_list_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("agents_list_auth");
    let (status, val) = api_get(state, "/agents/list").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(val["code"], "auth_required");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn agentaddress_status_and_chains_routes_work() {
    let (state, db_path) = test_state("agentaddress_status");
    let (status_a, val_a) = api_get(state.clone(), "/agentaddress/status").await;
    assert_eq!(
        status_a,
        StatusCode::OK,
        "agentaddress status should be available: {val_a}"
    );
    assert!(
        val_a["connected"].as_bool().is_some(),
        "connected should be bool: {val_a}"
    );

    let (status_c, val_c) = api_get(state, "/agentaddress/chains").await;
    assert_eq!(
        status_c,
        StatusCode::OK,
        "agentaddress chains should be available: {val_c}"
    );
    assert!(
        val_c["chains"].is_array(),
        "chains should be an array: {val_c}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn agentaddress_generate_requires_auth() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("agentaddress_generate_auth");
    let (status, val) = api_post(state, "/agentaddress/generate", json!({})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(val["code"], "auth_required");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn agentaddress_generate_genesis_requires_seed() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_agentaddress_genesis_requires_seed_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", None);

    let (state, db_path) = test_state("agentaddress_genesis_requires_seed");
    let (status, val) = api_post(
        state,
        "/agentaddress/generate",
        json!({"source":"genesis","persist_public_address":true}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::PRECONDITION_FAILED,
        "genesis source should require completed seed: {val}"
    );
    assert!(
        val["error"]
            .as_str()
            .unwrap_or_default()
            .contains("genesis seed not available"),
        "error should describe missing genesis precondition: {val}"
    );
    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn agentaddress_generate_genesis_is_deterministic_and_persists_source() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_agentaddress_genesis_deterministic_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );
    let _fixture_guard = EnvVarGuard::set("AGENTHALO_GENESIS_TEST_MODE", Some("success"));

    let (state, db_path) = test_state("agentaddress_genesis_deterministic");
    let (harvest_status, harvest_val) =
        api_post(state.clone(), "/genesis/harvest", json!({})).await;
    assert_eq!(
        harvest_status,
        StatusCode::OK,
        "genesis harvest should succeed before wallet derivation: {harvest_val}"
    );

    let (s1, v1) = api_post(
        state.clone(),
        "/agentaddress/generate",
        json!({"source":"genesis","persist_public_address":true}),
    )
    .await;
    assert_eq!(
        s1,
        StatusCode::OK,
        "first genesis generate should succeed: {v1}"
    );
    let addr1 = v1["data"]["evmAddress"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert!(
        addr1.starts_with("0x") && addr1.len() == 42,
        "first generated address should be EVM-shaped: {v1}"
    );
    assert_eq!(v1["source"], "genesis");

    let (_sd, _vd) = api_post(state.clone(), "/agentaddress/disconnect", json!({})).await;
    let (s2, v2) = api_post(
        state.clone(),
        "/agentaddress/generate",
        json!({"source":"genesis","persist_public_address":true}),
    )
    .await;
    assert_eq!(
        s2,
        StatusCode::OK,
        "second genesis generate should succeed: {v2}"
    );
    let addr2 = v2["data"]["evmAddress"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    assert_eq!(
        addr1, addr2,
        "genesis-derived address should be deterministic for fixed seed"
    );

    let (ss, sv) = api_get(state, "/agentaddress/status").await;
    assert_eq!(
        ss,
        StatusCode::OK,
        "agentaddress status should succeed: {sv}"
    );
    assert_eq!(sv["source"], "genesis_derived");

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn agentaddress_generate_genesis_requires_wallet_scope_when_locked() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_agentaddress_genesis_scope_lock_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("agentaddress_genesis_scope_lock");
    let (create_status, create_val) = api_post(
        state.clone(),
        "/crypto/create-password",
        json!({
            "password": "CorrectHorseBatteryStaple123!",
            "confirm": "CorrectHorseBatteryStaple123!"
        }),
    )
    .await;
    assert_eq!(
        create_status,
        StatusCode::OK,
        "password creation should succeed: {create_val}"
    );
    let (_lock_status, _lock_val) = api_post(state.clone(), "/crypto/lock", json!({})).await;

    let (status, val) = api_post(
        state,
        "/agentaddress/generate",
        json!({"source":"genesis","persist_public_address":true}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::LOCKED,
        "locked session should require wallet scope: {val}"
    );
    assert_eq!(val["code"], "crypto_locked");

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn wdk_import_rejects_invalid_bip39_seed() {
    let (state, db_path) = test_state("wdk_import_invalid_seed");
    let (status, val) = api_post(
        state,
        "/wdk/import",
        json!({
            "seed": "apple banana cherry dog elephant fish grape house igloo jelly kite lemon",
            "passphrase": "testpass123"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "invalid mnemonic should be rejected: {val}"
    );
    assert!(
        val["error"].as_str().unwrap_or_default().contains("BIP-39"),
        "error should mention BIP-39 validity: {val}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn wdk_create_requires_genesis_seed() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_wdk_create_requires_genesis_seed_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("wdk_create_requires_genesis_seed");
    let (status, val) = api_post(
        state,
        "/wdk/create",
        json!({
            "passphrase": "testpass123"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::PRECONDITION_FAILED,
        "wallet create should fail when genesis seed is unavailable: {val}"
    );
    assert!(
        val["error"]
            .as_str()
            .unwrap_or_default()
            .contains("genesis seed not available"),
        "error should indicate missing genesis seed precondition: {val}"
    );
    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn crypto_unlock_rejects_wrong_password_after_creation() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_crypto_unlock_wrong_password_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("crypto_unlock_wrong_password");
    let (s1, v1) = api_post(
        state.clone(),
        "/crypto/create-password",
        json!({
            "password": "CorrectHorseBatteryStaple123!",
            "confirm": "CorrectHorseBatteryStaple123!"
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "create password should succeed: {v1}");

    let (s2, v2) = api_post(state.clone(), "/crypto/lock", json!({})).await;
    assert_eq!(s2, StatusCode::OK, "lock should succeed: {v2}");

    let (s3, v3) = api_post(
        state,
        "/crypto/unlock",
        json!({"password": "wrong-password"}),
    )
    .await;
    assert_eq!(
        s3,
        StatusCode::UNAUTHORIZED,
        "unlock with wrong password must fail: {v3}"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn crypto_change_password_rejects_wrong_current_password() {
    let _guard = env_lock().lock().expect("lock env");
    let halo_home = std::env::temp_dir().join(format!(
        "dashboard_test_crypto_change_password_wrong_current_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&halo_home);
    std::fs::create_dir_all(&halo_home).expect("create temp halo home");
    let _home_guard = EnvVarGuard::set(
        "AGENTHALO_HOME",
        Some(halo_home.to_str().expect("temp home utf8 path")),
    );

    let (state, db_path) = test_state("crypto_change_password_wrong_current");
    let (s1, v1) = api_post(
        state.clone(),
        "/crypto/create-password",
        json!({
            "password": "CorrectHorseBatteryStaple123!",
            "confirm": "CorrectHorseBatteryStaple123!"
        }),
    )
    .await;
    assert_eq!(s1, StatusCode::OK, "create password should succeed: {v1}");

    let (s2, v2) = api_post(
        state,
        "/crypto/change-password",
        json!({
            "current_password": "incorrect-current",
            "new_password": "EvenStrongerPass123!",
            "confirm": "EvenStrongerPass123!"
        }),
    )
    .await;
    assert_eq!(
        s2,
        StatusCode::UNAUTHORIZED,
        "change-password with wrong current password must fail: {v2}"
    );

    let _ = std::fs::remove_dir_all(&halo_home);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn wdk_send_rejects_invalid_chain_before_sidecar_lookup() {
    let (state, db_path) = test_state("wdk_send_invalid_chain");
    let (status, val) = api_post(
        state,
        "/wdk/send",
        json!({
            "chain": "dogecoin",
            "to": "D123456789",
            "amount": "1000"
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "unsupported chain should fail: {val}"
    );
    assert!(
        val["error"]
            .as_str()
            .unwrap_or_default()
            .contains("unsupported chain"),
        "should return chain validation message: {val}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn auth_set_key_route_removed() {
    let (state, db_path, creds_path) = test_state_unauth("auth_set_key_removed");
    let (status, _body) = api_post(state, "/auth/set-key", json!({"api_key":"unused"})).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn auth_oauth_start_returns_bridge_url_for_supported_provider() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("auth_oauth_start");
    let (status, val) = api_get(state, "/auth/oauth/start/github?expires_in_minutes=5").await;
    assert_eq!(status, StatusCode::OK, "oauth start should succeed: {val}");
    assert_eq!(val["ok"], true);
    assert_eq!(val["provider"], "github");
    let oauth_url = val["oauth_url"].as_str().unwrap_or_default();
    assert!(
        oauth_url.starts_with("https://agenthalo.dev/auth/github?"),
        "oauth url should target bridge: {oauth_url}"
    );
    assert!(
        oauth_url.contains("state="),
        "oauth url should include signed state: {oauth_url}"
    );
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn auth_oauth_callback_persists_credentials() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("auth_oauth_callback");
    let (start_status, start_val) = api_get(
        state.clone(),
        "/auth/oauth/start/github?expires_in_minutes=5",
    )
    .await;
    assert_eq!(
        start_status,
        StatusCode::OK,
        "oauth start should succeed: {start_val}"
    );
    let oauth_url = start_val["oauth_url"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let state_param = oauth_url
        .split("state=")
        .nth(1)
        .map(|s| s.split('&').next().unwrap_or_default().to_string())
        .unwrap_or_default();
    assert!(
        !state_param.is_empty(),
        "state param should be present in oauth_url"
    );

    let callback_path = format!(
        "/auth/oauth/callback?provider=github&state={state}&token=test-oauth-token-123",
        state = state_param
    );
    let (cb_status, cb_body) = api_get_raw(state.clone(), &callback_path).await;
    assert_eq!(
        cb_status,
        StatusCode::OK,
        "oauth callback should return HTML success"
    );
    assert!(
        cb_body.contains("agenthalo-auth-oauth"),
        "callback page should postMessage back to opener"
    );

    let (cfg_status, cfg_val) = api_get(state, "/config").await;
    assert_eq!(
        cfg_status,
        StatusCode::OK,
        "config should load after oauth callback: {cfg_val}"
    );
    assert_eq!(
        cfg_val["authentication"]["authenticated"], true,
        "oauth callback should mark dashboard authenticated: {cfg_val}"
    );

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
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
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
    assert_eq!(val["next_steps"][0], "Open Setup");
    assert!(val["error"]
        .as_str()
        .unwrap_or_default()
        .contains("GitHub or Google"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn funding_webhook_missing_signature_rejected() {
    let _guard = env_lock().lock().expect("lock env");
    let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("funding-webhook-secret"));
    let _simulation_guard = EnvVarGuard::set("AGENTHALO_ONCHAIN_SIMULATION", Some("1"));

    let (state, db_path) = test_state("funding_webhook_missing_sig");
    let (created_status, created) = api_post(
        state.clone(),
        "/admin/keys",
        json!({"label":"Webhook Key","initial_balance_usd":0.0}),
    )
    .await;
    assert_eq!(
        created_status,
        StatusCode::OK,
        "key creation failed: {created}"
    );
    let key_id = created["key"]["key_id"]
        .as_str()
        .or_else(|| created["key"]["id"].as_str())
        .expect("key id")
        .to_string();

    let (status, body) = api_post(
        state,
        &format!("/admin/keys/{key_id}/fund"),
        json!({
            "source":{
                "type":"agentpmt_tokens",
                "receipt_id":"rcpt_missing_sig",
                "amount_usd": 10.0,
                "signature":"deadbeef"
            }
        }),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "expected 401: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("X-AgentPMT-Signature"),
        "missing header message expected: {body}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn funding_webhook_invalid_signature_rejected() {
    let _guard = env_lock().lock().expect("lock env");
    let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("funding-webhook-secret"));
    let _simulation_guard = EnvVarGuard::set("AGENTHALO_ONCHAIN_SIMULATION", Some("1"));

    let (state, db_path) = test_state("funding_webhook_invalid_sig");
    let (created_status, created) = api_post(
        state.clone(),
        "/admin/keys",
        json!({"label":"Webhook Key 2","initial_balance_usd":0.0}),
    )
    .await;
    assert_eq!(
        created_status,
        StatusCode::OK,
        "key creation failed: {created}"
    );
    let key_id = created["key"]["key_id"]
        .as_str()
        .or_else(|| created["key"]["id"].as_str())
        .expect("key id")
        .to_string();

    let source = json!({
        "type":"x402_direct",
        "transaction_hash":"0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
        "amount_base_units": 1_000_000u64,
        "network":"base"
    });
    let request_body = json!({ "source": source });
    let (status, body) = api_post_with_headers(
        state,
        &format!("/admin/keys/{key_id}/fund"),
        request_body,
        &[("x-agentpmt-signature", "deadbeef")],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "expected 401: {body}");
    assert!(
        body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("webhook signature verification failed"),
        "invalid signature message expected: {body}"
    );
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn funding_operator_credit_bypasses_webhook_signature() {
    let _guard = env_lock().lock().expect("lock env");
    let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("funding-webhook-secret"));

    let (state, db_path) = test_state("funding_operator_credit_bypass_sig");
    let (created_status, created) = api_post(
        state.clone(),
        "/admin/keys",
        json!({"label":"Operator Credit Key","initial_balance_usd":0.0}),
    )
    .await;
    assert_eq!(
        created_status,
        StatusCode::OK,
        "key creation failed: {created}"
    );
    let key_id = created["key"]["key_id"]
        .as_str()
        .or_else(|| created["key"]["id"].as_str())
        .expect("key id")
        .to_string();

    let (status, body) = api_post(
        state,
        &format!("/admin/keys/{key_id}/fund"),
        json!({
            "source": {
                "type":"operator_credit",
                "reason":"manual adjustment"
            },
            "amount_usd": 5.0
        }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "operator funding should pass: {body}"
    );
    assert_eq!(body["ok"], true);
    assert_eq!(body["funded_usd"], 5.0);
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn api_addons_route_can_toggle_p2pclaw() {
    let _guard = env_lock().lock().expect("lock env");
    let home = std::env::temp_dir().join(format!(
        "dashboard_addons_toggle_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let (state, db_path) = test_state("api_addons_toggle");
    let (s1, v1) = api_get(state.clone(), "/addons").await;
    assert_eq!(s1, StatusCode::OK, "addons get failed: {v1}");
    assert_eq!(v1["ok"], true);

    let (s2, v2) = api_post(
        state.clone(),
        "/addons",
        json!({"name":"p2pclaw","enabled":true}),
    )
    .await;
    assert_eq!(s2, StatusCode::OK, "addons post failed: {v2}");
    assert_eq!(v2["ok"], true);
    assert_eq!(v2["addons"]["p2pclaw_enabled"], true);

    let (s3, v3) = api_get(state, "/addons").await;
    assert_eq!(s3, StatusCode::OK);
    assert_eq!(v3["addons"]["p2pclaw_enabled"], true);

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn api_p2pclaw_configure_persists_config_and_vault_secret() {
    let _guard = env_lock().lock().expect("lock env");
    let home = std::env::temp_dir().join(format!(
        "dashboard_p2pclaw_cfg_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    write_wallet_json(
        &config::pq_wallet_path(),
        "dashboard-p2pclaw-key",
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    );

    let (state, db_path) = test_state("api_p2pclaw_configure");
    let (status, body) = api_post(
        state.clone(),
        "/p2pclaw/configure",
        json!({
            "endpoint_url":"http://localhost:3000",
            "agent_id":"agenthalo-alice",
            "agent_name":"Alice",
            "auth_secret":"vault-secret",
            "tier":"tier2"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "configure failed: {body}");
    assert_eq!(body["ok"], true);
    assert_eq!(body["auth_in_vault"], true);
    assert_eq!(body["config"]["endpoint_url"], "http://localhost:3000");
    assert_eq!(body["config"]["tier"], "tier2");

    let cfg_raw = std::fs::read_to_string(config::p2pclaw_config_path()).expect("read config");
    assert!(
        !cfg_raw.contains("vault-secret"),
        "plaintext secret must not be written to p2pclaw config"
    );

    let (vault_path, vault_file) = (config::pq_wallet_path(), config::vault_path());
    let vault = Vault::open(&vault_path, &vault_file).expect("open vault");
    let secret = vault
        .get_key("p2pclaw_auth")
        .expect("p2pclaw secret must be in vault");
    assert_eq!(secret, "vault-secret");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn api_p2pclaw_status_requires_authentication() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("api_p2pclaw_status_auth");
    let (status, body) = api_get(state, "/p2pclaw/status").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "p2pclaw status must require authentication: {body}"
    );
    assert_eq!(body["code"], "auth_required");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}

#[tokio::test]
async fn api_p2pclaw_briefing_requires_authentication() {
    let _guard = env_lock().lock().expect("lock env");
    let _auth_guard = EnvVarGuard::set("AGENTHALO_REQUIRE_DASHBOARD_AUTH", Some("1"));
    let (state, db_path, creds_path) = test_state_unauth("api_p2pclaw_briefing_auth");
    let (status, body) = api_get(state, "/p2pclaw/briefing").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "p2pclaw briefing must require authentication: {body}"
    );
    assert_eq!(body["code"], "auth_required");

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&creds_path);
}
