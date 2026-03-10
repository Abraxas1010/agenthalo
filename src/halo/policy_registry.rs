//! Versioned governance policy registry.
//!
//! Collects all governance policy parameters scattered across the codebase into
//! a single auditable snapshot with a content-addressed digest.
//!
//! Use cases:
//!   - Audit: "what governance rules were active when this decision was made?"
//!   - Drift detection: digest changes iff any governance parameter changes.
//!   - Cross-component consistency validation at startup or in tests.

use crate::blob_store::{
    default_blob_ceiling, default_blob_memory_governor_config,
    storage_reset_window_secs as blob_storage_reset_window_secs,
};
use crate::halo::chebyshev_evictor::{
    DEFAULT_CHEBYSHEV_DECAY_RATE, DEFAULT_CHEBYSHEV_IMPULSE_MAGNITUDE, DEFAULT_CHEBYSHEV_K,
};
use crate::halo::governor::GovernorConfig;
use crate::halo::governor_registry::GovernorRegistry;
use crate::halo::hash::{self, HashAlgorithm};
use crate::halo::identity::{IdentitySecurityTier, DEFAULT_SECURITY_TIER};
use crate::halo::trust::security_tier_trust_floor;
use crate::halo::trust_score::{
    TRUST_FLOOR_ANCHORED, TRUST_FLOOR_STAKED, TRUST_THRESHOLD_ATTEST, TRUST_THRESHOLD_FORMATION,
    TRUST_THRESHOLD_GOSSIP, TRUST_THRESHOLD_PING_ONLY, TRUST_THRESHOLD_ROUTE_HIGH,
    TRUST_THRESHOLD_ROUTE_LOW,
};
use crate::vector_index::{
    default_vector_ceiling, default_vector_memory_governor_config,
    storage_reset_window_secs as vector_storage_reset_window_secs,
};
use serde::{Deserialize, Serialize};

/// Schema version. Bump when the set of collected policies changes.
pub const POLICY_SCHEMA_VERSION: u32 = 1;

/// Production trust tier thresholds (from `trust.rs:trust_tier()`).
pub const TIER_THRESHOLD_HIGH: f64 = 0.85;
pub const TIER_THRESHOLD_MEDIUM: f64 = 0.65;
pub const TIER_THRESHOLD_CAUTIOUS: f64 = 0.40;

/// Production trust scoring weights (from `trust.rs:query_trust_score()`).
pub const WEIGHT_BASE: f64 = 0.20;
pub const WEIGHT_COMPLETION: f64 = 0.30;
pub const WEIGHT_PAID_SUCCESS: f64 = 0.25;
pub const WEIGHT_ATTESTATION: f64 = 0.15;
pub const WEIGHT_RECENCY: f64 = 0.10;
pub const WEIGHT_SESSION_BONUS: f64 = 0.02;

/// P2P challenge difficulty weights (from `trust_score.rs:compute_trust()`).
pub const CHALLENGE_WEIGHT_PING: f64 = 0.1;
pub const CHALLENGE_WEIGHT_STANDARD: f64 = 1.0;
pub const CHALLENGE_WEIGHT_DEEP: f64 = 5.0;
pub const CHALLENGE_FAILURE_PENALTY: f64 = -2.0;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PolicySnapshot {
    pub schema_version: u32,
    pub timestamp: u64,
    pub entries: Vec<PolicyEntry>,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PolicyEntry {
    pub id: String,
    pub source: String,
    pub category: PolicyCategory,
    pub values: Vec<(String, PolicyValue)>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formal_basis: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PolicyCategory {
    TrustFloor,
    TrustThreshold,
    TrustScoring,
    GovernorControl,
    EvictionGuard,
    NetworkHealth,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PolicyValue {
    Float(f64),
    Text(String),
}

impl PolicyValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PolicyValue::Float(v) => Some(*v),
            PolicyValue::Text(_) => None,
        }
    }
}

impl std::fmt::Display for PolicyValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyValue::Float(v) => write!(f, "{v}"),
            PolicyValue::Text(s) => write!(f, "{s}"),
        }
    }
}

/// Collect all governance policy parameters into a snapshot.
///
/// If `governor_registry` is provided, governor configurations are included.
/// Otherwise only static policy constants are collected.
pub fn collect_snapshot(
    governor_registry: Option<&GovernorRegistry>,
    timestamp: u64,
) -> PolicySnapshot {
    let mut entries = base_policy_entries();
    entries.extend(storage_governor_entries());

    if let Some(registry) = governor_registry {
        for config in registry.configs_all() {
            entries.push(governor_config_entry(
                "halo::governor_registry",
                &config.instance_id,
                &config,
            ));
        }
    } else {
        for config in default_runtime_governor_configs() {
            entries.push(governor_config_entry(
                "halo::governor_registry",
                &config.instance_id,
                &config,
            ));
        }
    }

    let digest = snapshot_digest(&entries);

    PolicySnapshot {
        schema_version: POLICY_SCHEMA_VERSION,
        timestamp,
        entries,
        digest,
    }
}

/// Collect a snapshot using the default governor registry configurations
/// (without requiring a live registry). Suitable for tests and audits.
pub fn collect_static_snapshot(timestamp: u64) -> PolicySnapshot {
    collect_snapshot(None, timestamp)
}

/// Recompute the content-addressed digest for a set of policy entries.
///
/// This deliberately excludes timestamp/schema metadata so callers can verify
/// that digest drift is driven by policy changes only.
pub fn recompute_digest(entries: &[PolicyEntry]) -> String {
    snapshot_digest(entries)
}

fn base_policy_entries() -> Vec<PolicyEntry> {
    vec![
        PolicyEntry {
            id: "p2p_trust_floors".into(),
            source: "halo::trust_score".into(),
            category: PolicyCategory::TrustFloor,
            values: vec![
                ("anchored".into(), PolicyValue::Float(TRUST_FLOOR_ANCHORED)),
                ("staked".into(), PolicyValue::Float(TRUST_FLOOR_STAKED)),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "production_trust_floors".into(),
            source: "halo::trust".into(),
            category: PolicyCategory::TrustFloor,
            values: vec![
                (
                    "max_safe".into(),
                    PolicyValue::Float(security_tier_trust_floor(&IdentitySecurityTier::MaxSafe)),
                ),
                (
                    "less_safe".into(),
                    PolicyValue::Float(security_tier_trust_floor(&IdentitySecurityTier::LessSafe)),
                ),
                (
                    "low_security".into(),
                    PolicyValue::Float(security_tier_trust_floor(
                        &IdentitySecurityTier::LowSecurity,
                    )),
                ),
                (
                    "default_tier".into(),
                    PolicyValue::Text(format!("{:?}", DEFAULT_SECURITY_TIER)),
                ),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "p2p_trust_thresholds".into(),
            source: "halo::trust_score".into(),
            category: PolicyCategory::TrustThreshold,
            values: vec![
                (
                    "ping_only".into(),
                    PolicyValue::Float(TRUST_THRESHOLD_PING_ONLY),
                ),
                ("gossip".into(), PolicyValue::Float(TRUST_THRESHOLD_GOSSIP)),
                (
                    "route_low".into(),
                    PolicyValue::Float(TRUST_THRESHOLD_ROUTE_LOW),
                ),
                (
                    "route_high".into(),
                    PolicyValue::Float(TRUST_THRESHOLD_ROUTE_HIGH),
                ),
                ("attest".into(), PolicyValue::Float(TRUST_THRESHOLD_ATTEST)),
                (
                    "formation".into(),
                    PolicyValue::Float(TRUST_THRESHOLD_FORMATION),
                ),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "production_trust_tiers".into(),
            source: "halo::trust".into(),
            category: PolicyCategory::TrustThreshold,
            values: vec![
                ("high".into(), PolicyValue::Float(TIER_THRESHOLD_HIGH)),
                ("medium".into(), PolicyValue::Float(TIER_THRESHOLD_MEDIUM)),
                (
                    "cautious".into(),
                    PolicyValue::Float(TIER_THRESHOLD_CAUTIOUS),
                ),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "production_scoring_weights".into(),
            source: "halo::trust".into(),
            category: PolicyCategory::TrustScoring,
            values: vec![
                ("base".into(), PolicyValue::Float(WEIGHT_BASE)),
                ("completion".into(), PolicyValue::Float(WEIGHT_COMPLETION)),
                (
                    "paid_success".into(),
                    PolicyValue::Float(WEIGHT_PAID_SUCCESS),
                ),
                ("attestation".into(), PolicyValue::Float(WEIGHT_ATTESTATION)),
                ("recency".into(), PolicyValue::Float(WEIGHT_RECENCY)),
                (
                    "session_bonus".into(),
                    PolicyValue::Float(WEIGHT_SESSION_BONUS),
                ),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "challenge_weights".into(),
            source: "halo::trust_score".into(),
            category: PolicyCategory::TrustScoring,
            values: vec![
                ("ping".into(), PolicyValue::Float(CHALLENGE_WEIGHT_PING)),
                (
                    "standard".into(),
                    PolicyValue::Float(CHALLENGE_WEIGHT_STANDARD),
                ),
                ("deep".into(), PolicyValue::Float(CHALLENGE_WEIGHT_DEEP)),
                (
                    "failure_penalty".into(),
                    PolicyValue::Float(CHALLENGE_FAILURE_PENALTY),
                ),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "network_health_thresholds".into(),
            source: "halo::trust_score".into(),
            category: PolicyCategory::NetworkHealth,
            values: vec![
                (
                    "attack_challenge_failure_rate".into(),
                    PolicyValue::Float(0.5),
                ),
                (
                    "attack_topology_overlap_min".into(),
                    PolicyValue::Float(0.1),
                ),
                (
                    "attack_beacon_consensus_min".into(),
                    PolicyValue::Float(0.3),
                ),
                (
                    "degraded_min_discovery_channels".into(),
                    PolicyValue::Float(2.0),
                ),
                (
                    "degraded_min_reachable_beacons".into(),
                    PolicyValue::Float(2.0),
                ),
                (
                    "degraded_max_new_peer_rate".into(),
                    PolicyValue::Float(50.0),
                ),
            ],
            formal_basis: None,
        },
        PolicyEntry {
            id: "vector_storage_eviction".into(),
            source: "vector_index".into(),
            category: PolicyCategory::EvictionGuard,
            values: vec![
                (
                    "max_entries".into(),
                    PolicyValue::Float(default_vector_ceiling() as f64),
                ),
                (
                    "reset_window_secs".into(),
                    PolicyValue::Float(vector_storage_reset_window_secs() as f64),
                ),
                (
                    "chebyshev_k".into(),
                    PolicyValue::Float(DEFAULT_CHEBYSHEV_K),
                ),
                (
                    "decay_rate".into(),
                    PolicyValue::Float(DEFAULT_CHEBYSHEV_DECAY_RATE),
                ),
                (
                    "impulse_magnitude".into(),
                    PolicyValue::Float(DEFAULT_CHEBYSHEV_IMPULSE_MAGNITUDE),
                ),
            ],
            formal_basis: Some("HeytingLean.Bridge.Sharma.AetherChebyshev.chebyshev_finite".into()),
        },
        PolicyEntry {
            id: "blob_storage_eviction".into(),
            source: "blob_store".into(),
            category: PolicyCategory::EvictionGuard,
            values: vec![
                (
                    "max_entries".into(),
                    PolicyValue::Float(default_blob_ceiling() as f64),
                ),
                (
                    "reset_window_secs".into(),
                    PolicyValue::Float(blob_storage_reset_window_secs() as f64),
                ),
                (
                    "chebyshev_k".into(),
                    PolicyValue::Float(DEFAULT_CHEBYSHEV_K),
                ),
                (
                    "decay_rate".into(),
                    PolicyValue::Float(DEFAULT_CHEBYSHEV_DECAY_RATE),
                ),
                (
                    "impulse_magnitude".into(),
                    PolicyValue::Float(DEFAULT_CHEBYSHEV_IMPULSE_MAGNITUDE),
                ),
            ],
            formal_basis: Some("HeytingLean.Bridge.Sharma.AetherChebyshev.chebyshev_finite".into()),
        },
    ]
}

fn governor_config_entry(source: &str, instance_id: &str, config: &GovernorConfig) -> PolicyEntry {
    PolicyEntry {
        id: format!("governor_{}", instance_id.replace('-', "_")),
        source: source.into(),
        category: PolicyCategory::GovernorControl,
        values: vec![
            ("alpha".into(), PolicyValue::Float(config.alpha)),
            ("beta".into(), PolicyValue::Float(config.beta)),
            ("dt".into(), PolicyValue::Float(config.dt)),
            ("eps_min".into(), PolicyValue::Float(config.eps_min)),
            ("eps_max".into(), PolicyValue::Float(config.eps_max)),
            ("target".into(), PolicyValue::Float(config.target)),
            (
                "gamma".into(),
                PolicyValue::Float(config.alpha + config.beta / config.dt),
            ),
        ],
        formal_basis: Some(config.formal_basis.clone()),
    }
}

fn storage_governor_entries() -> Vec<PolicyEntry> {
    [
        ("vector_index", default_vector_memory_governor_config()),
        ("blob_store", default_blob_memory_governor_config()),
    ]
    .into_iter()
    .map(|(source, config)| governor_config_entry(source, &config.instance_id, &config))
    .collect()
}

/// The 5 default runtime governor configurations (mirrors `build_default_registry()`).
pub fn default_runtime_governor_configs() -> Vec<GovernorConfig> {
    vec![
        GovernorConfig {
            instance_id: "gov-proxy".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 50.0,
            target: 2.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.lyapunov_descent".to_string(),
        },
        GovernorConfig {
            instance_id: "gov-comms".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 32.0,
            target: 10.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime".to_string(),
        },
        GovernorConfig {
            instance_id: "gov-compute".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 1.0,
            eps_max: 10.0,
            target: 8.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime".to_string(),
        },
        GovernorConfig {
            instance_id: "gov-cost".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 0.01,
            eps_max: 10.0,
            target: 1.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime".to_string(),
        },
        GovernorConfig {
            instance_id: "gov-pty".to_string(),
            alpha: 0.01,
            beta: 0.05,
            dt: 1.0,
            eps_min: 30.0,
            eps_max: 900.0,
            target: 120.0,
            formal_basis: "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime".to_string(),
        },
    ]
}

fn snapshot_digest(entries: &[PolicyEntry]) -> String {
    let mut payload = format!("agenthalo.policy.v{POLICY_SCHEMA_VERSION}");
    for entry in entries {
        payload.push(':');
        payload.push_str(&entry.id);
        for (key, value) in &entry.values {
            payload.push(':');
            payload.push_str(key);
            payload.push('=');
            payload.push_str(&value.to_string());
        }
    }
    hash::hash_hex(&HashAlgorithm::CURRENT, payload.as_bytes())
}

/// Validate cross-component invariants across all governance policies.
///
/// Returns a list of violated invariants. Empty list = all invariants hold.
pub fn validate_invariants(snapshot: &PolicySnapshot) -> Vec<String> {
    let mut violations = Vec::new();

    // Invariant 1: All production floors must be below the cautious threshold.
    let cautious = TIER_THRESHOLD_CAUTIOUS;
    if let Some(entry) = snapshot
        .entries
        .iter()
        .find(|e| e.id == "production_trust_floors")
    {
        for (tier_name, value) in &entry.values {
            if let Some(floor) = value.as_f64() {
                if floor >= cautious {
                    violations.push(format!(
                        "production floor '{tier_name}' ({floor}) >= cautious threshold ({cautious})"
                    ));
                }
            }
        }
    }

    // Invariant 2: P2P trust thresholds must be monotonically increasing.
    if let Some(entry) = snapshot
        .entries
        .iter()
        .find(|e| e.id == "p2p_trust_thresholds")
    {
        let ordered_keys = [
            "ping_only",
            "gossip",
            "route_low",
            "route_high",
            "attest",
            "formation",
        ];
        let values: Vec<f64> = ordered_keys
            .iter()
            .filter_map(|key| {
                entry
                    .values
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| v.as_f64())
            })
            .collect();
        for window in values.windows(2) {
            if window[0] > window[1] {
                violations.push(format!(
                    "P2P trust threshold ordering violated: {:.1} > {:.1}",
                    window[0], window[1]
                ));
            }
        }
    }

    // Invariant 3: Production scoring weights must sum to ≤ 1.0 (base + components).
    if let Some(entry) = snapshot
        .entries
        .iter()
        .find(|e| e.id == "production_scoring_weights")
    {
        let weight_sum: f64 = entry
            .values
            .iter()
            .filter(|(k, _)| k != "session_bonus") // bonus is additive, not part of base weights
            .filter_map(|(_, v)| v.as_f64())
            .sum();
        if weight_sum > 1.0 + 1e-10 {
            violations.push(format!(
                "production scoring weights sum to {weight_sum:.4} > 1.0"
            ));
        }
    }

    // Invariant 4: All governor instances must satisfy the gain condition γ < 1.
    for entry in snapshot
        .entries
        .iter()
        .filter(|e| e.category == PolicyCategory::GovernorControl)
    {
        if let Some(gamma) = entry
            .values
            .iter()
            .find(|(k, _)| k == "gamma")
            .and_then(|(_, v)| v.as_f64())
        {
            if gamma >= 1.0 {
                violations.push(format!(
                    "governor '{}' has γ={gamma:.6} >= 1.0 (gain condition violated)",
                    entry.id
                ));
            }
        }
    }

    // Invariant 5: P2P diode floor for Staked must be below route_high threshold.
    // (A floored peer loses high-value routing but is not annihilated.)
    if TRUST_FLOOR_STAKED >= TRUST_THRESHOLD_ROUTE_HIGH {
        violations.push(format!(
            "TRUST_FLOOR_STAKED ({TRUST_FLOOR_STAKED}) >= TRUST_THRESHOLD_ROUTE_HIGH ({TRUST_THRESHOLD_ROUTE_HIGH}): \
             floor should not auto-qualify for high-value routing"
        ));
    }

    // Invariant 6: Challenge failure penalty must be negative.
    if CHALLENGE_FAILURE_PENALTY >= 0.0 {
        violations.push(format!(
            "challenge failure penalty ({CHALLENGE_FAILURE_PENALTY}) is non-negative"
        ));
    }

    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_snapshot_is_deterministic() {
        let a = collect_static_snapshot(1000);
        let b = collect_static_snapshot(2000);
        // Digest should be the same (timestamp is not part of digest).
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.entries.len(), b.entries.len());
    }

    #[test]
    fn static_snapshot_has_all_policy_categories() {
        let snapshot = collect_static_snapshot(0);
        let categories: Vec<PolicyCategory> = snapshot.entries.iter().map(|e| e.category).collect();
        assert!(categories.contains(&PolicyCategory::TrustFloor));
        assert!(categories.contains(&PolicyCategory::TrustThreshold));
        assert!(categories.contains(&PolicyCategory::TrustScoring));
        assert!(categories.contains(&PolicyCategory::GovernorControl));
        assert!(categories.contains(&PolicyCategory::EvictionGuard));
        assert!(categories.contains(&PolicyCategory::NetworkHealth));
    }

    #[test]
    fn static_snapshot_includes_all_governors() {
        let snapshot = collect_static_snapshot(0);
        let gov_ids: Vec<&str> = snapshot
            .entries
            .iter()
            .filter(|e| e.category == PolicyCategory::GovernorControl)
            .map(|e| e.id.as_str())
            .collect();
        assert!(gov_ids.contains(&"governor_gov_proxy"));
        assert!(gov_ids.contains(&"governor_gov_comms"));
        assert!(gov_ids.contains(&"governor_gov_compute"));
        assert!(gov_ids.contains(&"governor_gov_cost"));
        assert!(gov_ids.contains(&"governor_gov_pty"));
        assert!(gov_ids.contains(&"governor_gov_memory_vector"));
        assert!(gov_ids.contains(&"governor_gov_memory_blob"));
    }

    #[test]
    fn all_invariants_hold() {
        let snapshot = collect_static_snapshot(0);
        let violations = validate_invariants(&snapshot);
        assert!(
            violations.is_empty(),
            "governance invariant violations: {violations:?}"
        );
    }

    #[test]
    fn default_runtime_governor_configs_match_registry() {
        // Verify that default_runtime_governor_configs() matches build_default_registry().
        let configs = default_runtime_governor_configs();
        let registry = crate::halo::governor_registry::build_default_registry();
        let snapshots = registry.snapshot_all();
        assert_eq!(configs.len(), snapshots.len());
        for config in &configs {
            let snapshot = snapshots
                .iter()
                .find(|s| s.instance_id == config.instance_id)
                .unwrap_or_else(|| {
                    panic!(
                        "governor '{}' not found in default registry",
                        config.instance_id
                    )
                });
            assert_eq!(snapshot.target, config.target);
        }
    }

    #[test]
    fn live_snapshot_uses_real_governor_configs() {
        let registry = crate::halo::governor_registry::build_default_registry();
        let snapshot = collect_snapshot(Some(&registry), 0);
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.id == "governor_gov_proxy")
            .expect("gov-proxy entry");
        let alpha = entry
            .values
            .iter()
            .find(|(key, _)| key == "alpha")
            .and_then(|(_, value)| value.as_f64())
            .expect("alpha");
        let beta = entry
            .values
            .iter()
            .find(|(key, _)| key == "beta")
            .and_then(|(_, value)| value.as_f64())
            .expect("beta");
        let eps_min = entry
            .values
            .iter()
            .find(|(key, _)| key == "eps_min")
            .and_then(|(_, value)| value.as_f64())
            .expect("eps_min");
        let eps_max = entry
            .values
            .iter()
            .find(|(key, _)| key == "eps_max")
            .and_then(|(_, value)| value.as_f64())
            .expect("eps_max");
        assert_eq!(alpha, 0.01);
        assert_eq!(beta, 0.05);
        assert_eq!(eps_min, 1.0);
        assert_eq!(eps_max, 50.0);
    }

    #[test]
    fn digest_changes_when_policy_changes() {
        let snapshot = collect_static_snapshot(0);
        let original_digest = snapshot.digest.clone();

        // Mutate a policy entry and verify digest changes.
        let mut entries = snapshot.entries.clone();
        if let Some(entry) = entries.iter_mut().find(|e| e.id == "p2p_trust_floors") {
            entry.values[0].1 = PolicyValue::Float(999.0);
        }
        let mutated_digest = snapshot_digest(&entries);
        assert_ne!(original_digest, mutated_digest);
    }
}
