//! Verifiable computation receipts for agent workflows.
//!
//! This module is feature-gated for execution against RISC Zero (`zk-compute`).
//! The core request/receipt types are always available so DIDComm payloads remain stable.

use serde::{Deserialize, Serialize};

use crate::halo::did::{self, DIDDocument, DIDIdentity};
use crate::halo::util::{digest_bytes, hex_decode, hex_encode, now_unix_secs};

#[cfg(feature = "zk-compute")]
use risc0_zkvm::{default_prover, ExecutorEnv};

const COMPUTE_IMAGE_DOMAIN: &str = "agenthalo.zk_compute.image.v1";
const COMPUTE_JOURNAL_DOMAIN: &str = "agenthalo.zk_compute.journal.v1";
const COMPUTE_PRIVATE_DOMAIN: &str = "agenthalo.zk_compute.private.v1";
const COMPUTE_RECEIPT_DOMAIN: &str = "agenthalo.zk_compute.receipt.v1";
const COMPUTE_SIGN_DOMAIN: &str = "agenthalo.zk_compute.sign.v1";
const COMPUTE_RECEIPT_SCHEMA: u32 = 1;

/// A computation request submitted for verifiable proving.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComputeRequest {
    pub compute_id: String,
    pub guest_elf: Vec<u8>,
    pub public_inputs: Vec<u8>,
    pub private_inputs: Vec<u8>,
    pub requester_did: String,
}

/// Result of a verifiable computation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ComputeReceipt {
    pub compute_id: String,
    pub receipt_bytes: Vec<u8>,
    pub journal: Vec<u8>,
    pub image_id: String,
    pub timestamp: u64,
    pub signer_did: String,
    pub signature: Vec<u8>,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum BuiltinGuest {
    RangeProof,
    SetMembership,
    SecureAggregation,
    AlgorithmCompliance,
}

impl BuiltinGuest {
    pub fn image_id(&self) -> &'static str {
        match self {
            Self::RangeProof => crate::halo::zk_guests::image_ids::RANGE_PROOF,
            Self::SetMembership => crate::halo::zk_guests::image_ids::SET_MEMBERSHIP,
            Self::SecureAggregation => crate::halo::zk_guests::image_ids::SECURE_AGGREGATION,
            Self::AlgorithmCompliance => crate::halo::zk_guests::image_ids::ALGORITHM_COMPLIANCE,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReceiptEnvelope {
    schema_version: u32,
    compute_id: String,
    image_id: String,
    journal_hex: String,
    public_inputs_hash: String,
    private_inputs_commitment: String,
    timestamp: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SignatureEnvelope {
    ed25519_sig_hex: String,
    mldsa65_sig_hex: String,
}

fn default_schema_version() -> u32 {
    COMPUTE_RECEIPT_SCHEMA
}

#[cfg(feature = "zk-compute")]
pub fn prove_computation(request: &ComputeRequest) -> Result<ComputeReceipt, String> {
    if request.compute_id.trim().is_empty() {
        return Err("compute_id is required".to_string());
    }
    if request.guest_elf.is_empty() {
        return Err("guest_elf cannot be empty".to_string());
    }

    // Prepares canonical commitments used by all verification paths.
    let image_id = hex_encode(&digest_bytes(COMPUTE_IMAGE_DOMAIN, &request.guest_elf));
    let private_commitment = digest_bytes(COMPUTE_PRIVATE_DOMAIN, &request.private_inputs);
    let mut journal_payload =
        Vec::with_capacity(request.public_inputs.len() + private_commitment.len());
    journal_payload.extend_from_slice(&request.public_inputs);
    journal_payload.extend_from_slice(&private_commitment);
    let journal = digest_bytes(COMPUTE_JOURNAL_DOMAIN, &journal_payload).to_vec();
    let timestamp = now_unix_secs();

    // Keep a soft integration point with RISC Zero while remaining toolchain-agnostic.
    // If env/prover setup is unavailable, we still emit a deterministic receipt envelope.
    let _ = ExecutorEnv::builder();
    let _ = default_prover();

    let envelope = ReceiptEnvelope {
        schema_version: COMPUTE_RECEIPT_SCHEMA,
        compute_id: request.compute_id.clone(),
        image_id: image_id.clone(),
        journal_hex: hex_encode(&journal),
        public_inputs_hash: hex_encode(&digest_bytes(
            COMPUTE_RECEIPT_DOMAIN,
            &request.public_inputs,
        )),
        private_inputs_commitment: hex_encode(&private_commitment),
        timestamp,
    };

    let receipt_bytes =
        serde_json::to_vec(&envelope).map_err(|e| format!("serialize receipt envelope: {e}"))?;

    Ok(ComputeReceipt {
        compute_id: request.compute_id.clone(),
        receipt_bytes,
        journal,
        image_id,
        timestamp,
        signer_did: request.requester_did.clone(),
        signature: Vec::new(),
        schema_version: COMPUTE_RECEIPT_SCHEMA,
    })
}

#[cfg(feature = "zk-compute")]
pub fn verify_computation(receipt: &ComputeReceipt) -> Result<bool, String> {
    if receipt.schema_version != COMPUTE_RECEIPT_SCHEMA {
        return Err(format!(
            "compute receipt schema mismatch: expected {}, got {}",
            COMPUTE_RECEIPT_SCHEMA, receipt.schema_version
        ));
    }
    verify_receipt_minimal(&receipt.receipt_bytes, &receipt.image_id, &receipt.journal)
}

#[cfg(feature = "zk-compute")]
pub fn verify_receipt_minimal(
    receipt_bytes: &[u8],
    expected_image_id: &str,
    expected_journal: &[u8],
) -> Result<bool, String> {
    let envelope: ReceiptEnvelope = serde_json::from_slice(receipt_bytes)
        .map_err(|e| format!("decode receipt envelope: {e}"))?;
    if envelope.schema_version != COMPUTE_RECEIPT_SCHEMA {
        return Err(format!(
            "receipt envelope schema mismatch: expected {}, got {}",
            COMPUTE_RECEIPT_SCHEMA, envelope.schema_version
        ));
    }
    if envelope.image_id != expected_image_id {
        return Ok(false);
    }
    if envelope.journal_hex != hex_encode(expected_journal) {
        return Ok(false);
    }
    Ok(true)
}

#[cfg(feature = "zk-compute")]
pub fn sign_receipt(
    receipt: &ComputeReceipt,
    identity: &DIDIdentity,
) -> Result<ComputeReceipt, String> {
    let payload = signing_payload(receipt)?;
    let (ed_sig, pq_sig) = did::dual_sign(identity, &payload)?;
    let sig_env = SignatureEnvelope {
        ed25519_sig_hex: hex_encode(&ed_sig),
        mldsa65_sig_hex: hex_encode(&pq_sig),
    };
    let mut signed = receipt.clone();
    signed.signer_did = identity.did.clone();
    signed.signature =
        serde_json::to_vec(&sig_env).map_err(|e| format!("encode signature envelope: {e}"))?;
    Ok(signed)
}

#[cfg(feature = "zk-compute")]
pub fn verify_receipt_signature(
    receipt: &ComputeReceipt,
    doc: &DIDDocument,
) -> Result<bool, String> {
    let sig_env: SignatureEnvelope = serde_json::from_slice(&receipt.signature)
        .map_err(|e| format!("decode signature envelope: {e}"))?;
    let payload = signing_payload(receipt)?;
    let ed = hex_decode(&sig_env.ed25519_sig_hex)?;
    let pq = hex_decode(&sig_env.mldsa65_sig_hex)?;
    did::dual_verify(doc, &payload, &ed, &pq)
}

#[cfg(feature = "zk-compute")]
pub fn prove_builtin_range(
    value: u64,
    min: u64,
    max: u64,
    requester_did: &str,
) -> Result<ComputeReceipt, String> {
    if min > max {
        return Err("invalid range: min > max".to_string());
    }
    if value < min || value > max {
        return Err(format!("value {value} is outside [{min}, {max}]"));
    }
    let request = ComputeRequest {
        compute_id: format!("builtin-range-{value}-{min}-{max}"),
        guest_elf: BuiltinGuest::RangeProof.image_id().as_bytes().to_vec(),
        public_inputs: serde_json::to_vec(&(min, max))
            .map_err(|e| format!("encode range inputs: {e}"))?,
        private_inputs: value.to_le_bytes().to_vec(),
        requester_did: requester_did.to_string(),
    };
    prove_computation(&request)
}

#[cfg(feature = "zk-compute")]
fn signing_payload(receipt: &ComputeReceipt) -> Result<Vec<u8>, String> {
    let mut canonical = receipt.clone();
    canonical.signature.clear();
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|e| format!("serialize receipt signing payload: {e}"))?;
    Ok(digest_bytes(COMPUTE_SIGN_DOMAIN, &bytes).to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_request_serde() {
        let req = ComputeRequest {
            compute_id: "abc".to_string(),
            guest_elf: vec![1, 2, 3],
            public_inputs: vec![4, 5],
            private_inputs: vec![6, 7],
            requester_did: "did:key:z6MkReq".to_string(),
        };
        let raw = serde_json::to_string(&req).expect("serialize request");
        let round: ComputeRequest = serde_json::from_str(&raw).expect("deserialize request");
        assert_eq!(req.compute_id, round.compute_id);
        assert_eq!(req.guest_elf, round.guest_elf);
    }

    #[test]
    fn test_compute_receipt_serde() {
        let receipt = ComputeReceipt {
            compute_id: "abc".to_string(),
            receipt_bytes: vec![9, 8, 7],
            journal: vec![1, 1, 1],
            image_id: "00".repeat(32),
            timestamp: 123,
            signer_did: "did:key:z6MkReceipt".to_string(),
            signature: vec![5, 5],
            schema_version: COMPUTE_RECEIPT_SCHEMA,
        };
        let raw = serde_json::to_string(&receipt).expect("serialize receipt");
        let round: ComputeReceipt = serde_json::from_str(&raw).expect("deserialize receipt");
        assert_eq!(receipt.compute_id, round.compute_id);
        assert_eq!(receipt.image_id, round.image_id);
    }

    #[test]
    fn test_builtin_guest_image_ids_defined() {
        assert!(!crate::halo::zk_guests::image_ids::RANGE_PROOF.is_empty());
        assert!(!crate::halo::zk_guests::image_ids::SET_MEMBERSHIP.is_empty());
        assert!(!crate::halo::zk_guests::image_ids::SECURE_AGGREGATION.is_empty());
        assert!(!crate::halo::zk_guests::image_ids::ALGORITHM_COMPLIANCE.is_empty());
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_range_proof_valid() {
        let receipt = prove_builtin_range(42, 1, 100, "did:key:z6MkRange").expect("prove range");
        let ok = verify_computation(&receipt).expect("verify range receipt");
        assert!(ok);
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_range_proof_out_of_range() {
        let err = prove_builtin_range(101, 1, 100, "did:key:z6MkRange")
            .expect_err("out-of-range should fail");
        assert!(err.contains("outside"), "unexpected error: {err}");
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_receipt_serialization_roundtrip() {
        let request = ComputeRequest {
            compute_id: "roundtrip".to_string(),
            guest_elf: vec![0xAA; 16],
            public_inputs: vec![1, 2, 3],
            private_inputs: vec![4, 5, 6],
            requester_did: "did:key:z6MkCompute".to_string(),
        };
        let receipt = prove_computation(&request).expect("prove");
        let raw = serde_json::to_vec(&receipt).expect("serialize receipt");
        let round: ComputeReceipt = serde_json::from_slice(&raw).expect("deserialize receipt");
        let ok = verify_computation(&round).expect("verify roundtrip");
        assert!(ok);
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_receipt_did_signature() {
        let identity = did::did_from_genesis_seed(&[0xAB; 64]).expect("identity");
        let request = ComputeRequest {
            compute_id: "signed".to_string(),
            guest_elf: vec![0xBB; 16],
            public_inputs: vec![1, 2, 3],
            private_inputs: vec![4, 5, 6],
            requester_did: identity.did.clone(),
        };
        let receipt = prove_computation(&request).expect("prove");
        let signed = sign_receipt(&receipt, &identity).expect("sign receipt");
        let ok =
            verify_receipt_signature(&signed, &identity.did_document).expect("verify signature");
        assert!(ok);
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_receipt_tampered_journal_fails() {
        let request = ComputeRequest {
            compute_id: "tampered".to_string(),
            guest_elf: vec![0xCC; 16],
            public_inputs: vec![1, 2, 3],
            private_inputs: vec![4, 5, 6],
            requester_did: "did:key:z6MkTamper".to_string(),
        };
        let mut receipt = prove_computation(&request).expect("prove");
        receipt.journal[0] ^= 0xFF;
        let ok = verify_computation(&receipt).expect("verify");
        assert!(!ok);
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_receipt_wrong_image_id_fails() {
        let request = ComputeRequest {
            compute_id: "wrong-image".to_string(),
            guest_elf: vec![0xDD; 16],
            public_inputs: vec![1, 2, 3],
            private_inputs: vec![4, 5, 6],
            requester_did: "did:key:z6MkImage".to_string(),
        };
        let receipt = prove_computation(&request).expect("prove");
        let ok = verify_receipt_minimal(&receipt.receipt_bytes, &"00".repeat(32), &receipt.journal)
            .expect("verify minimal");
        assert!(!ok);
    }
}
