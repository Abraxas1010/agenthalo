//! Composite CAB generation/submit utilities for multi-chain attestations.

use crate::halo::attest::AttestationResult;
use crate::halo::circuit::{
    load_or_setup_attestation_keys_with_policy, prove_attestation, verify_attestation_proof,
};
use crate::halo::onchain::{
    extract_hash, load_onchain_config_or_default, onchain_simulation_enabled, run_cast,
};
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
    MissingPrivateKeyEnv(String),
    Proof(String),
    Submission(String),
}

impl Display for CompositeCabError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyChainSet => write!(f, "chain_ids must be non-empty"),
            Self::MissingContractAddress => write!(f, "contract_address must be non-empty"),
            Self::MissingPrivateKeyEnv(name) => {
                write!(
                    f,
                    "missing private key env var `{name}` required for on-chain submission"
                )
            }
            Self::Proof(msg) => write!(f, "proof generation failed: {msg}"),
            Self::Submission(msg) => write!(f, "on-chain submission failed: {msg}"),
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

    fn statement_hash(&self, feasibility_root: &str, replay_seq: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"nucleusdb.composite_cab.statement.v1|");
        hasher.update(feasibility_root.as_bytes());
        hasher.update(replay_seq.to_le_bytes());
        for chain_id in &self.chain_ids {
            hasher.update(chain_id.to_le_bytes());
        }
        hasher.finalize().into()
    }

    pub fn generate_proof(&self) -> Result<CompositeCabProof, CompositeCabError> {
        let witness = compliance_witness(self.db);
        let compliance = compliance_inputs_from_pcn_witness(&witness, None);
        let hash = self.statement_hash(&compliance.feasibility_root, compliance.replay_seq);
        let digest_hex = hex_encode(&hash);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let attestation = AttestationResult {
            session_id: None,
            blinded_session_ref: Some(format!("composite-cab-{}", compliance.replay_seq)),
            merkle_root: compliance.feasibility_root.clone(),
            event_count: self.chain_ids.len() as u64,
            content_hashes: self.chain_ids.iter().map(|id| id.to_string()).collect(),
            witness_algorithm: "ML-DSA-65".to_string(),
            attestation_digest: digest_hex,
            timestamp,
            anonymous: true,
            proof_type: "groth16-composite-cab".to_string(),
            anonymous_membership_proof: None,
            groth16_proof: None,
            groth16_public_inputs: None,
            tx_hash: None,
            contract_address: None,
            block_number: None,
            chain: None,
        };

        let circuit_policy = load_onchain_config_or_default().circuit_policy;
        let max_events = self.chain_ids.len().max(1);
        let (pk, vk, _) =
            load_or_setup_attestation_keys_with_policy(Some(max_events), circuit_policy)
                .map_err(CompositeCabError::Proof)?;
        let bundle = prove_attestation(&pk, &attestation).map_err(CompositeCabError::Proof)?;
        let verified = verify_attestation_proof(&vk, &bundle).map_err(CompositeCabError::Proof)?;
        if !verified {
            return Err(CompositeCabError::Proof(
                "local proof verification failed".to_string(),
            ));
        }

        let mut public_signals = Vec::with_capacity(2 + self.chain_ids.len());
        public_signals.push(compliance.replay_seq.to_string());
        public_signals.push(format!("0x{}", compliance.feasibility_root));
        for chain_id in &self.chain_ids {
            public_signals.push(chain_id.to_string());
        }
        Ok(CompositeCabProof {
            proof_hex: format!("0x{}", bundle.proof_hex),
            public_signals,
            chain_ids: self.chain_ids.clone(),
            composite_cab_hash: hash,
            replay_seq: compliance.replay_seq,
        })
    }

    pub fn submit_attestation(
        &self,
        proof: &CompositeCabProof,
        contract_address: &str,
    ) -> Result<TxHash, CompositeCabError> {
        if contract_address.trim().is_empty() {
            return Err(CompositeCabError::MissingContractAddress);
        }
        if onchain_simulation_enabled() {
            let mut hasher = Sha256::new();
            hasher.update(b"nucleusdb.composite_cab.simulation_submit.v1|");
            hasher.update(contract_address.as_bytes());
            hasher.update(proof.proof_hex.as_bytes());
            hasher.update(proof.public_signals.join(",").as_bytes());
            hasher.update(
                proof
                    .chain_ids
                    .iter()
                    .flat_map(|id| id.to_le_bytes())
                    .collect::<Vec<_>>(),
            );
            let digest: [u8; 32] = hasher.finalize().into();
            return Ok(format!("0x{}", hex_encode(&digest)));
        }

        let onchain_cfg = load_onchain_config_or_default();
        let private_key_env = onchain_cfg.private_key_env.trim().to_string();
        let private_key = std::env::var(&private_key_env)
            .map_err(|_| CompositeCabError::MissingPrivateKeyEnv(private_key_env.clone()))?;
        let signals = format!("[{}]", proof.public_signals.join(","));
        let chains = format!(
            "[{}]",
            proof
                .chain_ids
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        let args = vec![
            "send".to_string(),
            "--async".to_string(),
            "--rpc-url".to_string(),
            onchain_cfg.rpc_url.clone(),
            "--private-key".to_string(),
            private_key,
            contract_address.trim().to_string(),
            "submitCompositeAttestation(bytes,uint256[],uint256[])".to_string(),
            proof.proof_hex.clone(),
            signals,
            chains,
        ];
        let out = run_cast(&args, &[]).map_err(CompositeCabError::Submission)?;
        extract_hash(&out).ok_or_else(|| {
            CompositeCabError::Submission(format!(
                "failed to parse tx hash from cast output: {out}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::protocol::VcBackend;
    use crate::state::State;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn new_test_db() -> NucleusDb {
        NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            default_witness_cfg(),
        )
    }

    #[test]
    fn generate_proof_returns_real_payload() {
        let db = new_test_db();
        let gen = CompositeCabGenerator::new(&db, vec![1]).expect("generator");
        let proof = gen.generate_proof().expect("proof");
        assert!(proof.proof_hex.starts_with("0x"));
        assert_ne!(proof.proof_hex, "0x");
        assert_eq!(proof.chain_ids, vec![1]);
        assert!(!proof.public_signals.is_empty());
    }

    #[test]
    fn submit_attestation_simulation_returns_tx_hash() {
        let _guard = env_lock().lock().expect("lock env");
        let db = new_test_db();
        let gen = CompositeCabGenerator::new(&db, vec![1]).expect("generator");
        std::env::set_var("AGENTHALO_ONCHAIN_SIMULATION", "1");
        let proof = gen.generate_proof().expect("proof");
        let tx_hash = gen
            .submit_attestation(&proof, "0x1234")
            .expect("simulation hash");
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);
        std::env::remove_var("AGENTHALO_ONCHAIN_SIMULATION");
    }
}
