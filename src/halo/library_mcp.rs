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
