use crate::embeddings::{EmbeddingModel, DEFAULT_EMBEDDING_DIMS};
use crate::protocol::{CommitError, NucleusDb};
use crate::state::Delta;
use crate::typed_value::TypedValue;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

pub const MEMORY_KEY_PREFIX: &str = "mem:chunk:";
const MEMORY_META_SUFFIX: &str = ":meta";
const MEMORY_VECTOR_SUFFIX: &str = ":vec";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecord {
    pub key: String,
    pub text: String,
    pub source: Option<String>,
    pub created: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallRecord {
    pub key: String,
    pub distance: f64,
    pub text: String,
    pub source: Option<String>,
    pub created: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryStats {
    pub total_memories: usize,
    pub total_dims: usize,
    pub model: String,
    pub index_size: usize,
}

pub struct MemoryStore {
    embedding_model: EmbeddingModel,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new(EmbeddingModel::default())
    }
}

impl MemoryStore {
    pub fn new(embedding_model: EmbeddingModel) -> Self {
        Self { embedding_model }
    }

    pub fn embedding_model(&self) -> &EmbeddingModel {
        &self.embedding_model
    }

    pub fn store_memory(
        &self,
        db: &mut NucleusDb,
        text: &str,
        source: Option<&str>,
    ) -> Result<MemoryRecord, String> {
        let memory_text = text.trim();
        if memory_text.is_empty() {
            return Err("memory text must not be empty".to_string());
        }
        let key = key_for_text(memory_text);
        let vector_key = format!("{key}{MEMORY_VECTOR_SUFFIX}");
        let meta_key = format!("{key}{MEMORY_META_SUFFIX}");
        let now = Utc::now().to_rfc3339();
        let source_clean = source.map(str::trim).filter(|s| !s.is_empty());

        if let Some(TypedValue::Text(existing)) = db.get_typed(&key) {
            if existing == memory_text && db.get_typed(&vector_key).is_some() {
                let existing_meta = db.get_typed(&meta_key).and_then(|tv| match tv {
                    TypedValue::Json(meta) => Some(meta),
                    _ => None,
                });
                let existing_source = existing_meta
                    .as_ref()
                    .and_then(|meta| meta.get("source"))
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string);
                let existing_created = existing_meta
                    .as_ref()
                    .and_then(|meta| meta.get("created"))
                    .and_then(|v| v.as_str())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| now.clone());
                return Ok(MemoryRecord {
                    key,
                    text: existing,
                    source: existing_source.or_else(|| source_clean.map(ToString::to_string)),
                    created: existing_created,
                });
            }
        }

        let embedding = self
            .embedding_model
            .embed(memory_text, "search_document: ")
            .map_err(|e| format!("embed memory: {e}"))?;
        if embedding.len() != DEFAULT_EMBEDDING_DIMS {
            return Err(format!(
                "embedding dimension mismatch: expected {DEFAULT_EMBEDDING_DIMS}, got {}",
                embedding.len()
            ));
        }

        let meta = json!({
            "source": source_clean,
            "created": now,
            "dims": DEFAULT_EMBEDDING_DIMS,
            "model": self.embedding_model.model_name(),
        });

        let (idx_text, cell_text) = db
            .put_typed(&key, TypedValue::Text(memory_text.to_string()))
            .map_err(|e| format!("store memory text failed: {e}"))?;
        let (idx_meta, cell_meta) = db
            .put_typed(&meta_key, TypedValue::Json(meta))
            .map_err(|e| format!("store memory metadata failed: {e}"))?;
        let (idx_vec, cell_vec) = db
            .put_typed(&vector_key, TypedValue::Vector(embedding))
            .map_err(|e| format!("store memory embedding failed: {e}"))?;

        let delta = Delta::new(vec![
            (idx_text, cell_text),
            (idx_meta, cell_meta),
            (idx_vec, cell_vec),
        ]);
        if let Err(err) = db.commit(delta, &[]) {
            return Err(format!(
                "memory commit failed: {}",
                format_commit_error(err)
            ));
        }

        Ok(MemoryRecord {
            key,
            text: memory_text.to_string(),
            source: source_clean.map(ToString::to_string),
            created: now,
        })
    }

    pub fn ingest_document(
        &self,
        db: &mut NucleusDb,
        document: &str,
        source: Option<&str>,
    ) -> Result<Vec<MemoryRecord>, String> {
        let chunks = chunk_document(document);
        let mut out = Vec::new();
        for chunk in chunks {
            if chunk.trim().len() < 20 {
                continue;
            }
            let stored = self.store_memory(db, &chunk, source)?;
            out.push(stored);
        }
        Ok(out)
    }

    pub fn recall(
        &self,
        db: &NucleusDb,
        query: &str,
        k: usize,
    ) -> Result<Vec<MemoryRecallRecord>, String> {
        let q = query.trim();
        if q.is_empty() {
            return Err("query must not be empty".to_string());
        }
        let k = k.clamp(1, 20);
        let query_vec = self
            .embedding_model
            .embed(q, "search_query: ")
            .map_err(|e| format!("embed query: {e}"))?;
        let mut results = db
            .vector_index
            .all_keys()
            .into_iter()
            .filter(|key| key.starts_with(MEMORY_KEY_PREFIX) && key.ends_with(MEMORY_VECTOR_SUFFIX))
            .filter_map(|key| {
                let vec = db.vector_index.get(&key)?;
                let distance = crate::embeddings::cosine_distance(&query_vec, vec).ok()?;
                Some((key, distance))
            })
            .collect::<Vec<_>>();
        results.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);

        let mapped = results
            .into_iter()
            .filter_map(|(vector_key, distance)| {
                let base_key = vector_key.strip_suffix(MEMORY_VECTOR_SUFFIX)?.to_string();
                let typed = db.get_typed(&base_key)?;
                let text = match typed {
                    TypedValue::Text(t) => t,
                    _ => return None,
                };
                let meta_key = format!("{base_key}{MEMORY_META_SUFFIX}");
                let (source, created) = match db.get_typed(&meta_key) {
                    Some(TypedValue::Json(meta)) => {
                        let source = meta
                            .get("source")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                        let created = meta
                            .get("created")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                        (source, created)
                    }
                    _ => (None, None),
                };
                Some(MemoryRecallRecord {
                    key: base_key,
                    distance,
                    text,
                    source,
                    created,
                })
            })
            .collect::<Vec<_>>();

        Ok(mapped)
    }

    pub fn stats(&self, db: &NucleusDb) -> MemoryStats {
        let total_memories = db
            .keymap
            .all_keys()
            .into_iter()
            .filter(|(k, _)| {
                k.starts_with(MEMORY_KEY_PREFIX)
                    && !k.ends_with(MEMORY_META_SUFFIX)
                    && !k.ends_with(MEMORY_VECTOR_SUFFIX)
            })
            .count();
        let index_size = db
            .vector_index
            .all_keys()
            .into_iter()
            .filter(|k| k.starts_with(MEMORY_KEY_PREFIX) && k.ends_with(MEMORY_VECTOR_SUFFIX))
            .count();
        MemoryStats {
            total_memories,
            total_dims: db.vector_index.dims().unwrap_or(DEFAULT_EMBEDDING_DIMS),
            model: self.embedding_model.model_name().to_string(),
            index_size,
        }
    }
}

pub fn key_for_text(text: &str) -> String {
    let digest = Sha256::digest(text.trim().as_bytes());
    let hex = hex::encode(digest);
    format!("{MEMORY_KEY_PREFIX}{}", &hex[..16])
}

pub fn chunk_document(document: &str) -> Vec<String> {
    const SOFT_WORD_LIMIT: usize = 512;
    const HARD_WORD_LIMIT: usize = 2048;
    const MIN_CHARS: usize = 20;

    let mut sections: Vec<String> = Vec::new();
    let mut current = Vec::<String>::new();
    for line in document.lines() {
        if is_heading_boundary(line) && !current.is_empty() {
            sections.push(current.join("\n").trim().to_string());
            current.clear();
        }
        current.push(line.to_string());
    }
    if !current.is_empty() {
        sections.push(current.join("\n").trim().to_string());
    }
    if sections.is_empty() {
        sections.push(document.to_string());
    }

    let mut chunks = Vec::new();
    for section in sections {
        let mut bucket = String::new();
        for paragraph in section.split("\n\n") {
            let p = paragraph.trim();
            if p.is_empty() {
                continue;
            }
            let words = p.split_whitespace().count();
            if words > HARD_WORD_LIMIT {
                let tokens = p.split_whitespace().collect::<Vec<_>>();
                for slice in tokens.chunks(HARD_WORD_LIMIT) {
                    let candidate = slice.join(" ");
                    if candidate.len() >= MIN_CHARS {
                        chunks.push(candidate);
                    }
                }
                continue;
            }

            let current_words = bucket.split_whitespace().count();
            if !bucket.is_empty() && current_words + words > SOFT_WORD_LIMIT {
                if bucket.len() >= MIN_CHARS {
                    chunks.push(bucket.trim().to_string());
                }
                bucket.clear();
            }
            if !bucket.is_empty() {
                bucket.push_str("\n\n");
            }
            bucket.push_str(p);
        }
        if bucket.len() >= MIN_CHARS {
            chunks.push(bucket.trim().to_string());
        }
    }

    chunks
}

fn is_heading_boundary(line: &str) -> bool {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return false;
    }
    let hash_count = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hash_count) {
        return false;
    }
    trimmed
        .chars()
        .nth(hash_count)
        .map(|c| c.is_ascii_whitespace())
        .unwrap_or(false)
}

fn format_commit_error(err: CommitError) -> String {
    match err {
        CommitError::SheafIncoherent => "sheaf coherence check failed".to_string(),
        CommitError::WitnessQuorumFailed => "witness quorum check failed".to_string(),
        CommitError::EmptyWitnessSet => "witness set is empty".to_string(),
        CommitError::WitnessSigningFailed(e) => format!("witness signing failed: {e:?}"),
        CommitError::SecurityPolicyInvalid(e) => format!("security policy invalid: {e:?}"),
        CommitError::SecurityRefinementFailed(e) => format!("security refinement failed: {e:?}"),
        CommitError::MonotoneViolation => "append-only monotone check failed".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::embeddings::{EmbeddingModel, DEFAULT_EMBEDDING_DIMS, DEFAULT_MODEL_NAME};
    use crate::protocol::VcBackend;
    use crate::state::State;

    fn test_db() -> NucleusDb {
        let mut cfg = default_witness_cfg();
        cfg.signing_algorithm = crate::witness::WitnessSignatureAlgorithm::MlDsa65;
        NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, cfg)
    }

    fn test_store() -> MemoryStore {
        MemoryStore::new(EmbeddingModel::new_hash_test_backend(
            DEFAULT_MODEL_NAME,
            DEFAULT_EMBEDDING_DIMS,
        ))
    }

    #[test]
    fn test_store_and_recall_roundtrip() {
        let mut db = test_db();
        let store = test_store();
        store
            .store_memory(
                &mut db,
                "The VectorIndex uses cosine distance for similarity search.",
                Some("session:test"),
            )
            .expect("store");
        let hits = store
            .recall(&db, "how does vector similarity search work", 5)
            .expect("recall");
        assert!(!hits.is_empty(), "expected at least one recall hit");
        assert!(hits[0].key.starts_with(MEMORY_KEY_PREFIX));
    }

    #[test]
    fn test_chunk_by_headers() {
        let doc = "## one\nalpha section contains enough text for chunking.\n\nbeta paragraph also has enough text.\n\n## two\ngamma delta section remains independently chunked.";
        let chunks = chunk_document(doc);
        assert!(chunks.len() >= 2, "expected 2+ chunks");
        assert!(chunks.iter().any(|c| c.contains("alpha")));
        assert!(chunks.iter().any(|c| c.contains("gamma")));
    }

    #[test]
    fn test_chunk_with_mixed_header_depths() {
        let doc = "# one\nalpha section contains enough words to be retained.\n\n### two\nbeta section also contains enough words to be retained.\n\n#### three\ngamma section remains long enough for chunk retention.";
        let chunks = chunk_document(doc);
        assert!(
            chunks.len() >= 3,
            "expected mixed heading levels to split sections"
        );
    }

    #[test]
    fn test_chunk_max_size() {
        let long = std::iter::repeat_n("word", 3000)
            .collect::<Vec<_>>()
            .join(" ");
        let chunks = chunk_document(&long);
        assert!(chunks.len() >= 2, "expected split of oversized chunk");
    }

    #[test]
    fn test_chunk_min_size() {
        let doc = "## tiny\nx\n\n## real\nthis is long enough to keep";
        let chunks = chunk_document(doc);
        assert!(chunks.iter().all(|c| c.len() >= 20));
    }

    #[test]
    fn test_idempotent_store() {
        let mut db = test_db();
        let store = test_store();
        let a = store
            .store_memory(&mut db, "idempotent memory text", Some("session:a"))
            .expect("store a");
        let before_entries = db.entries.len();
        let b = store
            .store_memory(&mut db, "idempotent memory text", Some("session:b"))
            .expect("store b");
        let after_entries = db.entries.len();
        assert_eq!(a.key, b.key);
        assert_eq!(before_entries, after_entries);
        assert_eq!(
            a.created, b.created,
            "idempotent read should preserve created timestamp"
        );
    }

    #[test]
    fn test_seal_chain_integrity() {
        let mut db = test_db();
        let store = test_store();
        let _ = store
            .store_memory(&mut db, "seal chain memory one", Some("test"))
            .expect("store one");
        let _ = store
            .store_memory(&mut db, "seal chain memory two", Some("test"))
            .expect("store two");
        assert!(db.entries.len() >= 2);
        let key = key_for_text("seal chain memory one");
        let idx = db.keymap.get(&key).expect("key index");
        let (value, proof, root) = db.query(idx).expect("query");
        assert!(db.verify_query(idx, value, &proof, root));
    }
}
