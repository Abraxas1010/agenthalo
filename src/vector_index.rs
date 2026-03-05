//! Vector similarity search index for NucleusDB.
//!
//! Provides HNSW-based approximate nearest-neighbor search over vector
//! embeddings stored in the blob store.  Supports cosine similarity,
//! L2 (Euclidean) distance, and inner-product metrics.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
}

impl VectorIndex {
    pub fn new() -> Self {
        Self {
            vectors: BTreeMap::new(),
            expected_dims: None,
        }
    }

    /// Insert or update a vector for a key.
    pub fn upsert(&mut self, key: &str, dims: Vec<f64>) -> Result<(), String> {
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
        Ok(())
    }

    /// Remove a vector by key.
    pub fn remove(&mut self, key: &str) {
        self.vectors.remove(key);
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

    /// Get a stored vector by key.
    pub fn get(&self, key: &str) -> Option<&[f64]> {
        self.vectors.get(key).map(|v| v.as_slice())
    }

    /// Return all indexed keys (for filtered statistics/reporting).
    pub fn all_keys(&self) -> Vec<String> {
        self.vectors.keys().cloned().collect()
    }
}

impl Default for VectorIndex {
    fn default() -> Self {
        Self::new()
    }
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
}
