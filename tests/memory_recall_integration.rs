use nucleusdb::mcp::tools::{
    MemoryIngestRequest, MemoryRecallRequest, MemoryStoreRequest, NucleusDbMcpService,
};
use nucleusdb::memory::{MemoryStore, MEMORY_KEY_PREFIX};
use nucleusdb::protocol::{NucleusDb, VcBackend};
use nucleusdb::state::State;

use rmcp::handler::server::wrapper::Parameters;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

struct HashBackendGuard {
    _guard: MutexGuard<'static, ()>,
    previous: Option<String>,
}

impl Drop for HashBackendGuard {
    fn drop(&mut self) {
        match self.previous.take() {
            Some(v) => {
                // SAFETY: test-only env mutation is serialized by the guard lock.
                unsafe { std::env::set_var("AGENTHALO_EMBEDDING_BACKEND", v) };
            }
            None => {
                // SAFETY: test-only env mutation is serialized by the guard lock.
                unsafe { std::env::remove_var("AGENTHALO_EMBEDDING_BACKEND") };
            }
        }
    }
}

fn enable_hash_backend() -> HashBackendGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let guard = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("lock env");
    let previous = std::env::var("AGENTHALO_EMBEDDING_BACKEND").ok();
    // SAFETY: test-only env mutation is serialized by env_lock().
    unsafe { std::env::set_var("AGENTHALO_EMBEDDING_BACKEND", "hash-test") };
    HashBackendGuard {
        _guard: guard,
        previous,
    }
}

fn test_store() -> MemoryStore {
    MemoryStore::new(
        nucleusdb::embeddings::EmbeddingModel::new_hash_test_backend(
            nucleusdb::embeddings::DEFAULT_MODEL_NAME,
            nucleusdb::embeddings::DEFAULT_EMBEDDING_DIMS,
        ),
    )
}

fn temp_db_path(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "memory_recall_integration_{tag}_{}_{}.ndb",
        std::process::id(),
        nanos
    ))
}

fn cleanup_db_files(db_path: &Path) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(PathBuf::from(format!("{}.wal", db_path.to_string_lossy())));
}

fn test_db() -> NucleusDb {
    let mut cfg = nucleusdb::cli::default_witness_cfg();
    cfg.signing_algorithm = nucleusdb::witness::WitnessSignatureAlgorithm::MlDsa65;
    NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, cfg)
}

#[test]
fn test_store_and_recall_roundtrip() {
    let mut db = test_db();
    let memory = test_store();
    memory
        .store_memory(
            &mut db,
            "NucleusDB vector search uses cosine distance by default.",
            Some("session:roundtrip"),
        )
        .expect("store");
    let hits = memory
        .recall(&db, "find vector similarity defaults", 5)
        .expect("recall");
    assert!(!hits.is_empty());
    assert!(hits[0].key.starts_with(MEMORY_KEY_PREFIX));
}

#[test]
fn test_memory_ingest_document() {
    let mut db = test_db();
    let memory = test_store();
    let doc =
        "## One\nVector commitments bind state.\n\n## Two\nWitness signatures attest commits.";
    let stored = memory
        .ingest_document(&mut db, doc, Some("user:doc"))
        .expect("ingest");
    assert!(stored.len() >= 2, "expected at least two chunks");
}

#[test]
fn test_memory_prefix_isolation() {
    let mut db = test_db();
    let memory = test_store();
    memory
        .store_memory(&mut db, "memory key content", Some("session:test"))
        .expect("memory store");
    let (idx, cell) = db
        .put_typed(
            "other:key",
            nucleusdb::typed_value::TypedValue::Text("other".to_string()),
        )
        .expect("put other");
    let delta = nucleusdb::state::Delta::new(vec![(idx, cell)]);
    let _ = db.commit(delta, &[]).expect("commit other");

    let hits = memory.recall(&db, "memory key", 10).expect("recall");
    assert!(hits.iter().all(|h| h.key.starts_with(MEMORY_KEY_PREFIX)));
}

#[test]
fn test_memory_survives_restart() {
    let db_path = temp_db_path("survives_restart");
    let memory = test_store();
    {
        let mut db = test_db();
        memory
            .store_memory(
                &mut db,
                "persisted memory survives snapshot reload",
                Some("session:persist"),
            )
            .expect("store");
        db.save_persistent(&db_path).expect("save snapshot");
    }
    {
        let mut cfg = nucleusdb::cli::default_witness_cfg();
        cfg.signing_algorithm = nucleusdb::witness::WitnessSignatureAlgorithm::MlDsa65;
        let db = NucleusDb::load_persistent(&db_path, cfg).expect("load snapshot");
        let hits = memory
            .recall(&db, "reload persisted memory", 5)
            .expect("recall");
        assert!(!hits.is_empty(), "memory should survive restart");
    }
    cleanup_db_files(&db_path);
}

#[tokio::test]
async fn test_mcp_memory_recall() {
    let _env_guard = enable_hash_backend();
    let db_path = temp_db_path("mcp_roundtrip");
    let service = NucleusDbMcpService::new(&db_path).expect("service");
    let stored = service
        .memory_store(Parameters(MemoryStoreRequest {
            text: "MCP memory tool stores and seals chunked text".to_string(),
            source: Some("session:mcp".to_string()),
        }))
        .await
        .expect("memory_store");
    assert!(stored.0.key.starts_with(MEMORY_KEY_PREFIX));

    let recall = service
        .memory_recall(Parameters(MemoryRecallRequest {
            query: "how does the MCP memory tool work".to_string(),
            k: Some(5),
        }))
        .await
        .expect("memory_recall");
    assert!(recall.0.count >= 1);

    let ingest = service
        .memory_ingest(Parameters(MemoryIngestRequest {
            document: "## A\nfirst chunk for mcp\n\n## B\nsecond chunk for mcp".to_string(),
            source: Some("session:mcp".to_string()),
        }))
        .await
        .expect("memory_ingest");
    assert!(ingest.0.chunks >= 1);

    cleanup_db_files(&db_path);
}
