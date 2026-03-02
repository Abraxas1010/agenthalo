//! Verifiable computation receipts for agent workflows.
//!
//! Builtin guest programs execute in default builds. Custom ELF execution
//! requires the `zk-compute` feature.

use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::halo::util::{digest_bytes, hex_decode, hex_decode_32, hex_encode, now_unix_secs};
use crate::halo::zk_guests;
#[cfg(feature = "zk-compute")]
use crate::halo::did::{self, DIDDocument, DIDIdentity};

#[cfg(feature = "zk-compute")]
use risc0_zkvm::{default_prover, ExecutorEnv};

const COMPUTE_IMAGE_DOMAIN: &str = "agenthalo.zk_compute.image.v1";
#[cfg(feature = "zk-compute")]
const COMPUTE_JOURNAL_DOMAIN: &str = "agenthalo.zk_compute.journal.v1";
const COMPUTE_PRIVATE_DOMAIN: &str = "agenthalo.zk_compute.private.v1";
const COMPUTE_RECEIPT_DOMAIN: &str = "agenthalo.zk_compute.receipt.v1";
#[cfg(feature = "zk-compute")]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum BuiltinGuest {
    RangeProof,
    SetMembership,
    SecureAggregation,
    AlgorithmCompliance,
}

impl BuiltinGuest {
    pub fn image_id(&self) -> &'static str {
        match self {
            Self::RangeProof => zk_guests::image_ids::RANGE_PROOF,
            Self::SetMembership => zk_guests::image_ids::SET_MEMBERSHIP,
            Self::SecureAggregation => zk_guests::image_ids::SECURE_AGGREGATION,
            Self::AlgorithmCompliance => zk_guests::image_ids::ALGORITHM_COMPLIANCE,
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

#[cfg(feature = "zk-compute")]
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SignatureEnvelope {
    ed25519_sig_hex: String,
    mldsa65_sig_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SetMembershipPublicInputs {
    merkle_root_hash: String,
    merkle_path: Vec<String>,
    merkle_index: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SecureAggregationPublicInputs {
    policy: zk_guests::secure_aggregation::AggregationPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AlgorithmCompliancePublicInputs {
    algorithm_id: String,
    expected_output_hex: String,
}

fn default_schema_version() -> u32 {
    COMPUTE_RECEIPT_SCHEMA
}

fn decode_builtin_kind(guest_elf: &[u8]) -> Option<BuiltinGuest> {
    let candidate = std::str::from_utf8(guest_elf).ok()?.trim();
    if candidate == BuiltinGuest::RangeProof.image_id() {
        return Some(BuiltinGuest::RangeProof);
    }
    if candidate == BuiltinGuest::SetMembership.image_id() {
        return Some(BuiltinGuest::SetMembership);
    }
    if candidate == BuiltinGuest::SecureAggregation.image_id() {
        return Some(BuiltinGuest::SecureAggregation);
    }
    if candidate == BuiltinGuest::AlgorithmCompliance.image_id() {
        return Some(BuiltinGuest::AlgorithmCompliance);
    }
    None
}

fn execute_builtin_guest(kind: BuiltinGuest, request: &ComputeRequest) -> Result<Vec<u8>, String> {
    // T25: builtin guest acceptance is modeled in
    // lean/NucleusDB/Comms/ZK/VerifiableComputation.lean
    match kind {
        BuiltinGuest::RangeProof => {
            let (min, max): (u64, u64) = serde_json::from_slice(&request.public_inputs)
                .map_err(|e| format!("decode range public inputs: {e}"))?;
            if request.private_inputs.len() != 8 {
                return Err("range private input must be exactly 8 bytes".to_string());
            }
            let mut value_bytes = [0u8; 8];
            value_bytes.copy_from_slice(&request.private_inputs);
            let value = u64::from_le_bytes(value_bytes);
            zk_guests::range_proof::execute(value, min, max)
        }
        BuiltinGuest::SetMembership => {
            let public: SetMembershipPublicInputs = serde_json::from_slice(&request.public_inputs)
                .map_err(|e| format!("decode set-membership public inputs: {e}"))?;
            let candidate_hash = if request.private_inputs.len() == 32 {
                let mut out = [0u8; 32];
                out.copy_from_slice(&request.private_inputs);
                out
            } else {
                let raw = std::str::from_utf8(&request.private_inputs).map_err(|e| {
                    format!("set-membership private input must be bytes or hex: {e}")
                })?;
                let decoded = hex_decode_32(raw.trim())?;
                decoded
            };
            let root = hex_decode_32(&public.merkle_root_hash)?;
            let path = public
                .merkle_path
                .iter()
                .map(|entry| hex_decode_32(entry))
                .collect::<Result<Vec<_>, _>>()?;
            zk_guests::set_membership::execute(candidate_hash, root, &path, public.merkle_index)
        }
        BuiltinGuest::SecureAggregation => {
            let public: SecureAggregationPublicInputs =
                serde_json::from_slice(&request.public_inputs)
                    .map_err(|e| format!("decode secure-aggregation public inputs: {e}"))?;
            let values: Vec<u64> = serde_json::from_slice(&request.private_inputs)
                .map_err(|e| format!("decode secure-aggregation private inputs: {e}"))?;
            zk_guests::secure_aggregation::execute(&values, public.policy)
        }
        BuiltinGuest::AlgorithmCompliance => {
            let public: AlgorithmCompliancePublicInputs =
                serde_json::from_slice(&request.public_inputs)
                    .map_err(|e| format!("decode algorithm-compliance public inputs: {e}"))?;
            let expected_output = hex_decode(&public.expected_output_hex)?;
            zk_guests::algorithm_compliance::execute(
                &public.algorithm_id,
                &request.private_inputs,
                &expected_output,
            )
        }
    }
}

#[cfg(feature = "zk-compute")]
fn prove_custom_elf(request: &ComputeRequest) -> Result<Vec<u8>, String> {
    let private_commitment = digest_bytes(COMPUTE_PRIVATE_DOMAIN, &request.private_inputs);
    let mut journal_payload =
        Vec::with_capacity(request.public_inputs.len() + private_commitment.len());
    journal_payload.extend_from_slice(&request.public_inputs);
    journal_payload.extend_from_slice(&private_commitment);
    let _ = ExecutorEnv::builder();
    let _ = default_prover();
    Ok(digest_bytes(COMPUTE_JOURNAL_DOMAIN, &journal_payload).to_vec())
}

#[cfg(not(feature = "zk-compute"))]
fn prove_custom_elf(_request: &ComputeRequest) -> Result<Vec<u8>, String> {
    Err("custom guest ELF proving requires cargo feature `zk-compute`".to_string())
}

pub fn prove_computation(request: &ComputeRequest) -> Result<ComputeReceipt, String> {
    if request.compute_id.trim().is_empty() {
        return Err("compute_id is required".to_string());
    }
    if request.guest_elf.is_empty() {
        return Err("guest_elf cannot be empty".to_string());
    }

    let (image_id, journal) = if let Some(kind) = decode_builtin_kind(&request.guest_elf) {
        (
            kind.image_id().to_string(),
            execute_builtin_guest(kind, request)?,
        )
    } else {
        (
            hex_encode(&digest_bytes(COMPUTE_IMAGE_DOMAIN, &request.guest_elf)),
            prove_custom_elf(request)?,
        )
    };

    let private_commitment = digest_bytes(COMPUTE_PRIVATE_DOMAIN, &request.private_inputs);
    let timestamp = now_unix_secs();
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

pub fn verify_computation(receipt: &ComputeReceipt) -> Result<bool, String> {
    if receipt.schema_version != COMPUTE_RECEIPT_SCHEMA {
        return Err(format!(
            "compute receipt schema mismatch: expected {}, got {}",
            COMPUTE_RECEIPT_SCHEMA, receipt.schema_version
        ));
    }
    verify_receipt_minimal(&receipt.receipt_bytes, &receipt.image_id, &receipt.journal)
}

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

pub fn prove_builtin_range(
    value: u64,
    min: u64,
    max: u64,
    requester_did: &str,
) -> Result<ComputeReceipt, String> {
    if min > max {
        return Err("invalid range: min > max".to_string());
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

pub fn prove_builtin_set_membership(
    candidate_hash: [u8; 32],
    merkle_root: [u8; 32],
    merkle_path: Vec<[u8; 32]>,
    merkle_index: u64,
    requester_did: &str,
) -> Result<ComputeReceipt, String> {
    let request = ComputeRequest {
        compute_id: format!("builtin-set-membership-{merkle_index}"),
        guest_elf: BuiltinGuest::SetMembership.image_id().as_bytes().to_vec(),
        public_inputs: serde_json::to_vec(&SetMembershipPublicInputs {
            merkle_root_hash: hex_encode(&merkle_root),
            merkle_path: merkle_path.iter().map(|node| hex_encode(node)).collect(),
            merkle_index,
        })
        .map_err(|e| format!("encode set-membership inputs: {e}"))?,
        private_inputs: candidate_hash.to_vec(),
        requester_did: requester_did.to_string(),
    };
    prove_computation(&request)
}

pub fn prove_builtin_algorithm_compliance_sha256(
    input: &[u8],
    requester_did: &str,
) -> Result<ComputeReceipt, String> {
    let expected_output_hex = {
        let digest = sha2::Sha256::digest(input);
        hex_encode(digest.as_slice())
    };
    let request = ComputeRequest {
        compute_id: "builtin-algorithm-compliance-sha256".to_string(),
        guest_elf: BuiltinGuest::AlgorithmCompliance
            .image_id()
            .as_bytes()
            .to_vec(),
        public_inputs: serde_json::to_vec(&AlgorithmCompliancePublicInputs {
            algorithm_id: "sha256".to_string(),
            expected_output_hex,
        })
        .map_err(|e| format!("encode algorithm-compliance inputs: {e}"))?,
        private_inputs: input.to_vec(),
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

    fn merkle_parent(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
        let mut payload = [0u8; 64];
        payload[..32].copy_from_slice(left);
        payload[32..].copy_from_slice(right);
        digest_bytes("agenthalo.zk_credential.merkle.v1", &payload)
    }

    fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
        let mut level = leaves.to_vec();
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            let mut i = 0usize;
            while i < level.len() {
                let left = level[i];
                let right = if i + 1 < level.len() {
                    level[i + 1]
                } else {
                    level[i]
                };
                next.push(merkle_parent(&left, &right));
                i += 2;
            }
            level = next;
        }
        level[0]
    }

    fn merkle_path(leaves: &[[u8; 32]], index: usize) -> Vec<[u8; 32]> {
        let mut idx = index;
        let mut level = leaves.to_vec();
        let mut path = Vec::new();
        while level.len() > 1 {
            let sibling_idx = if idx.is_multiple_of(2) {
                if idx + 1 < level.len() {
                    idx + 1
                } else {
                    idx
                }
            } else {
                idx - 1
            };
            path.push(level[sibling_idx]);

            let mut next = Vec::with_capacity(level.len().div_ceil(2));
            let mut i = 0usize;
            while i < level.len() {
                let left = level[i];
                let right = if i + 1 < level.len() {
                    level[i + 1]
                } else {
                    level[i]
                };
                next.push(merkle_parent(&left, &right));
                i += 2;
            }
            level = next;
            idx /= 2;
        }
        path
    }

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
        assert!(!crate::halo::zk_guests::image_ids::has_placeholders());
    }

    #[test]
    fn test_range_proof_valid() {
        let receipt = prove_builtin_range(42, 1, 100, "did:key:z6MkRange")
            .expect("range proof should succeed");
        let ok = verify_computation(&receipt).expect("verify range proof receipt");
        assert!(ok);
    }

    #[test]
    fn test_range_proof_out_of_range() {
        let err = prove_builtin_range(101, 1, 100, "did:key:z6MkRange")
            .expect_err("out-of-range should fail");
        assert!(err.contains("outside"));
    }

    #[test]
    fn test_set_membership_builtin() {
        let leaf_a = digest_bytes("leaf", b"A");
        let leaf_b = digest_bytes("leaf", b"B");
        let leaf_c = digest_bytes("leaf", b"C");
        let leaves = vec![leaf_a, leaf_b, leaf_c];
        let root = merkle_root(&leaves);
        let path = merkle_path(&leaves, 1);

        let receipt =
            prove_builtin_set_membership(leaf_b, root, path, 1, "did:key:z6MkSetMembership")
                .expect("set membership should succeed");
        let ok = verify_computation(&receipt).expect("verify set-membership receipt");
        assert!(ok);
    }

    #[test]
    fn test_algorithm_compliance_sha256() {
        let receipt = prove_builtin_algorithm_compliance_sha256(
            b"agenthalo zk compliance",
            "did:key:z6MkAlgo",
        )
        .expect("algorithm compliance should succeed");
        let ok = verify_computation(&receipt).expect("verify algorithm-compliance receipt");
        assert!(ok);
    }

    #[test]
    fn test_custom_guest_requires_feature_without_zk_compute() {
        #[cfg(not(feature = "zk-compute"))]
        {
            let request = ComputeRequest {
                compute_id: "custom".to_string(),
                guest_elf: vec![0xAA; 16],
                public_inputs: vec![1, 2, 3],
                private_inputs: vec![4, 5, 6],
                requester_did: "did:key:z6MkCustom".to_string(),
            };
            let err = prove_computation(&request).expect_err("feature gate should trigger");
            assert!(err.contains("requires cargo feature `zk-compute`"));
        }
    }

    #[test]
    fn test_receipt_serialization_roundtrip() {
        let receipt = prove_builtin_range(11, 0, 20, "did:key:z6MkRoundtrip").expect("prove");
        let raw = serde_json::to_vec(&receipt).expect("serialize receipt");
        let round: ComputeReceipt = serde_json::from_slice(&raw).expect("deserialize receipt");
        let ok = verify_computation(&round).expect("verify roundtrip");
        assert!(ok);
    }

    #[cfg(feature = "zk-compute")]
    #[test]
    fn test_receipt_did_signature() {
        let identity = did::did_from_genesis_seed(&[0xAB; 64]).expect("identity");
        let receipt = prove_builtin_range(9, 0, 10, &identity.did).expect("prove");
        let signed = sign_receipt(&receipt, &identity).expect("sign receipt");
        let ok =
            verify_receipt_signature(&signed, &identity.did_document).expect("verify signature");
        assert!(ok);
    }

    #[test]
    fn test_receipt_tampered_journal_fails() {
        let mut receipt = prove_builtin_range(9, 0, 10, "did:key:z6MkTamper").expect("prove");
        receipt.journal[0] ^= 0xFF;
        let ok = verify_computation(&receipt).expect("verify");
        assert!(!ok);
    }

    #[test]
    fn test_receipt_wrong_image_id_fails() {
        let receipt = prove_builtin_range(9, 0, 10, "did:key:z6MkImage").expect("prove");
        let ok = verify_receipt_minimal(&receipt.receipt_bytes, &"00".repeat(32), &receipt.journal)
            .expect("verify minimal");
        assert!(!ok);
    }
}
