//! AETHER Chebyshev guard primitives plus an engineering liveness wrapper.
//!
//! Provenance: `artifacts/aether_verified/rust/aether_chebyshev.rs`
//! Formal basis: `HeytingLean.Bridge.Sharma.AetherChebyshev.chebyshev_finite`
//!
//! The `reclaimable_count` and `chebyshev_guard_check` functions are a direct
//! port of the verified core. `ChebyshevEvictor` wraps those primitives in a
//! stateful API and is therefore an engineering layer, not formally verified.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub(crate) const DEFAULT_CHEBYSHEV_K: f64 = 2.0;
pub(crate) const DEFAULT_CHEBYSHEV_DECAY_RATE: f64 = 0.05;
pub(crate) const DEFAULT_CHEBYSHEV_IMPULSE_MAGNITUDE: f64 = 1.0;

fn mean(x: &[f64]) -> f64 {
    if x.is_empty() {
        0.0
    } else {
        x.iter().sum::<f64>() / x.len() as f64
    }
}

fn stddev(x: &[f64]) -> f64 {
    if x.is_empty() {
        return 0.0;
    }
    let m = mean(x);
    let variance = x
        .iter()
        .map(|v| {
            let d = *v - m;
            d * d
        })
        .sum::<f64>()
        / x.len() as f64;
    variance.sqrt()
}

/// VERIFIED CORE (direct AETHER port).
pub fn reclaimable_count(x: &[f64], k: f64) -> usize {
    let m = mean(x);
    let sd = stddev(x);
    let threshold = m - k * sd;
    x.iter().filter(|value| **value <= threshold).count()
}

/// VERIFIED CORE (direct AETHER port).
pub fn chebyshev_guard_check(x: &[f64], k: f64) -> bool {
    if x.is_empty() || k <= 0.0 {
        return false;
    }
    let sd = stddev(x);
    if sd <= 0.0 {
        return true;
    }
    let reclaim = reclaimable_count(x, k) as f64;
    reclaim <= (x.len() as f64) / (k * k) + 1e-12
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct EvictorStats {
    pub tracked_items: usize,
    pub guarded_items: usize,
    pub reclaimable_items: usize,
    pub last_candidate_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChebyshevEvictor {
    pub liveness: BTreeMap<String, f64>,
    pub k: f64,
    pub decay_rate: f64,
    pub impulse_magnitude: f64,
    #[serde(default)]
    pub stats: EvictorStats,
}

impl Default for ChebyshevEvictor {
    fn default() -> Self {
        Self::new(
            DEFAULT_CHEBYSHEV_K,
            DEFAULT_CHEBYSHEV_DECAY_RATE,
            DEFAULT_CHEBYSHEV_IMPULSE_MAGNITUDE,
        )
    }
}

impl ChebyshevEvictor {
    pub fn new(k: f64, decay_rate: f64, impulse_magnitude: f64) -> Self {
        Self {
            liveness: BTreeMap::new(),
            k,
            decay_rate,
            impulse_magnitude,
            stats: EvictorStats::default(),
        }
    }

    pub fn record_access(&mut self, key: &str) {
        let entry = self.liveness.entry(key.to_string()).or_insert(0.0);
        *entry += self.impulse_magnitude;
        self.refresh_stats(0);
    }

    pub fn tick(&mut self) {
        self.decay_steps(1);
    }

    pub fn decay_steps(&mut self, steps: u64) {
        if steps == 0 {
            return;
        }
        let factor = (1.0 - self.decay_rate).clamp(0.0, 1.0).powf(steps as f64);
        for value in self.liveness.values_mut() {
            *value *= factor;
        }
        self.refresh_stats(0);
    }

    pub fn remove_key(&mut self, key: &str) {
        self.liveness.remove(key);
        self.refresh_stats(0);
    }

    pub fn eviction_candidates(&mut self, count: usize) -> Vec<String> {
        if self.liveness.is_empty() || count == 0 {
            self.refresh_stats(0);
            return Vec::new();
        }
        let values = self.values();
        if !chebyshev_guard_check(&values, self.k) {
            self.refresh_stats(0);
            return Vec::new();
        }
        let threshold = reclaimable_threshold(&values, self.k);
        let mut reclaimable = self
            .liveness
            .iter()
            .filter(|(_, score)| **score <= threshold)
            .map(|(key, score)| (key.clone(), *score))
            .collect::<Vec<_>>();
        reclaimable.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let keys = reclaimable
            .into_iter()
            .take(count)
            .map(|(key, _)| key)
            .collect::<Vec<_>>();
        self.refresh_stats(keys.len());
        keys
    }

    pub fn is_guarded(&self, key: &str) -> bool {
        let Some(score) = self.liveness.get(key).copied() else {
            return false;
        };
        let values = self.values();
        if values.is_empty() {
            return false;
        }
        let threshold = reclaimable_threshold(&values, self.k);
        score > threshold
    }

    pub fn guarded_count(&self) -> usize {
        let values = self.values();
        if values.is_empty() {
            return 0;
        }
        let threshold = reclaimable_threshold(&values, self.k);
        self.liveness
            .values()
            .filter(|value| **value > threshold)
            .count()
    }

    /// Engineering helper for bounded-memory callers when the verified
    /// reclaimable set is insufficient to restore capacity.
    pub fn least_live_keys(&self, count: usize) -> Vec<String> {
        let mut ranked = self
            .liveness
            .iter()
            .map(|(key, score)| (key.clone(), *score))
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.into_iter().take(count).map(|(key, _)| key).collect()
    }

    pub fn values(&self) -> Vec<f64> {
        self.liveness.values().copied().collect()
    }

    fn refresh_stats(&mut self, last_candidate_count: usize) {
        let values = self.values();
        let reclaimable_items = if values.is_empty() {
            0
        } else {
            reclaimable_count(&values, self.k)
        };
        let guarded_items = values.len().saturating_sub(reclaimable_items);
        self.stats = EvictorStats {
            tracked_items: values.len(),
            guarded_items,
            reclaimable_items,
            last_candidate_count,
        };
    }
}

fn reclaimable_threshold(values: &[f64], k: f64) -> f64 {
    mean(values) - k * stddev(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_guard_check() {
        let x = [1.0, 2.0, 3.0, 4.0, 10.0, 11.0];
        assert!(chebyshev_guard_check(&x, 2.0));
    }

    #[test]
    fn reclaimable_includes_threshold_equality() {
        let x = [0.0, 2.0, 4.0];
        assert_eq!(reclaimable_count(&x, 0.0), 2);
    }

    #[test]
    fn stateful_wrapper_returns_low_liveness_candidates() {
        let mut evictor = ChebyshevEvictor::new(1.0, 0.0, 1.0);
        evictor.record_access("hot");
        evictor.record_access("hot");
        evictor.record_access("warm");
        evictor.liveness.insert("cold".to_string(), 0.0);
        let candidates = evictor.eviction_candidates(2);
        assert!(candidates.contains(&"cold".to_string()));
        assert!(!candidates.contains(&"hot".to_string()));
    }

    #[test]
    fn tick_decays_liveness() {
        let mut evictor = ChebyshevEvictor::new(2.0, 0.25, 2.0);
        evictor.record_access("alpha");
        let before = evictor.liveness["alpha"];
        evictor.tick();
        assert!(evictor.liveness["alpha"] < before);
    }
}
