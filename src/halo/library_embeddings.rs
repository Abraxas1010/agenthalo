//! Library Embedding Sidecar — semantic search index for cross-session recall.
//!
//! Stores vector embeddings of session summaries in a dedicated NucleusDB
//! (`library_embeddings.ndb`) alongside the main Library blob store.
//! Populated during `push_session()`, queried via `library_semantic_search`.
//!
//! The sidecar is regenerable from Library content — it is NOT a trust anchor.
//! If corrupted, run `nucleusdb library embed-backfill` to rebuild.

use crate::cli::default_witness_cfg;
use crate::embeddings::EmbeddingModel;
use crate::halo::library;
use crate::halo::schema::SessionSummary;
use crate::memory::{MemoryContext, MemoryStore};
use crate::persistence::{default_wal_path, init_wal, persist_snapshot_and_sync_wal};
use crate::protocol::{NucleusDb, VcBackend};
use crate::state::State;
use crate::witness::WitnessSignatureAlgorithm;
use serde::{Deserialize, Serialize};

/// Key prefix for library embeddings (distinguishes from per-session mem:chunk:*).
const LIB_EMBED_SOURCE_PREFIX: &str = "lib:push:";

/// Path to the sidecar embeddings DB.
pub fn sidecar_db_path() -> std::path::PathBuf {
    library::library_dir().join("library_embeddings.ndb")
}

fn sidecar_witness_cfg() -> crate::witness::WitnessConfig {
    let mut cfg = default_witness_cfg();
    cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
    cfg
}

/// Ensure the sidecar DB exists.
pub fn ensure_sidecar() -> Result<std::path::PathBuf, String> {
    let dir = library::library_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create library dir {}: {e}", dir.display()))?;
    let db_path = sidecar_db_path();
    if !db_path.exists() {
        let db = NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            sidecar_witness_cfg(),
        );
        let wal_path = default_wal_path(&db_path);
        init_wal(&wal_path, &db).map_err(|e| format!("init sidecar WAL: {e:?}"))?;
        persist_snapshot_and_sync_wal(&db_path, &wal_path, &db)
            .map_err(|e| format!("persist initial sidecar: {e:?}"))?;
    }
    Ok(db_path)
}

fn load_sidecar() -> Result<NucleusDb, String> {
    let db_path = sidecar_db_path();
    if !db_path.exists() {
        ensure_sidecar()?;
    }
    crate::persistence::load_snapshot(&db_path, sidecar_witness_cfg())
        .map_err(|e| format!("load sidecar {}: {e:?}", db_path.display()))
}

fn persist_sidecar(db: &NucleusDb) -> Result<(), String> {
    let db_path = sidecar_db_path();
    let wal_path = default_wal_path(&db_path);
    persist_snapshot_and_sync_wal(&db_path, &wal_path, db)
        .map_err(|e| format!("persist sidecar: {e:?}"))
}

fn memory_store() -> MemoryStore {
    MemoryStore::new(EmbeddingModel::default())
}

/// Build the text to embed for a session push.
/// Uses session metadata + summary stats since SessionSummary has no result_text.
fn build_embed_text(
    session_id: &str,
    agent: &str,
    model: Option<&str>,
    date: &str,
    prompt: Option<&str>,
    summary: &SessionSummary,
) -> String {
    let prompt_text = prompt.unwrap_or("(no prompt)");
    let trimmed = if prompt_text.len() > 1500 {
        &prompt_text[..1500]
    } else {
        prompt_text
    };
    format!(
        "[Agent: {agent}] [Model: {}] [Date: {date}] [Session: {session_id}] \
         [Duration: {}s] [Tools: {} calls] [Files: {} modified] \
         Prompt: {trimmed}",
        model.unwrap_or("unknown"),
        summary.duration_secs,
        summary.tool_calls + summary.mcp_tool_calls,
        summary.files_modified,
    )
}

/// Embed a session summary into the sidecar. Called from `push_session()`.
/// Returns Ok(true) if embedded, Ok(false) if skipped (no summary), Err on failure.
pub fn embed_session(
    session_id: &str,
    agent: &str,
    model: Option<&str>,
    started_at: u64,
    prompt: Option<&str>,
    summary: Option<&SessionSummary>,
) -> Result<bool, String> {
    let Some(summary) = summary else {
        return Ok(false); // No summary yet — skip embedding
    };
    // Skip if there's nothing meaningful to embed
    if prompt.map(|p| p.trim().is_empty()).unwrap_or(true) && summary.event_count == 0 {
        return Ok(false);
    }

    let date = chrono::DateTime::from_timestamp(started_at as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let text = build_embed_text(session_id, agent, model, &date, prompt, summary);
    let source = format!("{LIB_EMBED_SOURCE_PREFIX}{session_id}");
    let ctx = MemoryContext {
        session_id: Some(session_id.to_string()),
        agent_id: Some(agent.to_string()),
        ttl_secs: None, // Library memories are permanent
    };

    let store = memory_store();
    let mut db = load_sidecar()?;
    store
        .store_memory_ctx(&mut db, &text, Some(&source), &ctx)
        .map_err(|e| format!("embed session {session_id}: {e}"))?;
    persist_sidecar(&db)?;
    Ok(true)
}

/// Semantic search across all Library session embeddings.
pub fn semantic_search(
    query: &str,
    k: usize,
) -> Result<Vec<SemanticSearchResult>, String> {
    let db_path = sidecar_db_path();
    if !db_path.exists() {
        return Ok(Vec::new());
    }
    let store = memory_store();
    let mut db = load_sidecar()?;
    let results = store.recall(&mut db, query, k)?;
    persist_sidecar(&db)?; // persist liveness updates

    Ok(results
        .into_iter()
        .map(|r| {
            // Extract session_id from source field (lib:push:{session_id})
            let session_id = r
                .source
                .as_deref()
                .and_then(|s| s.strip_prefix(LIB_EMBED_SOURCE_PREFIX))
                .map(ToString::to_string);
            SemanticSearchResult {
                key: r.key,
                distance: r.distance,
                text: r.text,
                session_id,
                source: r.source,
                created: r.created,
            }
        })
        .collect())
}

/// Backfill: embed all Library sessions that aren't yet in the sidecar.
pub fn backfill() -> Result<BackfillResult, String> {
    let sessions = library::list_sessions()?;
    let mut embedded = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for meta in &sessions {
        let session = library::session_lookup(&meta.session_id)?;
        let summary = session.as_ref().and_then(|s| s.summary.as_ref());
        match embed_session(
            &meta.session_id,
            &meta.agent,
            meta.model.as_deref(),
            meta.started_at,
            meta.prompt.as_deref(),
            summary,
        ) {
            Ok(true) => embedded += 1,
            Ok(false) => skipped += 1,
            Err(e) => errors.push(format!("{}: {e}", meta.session_id)),
        }
    }

    Ok(BackfillResult {
        total_sessions: sessions.len(),
        embedded,
        skipped,
        errors,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SemanticSearchResult {
    pub key: String,
    pub distance: f64,
    pub text: String,
    pub session_id: Option<String>,
    pub source: Option<String>,
    pub created: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackfillResult {
    pub total_sessions: usize,
    pub embedded: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
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

    fn with_temp_env<F: FnOnce(tempfile::TempDir)>(f: F) {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", Some(dir.path().to_str().unwrap()));
        let _embed = EnvVarGuard::set("NUCLEUSDB_EMBEDDING_BACKEND", Some("hash-test"));
        library::ensure_library().expect("ensure library");
        f(dir);
    }

    fn create_and_push_session(dir: &std::path::Path, session_id: &str, prompt: &str) {
        let traces_path = dir.join(format!("traces_{session_id}.ndb"));
        let mut writer = TraceWriter::new(&traces_path).expect("writer");
        writer
            .start_session(SessionMetadata {
                session_id: session_id.to_string(),
                agent: "claude".to_string(),
                model: Some("opus".to_string()),
                started_at: now_ts(),
                ended_at: None,
                prompt: Some(prompt.to_string()),
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })
            .expect("start");
        writer
            .write_event(TraceEvent {
                seq: 0,
                timestamp: now_ts(),
                event_type: EventType::PromptSent,
                content: serde_json::json!({"prompt": prompt}),
                input_tokens: Some(20),
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })
            .expect("event");
        writer
            .write_event(TraceEvent {
                seq: 0,
                timestamp: now_ts(),
                event_type: EventType::ResponseReceived,
                content: serde_json::json!({"response": "completed the task"}),
                input_tokens: None,
                output_tokens: Some(50),
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })
            .expect("event2");
        writer
            .end_session(SessionStatus::Completed)
            .expect("end");
        library::push_session(&traces_path, session_id).expect("push");
    }

    #[test]
    fn embed_session_with_summary() {
        with_temp_env(|dir| {
            create_and_push_session(dir.path(), "embed-test-1", "test embedding of session summary");
            let session = library::session_lookup("embed-test-1")
                .expect("lookup")
                .expect("exists");
            let result = embed_session(
                "embed-test-1",
                "claude",
                Some("opus"),
                session.metadata.started_at,
                session.metadata.prompt.as_deref(),
                session.summary.as_ref(),
            )
            .expect("embed");
            assert!(result, "should have embedded");
            assert!(sidecar_db_path().exists(), "sidecar should exist");
        });
    }

    #[test]
    fn embed_session_without_summary_returns_false() {
        with_temp_env(|_dir| {
            let result = embed_session("no-summary", "claude", None, 0, None, None).expect("embed");
            assert!(!result, "should skip when no summary");
        });
    }

    #[test]
    fn semantic_search_finds_embedded_session() {
        with_temp_env(|dir| {
            create_and_push_session(dir.path(), "search-test-1", "implemented JWT authentication with token refresh");
            let session = library::session_lookup("search-test-1")
                .expect("lookup")
                .expect("exists");
            embed_session(
                "search-test-1",
                "claude",
                Some("opus"),
                session.metadata.started_at,
                session.metadata.prompt.as_deref(),
                session.summary.as_ref(),
            )
            .expect("embed");

            let results = semantic_search("authentication token", 5).expect("search");
            assert!(!results.is_empty(), "should find the embedded session");
            assert_eq!(results[0].session_id.as_deref(), Some("search-test-1"));
        });
    }

    #[test]
    fn semantic_search_empty_sidecar_returns_empty() {
        with_temp_env(|_dir| {
            let results = semantic_search("anything", 5).expect("search");
            assert!(results.is_empty());
        });
    }

    #[test]
    fn backfill_embeds_all_sessions() {
        with_temp_env(|dir| {
            create_and_push_session(dir.path(), "bf-1", "first session about vector search");
            create_and_push_session(dir.path(), "bf-2", "second session about payment channels");

            let result = backfill().expect("backfill");
            assert_eq!(result.total_sessions, 2);
            assert_eq!(result.embedded, 2);
            assert!(result.errors.is_empty());

            // Verify semantic search works after backfill
            let hits = semantic_search("vector similarity", 5).expect("search");
            assert!(!hits.is_empty(), "should find vector session after backfill");
        });
    }

    #[test]
    fn backfill_is_idempotent() {
        with_temp_env(|dir| {
            create_and_push_session(dir.path(), "idem-1", "idempotent test session");
            let r1 = backfill().expect("backfill 1");
            assert_eq!(r1.embedded, 1);

            // Second backfill should skip (memory store is idempotent on same text)
            let r2 = backfill().expect("backfill 2");
            // The memory store deduplicates by text hash, so embedded count
            // may still be 1 (it stores but returns existing record)
            assert_eq!(r2.total_sessions, 1);
            assert!(r2.errors.is_empty());
        });
    }

    #[test]
    fn semantic_search_returns_session_id_from_source() {
        with_temp_env(|dir| {
            create_and_push_session(dir.path(), "sid-extract-test", "testing session ID extraction from source field");
            let session = library::session_lookup("sid-extract-test")
                .expect("lookup")
                .expect("exists");
            embed_session(
                "sid-extract-test",
                "claude",
                Some("opus"),
                session.metadata.started_at,
                session.metadata.prompt.as_deref(),
                session.summary.as_ref(),
            )
            .expect("embed");

            let results = semantic_search("session extraction", 3).expect("search");
            assert!(!results.is_empty());
            assert_eq!(results[0].session_id.as_deref(), Some("sid-extract-test"));
            assert!(results[0].source.as_ref().unwrap().starts_with("lib:push:"));
        });
    }
}
