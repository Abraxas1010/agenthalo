use crate::trust::onchain::AgentOnchainStatus;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Erc8004CapabilityBridge {
    pub did: String,
    pub agent_address: String,
    pub chain_id: u64,
    pub contract_address: String,
    pub verified_onchain: bool,
    pub active: bool,
    pub puf_tier: Option<u8>,
    pub reputation_score: u64,
    pub last_attestation: Option<u64>,
    pub capability_ids: Vec<String>,
}

pub fn bridge_from_onchain_status(
    did: &str,
    agent_address: &str,
    chain_id: u64,
    contract_address: &str,
    status: &AgentOnchainStatus,
    capability_ids: &[String],
) -> Erc8004CapabilityBridge {
    Erc8004CapabilityBridge {
        did: did.to_string(),
        agent_address: agent_address.to_ascii_lowercase(),
        chain_id,
        contract_address: contract_address.to_ascii_lowercase(),
        verified_onchain: status.verified,
        active: status.active.unwrap_or(status.verified),
        puf_tier: status.tier,
        reputation_score: status
            .tier
            .map(|tier| tier as u64 * 100)
            .unwrap_or(0)
            .saturating_add(status.last_replay_seq.unwrap_or(0)),
        last_attestation: status.last_attestation,
        capability_ids: capability_ids.to_vec(),
    }
}

pub fn bridge_is_high_trust(binding: &Erc8004CapabilityBridge, min_tier: u8) -> bool {
    binding.verified_onchain
        && binding.active
        && binding.puf_tier.unwrap_or(0) >= min_tier
        && !binding.capability_ids.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onchain_status_maps_into_capability_bridge() {
        let status = AgentOnchainStatus {
            verified: true,
            active: Some(true),
            puf_digest: None,
            tier: Some(4),
            last_attestation: Some(123),
            last_replay_seq: Some(7),
            raw_verify: "ok".to_string(),
            raw_status: "ok".to_string(),
        };
        let bridge = bridge_from_onchain_status(
            "did:key:test",
            "0xABCD",
            8453,
            "0xCDEF",
            &status,
            &["cap-1".to_string()],
        );
        assert_eq!(bridge.agent_address, "0xabcd");
        assert_eq!(bridge.contract_address, "0xcdef");
        assert!(bridge_is_high_trust(&bridge, 3));
    }
}
