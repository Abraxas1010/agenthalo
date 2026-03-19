//! Library MCP tools — read-only query surface for agents.
//!
//! These functions are called by the MCP server to give agents
//! access to the persistent Library.

use crate::halo::library;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

// ── Response types (required for MCP output schema) ─────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySearchResponse {
    pub results: Vec<LibrarySearchResult>,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySearchResult {
    pub key: String,
    pub preview: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibraryBrowseResponse {
    pub records: Vec<LibraryBrowseRecord>,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibraryBrowseRecord {
    pub key: String,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySessionLookupResponse {
    pub found: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<library::LibrarySession>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySessionsResponse {
    pub sessions: Vec<crate::halo::schema::SessionMetadata>,
    pub count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibraryStatusResponse {
    pub initialized: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<library::LibraryStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySearchRequest {
    /// Search query (full-text, whitespace-separated terms).
    pub query: String,
    /// Maximum number of results to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibraryBrowseRequest {
    /// Key prefix to browse. Use "lib:session:" for sessions, "lib:evt:" for events.
    #[serde(default = "default_browse_prefix")]
    pub prefix: String,
    /// Maximum records to return.
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Number of records to skip.
    #[serde(default)]
    pub offset: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySqlRequest {
    /// SQL query (only SELECT is allowed).
    pub sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LibrarySessionLookupRequest {
    /// Session ID to look up.
    pub session_id: String,
}

fn default_limit() -> usize {
    20
}
fn default_browse_prefix() -> String {
    "lib:".to_string()
}

/// MCP tool: library_search — full-text search across Library records.
pub fn tool_search(req: LibrarySearchRequest) -> Result<LibrarySearchResponse, String> {
    if !library::library_exists() {
        return Ok(LibrarySearchResponse {
            results: Vec::new(),
            count: 0,
            message: Some("Library not initialized. Run `agenthalo library push-all` to populate.".to_string()),
        });
    }
    let raw = library::search(&req.query, req.limit)?;
    let results: Vec<LibrarySearchResult> = raw
        .iter()
        .map(|(key, value, score)| LibrarySearchResult {
            key: key.clone(),
            preview: if value.len() > 500 { format!("{}...", &value[..500]) } else { value.clone() },
            score: *score,
        })
        .collect();
    let count = results.len();
    Ok(LibrarySearchResponse { results, count, message: None })
}

/// MCP tool: library_browse — browse Library records by key prefix.
pub fn tool_browse(req: LibraryBrowseRequest) -> Result<LibraryBrowseResponse, String> {
    if !library::library_exists() {
        return Ok(LibraryBrowseResponse { records: Vec::new(), count: 0, message: Some("Library not initialized.".to_string()) });
    }
    let raw = library::browse(&req.prefix, req.limit, req.offset)?;
    let records: Vec<LibraryBrowseRecord> = raw
        .iter()
        .map(|(key, value)| LibraryBrowseRecord {
            key: key.clone(),
            preview: if value.len() > 500 { format!("{}...", &value[..500]) } else { value.clone() },
        })
        .collect();
    let count = records.len();
    Ok(LibraryBrowseResponse { records, count, message: None })
}

/// MCP tool: library_sql — read-only SQL queries against the Library.
pub fn tool_sql(req: LibrarySqlRequest) -> Result<Value, String> {
    let sql = req.sql.trim();
    // Enforce read-only.
    let first_word = sql.split_whitespace().next().unwrap_or("").to_uppercase();
    if first_word != "SELECT" {
        return Err(
            "Library SQL is read-only. Only SELECT statements are allowed. \
             To add data, use the push protocol (session end, dashboard push button, \
             or `agenthalo library push`)."
                .to_string(),
        );
    }
    if !library::library_exists() {
        return Ok(json!({ "rows": [], "message": "Library not initialized." }));
    }
    // Use browse as a simplified SQL substitute — real SQL requires the
    // NucleusDB SQL executor which needs a mutable reference. For read-only
    // queries, we parse the WHERE clause for key patterns.
    // For now, return a helpful message pointing to browse/search.
    Ok(json!({
        "message": "SQL interface delegates to Library browse/search. Use library_search for full-text or library_browse for key prefix queries.",
        "hint": "library_search for content queries, library_browse for key-based navigation"
    }))
}

/// MCP tool: library_session_lookup — look up a specific session.
pub fn tool_session_lookup(req: LibrarySessionLookupRequest) -> Result<LibrarySessionLookupResponse, String> {
    if !library::library_exists() {
        return Ok(LibrarySessionLookupResponse { found: false, session: None, message: Some("Library not initialized.".to_string()) });
    }
    match library::session_lookup(&req.session_id)? {
        Some(session) => Ok(LibrarySessionLookupResponse { found: true, session: Some(session), message: None }),
        None => Ok(LibrarySessionLookupResponse { found: false, session: None, message: Some("Session not found in Library.".to_string()) }),
    }
}

/// MCP tool: library_sessions — list all sessions in the Library.
pub fn tool_sessions() -> Result<LibrarySessionsResponse, String> {
    if !library::library_exists() {
        return Ok(LibrarySessionsResponse { sessions: Vec::new(), count: 0, message: Some("Library not initialized.".to_string()) });
    }
    let sessions = library::list_sessions()?;
    let count = sessions.len();
    Ok(LibrarySessionsResponse { sessions, count, message: None })
}

/// MCP tool: library_status — Library health and stats.
pub fn tool_status() -> Result<LibraryStatusResponse, String> {
    if !library::library_exists() {
        return Ok(LibraryStatusResponse { initialized: false, stats: None });
    }
    let stats = library::stats()?;
    Ok(LibraryStatusResponse { initialized: true, stats: Some(stats) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::library;
    use crate::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
    use crate::halo::trace::TraceWriter;
    use crate::test_support::{lock_env, EnvVarGuard};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_ts() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    /// Set up a temp AGENTHALO_HOME, initialize the library, and push a test session.
    fn with_populated_library<F: FnOnce()>(f: F) {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", Some(dir.path().to_str().unwrap()));
        library::ensure_library().expect("ensure library");

        // Create a traces DB with one session.
        let traces_path = dir.path().join("traces.ndb");
        let mut writer = TraceWriter::new(&traces_path).expect("writer");
        writer
            .start_session(SessionMetadata {
                session_id: "mcp-test-sess".to_string(),
                agent: "claude".to_string(),
                model: Some("opus".to_string()),
                started_at: now_ts(),
                ended_at: None,
                prompt: Some("library mcp test".to_string()),
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })
            .expect("start session");
        writer
            .write_event(TraceEvent {
                seq: 0,
                timestamp: now_ts(),
                event_type: EventType::PromptSent,
                content: serde_json::json!({"prompt": "search for eigenform lemma"}),
                input_tokens: Some(15),
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })
            .expect("write event");
        writer.end_session(SessionStatus::Completed).expect("end");
        library::push_session(&traces_path, "mcp-test-sess").expect("push");

        f();
    }

    fn with_empty_library<F: FnOnce()>(f: F) {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", Some(dir.path().to_str().unwrap()));
        // Do NOT initialize — library does not exist.
        f();
    }

    // ── tool_status ──────────────────────────────────────────────────

    #[test]
    fn status_uninitialized_reports_false() {
        with_empty_library(|| {
            let resp = tool_status().expect("status");
            assert!(!resp.initialized);
            assert!(resp.stats.is_none());
        });
    }

    #[test]
    fn status_initialized_reports_stats() {
        with_populated_library(|| {
            let resp = tool_status().expect("status");
            assert!(resp.initialized);
            let stats = resp.stats.expect("stats present");
            assert!(stats.total_keys > 0);
            assert_eq!(stats.total_sessions, 1);
        });
    }

    // ── tool_search ──────────────────────────────────────────────────

    #[test]
    fn search_uninitialized_returns_empty_with_message() {
        with_empty_library(|| {
            let resp = tool_search(LibrarySearchRequest {
                query: "eigenform".to_string(),
                limit: 10,
            })
            .expect("search");
            assert_eq!(resp.count, 0);
            assert!(resp.message.is_some());
            assert!(resp.message.unwrap().contains("not initialized"));
        });
    }

    #[test]
    fn search_finds_pushed_content() {
        with_populated_library(|| {
            let resp = tool_search(LibrarySearchRequest {
                query: "eigenform".to_string(),
                limit: 10,
            })
            .expect("search");
            assert!(resp.count > 0, "should find 'eigenform' in pushed event");
            assert!(!resp.results.is_empty());
            assert!(resp.results[0].score > 0.0);
        });
    }

    #[test]
    fn search_no_match_returns_empty() {
        with_populated_library(|| {
            let resp = tool_search(LibrarySearchRequest {
                query: "zyxwvunonexistent".to_string(),
                limit: 10,
            })
            .expect("search");
            assert_eq!(resp.count, 0);
        });
    }

    // ── tool_browse ──────────────────────────────────────────────────

    #[test]
    fn browse_uninitialized_returns_empty() {
        with_empty_library(|| {
            let resp = tool_browse(LibraryBrowseRequest {
                prefix: "lib:".to_string(),
                limit: 10,
                offset: 0,
            })
            .expect("browse");
            assert_eq!(resp.count, 0);
            assert!(resp.message.is_some());
        });
    }

    #[test]
    fn browse_returns_session_records() {
        with_populated_library(|| {
            let resp = tool_browse(LibraryBrowseRequest {
                prefix: "lib:session:".to_string(),
                limit: 10,
                offset: 0,
            })
            .expect("browse");
            assert!(resp.count > 0, "should find session records");
        });
    }

    // ── tool_sql ─────────────────────────────────────────────────────

    #[test]
    fn sql_rejects_non_select() {
        let err = tool_sql(LibrarySqlRequest {
            sql: "DELETE FROM library".to_string(),
        });
        assert!(err.is_err());
        assert!(err.unwrap_err().contains("read-only"));
    }

    #[test]
    fn sql_select_uninitialized_returns_message() {
        with_empty_library(|| {
            let resp = tool_sql(LibrarySqlRequest {
                sql: "SELECT * FROM sessions".to_string(),
            })
            .expect("sql");
            assert!(resp.get("message").is_some());
        });
    }

    #[test]
    fn sql_select_initialized_returns_hint() {
        with_populated_library(|| {
            let resp = tool_sql(LibrarySqlRequest {
                sql: "SELECT * FROM sessions".to_string(),
            })
            .expect("sql");
            assert!(resp.get("hint").is_some());
        });
    }

    // ── tool_session_lookup ──────────────────────────────────────────

    #[test]
    fn session_lookup_uninitialized() {
        with_empty_library(|| {
            let resp = tool_session_lookup(LibrarySessionLookupRequest {
                session_id: "nonexistent".to_string(),
            })
            .expect("lookup");
            assert!(!resp.found);
            assert!(resp.message.is_some());
        });
    }

    #[test]
    fn session_lookup_finds_pushed_session() {
        with_populated_library(|| {
            let resp = tool_session_lookup(LibrarySessionLookupRequest {
                session_id: "mcp-test-sess".to_string(),
            })
            .expect("lookup");
            assert!(resp.found);
            let session = resp.session.expect("session present");
            assert_eq!(session.metadata.agent, "claude");
        });
    }

    #[test]
    fn session_lookup_missing_returns_not_found() {
        with_populated_library(|| {
            let resp = tool_session_lookup(LibrarySessionLookupRequest {
                session_id: "does-not-exist".to_string(),
            })
            .expect("lookup");
            assert!(!resp.found);
            assert!(resp.session.is_none());
        });
    }

    // ── tool_sessions ────────────────────────────────────────────────

    #[test]
    fn sessions_uninitialized_returns_empty() {
        with_empty_library(|| {
            let resp = tool_sessions().expect("sessions");
            assert_eq!(resp.count, 0);
            assert!(resp.message.is_some());
        });
    }

    #[test]
    fn sessions_lists_pushed() {
        with_populated_library(|| {
            let resp = tool_sessions().expect("sessions");
            assert_eq!(resp.count, 1);
            assert_eq!(resp.sessions[0].session_id, "mcp-test-sess");
        });
    }

    // ── preview truncation ───────────────────────────────────────────

    #[test]
    fn search_result_preview_truncated_at_500() {
        with_populated_library(|| {
            // The search results truncate values > 500 chars.
            let resp = tool_search(LibrarySearchRequest {
                query: "claude".to_string(),
                limit: 10,
            })
            .expect("search");
            for result in &resp.results {
                assert!(
                    result.preview.len() <= 503, // 500 + "..."
                    "preview should be truncated: len={}",
                    result.preview.len()
                );
            }
        });
    }
}
