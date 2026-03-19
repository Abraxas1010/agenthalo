//! Persistent Library — system-wide NucleusDB for accumulated agent knowledge.
//!
//! The Library lives at `~/.agenthalo/library/library.ndb` and receives pushed
//! deltas from individual agent sessions. Agents query the Library read-only
//! via MCP tools. Writes happen only through the push protocol.

use crate::cli::default_witness_cfg;
use crate::halo::config;
use crate::halo::schema::{SessionMetadata, SessionSummary, TraceEvent};
use crate::halo::trace;
use crate::persistence::{default_wal_path, init_wal, load_snapshot, persist_snapshot_and_sync_wal};
use crate::protocol::NucleusDb;
use crate::state::{Delta, State};
use crate::witness::{WitnessConfig, WitnessSignatureAlgorithm};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Key prefixes ────────────────────────────────────────────────────

const LIB_SESSION_PREFIX: &str = "lib:session:";
const LIB_SUMMARY_PREFIX: &str = "lib:summary:";
const LIB_EVENT_PREFIX: &str = "lib:evt:";
const LIB_AGENT_IDX: &str = "lib:idx:agent:";
const LIB_DATE_IDX: &str = "lib:idx:date:";
const LIB_MODEL_IDX: &str = "lib:idx:model:";
const LIB_WATERMARK_PREFIX: &str = "lib:push:watermark:";
const LIB_COST_DAILY: &str = "lib:cost:daily:";
const LIB_COST_MONTHLY: &str = "lib:cost:monthly:";

// ── Public types ────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushResult {
    pub session_id: String,
    pub events_pushed: u64,
    pub watermark_before: u64,
    pub watermark_after: u64,
    pub pushed_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushLogEntry {
    pub session_id: String,
    pub events_pushed: u64,
    pub watermark_before: u64,
    pub watermark_after: u64,
    pub pushed_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibraryStats {
    pub total_keys: usize,
    pub total_sessions: usize,
    pub hot_size_bytes: u64,
    pub library_path: String,
    pub push_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LibrarySession {
    pub metadata: SessionMetadata,
    pub summary: Option<SessionSummary>,
    pub event_count: u64,
}

// ── Paths ───────────────────────────────────────────────────────────

pub fn library_dir() -> PathBuf {
    config::halo_dir().join("library")
}

pub fn library_db_path() -> PathBuf {
    library_dir().join("library.ndb")
}

pub fn library_push_log_path() -> PathBuf {
    library_dir().join("push_log.jsonl")
}

pub fn library_config_path() -> PathBuf {
    library_dir().join("library_config.json")
}

pub fn library_exists() -> bool {
    library_db_path().exists()
}

// ── Library initialization ──────────────────────────────────────────

fn library_witness_cfg() -> WitnessConfig {
    let mut cfg = default_witness_cfg();
    cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
    cfg
}

/// Ensure the library directory and empty DB exist.
pub fn ensure_library() -> Result<PathBuf, String> {
    let dir = library_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create library dir {}: {e}", dir.display()))?;
    let db_path = library_db_path();
    if !db_path.exists() {
        let db = NucleusDb::new(
            State::new(vec![]),
            crate::protocol::VcBackend::BinaryMerkle,
            library_witness_cfg(),
        );
        let wal_path = default_wal_path(&db_path);
        init_wal(&wal_path, &db)
            .map_err(|e| format!("init library WAL: {e:?}"))?;
        persist_snapshot_and_sync_wal(&db_path, &wal_path, &db)
            .map_err(|e| format!("persist initial library: {e:?}"))?;
    }
    Ok(db_path)
}

fn load_library() -> Result<NucleusDb, String> {
    let db_path = library_db_path();
    if !db_path.exists() {
        ensure_library()?;
    }
    load_snapshot(&db_path, library_witness_cfg())
        .map_err(|e| format!("load library {}: {e:?}", db_path.display()))
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Blob helpers (reuse trace.rs pattern) ───────────────────────────

fn bytes_to_u64_chunks(bytes: &[u8]) -> Vec<u64> {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(8));
    for chunk in bytes.chunks(8) {
        let mut arr = [0u8; 8];
        arr[..chunk.len()].copy_from_slice(chunk);
        out.push(u64::from_le_bytes(arr));
    }
    out
}

fn u64_chunks_to_bytes(chunks: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunks.len() * 8);
    for chunk in chunks {
        out.extend_from_slice(&chunk.to_le_bytes());
    }
    out
}

fn append_blob_writes(base_key: &str, payload: &[u8], writes: &mut Vec<(String, u64)>) {
    writes.push((format!("{base_key}:len"), payload.len() as u64));
    let chunks = bytes_to_u64_chunks(payload);
    for (idx, chunk) in chunks.iter().enumerate() {
        writes.push((format!("{base_key}:chunk:{idx}"), *chunk));
    }
}

fn read_blob(db: &NucleusDb, base_key: &str) -> Result<Option<Vec<u8>>, String> {
    let len_key = format!("{base_key}:len");
    let Some(len) = read_val(db, &len_key) else {
        return Ok(None);
    };
    let len = len as usize;
    let chunk_count = len.div_ceil(8);
    let mut chunks = Vec::with_capacity(chunk_count);
    for idx in 0..chunk_count {
        let key = format!("{base_key}:chunk:{idx}");
        let chunk = read_val(db, &key)
            .ok_or_else(|| format!("missing library chunk {key}"))?;
        chunks.push(chunk);
    }
    let mut bytes = u64_chunks_to_bytes(&chunks);
    bytes.truncate(len);
    Ok(Some(bytes))
}

fn read_val(db: &NucleusDb, key: &str) -> Option<u64> {
    let idx = db.keymap.get(key)?;
    db.state.values.get(idx).copied()
}

fn commit_writes(db: &mut NucleusDb, writes: Vec<(String, u64)>) -> Result<(), String> {
    if writes.is_empty() {
        return Ok(());
    }
    let delta = writes
        .into_iter()
        .map(|(key, value)| {
            let idx = db.keymap.get_or_create(&key);
            (idx, value)
        })
        .collect();
    db.commit(Delta::new(delta), &[])
        .map_err(|e| format!("commit to library: {e:?}"))?;
    Ok(())
}

fn persist_library(db: &NucleusDb) -> Result<(), String> {
    let db_path = library_db_path();
    let wal_path = default_wal_path(&db_path);
    persist_snapshot_and_sync_wal(&db_path, &wal_path, db)
        .map_err(|e| format!("persist library: {e:?}"))
}

// ── Push protocol ───────────────────────────────────────────────────

/// Push new session data from a traces.ndb into the Library.
/// Only pushes events with seq > last watermark. Idempotent.
pub fn push_session(traces_db_path: &Path, session_id: &str) -> Result<PushResult, String> {
    let mut lib = load_library()?;
    let watermark_key = format!("{LIB_WATERMARK_PREFIX}{session_id}");
    let watermark_before = read_val(&lib, &watermark_key).unwrap_or(0);

    // Load session data from traces DB.
    let meta = trace::list_sessions(traces_db_path)?
        .into_iter()
        .find(|m| m.session_id == session_id);
    let Some(meta) = meta else {
        return Err(format!("session '{session_id}' not found in {}", traces_db_path.display()));
    };
    let summary = trace::session_summary(traces_db_path, session_id)?;
    let events = trace::session_events(traces_db_path, session_id)?;

    // Filter to events not yet pushed.
    let new_events: Vec<&TraceEvent> = events
        .iter()
        .filter(|e| e.seq > watermark_before)
        .collect();

    let events_pushed = new_events.len() as u64;
    let watermark_after = new_events
        .iter()
        .map(|e| e.seq)
        .max()
        .unwrap_or(watermark_before);

    // Build writes.
    let mut writes = Vec::new();

    // Session metadata (always overwrite with latest).
    let meta_base = format!("{LIB_SESSION_PREFIX}{session_id}");
    let meta_raw = serde_json::to_vec(&meta)
        .map_err(|e| format!("serialize session metadata: {e}"))?;
    append_blob_writes(&meta_base, &meta_raw, &mut writes);

    // Summary (always overwrite with latest).
    if let Some(ref sum) = summary {
        let sum_base = format!("{LIB_SUMMARY_PREFIX}{session_id}");
        let sum_raw = serde_json::to_vec(sum)
            .map_err(|e| format!("serialize session summary: {e}"))?;
        append_blob_writes(&sum_base, &sum_raw, &mut writes);
    }

    // Index entries.
    writes.push((
        format!("{LIB_AGENT_IDX}{}:{}:{}", meta.agent, meta.started_at, session_id),
        1,
    ));
    let date = day_label(meta.started_at);
    writes.push((
        format!("{LIB_DATE_IDX}{date}:{}:{}", meta.started_at, session_id),
        1,
    ));
    if let Some(ref model) = meta.model {
        writes.push((
            format!("{LIB_MODEL_IDX}{model}:{}:{}", meta.started_at, session_id),
            1,
        ));
    }

    // New events.
    for event in &new_events {
        let evt_base = format!("{LIB_EVENT_PREFIX}{session_id}:{}", event.seq);
        let evt_raw = serde_json::to_vec(event)
            .map_err(|e| format!("serialize event: {e}"))?;
        append_blob_writes(&evt_base, &evt_raw, &mut writes);
    }

    // Update watermark.
    writes.push((watermark_key, watermark_after));

    // Cost aggregation (from summary if available).
    if let Some(ref sum) = summary {
        let day = day_label(meta.started_at);
        let month = month_label(meta.started_at);
        // Only aggregate on first push (watermark_before == 0).
        if watermark_before == 0 {
            append_cost_writes(&lib, &mut writes, &format!("{LIB_COST_DAILY}{day}"), sum);
            append_cost_writes(&lib, &mut writes, &format!("{LIB_COST_MONTHLY}{month}"), sum);
        }
    }

    commit_writes(&mut lib, writes)?;
    persist_library(&lib)?;

    let pushed_at = now_unix();
    let result = PushResult {
        session_id: session_id.to_string(),
        events_pushed,
        watermark_before,
        watermark_after,
        pushed_at,
    };

    // Append to push log.
    append_push_log(&result);

    Ok(result)
}

fn append_cost_writes(
    db: &NucleusDb,
    writes: &mut Vec<(String, u64)>,
    prefix: &str,
    summary: &SessionSummary,
) {
    let sessions = read_val(db, &format!("{prefix}:sessions")).unwrap_or(0) + 1;
    let input = read_val(db, &format!("{prefix}:input_tokens")).unwrap_or(0)
        + summary.total_input_tokens;
    let output = read_val(db, &format!("{prefix}:output_tokens")).unwrap_or(0)
        + summary.total_output_tokens;
    let cost_x10000 = read_val(db, &format!("{prefix}:cost_x10000")).unwrap_or(0)
        + (summary.estimated_cost_usd * 10_000.0) as u64;

    writes.push((format!("{prefix}:sessions"), sessions));
    writes.push((format!("{prefix}:input_tokens"), input));
    writes.push((format!("{prefix}:output_tokens"), output));
    writes.push((format!("{prefix}:cost_x10000"), cost_x10000));
}

fn append_push_log(result: &PushResult) {
    let log_path = library_push_log_path();
    let entry = PushLogEntry {
        session_id: result.session_id.clone(),
        events_pushed: result.events_pushed,
        watermark_before: result.watermark_before,
        watermark_after: result.watermark_after,
        pushed_at: result.pushed_at,
    };
    if let Ok(line) = serde_json::to_string(&entry) {
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{line}")
            });
    }
}

// ── Query interface ─────────────────────────────────────────────────

/// Browse Library records by key prefix.
pub fn browse(prefix: &str, limit: usize, offset: usize) -> Result<Vec<(String, String)>, String> {
    let db = load_library()?;
    let mut matches: Vec<(String, String)> = Vec::new();
    for (key, _idx) in db.keymap.all_keys() {
        if key.starts_with(prefix) && key.ends_with(":len") {
            let base = key.trim_end_matches(":len");
            if let Ok(Some(bytes)) = read_blob(&db, base) {
                let value = String::from_utf8_lossy(&bytes).to_string();
                matches.push((base.to_string(), value));
            }
        }
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(matches.into_iter().skip(offset).take(limit).collect())
}

/// Full-text search across Library records.
pub fn search(query: &str, limit: usize) -> Result<Vec<(String, String, f64)>, String> {
    let db = load_library()?;
    let query_lower = query.to_lowercase();
    let terms: Vec<&str> = query_lower.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut results: Vec<(String, String, f64)> = Vec::new();
    for (key, _idx) in db.keymap.all_keys() {
        if !key.ends_with(":len") {
            continue;
        }
        let base = key.trim_end_matches(":len");
        if let Ok(Some(bytes)) = read_blob(&db, base) {
            let value = String::from_utf8_lossy(&bytes);
            let value_lower = value.to_lowercase();
            let matched = terms.iter().filter(|t| value_lower.contains(**t)).count();
            if matched > 0 {
                let score = matched as f64 / terms.len() as f64;
                results.push((base.to_string(), value.to_string(), score));
            }
        }
    }
    results.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(limit);
    Ok(results)
}

/// Look up a specific session from the Library.
pub fn session_lookup(session_id: &str) -> Result<Option<LibrarySession>, String> {
    let db = load_library()?;
    let meta_base = format!("{LIB_SESSION_PREFIX}{session_id}");
    let Some(meta_bytes) = read_blob(&db, &meta_base)? else {
        return Ok(None);
    };
    let metadata: SessionMetadata = serde_json::from_slice(&meta_bytes)
        .map_err(|e| format!("parse library session metadata: {e}"))?;

    let sum_base = format!("{LIB_SUMMARY_PREFIX}{session_id}");
    let summary = read_blob(&db, &sum_base)?
        .and_then(|bytes| serde_json::from_slice::<SessionSummary>(&bytes).ok());

    // Count events.
    let evt_prefix = format!("{LIB_EVENT_PREFIX}{session_id}:");
    let event_count = db
        .keymap
        .all_keys()
        .filter(|(k, _)| k.starts_with(&evt_prefix) && k.ends_with(":len"))
        .count() as u64;

    Ok(Some(LibrarySession {
        metadata,
        summary,
        event_count,
    }))
}

/// List all sessions in the Library.
pub fn list_sessions() -> Result<Vec<SessionMetadata>, String> {
    let db = load_library()?;
    let mut out = Vec::new();
    for (key, _) in db.keymap.all_keys() {
        if key.starts_with(LIB_SESSION_PREFIX) && key.ends_with(":len") {
            let base = key.trim_end_matches(":len");
            if let Ok(Some(bytes)) = read_blob(&db, base) {
                if let Ok(meta) = serde_json::from_slice::<SessionMetadata>(&bytes) {
                    out.push(meta);
                }
            }
        }
    }
    out.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    Ok(out)
}

/// Get Library statistics.
pub fn stats() -> Result<LibraryStats, String> {
    let db = load_library()?;
    let total_keys = db.keymap.all_keys().count();
    let total_sessions = db
        .keymap
        .all_keys()
        .filter(|(k, _)| k.starts_with(LIB_SESSION_PREFIX) && k.ends_with(":len"))
        .count();
    let db_path = library_db_path();
    let hot_size_bytes = std::fs::metadata(&db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let push_log = library_push_log_path();
    let push_count = if push_log.exists() {
        std::fs::read_to_string(&push_log)
            .map(|s| s.lines().count())
            .unwrap_or(0)
    } else {
        0
    };

    Ok(LibraryStats {
        total_keys,
        total_sessions,
        hot_size_bytes,
        library_path: db_path.display().to_string(),
        push_count,
    })
}

/// Read the push log.
pub fn push_log() -> Result<Vec<PushLogEntry>, String> {
    let path = library_push_log_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read push log: {e}"))?;
    let mut entries = Vec::new();
    for line in raw.lines() {
        if let Ok(entry) = serde_json::from_str::<PushLogEntry>(line) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Push all sessions from a traces DB to the Library.
pub fn push_all_sessions(traces_db_path: &Path) -> Result<Vec<PushResult>, String> {
    let sessions = trace::list_sessions(traces_db_path)?;
    let mut results = Vec::new();
    for meta in sessions {
        match push_session(traces_db_path, &meta.session_id) {
            Ok(r) => results.push(r),
            Err(e) => {
                eprintln!("library push for {}: {e}", meta.session_id);
            }
        }
    }
    Ok(results)
}

// ── Date helpers ────────────────────────────────────────────────────

fn day_label(ts: u64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "1970-01-01".to_string())
}

fn month_label(ts: u64) -> String {
    chrono::DateTime::from_timestamp(ts as i64, 0)
        .map(|dt| dt.format("%Y-%m").to_string())
        .unwrap_or_else(|| "1970-01".to_string())
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::schema::SessionStatus;
    use crate::test_support::{lock_env, EnvVarGuard};

    fn with_temp_library<F: FnOnce()>(f: F) {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", Some(dir.path().to_str().unwrap()));
        ensure_library().expect("ensure library");
        f();
    }

    fn create_test_traces_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("test_traces.ndb");
        let mut writer = trace::TraceWriter::new(&db_path).expect("writer");
        writer
            .start_session(SessionMetadata {
                session_id: "test-sess-1".to_string(),
                agent: "claude".to_string(),
                model: Some("opus".to_string()),
                started_at: now_unix(),
                ended_at: None,
                prompt: Some("test prompt".to_string()),
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })
            .expect("start session");
        writer
            .write_event(TraceEvent {
                seq: 0,
                timestamp: now_unix(),
                event_type: crate::halo::schema::EventType::PromptSent,
                content: serde_json::json!({"prompt": "hello world"}),
                input_tokens: Some(10),
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })
            .expect("write event");
        writer
            .write_event(TraceEvent {
                seq: 0,
                timestamp: now_unix(),
                event_type: crate::halo::schema::EventType::ResponseReceived,
                content: serde_json::json!({"response": "hi"}),
                input_tokens: None,
                output_tokens: Some(5),
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })
            .expect("write event 2");
        writer
            .end_session(SessionStatus::Completed)
            .expect("end session");
        (dir, db_path)
    }

    #[test]
    fn ensure_library_creates_db() {
        with_temp_library(|| {
            assert!(library_db_path().exists());
        });
    }

    #[test]
    fn push_session_and_lookup() {
        with_temp_library(|| {
            let (_dir, traces_path) = create_test_traces_db();
            let result = push_session(&traces_path, "test-sess-1").expect("push");
            assert_eq!(result.session_id, "test-sess-1");
            assert!(result.events_pushed > 0);
            assert_eq!(result.watermark_before, 0);
            assert!(result.watermark_after > 0);

            // Lookup.
            let session = session_lookup("test-sess-1")
                .expect("lookup")
                .expect("session exists");
            assert_eq!(session.metadata.agent, "claude");
            assert!(session.summary.is_some());
            assert!(session.event_count > 0);
        });
    }

    #[test]
    fn push_is_idempotent() {
        with_temp_library(|| {
            let (_dir, traces_path) = create_test_traces_db();
            let r1 = push_session(&traces_path, "test-sess-1").expect("push 1");
            let r2 = push_session(&traces_path, "test-sess-1").expect("push 2");
            assert!(r1.events_pushed > 0);
            assert_eq!(r2.events_pushed, 0); // No new events.
            assert_eq!(r1.watermark_after, r2.watermark_before);
        });
    }

    #[test]
    fn search_finds_pushed_content() {
        with_temp_library(|| {
            let (_dir, traces_path) = create_test_traces_db();
            push_session(&traces_path, "test-sess-1").expect("push");

            let results = search("hello world", 10).expect("search");
            assert!(!results.is_empty(), "search should find 'hello world'");
        });
    }

    #[test]
    fn browse_lists_sessions() {
        with_temp_library(|| {
            let (_dir, traces_path) = create_test_traces_db();
            push_session(&traces_path, "test-sess-1").expect("push");

            let results = browse("lib:session:", 50, 0).expect("browse");
            assert!(!results.is_empty());
        });
    }

    #[test]
    fn stats_reports_correctly() {
        with_temp_library(|| {
            let (_dir, traces_path) = create_test_traces_db();
            push_session(&traces_path, "test-sess-1").expect("push");

            let s = stats().expect("stats");
            assert_eq!(s.total_sessions, 1);
            assert!(s.total_keys > 0);
            assert!(s.push_count > 0);
        });
    }

    #[test]
    fn list_sessions_after_push() {
        with_temp_library(|| {
            let (_dir, traces_path) = create_test_traces_db();
            push_session(&traces_path, "test-sess-1").expect("push");

            let sessions = list_sessions().expect("list");
            assert_eq!(sessions.len(), 1);
            assert_eq!(sessions[0].session_id, "test-sess-1");
        });
    }
}
