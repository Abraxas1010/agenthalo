//! Standalone composite CAB stubs.
//!
//! Composite CAB proof generation/submission is part of the removed HALO
//! attestation layer. The standalone build keeps a small fail-closed API so
//! downstream code can report unsupported operations explicitly.

use crate::protocol::NucleusDb;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

pub type TxHash = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompositeCabProof {
    pub proof_hex: String,
    pub public_signals: Vec<String>,
    pub chain_ids: Vec<u64>,
    pub composite_cab_hash: [u8; 32],
    pub replay_seq: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositeCabError {
    EmptyChainSet,
    Unsupported,
}

impl Display for CompositeCabError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyChainSet => write!(f, "chain_ids must be non-empty"),
            Self::Unsupported => write!(
                f,
                "composite CAB generation requires HALO attestation infrastructure and is unavailable in standalone NucleusDB"
            ),
        }
    }
}

impl std::error::Error for CompositeCabError {}

pub struct CompositeCabGenerator<'a> {
    #[allow(dead_code)]
    db: &'a NucleusDb,
    chain_ids: Vec<u64>,
}

impl<'a> CompositeCabGenerator<'a> {
    pub fn new(db: &'a NucleusDb, chain_ids: Vec<u64>) -> Result<Self, CompositeCabError> {
        if chain_ids.is_empty() {
            return Err(CompositeCabError::EmptyChainSet);
        }
        Ok(Self { db, chain_ids })
    }

    pub fn chain_ids(&self) -> &[u64] {
        &self.chain_ids
    }

    pub fn generate_proof(&self) -> Result<CompositeCabProof, CompositeCabError> {
        Err(CompositeCabError::Unsupported)
    }

    pub fn submit_attestation(
        &self,
        _proof: &CompositeCabProof,
        _contract_address: &str,
    ) -> Result<TxHash, CompositeCabError> {
        Err(CompositeCabError::Unsupported)
    }
}
