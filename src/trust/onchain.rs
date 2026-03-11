//! Standalone on-chain trust bridge stubs.
//!
//! AgentHALO's attestation bridge depended on HALO-only modules (`puf`, `pcn`,
//! Nym, cast integration). Standalone NucleusDB keeps the public API surface so
//! callers can fail closed without pulling the removed stack back in.

use std::fmt::{Display, Formatter};

#[derive(Clone, Debug)]
pub struct AgentOnchainStatus {
    pub verified: bool,
    pub active: Option<bool>,
    pub puf_digest: Option<String>,
    pub tier: Option<u8>,
    pub last_attestation: Option<u64>,
    pub last_replay_seq: Option<u64>,
    pub raw_verify: String,
    pub raw_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustBridgeError {
    Unsupported,
}

impl Display for TrustBridgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => write!(
                f,
                "on-chain trust bridge requires HALO attestation infrastructure and is unavailable in standalone NucleusDB"
            ),
        }
    }
}

impl std::error::Error for TrustBridgeError {}

pub fn verify_agent_onchain(
    _rpc_url: &str,
    _contract_address: &str,
    _agent_address: &str,
) -> Result<AgentOnchainStatus, TrustBridgeError> {
    Err(TrustBridgeError::Unsupported)
}
