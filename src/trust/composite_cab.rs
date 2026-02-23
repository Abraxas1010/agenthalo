//! Composite CAB generation/submit scaffold for multi-chain attestations.

use crate::license::compliance_inputs_from_pcn_witness;
use crate::pcn::compliance_witness;
use crate::protocol::NucleusDb;
use crate::transparency::ct6962::hex_encode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

pub type TxHash = String;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompositeCabProof {
    pub proof_hex: String,
    pub public_signals: Vec<String>,
    pub chain_ids: Vec<u64>,
    pub composite_cab_hash: [u8; 32],
    pub replay_seq: u64,
}

impl CompositeCabProof {
    pub fn composite_cab_hash_hex(&self) -> String {
        hex_encode(&self.composite_cab_hash)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositeCabError {
    EmptyChainSet,
    MissingContractAddress,
}

impl Display for CompositeCabError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyChainSet => write!(f, "chain_ids must be non-empty"),
            Self::MissingContractAddress => write!(f, "contract_address must be non-empty"),
        }
    }
}

impl std::error::Error for CompositeCabError {}

pub struct CompositeCabGenerator<'a> {
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

    pub fn build_placeholder(&self) -> CompositeCabProof {
        let witness = compliance_witness(self.db);
        let compliance = compliance_inputs_from_pcn_witness(&witness, None);
        let mut hasher = Sha256::new();
        hasher.update(b"nucleusdb.composite_cab.placeholder.v1|");
        hasher.update(compliance.feasibility_root.as_bytes());
        hasher.update(compliance.replay_seq.to_le_bytes());
        for chain_id in &self.chain_ids {
            hasher.update(chain_id.to_le_bytes());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        hasher.update(now.to_le_bytes());
        let hash: [u8; 32] = hasher.finalize().into();
        let mut public_signals = Vec::with_capacity(2 + self.chain_ids.len());
        public_signals.push(compliance.replay_seq.to_string());
        public_signals.push(format!("0x{}", compliance.feasibility_root));
        for chain_id in &self.chain_ids {
            public_signals.push(chain_id.to_string());
        }
        CompositeCabProof {
            proof_hex: "0x".to_string(),
            public_signals,
            chain_ids: self.chain_ids.clone(),
            composite_cab_hash: hash,
            replay_seq: compliance.replay_seq,
        }
    }

    pub fn generate_proof(&self) -> Result<CompositeCabProof, CompositeCabError> {
        // TODO(phase4): wire real Groth16/Plonk generation once composite CAB circuit
        // witness extraction is finalized and benchmarked.
        todo!("generate composite CAB zk proof from multi-chain witness")
    }

    pub fn submit_attestation(
        &self,
        _proof: &CompositeCabProof,
        contract_address: &str,
    ) -> Result<TxHash, CompositeCabError> {
        if contract_address.trim().is_empty() {
            return Err(CompositeCabError::MissingContractAddress);
        }
        // TODO(phase4): submit via cast/ethers once ABI and signing policy are finalized.
        todo!("submit composite CAB proof to TrustVerifierMultiChain")
    }
}
