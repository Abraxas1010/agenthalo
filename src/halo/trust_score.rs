use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChallengeDifficulty {
    Ping,
    Standard,
    Deep,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VerificationRecord {
    pub peer_did: String,
    pub capability_domain: String,
    pub challenge_difficulty: ChallengeDifficulty,
    pub passed: bool,
    pub elapsed_ms: u64,
    pub verified_at: u64,
}

pub fn compute_trust(records: &[VerificationRecord], now: u64, half_life_secs: u64) -> f64 {
    if half_life_secs == 0 {
        return 0.0;
    }
    records
        .iter()
        .map(|record| {
            let age = now.saturating_sub(record.verified_at);
            let decay_steps = age / half_life_secs;
            let decay = 0.5_f64.powi(decay_steps.min(i32::MAX as u64) as i32);
            let difficulty_weight = match record.challenge_difficulty {
                ChallengeDifficulty::Ping => 0.1,
                ChallengeDifficulty::Standard => 1.0,
                ChallengeDifficulty::Deep => 5.0,
            };
            let sign = if record.passed { 1.0 } else { -2.0 };
            sign * decay * difficulty_weight
        })
        .sum::<f64>()
        .max(0.0)
}

pub const TRUST_THRESHOLD_PING_ONLY: f64 = 0.0;
pub const TRUST_THRESHOLD_GOSSIP: f64 = 0.1;
pub const TRUST_THRESHOLD_ROUTE_LOW: f64 = 0.5;
pub const TRUST_THRESHOLD_ROUTE_HIGH: f64 = 5.0;
pub const TRUST_THRESHOLD_ATTEST: f64 = 10.0;
pub const TRUST_THRESHOLD_FORMATION: f64 = 15.0;

/// Diode floor constants: minimum trust that cannot be driven below by
/// adversarial challenge bursts. Only applies to tiers that have earned
/// durable identity standing. Anonymous and Verified peers have no floor
/// (their trust can reach zero).
///
/// Floor values are deliberately below operational routing thresholds so
/// a floored peer loses privileges (routing requires 5.0) but is not
/// annihilated — preserving the ability to recover through legitimate
/// re-verification rather than requiring full re-onboarding.
pub const TRUST_FLOOR_ANCHORED: f64 = 0.5;
pub const TRUST_FLOOR_STAKED: f64 = 2.0;

/// One-way floor on trust score keyed by identity tier.
///
/// Prevents coordinated Sybil attacks from driving an established peer's
/// trust to zero within a single half-life window. The floor is a hard
/// minimum: `max(raw_trust, floor_for_tier)`.
///
/// Anonymous and Verified peers have no floor (returns `trust` unchanged).
pub fn diode_floor(trust: f64, tier: IdentityTier) -> f64 {
    let floor = match tier {
        IdentityTier::Anonymous | IdentityTier::Verified => return trust,
        IdentityTier::Anchored => TRUST_FLOOR_ANCHORED,
        IdentityTier::Staked => TRUST_FLOOR_STAKED,
    };
    trust.max(floor)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IdentityTier {
    Anonymous = 0,
    Verified = 1,
    Anchored = 2,
    Staked = 3,
}

pub fn required_tier(operation: &str) -> IdentityTier {
    match operation {
        "gossip" | "discover" | "challenge" => IdentityTier::Anonymous,
        "receive_routed_work" => IdentityTier::Verified,
        "high_value_task" | "multi_agent_formation" => IdentityTier::Anchored,
        "hold_escrow" | "bridge_custody" => IdentityTier::Staked,
        _ => IdentityTier::Verified,
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkHealthMetrics {
    pub challenge_failure_rate: f64,
    pub active_discovery_channels: u32,
    pub topology_overlap_ratio: f64,
    pub reachable_beacons: u32,
    pub beacon_consensus_ratio: f64,
    pub new_peer_rate_per_minute: f64,
    pub anchored_peer_ratio: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum NetworkPosture {
    Healthy,
    Degraded { actions: Vec<&'static str> },
    UnderAttack { actions: Vec<&'static str> },
}

pub fn assess_network_health(metrics: &NetworkHealthMetrics) -> NetworkPosture {
    if metrics.challenge_failure_rate > 0.5
        || metrics.topology_overlap_ratio < 0.1
        || metrics.beacon_consensus_ratio < 0.3
    {
        NetworkPosture::UnderAttack {
            actions: vec![
                "raise challenge difficulty to Deep for all new peers",
                "require Anchored identity tier for routed work",
                "re-verify all peers above TRUST_THRESHOLD_ROUTE_LOW",
                "alert operator via container health endpoint",
                "increase trust decay rate (halve the half-life)",
                "log full peer topology for forensic analysis",
            ],
        }
    } else if metrics.active_discovery_channels < 2
        || metrics.reachable_beacons < 2
        || metrics.new_peer_rate_per_minute > 50.0
    {
        NetworkPosture::Degraded {
            actions: vec![
                "raise challenge difficulty to Standard minimum",
                "fall back to direct DHT queries if beacons < quorum",
                "reduce trust decay half-life by 50%",
                "increase topology cross-check frequency",
            ],
        }
    } else {
        NetworkPosture::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_trust_applies_difficulty_and_decay() {
        let ping = VerificationRecord {
            peer_did: "did:key:a".to_string(),
            capability_domain: "prove/lean".to_string(),
            challenge_difficulty: ChallengeDifficulty::Ping,
            passed: true,
            elapsed_ms: 50,
            verified_at: 100,
        };
        let deep = VerificationRecord {
            peer_did: "did:key:a".to_string(),
            capability_domain: "prove/lean".to_string(),
            challenge_difficulty: ChallengeDifficulty::Deep,
            passed: true,
            elapsed_ms: 500,
            verified_at: 100,
        };
        let failed = VerificationRecord {
            peer_did: "did:key:a".to_string(),
            capability_domain: "prove/lean".to_string(),
            challenge_difficulty: ChallengeDifficulty::Standard,
            passed: false,
            elapsed_ms: 200,
            verified_at: 100,
        };
        let trust = compute_trust(&[ping, deep, failed], 100, 3600);
        assert!(trust > 3.0, "trust={trust}");
    }

    #[test]
    fn compute_trust_floors_at_zero() {
        let records = vec![VerificationRecord {
            peer_did: "did:key:a".to_string(),
            capability_domain: "prove/lean".to_string(),
            challenge_difficulty: ChallengeDifficulty::Deep,
            passed: false,
            elapsed_ms: 500,
            verified_at: 100,
        }];
        assert_eq!(compute_trust(&records, 100, 3600), 0.0);
    }

    #[test]
    fn compute_trust_halves_at_each_half_life_boundary() {
        let record = VerificationRecord {
            peer_did: "did:key:a".to_string(),
            capability_domain: "prove/lean".to_string(),
            challenge_difficulty: ChallengeDifficulty::Standard,
            passed: true,
            elapsed_ms: 10,
            verified_at: 0,
        };
        assert_eq!(compute_trust(std::slice::from_ref(&record), 0, 10), 1.0);
        assert_eq!(compute_trust(std::slice::from_ref(&record), 9, 10), 1.0);
        assert_eq!(compute_trust(std::slice::from_ref(&record), 10, 10), 0.5);
        assert_eq!(compute_trust(&[record], 20, 10), 0.25);
    }

    #[test]
    fn required_tiers_follow_operation_risk() {
        assert_eq!(required_tier("gossip"), IdentityTier::Anonymous);
        assert_eq!(required_tier("receive_routed_work"), IdentityTier::Verified);
        assert_eq!(required_tier("high_value_task"), IdentityTier::Anchored);
        assert_eq!(required_tier("hold_escrow"), IdentityTier::Staked);
    }

    #[test]
    fn diode_floor_protects_anchored_peer() {
        assert_eq!(
            diode_floor(0.0, IdentityTier::Anchored),
            TRUST_FLOOR_ANCHORED
        );
        assert_eq!(diode_floor(10.0, IdentityTier::Anchored), 10.0);
        assert_eq!(
            diode_floor(TRUST_FLOOR_ANCHORED - 0.1, IdentityTier::Anchored),
            TRUST_FLOOR_ANCHORED
        );
    }

    #[test]
    fn diode_floor_protects_staked_peer() {
        assert_eq!(diode_floor(0.0, IdentityTier::Staked), TRUST_FLOOR_STAKED);
        assert_eq!(diode_floor(15.0, IdentityTier::Staked), 15.0);
        assert_eq!(
            diode_floor(TRUST_FLOOR_STAKED - 0.5, IdentityTier::Staked),
            TRUST_FLOOR_STAKED
        );
    }

    #[test]
    fn diode_floor_is_transparent_for_low_tiers() {
        assert_eq!(diode_floor(0.0, IdentityTier::Anonymous), 0.0);
        assert_eq!(diode_floor(0.0, IdentityTier::Verified), 0.0);
        assert_eq!(diode_floor(5.0, IdentityTier::Anonymous), 5.0);
        assert_eq!(diode_floor(5.0, IdentityTier::Verified), 5.0);
    }

    #[test]
    fn sybil_burst_cannot_annihilate_staked_peer() {
        // Simulate a Sybil attack: 20 rapid failed Deep challenges against a
        // Staked peer that previously had strong trust from legitimate work.
        let legitimate = VerificationRecord {
            peer_did: "did:key:staked".to_string(),
            capability_domain: "prove/lean".to_string(),
            challenge_difficulty: ChallengeDifficulty::Deep,
            passed: true,
            elapsed_ms: 400,
            verified_at: 100,
        };
        let attack_records: Vec<VerificationRecord> = (0..20)
            .map(|i| VerificationRecord {
                peer_did: "did:key:staked".to_string(),
                capability_domain: "prove/lean".to_string(),
                challenge_difficulty: ChallengeDifficulty::Deep,
                passed: false,
                elapsed_ms: 10,
                verified_at: 200 + i,
            })
            .collect();

        let mut all_records = vec![legitimate];
        all_records.extend(attack_records);

        let raw_trust = compute_trust(&all_records, 220, 3600);
        // Raw trust is driven to zero by the attack burst
        assert_eq!(
            raw_trust, 0.0,
            "raw trust should be floored at 0.0 by .max(0.0)"
        );

        // But with the diode, the Staked peer retains minimum standing
        let protected = diode_floor(raw_trust, IdentityTier::Staked);
        assert_eq!(protected, TRUST_FLOOR_STAKED);
        // The peer loses high-value routing (requires 5.0) but is not annihilated
        assert!(protected < TRUST_THRESHOLD_ROUTE_HIGH);
        assert!(protected >= TRUST_FLOOR_STAKED);
    }

    #[test]
    fn network_health_escalates_defensive_posture() {
        assert!(matches!(
            assess_network_health(&NetworkHealthMetrics {
                challenge_failure_rate: 0.6,
                ..NetworkHealthMetrics::default()
            }),
            NetworkPosture::UnderAttack { .. }
        ));
        assert!(matches!(
            assess_network_health(&NetworkHealthMetrics {
                active_discovery_channels: 1,
                topology_overlap_ratio: 0.8,
                beacon_consensus_ratio: 0.9,
                ..NetworkHealthMetrics::default()
            }),
            NetworkPosture::Degraded { .. }
        ));
        assert_eq!(
            assess_network_health(&NetworkHealthMetrics {
                challenge_failure_rate: 0.05,
                active_discovery_channels: 3,
                topology_overlap_ratio: 0.7,
                reachable_beacons: 3,
                beacon_consensus_ratio: 0.9,
                new_peer_rate_per_minute: 2.0,
                anchored_peer_ratio: 0.4,
            }),
            NetworkPosture::Healthy
        );
    }
}
