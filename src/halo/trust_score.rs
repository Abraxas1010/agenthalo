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
            let decay = 0.5_f64.powf(age as f64 / half_life_secs as f64);
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
    fn required_tiers_follow_operation_risk() {
        assert_eq!(required_tier("gossip"), IdentityTier::Anonymous);
        assert_eq!(required_tier("receive_routed_work"), IdentityTier::Verified);
        assert_eq!(required_tier("high_value_task"), IdentityTier::Anchored);
        assert_eq!(required_tier("hold_escrow"), IdentityTier::Staked);
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
