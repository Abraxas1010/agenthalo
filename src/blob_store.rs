//! Content-addressable blob store for NucleusDB.
//!
//! Blob types (Text, Json, Bytes, Vector) store a content-hash in the state
//! vector and the actual payload here.  The blob store is keyed by the
//! user-facing key name (not content hash) so that each key→blob mapping is
//! unique and retrievable.

use crate::chebyshev_evictor::ChebyshevEvictor;
use crate::governor::{GovernorConfig, GovernorState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobStoreStats {
    pub tracked_blobs: usize,
    pub guarded_blobs: usize,
    pub reclaimable_blobs: usize,
    pub total_bytes: usize,
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

/// In-memory content-addressable blob store.
///
/// Keyed by the user-facing key name.  Persisted as part of the NucleusDB
/// snapshot.  Content-addressing is enforced at the typed_value layer: the
/// u64 cell in the state vector = SHA-256(key | "|" | blob_data)[0..8].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlobStore {
    blobs: BTreeMap<String, Vec<u8>>,
    #[serde(default)]
    evictor: ChebyshevEvictor,
    #[serde(default = "default_blob_ceiling")]
    max_entries: usize,
    #[serde(default = "default_blob_memory_governor")]
    memory_governor: GovernorState,
    #[serde(default)]
    last_eviction_count: usize,
    #[serde(default)]
    last_maintenance_unix: u64,
    #[serde(default)]
    last_activity_unix: u64,
}

impl BlobStore {
    pub fn new() -> Self {
        let now = now_unix();
        Self {
            blobs: BTreeMap::new(),
            evictor: ChebyshevEvictor::default(),
            max_entries: default_blob_ceiling(),
            memory_governor: default_blob_memory_governor(),
            last_eviction_count: 0,
            last_maintenance_unix: now,
            last_activity_unix: now,
        }
    }

    /// Store a blob for the given key.  Overwrites any existing blob.
    pub fn put(&mut self, key: &str, data: Vec<u8>) {
        let now = now_unix();
        self.apply_time_maintenance(now);
        self.blobs.insert(key.to_string(), data);
        self.evictor.record_access(key);
        self.last_activity_unix = now;
        self.enforce_pressure();
    }

    /// Retrieve a blob by key.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.blobs.get(key).map(|v| v.as_slice())
    }

    /// Retrieve a blob and record liveness in the engineering wrapper.
    pub fn get_with_access(&mut self, key: &str) -> Option<&[u8]> {
        let now = now_unix();
        self.apply_time_maintenance(now);
        if self.blobs.contains_key(key) {
            self.evictor.record_access(key);
            self.last_activity_unix = now;
        }
        self.blobs.get(key).map(|v| v.as_slice())
    }

    /// Remove a blob by key.
    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.evictor.remove_key(key);
        self.blobs.remove(key)
    }

    /// Check if a key has a blob.
    pub fn contains(&self, key: &str) -> bool {
        self.blobs.contains_key(key)
    }

    /// Number of stored blobs.
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// Total bytes across all blobs.
    pub fn total_bytes(&self) -> usize {
        self.blobs.values().map(|v| v.len()).sum()
    }

    /// Iterate all (key, blob) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.blobs.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }

    pub fn set_max_entries(&mut self, max_entries: usize) {
        self.apply_time_maintenance(now_unix());
        self.max_entries = max_entries.max(1);
        self.memory_governor.config.target = self.max_entries as f64;
        self.enforce_pressure();
    }

    pub fn stats(&self) -> BlobStoreStats {
        let gain_violated = self.memory_governor.validate_params().is_err();
        BlobStoreStats {
            tracked_blobs: self.blobs.len(),
            guarded_blobs: self.evictor.stats.guarded_items,
            reclaimable_blobs: self.evictor.stats.reclaimable_items,
            total_bytes: self.total_bytes(),
            max_entries: self.max_entries,
            last_eviction_count: self.last_eviction_count,
            governor_epsilon: self.memory_governor.epsilon,
            governor_error: self.memory_governor.error(self.blobs.len() as f64),
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

    pub fn maintenance_tick(&mut self, now_unix: u64) -> bool {
        self.apply_time_maintenance(now_unix)
    }

    pub fn soft_reset_governor(&mut self) {
        self.memory_governor.reset();
    }

    fn enforce_pressure(&mut self) {
        self.last_eviction_count = 0;
        if self.blobs.is_empty() {
            return;
        }
        let observed = self.blobs.len() as f64;
        let _ = self.memory_governor.step(observed);
        if self.blobs.len() <= self.max_entries {
            return;
        }
        let overage = self.blobs.len().saturating_sub(self.max_entries);
        let budget = self.memory_governor.epsilon.ceil().max(1.0) as usize;
        let candidate_count = budget.max(overage);
        let candidates = self.evictor.eviction_candidates(candidate_count);
        for key in candidates {
            if self.blobs.len() <= self.max_entries {
                break;
            }
            if self.evictor.is_guarded(&key) {
                continue;
            }
            self.blobs.remove(&key);
            self.evictor.remove_key(&key);
            self.last_eviction_count += 1;
        }
        if self.blobs.len() > self.max_entries {
            // Engineering boundary: the verified Chebyshev primitive is
            // stateless, so capacity enforcement falls back to least-live
            // eviction when the reclaimable set is insufficient.
            for key in self.evictor.least_live_keys(self.blobs.len()) {
                if self.blobs.len() <= self.max_entries {
                    break;
                }
                if self.blobs.remove(&key).is_some() {
                    self.evictor.remove_key(&key);
                    self.last_eviction_count += 1;
                }
            }
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
        if !self.blobs.is_empty()
            && !self.memory_governor.is_from_rest()
            && now_unix.saturating_sub(self.last_activity_unix) >= storage_reset_window_secs()
        {
            self.memory_governor.reset();
            changed = true;
        }
        changed
    }
}

impl Default for BlobStore {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn default_blob_ceiling() -> usize {
    100_000
}

pub(crate) fn storage_reset_window_secs() -> u64 {
    30
}

pub(crate) fn default_blob_memory_governor_config() -> GovernorConfig {
    GovernorConfig {
        instance_id: "gov-memory-blob".to_string(),
        alpha: 0.01,
        beta: 0.05,
        dt: 1.0,
        eps_min: 1.0,
        eps_max: 512.0,
        target: default_blob_ceiling() as f64,
        formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime".to_string(),
        ki: 0.001,
        kb: 0.0316, // sqrt(0.001)
        adaptive: Some(crate::governor::AdaptiveGainSchedule::default()),
    }
}

fn default_blob_memory_governor() -> GovernorState {
    GovernorState::new(default_blob_memory_governor_config())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_entries_survive_pressure_eviction() {
        let mut store = BlobStore::new();
        store.set_max_entries(2);
        store.put("hot", b"hot".to_vec());
        store.put("warm", b"warm".to_vec());
        let _ = store.get_with_access("hot");
        store.put("cold", b"cold".to_vec());
        assert!(
            store.get("hot").is_some(),
            "guarded blob must remain stored"
        );
        assert!(store.len() <= 2);
    }

    #[test]
    fn least_live_fallback_restores_capacity() {
        let mut store = BlobStore::new();
        store.set_max_entries(1);
        store.put("keep", b"keep".to_vec());
        store.put("drop", b"drop".to_vec());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn stats_report_governor_and_guard_counts() {
        let mut store = BlobStore::new();
        store.put("a", b"hello".to_vec());
        let stats = store.stats();
        assert_eq!(stats.tracked_blobs, 1);
        assert!(stats.governor_epsilon >= 1.0);
        assert!(!stats.formal_basis.is_empty());
    }

    #[test]
    fn maintenance_tick_decays_by_elapsed_time_not_operation_count() {
        let mut store = BlobStore::new();
        store.put("alpha", b"payload".to_vec());
        let before = store.evictor.liveness["alpha"];
        let future = store.last_maintenance_unix + 5;
        store.maintenance_tick(future);
        assert!(store.evictor.liveness["alpha"] < before);
    }

    #[test]
    fn quiescent_maintenance_restores_from_rest_regime() {
        let mut store = BlobStore::new();
        store.put("alpha", b"payload".to_vec());
        assert!(!store.memory_governor.is_from_rest());
        let future = store.last_activity_unix + storage_reset_window_secs();
        store.maintenance_tick(future);
        assert!(store.memory_governor.is_from_rest());
    }
}
