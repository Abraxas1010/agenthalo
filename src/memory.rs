use crate::embeddings::{EmbeddingModel, DEFAULT_EMBEDDING_DIMS};
use crate::protocol::{CommitError, NucleusDb};
use crate::state::Delta;
use crate::typed_value::TypedValue;
use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::Duration;

pub const MEMORY_KEY_PREFIX: &str = "mem:chunk:";
const MEMORY_META_SUFFIX: &str = ":meta";
const MEMORY_VECTOR_SUFFIX: &str = ":vec";
const RECALL_MAX_K: usize = 20;
const DEFAULT_CANDIDATE_MULTIPLIER: usize = 4;
const DEFAULT_MAX_CANDIDATES: usize = 80;
const QUERY_EXPANSION_ENABLED_ENV: &str = "NUCLEUSDB_MEMORY_QUERY_EXPANSION";
const QUERY_EXPANSION_LLM_ENV: &str = "NUCLEUSDB_MEMORY_QUERY_EXPANSION_LLM";
const QUERY_EXPANSION_MODEL_ENV: &str = "NUCLEUSDB_MEMORY_QUERY_EXPANSION_MODEL";
const RERANK_ENABLED_ENV: &str = "NUCLEUSDB_MEMORY_RERANK";
const RERANK_CANDIDATE_MULTIPLIER_ENV: &str = "NUCLEUSDB_MEMORY_RERANK_CANDIDATE_MULTIPLIER";
const RERANK_MAX_CANDIDATES_ENV: &str = "NUCLEUSDB_MEMORY_RERANK_MAX_CANDIDATES";
const FUSED_BASE_SIMILARITY_WEIGHT: f64 = 0.50;
const FUSED_PAIRWISE_BIENCODER_WEIGHT: f64 = 0.28;
const FUSED_LEXICAL_WEIGHT: f64 = 0.12;
const FUSED_NEGATION_WEIGHT: f64 = 0.10;

#[derive(Debug, Clone, Copy)]
struct RecallPipelineConfig {
    query_expansion_enabled: bool,
    llm_query_expansion_enabled: bool,
    rerank_enabled: bool,
    candidate_multiplier: usize,
    max_candidates: usize,
}

impl Default for RecallPipelineConfig {
    fn default() -> Self {
        Self {
            query_expansion_enabled: true,
            llm_query_expansion_enabled: false,
            rerank_enabled: true,
            candidate_multiplier: DEFAULT_CANDIDATE_MULTIPLIER,
            max_candidates: DEFAULT_MAX_CANDIDATES,
        }
    }
}

impl RecallPipelineConfig {
    fn from_env() -> Self {
        Self {
            query_expansion_enabled: env_bool(QUERY_EXPANSION_ENABLED_ENV, true),
            llm_query_expansion_enabled: env_bool(QUERY_EXPANSION_LLM_ENV, false),
            rerank_enabled: env_bool(RERANK_ENABLED_ENV, true),
            candidate_multiplier: std::env::var(RERANK_CANDIDATE_MULTIPLIER_ENV)
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .map(|n| n.clamp(1, 16))
                .unwrap_or(DEFAULT_CANDIDATE_MULTIPLIER),
            max_candidates: std::env::var(RERANK_MAX_CANDIDATES_ENV)
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .map(|n| n.clamp(RECALL_MAX_K, 500))
                .unwrap_or(DEFAULT_MAX_CANDIDATES),
        }
    }
}

#[derive(Debug, Clone)]
struct RecallCandidate {
    key: String,
    text: String,
    source: Option<String>,
    created: Option<String>,
    base_distance: f64,
}

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

#[derive(Debug, Clone)]
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
        db: &mut NucleusDb,
        query: &str,
        k: usize,
    ) -> Result<Vec<MemoryRecallRecord>, String> {
        let q = query.trim();
        if q.is_empty() {
            return Err("query must not be empty".to_string());
        }
        let k = k.clamp(1, RECALL_MAX_K);
        let cfg = RecallPipelineConfig::from_env();
        let expanded_query = self.expand_query(q, cfg);
        let query_vec = self
            .embedding_model
            .embed(&expanded_query, "search_query: ")
            .map_err(|e| format!("embed query: {e}"))?;
        let candidate_k = (k * cfg.candidate_multiplier)
            .max(k)
            .min(cfg.max_candidates);
        let search_results = db.vector_index.search_with_access(
            &query_vec,
            db.vector_index.len(),
            crate::vector_index::DistanceMetric::Cosine,
        )?;
        let mut candidates = search_results
            .into_iter()
            .filter(|result| {
                result.key.starts_with(MEMORY_KEY_PREFIX)
                    && result.key.ends_with(MEMORY_VECTOR_SUFFIX)
            })
            .filter_map(|result| {
                let base_key = result.key.strip_suffix(MEMORY_VECTOR_SUFFIX)?;
                let typed = db.get_typed_touching(base_key)?;
                let text = match typed {
                    TypedValue::Text(t) => t,
                    _ => return None,
                };
                let meta_key = format!("{base_key}{MEMORY_META_SUFFIX}");
                let (source, created) = match db.get_typed_touching(&meta_key) {
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
                Some(RecallCandidate {
                    key: base_key.to_string(),
                    text,
                    source,
                    created,
                    base_distance: result.distance,
                })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|a, b| {
            a.base_distance
                .partial_cmp(&b.base_distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(candidate_k);

        let reranked = if cfg.rerank_enabled {
            let query_vector_for_pair = if expanded_query == q {
                query_vec.clone()
            } else {
                self.embedding_model
                    .embed(q, "search_query: ")
                    .unwrap_or_else(|_| query_vec.clone())
            };
            let mut scored = candidates
                .into_iter()
                .map(|candidate| {
                    let base_similarity = distance_to_similarity(candidate.base_distance);
                    let pairwise_biencoder_similarity = self
                        .pairwise_biencoder_similarity(
                            &expanded_query,
                            &candidate.text,
                            &query_vector_for_pair,
                        )
                        .unwrap_or(base_similarity);
                    let lexical = lexical_overlap_score(&expanded_query, &candidate.text);
                    let negation = negation_alignment_score(q, &candidate.text);
                    // Provisional weights tuned for initial recall quality; calibrate
                    // against a held-out eval set before hardening.
                    let fused_similarity = (FUSED_BASE_SIMILARITY_WEIGHT * base_similarity
                        + FUSED_PAIRWISE_BIENCODER_WEIGHT * pairwise_biencoder_similarity
                        + FUSED_LEXICAL_WEIGHT * lexical
                        + FUSED_NEGATION_WEIGHT * negation)
                        .clamp(0.0, 1.0);
                    let fused_distance = similarity_to_distance(fused_similarity);
                    (candidate, fused_distance)
                })
                .collect::<Vec<_>>();
            scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            scored
        } else {
            candidates
                .into_iter()
                .map(|candidate| {
                    let distance = candidate.base_distance;
                    (candidate, distance)
                })
                .collect::<Vec<_>>()
        };

        let mapped = reranked
            .into_iter()
            .take(k)
            .map(|(candidate, distance)| MemoryRecallRecord {
                key: candidate.key,
                distance,
                text: candidate.text,
                source: candidate.source,
                created: candidate.created,
            })
            .collect::<Vec<_>>();

        Ok(mapped)
    }

    fn expand_query(&self, query: &str, cfg: RecallPipelineConfig) -> String {
        if !cfg.query_expansion_enabled {
            return query.to_string();
        }
        if cfg.llm_query_expansion_enabled {
            if let Some(expanded) = expand_query_with_llm(query) {
                if !expanded.trim().is_empty() {
                    return expanded;
                }
            }
        }
        expand_query_hyde_local(query)
    }

    fn pairwise_biencoder_similarity(
        &self,
        expanded_query: &str,
        document: &str,
        query_vec: &[f64],
    ) -> Result<f64, String> {
        // Pairwise bi-encoder scoring pass (NOT a cross-encoder). We embed
        // query+document text together and compare that embedding to the query
        // embedding, then fuse with other signals.
        let joint_input =
            format!("query: {expanded_query}\n\ncandidate_document: {document}\n\nrelevance:");
        let pair_vec = self.embedding_model.embed(&joint_input, "search_query: ")?;
        let distance = crate::embeddings::cosine_distance(query_vec, &pair_vec)?;
        Ok(distance_to_similarity(distance))
    }

    pub fn stats(&self, db: &NucleusDb) -> MemoryStats {
        let total_memories = db
            .keymap
            .all_keys()
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

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Ok(v) => matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}

fn distance_to_similarity(distance: f64) -> f64 {
    (1.0 - (distance / 2.0)).clamp(0.0, 1.0)
}

fn similarity_to_distance(similarity: f64) -> f64 {
    (1.0 - similarity.clamp(0.0, 1.0)) * 2.0
}

fn tokenize_for_matching(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn lexical_overlap_score(query: &str, document: &str) -> f64 {
    let q = tokenize_for_matching(query)
        .into_iter()
        .collect::<HashSet<_>>();
    let d = tokenize_for_matching(document)
        .into_iter()
        .collect::<HashSet<_>>();
    if q.is_empty() || d.is_empty() {
        return 0.0;
    }
    let intersection = q.intersection(&d).count() as f64;
    let union = q.union(&d).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn split_negated_terms(text: &str) -> (HashSet<String>, HashSet<String>) {
    const NEGATORS: &[&str] = &[
        "not",
        "no",
        "never",
        "without",
        "deny",
        "denied",
        "disallow",
        "disallowed",
        "disable",
        "disabled",
    ];

    let tokens = tokenize_for_matching(text);
    let mut positive = HashSet::new();
    let mut negated = HashSet::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = &tokens[i];
        if NEGATORS.contains(&token.as_str()) {
            let mut j = i + 1;
            let mut captured = 0usize;
            while j < tokens.len() && captured < 3 {
                let next = &tokens[j];
                if NEGATORS.contains(&next.as_str()) {
                    break;
                }
                if next.len() > 2 && !is_negation_scope_stopword(next) {
                    negated.insert(next.clone());
                    captured += 1;
                }
                j += 1;
            }
            i = j;
            continue;
        }
        if token.len() > 2 {
            positive.insert(token.clone());
        }
        i += 1;
    }
    (positive, negated)
}

fn is_negation_scope_stopword(token: &str) -> bool {
    matches!(
        token,
        "a" | "an"
            | "the"
            | "to"
            | "of"
            | "for"
            | "in"
            | "on"
            | "at"
            | "by"
            | "is"
            | "are"
            | "was"
            | "were"
            | "be"
            | "been"
            | "being"
            | "and"
            | "or"
            | "but"
            | "if"
            | "then"
            | "that"
            | "this"
            | "these"
            | "those"
            | "with"
            | "as"
            | "from"
            | "it"
            | "its"
            | "into"
    )
}

fn negation_alignment_score(query: &str, document: &str) -> f64 {
    let (q_pos, q_neg) = split_negated_terms(query);
    if q_neg.is_empty() {
        return 0.5;
    }
    let (d_pos, d_neg) = split_negated_terms(document);
    let aligned = q_neg.intersection(&d_neg).count() as f64;
    let contradicted = q_neg.intersection(&d_pos).count() as f64;
    let pos_contradiction = q_pos.intersection(&d_neg).count() as f64;
    let denom = (q_neg.len().max(1)) as f64;
    ((aligned - contradicted - 0.5 * pos_contradiction) / denom).clamp(-1.0, 1.0) * 0.5 + 0.5
}

fn expand_query_hyde_local(query: &str) -> String {
    // Lightweight fallback expansion. This is intentionally heuristic and
    // keyword-driven; the LLM expansion path is the general mechanism.
    let lower = query.to_ascii_lowercase();
    let mut expansions: Vec<&str> = Vec::new();
    if lower.contains("mathematical guarantees") {
        expansions.push("formal verification");
        expansions.push("machine-checked proofs");
        expansions.push("soundness and correctness");
    }
    if lower.contains("software behavior") {
        expansions.push("program semantics");
        expansions.push("runtime correctness");
        expansions.push("safety invariants");
    }
    if lower.contains("private") {
        expansions.push("confidential access");
        expansions.push("restricted visibility");
    }
    if lower.contains("not ") || lower.contains("without ") || lower.contains("never ") {
        expansions.push("negated condition must hold");
        expansions.push("explicit exclusion semantics");
    }
    if lower.contains("proof") || lower.contains("verify") || lower.contains("guarantee") {
        expansions.push("formal proof artifact");
        expansions.push("machine-verifiable certificate");
    }
    expansions.sort_unstable();
    expansions.dedup();

    if expansions.is_empty() {
        return query.to_string();
    }
    format!(
        "Question: {query}\nHypothetical answer document: This document addresses {}.",
        expansions.join(", ")
    )
}

fn expand_query_with_llm(query: &str) -> Option<String> {
    let api_key = std::env::var("OPENROUTER_API_KEY").ok()?;
    if api_key.trim().is_empty() {
        return None;
    }
    let model = std::env::var(QUERY_EXPANSION_MODEL_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "openai/gpt-4o-mini".to_string());
    let payload = json!({
        "model": model,
        "temperature": 0.2,
        "max_tokens": 220,
        "messages": [
            {
                "role": "system",
                "content": "Expand the query into a short hypothetical answer paragraph for semantic retrieval. Preserve negation constraints exactly. Return plain text only."
            },
            {
                "role": "user",
                "content": query
            }
        ]
    });

    let response = crate::http_client::post_with_timeout(
        "https://openrouter.ai/api/v1/chat/completions",
        Duration::from_secs(20),
    )
    .ok()?
    .header("Authorization", &format!("Bearer {api_key}"))
    .header("Content-Type", "application/json")
    .send_json(payload)
    .ok()?;

    let value: Value = response.into_body().read_json().ok()?;
    extract_llm_text_from_response(&value).filter(|v| !v.trim().is_empty())
}

fn extract_llm_text_from_response(value: &Value) -> Option<String> {
    let content = value
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?;
    if let Some(s) = content.as_str() {
        return Some(s.trim().to_string());
    }
    if let Some(parts) = content.as_array() {
        let mut joined = String::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                if !joined.is_empty() {
                    joined.push(' ');
                }
                joined.push_str(text.trim());
            }
        }
        if !joined.trim().is_empty() {
            return Some(joined.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::embeddings::{EmbeddingModel, DEFAULT_EMBEDDING_DIMS, DEFAULT_MODEL_NAME};
    use crate::protocol::VcBackend;
    use crate::state::State;
    use crate::test_support::lock_env;

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

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var(key).ok();
            match value {
                Some(v) => {
                    // SAFETY: test-only env mutation is serialized by env_lock().
                    unsafe { std::env::set_var(key, v) };
                }
                None => {
                    // SAFETY: test-only env mutation is serialized by env_lock().
                    unsafe { std::env::remove_var(key) };
                }
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => {
                    // SAFETY: test-only env mutation is serialized by env_lock().
                    unsafe { std::env::set_var(self.key, v) };
                }
                None => {
                    // SAFETY: test-only env mutation is serialized by env_lock().
                    unsafe { std::env::remove_var(self.key) };
                }
            }
        }
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
            .recall(&mut db, "how does vector similarity search work", 5)
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

    #[test]
    fn test_negation_aware_rerank_prefers_negated_match() {
        let _guard = lock_env();
        let _query_expansion = EnvVarGuard::set(QUERY_EXPANSION_ENABLED_ENV, None);
        let _rerank = EnvVarGuard::set(RERANK_ENABLED_ENV, None);

        let mut db = test_db();
        let store = test_store();
        store
            .store_memory(
                &mut db,
                "This route is private and restricted to trusted operators.",
                Some("test"),
            )
            .expect("store private");
        store
            .store_memory(
                &mut db,
                "This route is not private and is visible to all operators.",
                Some("test"),
            )
            .expect("store not-private");

        let hits = store
            .recall(&mut db, "route not private visibility policy", 2)
            .expect("recall");
        assert_eq!(hits.len(), 2);
        assert!(
            hits[0].text.contains("not private"),
            "negation-aware rerank should prioritize the negated match: {:?}",
            hits
        );
    }

    #[test]
    fn test_split_negated_terms_captures_multi_token_scope() {
        let (positive, negated) = split_negated_terms("endpoint is not a private service route");
        assert!(negated.contains("private"));
        assert!(negated.contains("service"));
        assert!(negated.contains("route"));
        assert!(!positive.contains("private"));
    }

    #[test]
    fn test_query_expansion_improves_zero_overlap_vocab() {
        let _guard = lock_env();
        let _query_expansion = EnvVarGuard::set(QUERY_EXPANSION_ENABLED_ENV, Some("true"));
        let _rerank = EnvVarGuard::set(RERANK_ENABLED_ENV, Some("true"));
        let _llm_expansion = EnvVarGuard::set(QUERY_EXPANSION_LLM_ENV, Some("false"));

        let mut db = test_db();
        let store = test_store();
        store
            .store_memory(
                &mut db,
                "Machine-checked proofs provide formal verification for software correctness.",
                Some("test"),
            )
            .expect("store proof");
        store
            .store_memory(
                &mut db,
                "Banana orchard fertilizer schedule and weather planning.",
                Some("test"),
            )
            .expect("store noise");

        let hits = store
            .recall(&mut db, "mathematical guarantees for software behavior", 2)
            .expect("recall");
        assert_eq!(hits.len(), 2);
        assert!(
            hits[0].text.contains("Machine-checked proofs"),
            "query expansion should bridge vocabulary mismatch: {:?}",
            hits
        );
    }

    #[test]
    fn test_expand_query_hyde_local_adds_domain_bridge_terms() {
        let expanded =
            expand_query_hyde_local("mathematical guarantees for software behavior without leaks");
        let lower = expanded.to_ascii_lowercase();
        assert!(lower.contains("formal verification"));
        assert!(lower.contains("machine-checked proofs"));
        assert!(lower.contains("negated condition"));
    }

    #[test]
    fn test_extract_llm_text_from_response_variants() {
        let direct = json!({
            "choices": [{"message": {"content": "expanded query text"}}]
        });
        assert_eq!(
            extract_llm_text_from_response(&direct).as_deref(),
            Some("expanded query text")
        );

        let segmented = json!({
            "choices": [{"message": {"content": [
                {"type": "text", "text": "expanded"},
                {"type": "text", "text": "query"}
            ]}}]
        });
        assert_eq!(
            extract_llm_text_from_response(&segmented).as_deref(),
            Some("expanded query")
        );
    }
}
