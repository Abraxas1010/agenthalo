//! Vector similarity search index for NucleusDB.
//!
//! Provides HNSW-based approximate nearest-neighbor search over vector
//! embeddings stored in the blob store.  Supports cosine similarity,
//! L2 (Euclidean) distance, and inner-product metrics.

use crate::chebyshev_evictor::ChebyshevEvictor;
use crate::governor::{GovernorConfig, GovernorState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Distance metric for vector similarity search.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistanceMetric {
    Cosine,
    L2,
    InnerProduct,
}

impl DistanceMetric {
    pub fn from_str_tag(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "cosine" => Some(Self::Cosine),
            "l2" | "euclidean" => Some(Self::L2),
            "ip" | "inner_product" | "dot" => Some(Self::InnerProduct),
            _ => None,
        }
    }
}

/// A search result: key name + distance.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub key: String,
    pub distance: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VectorIndexStats {
    pub tracked_vectors: usize,
    pub guarded_vectors: usize,
    pub reclaimable_vectors: usize,
    pub max_entries: usize,
    pub last_eviction_count: usize,
    pub governor_epsilon: f64,
    pub governor_error: f64,
    pub governor_gamma: f64,
    pub governor_contraction_bound: f64,
    pub governor_regime: String,
    pub governor_stable: bool,
    pub governor_oscillating: bool,
    pub governor_gain_violated: bool,
    pub governor_clamp_active: bool,
    pub governor_integral: f64,
    pub governor_controller_mode: String,
    pub governor_adaptive_regime: String,
    pub formal_basis: String,
}

/// In-memory brute-force vector index.
///
/// For the MVP we use exact search (brute-force) which is correct and simple.
/// This can be upgraded to HNSW (via `hnsw_rs` crate) for large-scale
/// deployments without changing the API.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VectorIndex {
    /// key → Vec<f64> dimensions
    vectors: BTreeMap<String, Vec<f64>>,
    /// Expected dimensionality (set from first insert, enforced after).
    expected_dims: Option<usize>,
    /// ENGINEERING LAYER (NOT formally verified):
    /// Stateful liveness tracking built around the verified Chebyshev core.
    #[serde(default)]
    evictor: ChebyshevEvictor,
    /// Engineering ceiling for bounded-growth runtime behavior.
    #[serde(default = "default_vector_ceiling")]
    max_entries: usize,
    /// Uses the verified AETHER PD governor equations, but the multi-step
    /// storage policy built around it is engineering rather than formally proved.
    #[serde(default = "default_vector_memory_governor")]
    memory_governor: GovernorState,
    #[serde(default)]
    last_eviction_count: usize,
    #[serde(default)]
    last_maintenance_unix: u64,
    #[serde(default)]
    last_activity_unix: u64,
}

impl VectorIndex {
    pub fn new() -> Self {
        let now = now_unix();
        Self {
            vectors: BTreeMap::new(),
            expected_dims: None,
            evictor: ChebyshevEvictor::default(),
            max_entries: default_vector_ceiling(),
            memory_governor: default_vector_memory_governor(),
            last_eviction_count: 0,
            last_maintenance_unix: now,
            last_activity_unix: now,
        }
    }

    /// Insert or update a vector for a key.
    pub fn upsert(&mut self, key: &str, dims: Vec<f64>) -> Result<(), String> {
        let now = now_unix();
        self.apply_time_maintenance(now);
        if dims.is_empty() {
            return Err("vector must have at least one dimension".to_string());
        }
        if let Some(expected) = self.expected_dims {
            if dims.len() != expected {
                return Err(format!(
                    "dimension mismatch: expected {expected}, got {}",
                    dims.len()
                ));
            }
        } else {
            self.expected_dims = Some(dims.len());
        }
        self.vectors.insert(key.to_string(), dims);
        self.evictor.record_access(key);
        self.last_activity_unix = now;
        self.enforce_pressure();
        Ok(())
    }

    /// Remove a vector by key.
    pub fn remove(&mut self, key: &str) {
        self.vectors.remove(key);
        self.evictor.remove_key(key);
        if self.vectors.is_empty() {
            self.expected_dims = None;
        }
    }

    /// Number of indexed vectors.
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }

    /// Expected dimensionality (None if empty).
    pub fn dims(&self) -> Option<usize> {
        self.expected_dims
    }

    /// Search for the k nearest neighbors to `query`.
    pub fn search(
        &self,
        query: &[f64],
        k: usize,
        metric: DistanceMetric,
    ) -> Result<Vec<SearchResult>, String> {
        if self.vectors.is_empty() {
            return Ok(vec![]);
        }
        if let Some(expected) = self.expected_dims {
            if query.len() != expected {
                return Err(format!(
                    "query dimension mismatch: expected {expected}, got {}",
                    query.len()
                ));
            }
        }

        let mut scored: Vec<(String, f64)> = self
            .vectors
            .iter()
            .map(|(key, vec)| {
                let dist = compute_distance(query, vec, metric);
                (key.clone(), dist)
            })
            .collect();

        // Sort by distance ascending (smaller = more similar for L2/cosine-distance).
        // For inner product, larger is more similar, so negate for sorting.
        match metric {
            DistanceMetric::InnerProduct => {
                scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            }
            _ => {
                scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
            }
        }

        let results: Vec<SearchResult> = scored
            .into_iter()
            .take(k)
            .map(|(key, distance)| SearchResult { key, distance })
            .collect();

        Ok(results)
    }

    /// Search and record liveness on returned neighbors. This is the storage
    /// runtime path used by the standalone NucleusDB product; the immutable `search()` method remains
    /// available for callers that need a read-only query.
    pub fn search_with_access(
        &mut self,
        query: &[f64],
        k: usize,
        metric: DistanceMetric,
    ) -> Result<Vec<SearchResult>, String> {
        let now = now_unix();
        self.apply_time_maintenance(now);
        let results = self.search(query, k, metric)?;
        for result in &results {
            self.evictor.record_access(&result.key);
        }
        if !results.is_empty() {
            self.last_activity_unix = now;
        }
        self.enforce_pressure();
        Ok(results)
    }

    /// Get a stored vector by key.
    pub fn get(&self, key: &str) -> Option<&[f64]> {
        self.vectors.get(key).map(|v| v.as_slice())
    }

    pub fn set_max_entries(&mut self, max_entries: usize) {
        self.apply_time_maintenance(now_unix());
        self.max_entries = max_entries.max(1);
        self.memory_governor.config.target = self.max_entries as f64;
        self.enforce_pressure();
    }

    pub fn eviction_stats(&self) -> VectorIndexStats {
        let gain_violated = self.memory_governor.validate_params().is_err();
        VectorIndexStats {
            tracked_vectors: self.vectors.len(),
            guarded_vectors: self.evictor.stats.guarded_items,
            reclaimable_vectors: self.evictor.stats.reclaimable_items,
            max_entries: self.max_entries,
            last_eviction_count: self.last_eviction_count,
            governor_epsilon: self.memory_governor.epsilon,
            governor_error: self.memory_governor.error(self.vectors.len() as f64),
            governor_gamma: self.memory_governor.gamma(),
            governor_contraction_bound: self.memory_governor.contraction_bound(),
            governor_regime: self.memory_governor.regime_label(),
            governor_stable: !gain_violated && !self.memory_governor.oscillating,
            governor_oscillating: self.memory_governor.oscillating,
            governor_gain_violated: gain_violated,
            governor_clamp_active: self.memory_governor.clamp_active,
            governor_integral: self.memory_governor.integral,
            governor_controller_mode: self.memory_governor.controller_mode().to_string(),
            governor_adaptive_regime: self.memory_governor.adaptive_regime_label().to_string(),
            formal_basis: self.memory_governor.config.formal_basis.clone(),
        }
    }

    /// Return all indexed keys (for filtered statistics/reporting).
    pub fn all_keys(&self) -> Vec<String> {
        self.vectors.keys().cloned().collect()
    }

    pub fn maintenance_tick(&mut self, now_unix: u64) -> bool {
        self.apply_time_maintenance(now_unix)
    }

    pub fn soft_reset_governor(&mut self) {
        self.memory_governor.reset();
    }

    fn enforce_pressure(&mut self) {
        self.last_eviction_count = 0;
        if self.vectors.is_empty() {
            return;
        }
        let observed = self.vectors.len() as f64;
        let _ = self.memory_governor.step(observed);
        if self.vectors.len() <= self.max_entries {
            return;
        }
        let overage = self.vectors.len().saturating_sub(self.max_entries);
        let budget = self.memory_governor.epsilon.ceil().max(1.0) as usize;
        let candidate_count = budget.max(overage);
        let candidates = self.evictor.eviction_candidates(candidate_count);
        for key in candidates {
            if self.vectors.len() <= self.max_entries {
                break;
            }
            if self.evictor.is_guarded(&key) {
                continue;
            }
            self.vectors.remove(&key);
            self.evictor.remove_key(&key);
            self.last_eviction_count += 1;
        }
        if self.vectors.len() > self.max_entries {
            // Engineering boundary: the verified Chebyshev core may yield an
            // empty/insufficient reclaimable set, so bounded capacity falls
            // back to least-live eviction in the stateful wrapper.
            for key in self.evictor.least_live_keys(self.vectors.len()) {
                if self.vectors.len() <= self.max_entries {
                    break;
                }
                if self.vectors.remove(&key).is_some() {
                    self.evictor.remove_key(&key);
                    self.last_eviction_count += 1;
                }
            }
        }
        if self.vectors.is_empty() {
            self.expected_dims = None;
        }
    }

    fn apply_time_maintenance(&mut self, now_unix: u64) -> bool {
        let mut changed = false;
        if self.last_maintenance_unix == 0 {
            self.last_maintenance_unix = now_unix;
        }
        let elapsed = now_unix.saturating_sub(self.last_maintenance_unix);
        if elapsed > 0 {
            self.evictor.decay_steps(elapsed);
            self.last_maintenance_unix = now_unix;
            changed = true;
        }
        if !self.vectors.is_empty()
            && !self.memory_governor.is_from_rest()
            && now_unix.saturating_sub(self.last_activity_unix) >= storage_reset_window_secs()
        {
            self.memory_governor.reset();
            changed = true;
        }
        changed
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn default_vector_ceiling() -> usize {
    100_000
}

pub(crate) fn storage_reset_window_secs() -> u64 {
    30
}

pub(crate) fn default_vector_memory_governor_config() -> GovernorConfig {
    GovernorConfig {
        instance_id: "gov-memory-vector".to_string(),
        alpha: 0.01,
        beta: 0.05,
        dt: 1.0,
        eps_min: 1.0,
        eps_max: 512.0,
        target: default_vector_ceiling() as f64,
        formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime".to_string(),
        ki: 0.001,
        kb: 0.0316, // sqrt(0.001)
        adaptive: Some(crate::governor::AdaptiveGainSchedule::default()),
    }
}

fn default_vector_memory_governor() -> GovernorState {
    GovernorState::new(default_vector_memory_governor_config())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Distance computations
// ---------------------------------------------------------------------------

fn compute_distance(a: &[f64], b: &[f64], metric: DistanceMetric) -> f64 {
    match metric {
        DistanceMetric::Cosine => cosine_distance(a, b),
        DistanceMetric::L2 => l2_distance(a, b),
        DistanceMetric::InnerProduct => inner_product(a, b),
    }
}

/// Cosine distance = 1 - cosine_similarity.  Range: [0, 2].
fn cosine_distance(a: &[f64], b: &[f64]) -> f64 {
    cosine_distance_checked(a, b).unwrap_or(1.0)
}

/// Cosine distance with explicit error reporting.
pub fn cosine_distance_checked(a: &[f64], b: &[f64]) -> Result<f64, String> {
    if a.len() != b.len() {
        return Err(format!(
            "cosine distance dimension mismatch: {} vs {}",
            a.len(),
            b.len()
        ));
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    let denom = norm_a * norm_b;
    if denom == 0.0 {
        return Err("cosine distance undefined for zero-norm vectors".to_string());
    }
    Ok(1.0 - (dot / denom))
}

/// L2 (Euclidean) distance.
fn l2_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// Inner product (dot product).  Larger = more similar.
fn inner_product(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical() {
        let v = vec![1.0, 0.0, 0.0];
        assert!((cosine_distance(&v, &v)).abs() < 1e-10);
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_distance(&a, &b) - 1.0).abs() < 1e-10);
    }

    #[test]
    fn l2_same_point() {
        let v = vec![3.0, 4.0];
        assert!((l2_distance(&v, &v)).abs() < 1e-10);
    }

    #[test]
    fn l2_known() {
        let a = vec![0.0, 0.0];
        let b = vec![3.0, 4.0];
        assert!((l2_distance(&a, &b) - 5.0).abs() < 1e-10);
    }

    #[test]
    fn search_returns_nearest() {
        let mut idx = VectorIndex::new();
        idx.upsert("a", vec![1.0, 0.0]).unwrap();
        idx.upsert("b", vec![0.0, 1.0]).unwrap();
        idx.upsert("c", vec![0.9, 0.1]).unwrap();

        let results = idx.search(&[1.0, 0.0], 2, DistanceMetric::Cosine).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].key, "a"); // identical vector
        assert_eq!(results[1].key, "c"); // most similar after a
    }

    #[test]
    fn dimension_mismatch_rejected() {
        let mut idx = VectorIndex::new();
        idx.upsert("a", vec![1.0, 0.0]).unwrap();
        let err = idx.upsert("b", vec![1.0, 0.0, 0.0]);
        assert!(err.is_err());
    }

    #[test]
    fn guarded_entries_survive_pressure_eviction() {
        let mut idx = VectorIndex::new();
        idx.set_max_entries(2);
        idx.upsert("hot", vec![1.0, 0.0]).unwrap();
        idx.upsert("warm", vec![0.0, 1.0]).unwrap();
        idx.search_with_access(&[1.0, 0.0], 1, DistanceMetric::Cosine)
            .unwrap();
        idx.upsert("cold", vec![0.5, 0.5]).unwrap();
        assert!(
            idx.get("hot").is_some(),
            "guarded vector must remain indexed"
        );
        assert!(idx.len() <= 2);
    }

    #[test]
    fn stats_report_governor_and_guard_counts() {
        let mut idx = VectorIndex::new();
        idx.upsert("a", vec![1.0, 0.0]).unwrap();
        let stats = idx.eviction_stats();
        assert_eq!(stats.tracked_vectors, 1);
        assert!(stats.governor_epsilon >= 1.0);
        assert!(!stats.formal_basis.is_empty());
    }

    #[test]
    fn maintenance_tick_decays_by_elapsed_time_not_operation_count() {
        let mut idx = VectorIndex::new();
        idx.upsert("alpha", vec![1.0, 0.0]).unwrap();
        let before = idx.evictor.liveness["alpha"];
        let future = idx.last_maintenance_unix + 5;
        idx.maintenance_tick(future);
        assert!(idx.evictor.liveness["alpha"] < before);
    }

    #[test]
    fn quiescent_maintenance_restores_from_rest_regime() {
        let mut idx = VectorIndex::new();
        idx.upsert("alpha", vec![1.0, 0.0]).unwrap();
        assert!(!idx.memory_governor.is_from_rest());
        let future = idx.last_activity_unix + storage_reset_window_secs();
        idx.maintenance_tick(future);
        assert!(idx.memory_governor.is_from_rest());
    }
}
