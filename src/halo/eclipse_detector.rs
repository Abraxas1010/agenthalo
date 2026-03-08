use crate::halo::discovery_candidates::{DiscoveryCandidate, DiscoveryChannel};
use crate::halo::trust_score::ChallengeDifficulty;
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapConfidence {
    High,
    Moderate,
    Suspicious,
    Unverifiable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootstrapAction {
    Proceed,
    ProceedWithExtraChallenge {
        min_difficulty: ChallengeDifficulty,
    },
    Quarantine {
        reason: &'static str,
        retry_other_channels: bool,
        alert_operator: bool,
    },
}

pub fn verify_topology(
    peer_provided: &[String],
    independent_sources: &[DiscoveryCandidate],
) -> BootstrapConfidence {
    let strong_ids: HashSet<&str> = independent_sources
        .iter()
        .filter(|candidate| {
            candidate.channels.iter().any(|channel| {
                !matches!(
                    channel,
                    DiscoveryChannel::PeerGossip { .. } | DiscoveryChannel::Cached
                )
            })
        })
        .map(|candidate| candidate.peer_id.as_str())
        .collect();
    let cached_fallback_ids: HashSet<&str> = independent_sources
        .iter()
        .filter(|candidate| {
            candidate
                .channels
                .iter()
                .any(|channel| !matches!(channel, DiscoveryChannel::PeerGossip { .. }))
        })
        .map(|candidate| candidate.peer_id.as_str())
        .collect();
    let independent_ids = if strong_ids.is_empty() {
        cached_fallback_ids
    } else {
        strong_ids
    };

    if independent_ids.is_empty() {
        return BootstrapConfidence::Unverifiable;
    }

    let overlap = peer_provided
        .iter()
        .filter(|peer| independent_ids.contains(peer.as_str()))
        .count();

    if overlap == 0 {
        BootstrapConfidence::Suspicious
    } else if overlap * 2 >= independent_ids.len() {
        BootstrapConfidence::High
    } else {
        BootstrapConfidence::Moderate
    }
}

pub fn bootstrap_policy(confidence: BootstrapConfidence) -> BootstrapAction {
    match confidence {
        BootstrapConfidence::High => BootstrapAction::Proceed,
        BootstrapConfidence::Moderate => BootstrapAction::ProceedWithExtraChallenge {
            min_difficulty: ChallengeDifficulty::Standard,
        },
        BootstrapConfidence::Suspicious => BootstrapAction::Quarantine {
            reason: "zero overlap between peer-provided and independent discovery channels",
            retry_other_channels: true,
            alert_operator: true,
        },
        BootstrapConfidence::Unverifiable => BootstrapAction::ProceedWithExtraChallenge {
            min_difficulty: ChallengeDifficulty::Deep,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::Multiaddr;

    fn candidate(peer_id: &str, channels: Vec<DiscoveryChannel>) -> DiscoveryCandidate {
        DiscoveryCandidate {
            peer_id: peer_id.to_string(),
            addrs: vec!["/ip4/127.0.0.1/tcp/9001"
                .parse::<Multiaddr>()
                .expect("addr")],
            channels,
            discovered_at: 100,
            trust: 0.0,
        }
    }

    #[test]
    fn verify_topology_flags_zero_overlap_as_suspicious() {
        let confidence = verify_topology(
            &["peer-x".to_string()],
            &[candidate("peer-a", vec![DiscoveryChannel::EnvVar])],
        );
        assert_eq!(confidence, BootstrapConfidence::Suspicious);
        assert!(matches!(
            bootstrap_policy(confidence),
            BootstrapAction::Quarantine { .. }
        ));
    }

    #[test]
    fn verify_topology_uses_cached_peers_only_as_last_resort() {
        let confidence = verify_topology(
            &["peer-a".to_string()],
            &[candidate(
                "peer-a",
                vec![
                    DiscoveryChannel::Cached,
                    DiscoveryChannel::PeerGossip {
                        from_peer: "peer-z".to_string(),
                    },
                ],
            )],
        );
        assert_eq!(confidence, BootstrapConfidence::High);
    }

    #[test]
    fn verify_topology_still_prefers_strong_channels_over_cached_overlap() {
        let confidence = verify_topology(
            &["peer-a".to_string()],
            &[
                candidate("peer-a", vec![DiscoveryChannel::Cached]),
                candidate("peer-b", vec![DiscoveryChannel::EnvVar]),
            ],
        );
        assert_eq!(confidence, BootstrapConfidence::Suspicious);
    }
}
