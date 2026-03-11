use crate::halo::attest::AttestationResult;
use crate::halo::circuit_policy::{
    key_hash_hex, load_metadata, save_metadata, CircuitArtifactMetadata, CircuitPolicy,
    CIRCUIT_METADATA_SCHEMA_VERSION,
};
use crate::halo::config;
use crate::halo::public_input_schema::{
    build_public_inputs, PUBLIC_INPUT_SCHEMA_VERSION, REQUIRED_PUBLIC_INPUTS,
};
use crate::halo::util::{digest_bytes, hex_decode, hex_encode};
use ark_bn254::{Bn254, Fq, Fr};
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::{prepare_verifying_key, Groth16, Proof, ProvingKey, VerifyingKey};
use ark_relations::lc;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError, Variable};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use num_bigint::BigUint;
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};

const CIRCUIT_SETUP_DOMAIN: &str = "agenthalo.circuit.setup.v1";
const CIRCUIT_PROVE_DOMAIN: &str = "agenthalo.circuit.prove.v1";
const MAX_EVENTS_DEFAULT: usize = 256;
/// Canonical theorem path used for external assurance claims.
pub const ATTESTATION_CIRCUIT_FORMAL_BASIS: &str =
    "HeytingLean.NucleusDB.Circuit.AttestationR1CS.attestation_circuit_satisfiable";
/// Runtime-local mirror theorem for the nucleusdb attestation slot model.
pub const ATTESTATION_CIRCUIT_FORMAL_BASIS_LOCAL: &str =
    "HeytingLean.NucleusDB.TrustLayer.AttestationCircuit.attestation_circuit_satisfiable";

/// Canonical/local theorem-path pair for audit surfaces.
pub fn attestation_circuit_formal_provenance() -> (&'static str, &'static str) {
    (
        ATTESTATION_CIRCUIT_FORMAL_BASIS,
        ATTESTATION_CIRCUIT_FORMAL_BASIS_LOCAL,
    )
}

pub fn attestation_circuit_formal_basis() -> &'static str {
    ATTESTATION_CIRCUIT_FORMAL_BASIS
}

pub fn attestation_circuit_formal_basis_local() -> &'static str {
    ATTESTATION_CIRCUIT_FORMAL_BASIS_LOCAL
}

#[derive(Clone, Debug)]
pub struct AttestationCircuit {
    pub merkle_lo_public: Option<Fr>,
    pub merkle_hi_public: Option<Fr>,
    pub digest_lo_public: Option<Fr>,
    pub digest_hi_public: Option<Fr>,
    pub event_count_public: Option<Fr>,
    pub merkle_lo_witness: Option<Fr>,
    pub merkle_hi_witness: Option<Fr>,
    pub digest_lo_witness: Option<Fr>,
    pub digest_hi_witness: Option<Fr>,
    pub event_count_witness: Option<Fr>,
}

impl AttestationCircuit {
    fn blank() -> Self {
        Self {
            merkle_lo_public: Some(Fr::from(0u64)),
            merkle_hi_public: Some(Fr::from(0u64)),
            digest_lo_public: Some(Fr::from(0u64)),
            digest_hi_public: Some(Fr::from(0u64)),
            event_count_public: Some(Fr::from(0u64)),
            merkle_lo_witness: Some(Fr::from(0u64)),
            merkle_hi_witness: Some(Fr::from(0u64)),
            digest_lo_witness: Some(Fr::from(0u64)),
            digest_hi_witness: Some(Fr::from(0u64)),
            event_count_witness: Some(Fr::from(0u64)),
        }
    }
}

impl ConstraintSynthesizer<Fr> for AttestationCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let merkle_lo_public = cs.new_input_variable(|| {
            self.merkle_lo_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let merkle_hi_public = cs.new_input_variable(|| {
            self.merkle_hi_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let digest_lo_public = cs.new_input_variable(|| {
            self.digest_lo_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let digest_hi_public = cs.new_input_variable(|| {
            self.digest_hi_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let event_count_public = cs.new_input_variable(|| {
            self.event_count_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let merkle_lo_witness = cs.new_witness_variable(|| {
            self.merkle_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let merkle_hi_witness = cs.new_witness_variable(|| {
            self.merkle_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let digest_lo_witness = cs.new_witness_variable(|| {
            self.digest_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let digest_hi_witness = cs.new_witness_variable(|| {
            self.digest_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let event_count_witness = cs.new_witness_variable(|| {
            self.event_count_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        enforce_equal(cs.clone(), merkle_lo_public, merkle_lo_witness)?;
        enforce_equal(cs.clone(), merkle_hi_public, merkle_hi_witness)?;
        enforce_equal(cs.clone(), digest_lo_public, digest_lo_witness)?;
        enforce_equal(cs.clone(), digest_hi_public, digest_hi_witness)?;
        enforce_equal(cs, event_count_public, event_count_witness)?;

        Ok(())
    }
}

fn enforce_equal(
    cs: ConstraintSystemRef<Fr>,
    left: Variable,
    right: Variable,
) -> Result<(), SynthesisError> {
    cs.enforce_constraint(lc!() + left - right, lc!() + Variable::One, lc!())?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AttestationProofBundle {
    pub proof_hex: String,
    pub proof_words: [String; 8],
    pub public_inputs: Vec<String>,
    #[serde(default = "default_public_input_schema_version")]
    pub public_input_schema_version: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircuitKeyInfo {
    pub pk_path: String,
    pub vk_path: String,
    pub metadata_path: String,
    pub max_events: usize,
    pub policy: CircuitPolicy,
    pub public_input_schema_version: u32,
    pub pk_sha256: String,
    pub vk_sha256: String,
}

fn default_public_input_schema_version() -> u32 {
    PUBLIC_INPUT_SCHEMA_VERSION
}

pub fn setup_attestation_circuit(
    max_events: usize,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>), String> {
    let mut seed_material = Vec::new();
    seed_material.extend_from_slice(&max_events.to_le_bytes());
    let seed = digest_bytes(CIRCUIT_SETUP_DOMAIN, &seed_material);
    let mut rng = ChaCha20Rng::from_seed(seed);
    let circuit = AttestationCircuit::blank();
    Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng)
        .map_err(|e| format!("circuit setup failed: {e}"))
}

pub fn load_or_setup_attestation_keys(
    max_events: Option<usize>,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>, CircuitKeyInfo), String> {
    load_or_setup_attestation_keys_with_policy(max_events, CircuitPolicy::DevDeterministic)
}

pub fn load_or_setup_attestation_keys_with_policy(
    max_events: Option<usize>,
    policy: CircuitPolicy,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>, CircuitKeyInfo), String> {
    config::ensure_halo_dir()?;
    config::ensure_circuit_dir()?;
    let max_events = max_events.unwrap_or(MAX_EVENTS_DEFAULT);
    let pk_path = config::circuit_pk_path();
    let vk_path = config::circuit_vk_path();
    let metadata_path = config::circuit_metadata_path();
    load_or_setup_attestation_keys_from_paths(
        max_events,
        policy,
        &pk_path,
        &vk_path,
        &metadata_path,
    )
}

fn load_or_setup_attestation_keys_from_paths(
    requested_max_events: usize,
    policy: CircuitPolicy,
    pk_path: &std::path::Path,
    vk_path: &std::path::Path,
    metadata_path: &std::path::Path,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>, CircuitKeyInfo), String> {
    let has_pk = pk_path.exists();
    let has_vk = vk_path.exists();
    let keys_exist = has_pk && has_vk;
    if has_pk ^ has_vk {
        return Err(format!(
            "partial circuit artifacts detected: pk({})={}, vk({})={}; remove both or regenerate",
            pk_path.display(),
            if has_pk { "present" } else { "missing" },
            vk_path.display(),
            if has_vk { "present" } else { "missing" }
        ));
    }

    if !keys_exist {
        if matches!(policy, CircuitPolicy::ProductionRequired) {
            return Err(format!(
                "production circuit policy requires existing CRS artifacts: missing `{}` and/or `{}`",
                pk_path.display(),
                vk_path.display()
            ));
        }
        return create_and_persist_keys(requested_max_events, pk_path, vk_path, metadata_path);
    }

    let pk_raw = std::fs::read(pk_path)
        .map_err(|e| format!("read proving key {}: {e}", pk_path.display()))?;
    let vk_raw = std::fs::read(vk_path)
        .map_err(|e| format!("read verifying key {}: {e}", vk_path.display()))?;
    let pk_sha256 = key_hash_hex(&pk_raw);
    let vk_sha256 = key_hash_hex(&vk_raw);

    let mut pk_slice = pk_raw.as_slice();
    let mut vk_slice = vk_raw.as_slice();
    let pk = ProvingKey::<Bn254>::deserialize_compressed(&mut pk_slice)
        .map_err(|e| format!("decode proving key: {e}"))?;
    let vk = VerifyingKey::<Bn254>::deserialize_compressed(&mut vk_slice)
        .map_err(|e| format!("decode verifying key: {e}"))?;

    let mut metadata = if metadata_path.exists() {
        load_metadata(metadata_path)?
    } else if matches!(policy, CircuitPolicy::ProductionRequired) {
        return Err(format!(
            "production circuit policy requires metadata artifact `{}`",
            metadata_path.display()
        ));
    } else {
        let new_meta = CircuitArtifactMetadata {
            schema_version: CIRCUIT_METADATA_SCHEMA_VERSION,
            setup_mode: CircuitPolicy::DevDeterministic,
            created_at: now_unix_secs(),
            max_events: requested_max_events,
            public_input_schema_version: PUBLIC_INPUT_SCHEMA_VERSION,
            pk_sha256: pk_sha256.clone(),
            vk_sha256: vk_sha256.clone(),
        };
        save_metadata(metadata_path, &new_meta)?;
        new_meta
    };

    if metadata.schema_version != CIRCUIT_METADATA_SCHEMA_VERSION {
        return Err(format!(
            "circuit metadata schema mismatch: expected {}, got {}",
            CIRCUIT_METADATA_SCHEMA_VERSION, metadata.schema_version
        ));
    }
    if metadata.public_input_schema_version != PUBLIC_INPUT_SCHEMA_VERSION {
        return Err(format!(
            "public input schema mismatch: expected {}, got {}",
            PUBLIC_INPUT_SCHEMA_VERSION, metadata.public_input_schema_version
        ));
    }
    if metadata.pk_sha256 != pk_sha256 || metadata.vk_sha256 != vk_sha256 {
        return Err("circuit artifact hash mismatch; pk/vk do not match metadata".to_string());
    }
    if matches!(policy, CircuitPolicy::ProductionRequired)
        && metadata.max_events != requested_max_events
    {
        return Err(format!(
            "production circuit metadata max_events mismatch: expected {}, got {}",
            requested_max_events, metadata.max_events
        ));
    }

    if matches!(policy, CircuitPolicy::DevDeterministic)
        && metadata.max_events != requested_max_events
    {
        metadata.max_events = requested_max_events;
        save_metadata(metadata_path, &metadata)?;
    }

    Ok((
        pk,
        vk,
        CircuitKeyInfo {
            pk_path: pk_path.display().to_string(),
            vk_path: vk_path.display().to_string(),
            metadata_path: metadata_path.display().to_string(),
            max_events: metadata.max_events,
            policy,
            public_input_schema_version: metadata.public_input_schema_version,
            pk_sha256,
            vk_sha256,
        },
    ))
}

fn create_and_persist_keys(
    max_events: usize,
    pk_path: &std::path::Path,
    vk_path: &std::path::Path,
    metadata_path: &std::path::Path,
) -> Result<(ProvingKey<Bn254>, VerifyingKey<Bn254>, CircuitKeyInfo), String> {
    let (pk, vk) = setup_attestation_circuit(max_events)?;
    let mut pk_raw = Vec::new();
    let mut vk_raw = Vec::new();
    pk.serialize_compressed(&mut pk_raw)
        .map_err(|e| format!("encode proving key: {e}"))?;
    vk.serialize_compressed(&mut vk_raw)
        .map_err(|e| format!("encode verifying key: {e}"))?;
    std::fs::write(pk_path, &pk_raw)
        .map_err(|e| format!("write proving key {}: {e}", pk_path.display()))?;
    std::fs::write(vk_path, &vk_raw)
        .map_err(|e| format!("write verifying key {}: {e}", vk_path.display()))?;

    let pk_sha256 = key_hash_hex(&pk_raw);
    let vk_sha256 = key_hash_hex(&vk_raw);
    let metadata = CircuitArtifactMetadata {
        schema_version: CIRCUIT_METADATA_SCHEMA_VERSION,
        setup_mode: CircuitPolicy::DevDeterministic,
        created_at: now_unix_secs(),
        max_events,
        public_input_schema_version: PUBLIC_INPUT_SCHEMA_VERSION,
        pk_sha256: pk_sha256.clone(),
        vk_sha256: vk_sha256.clone(),
    };
    save_metadata(metadata_path, &metadata)?;

    Ok((
        pk,
        vk,
        CircuitKeyInfo {
            pk_path: pk_path.display().to_string(),
            vk_path: vk_path.display().to_string(),
            metadata_path: metadata_path.display().to_string(),
            max_events,
            policy: CircuitPolicy::DevDeterministic,
            public_input_schema_version: PUBLIC_INPUT_SCHEMA_VERSION,
            pk_sha256,
            vk_sha256,
        },
    ))
}

pub fn prove_attestation(
    pk: &ProvingKey<Bn254>,
    attestation: &AttestationResult,
) -> Result<AttestationProofBundle, String> {
    let (merkle_lo, merkle_hi) = split_hash_u128(&attestation.merkle_root)?;
    let (digest_lo, digest_hi) = split_hash_u128(&attestation.attestation_digest)?;
    let event_count = attestation.event_count;

    let circuit = AttestationCircuit {
        merkle_lo_public: Some(Fr::from(merkle_lo)),
        merkle_hi_public: Some(Fr::from(merkle_hi)),
        digest_lo_public: Some(Fr::from(digest_lo)),
        digest_hi_public: Some(Fr::from(digest_hi)),
        event_count_public: Some(Fr::from(event_count)),
        merkle_lo_witness: Some(Fr::from(merkle_lo)),
        merkle_hi_witness: Some(Fr::from(merkle_hi)),
        digest_lo_witness: Some(Fr::from(digest_lo)),
        digest_hi_witness: Some(Fr::from(digest_hi)),
        event_count_witness: Some(Fr::from(event_count)),
    };

    let prove_seed = {
        let digest = hex_decode(&attestation.attestation_digest)?;
        digest_bytes(CIRCUIT_PROVE_DOMAIN, &digest)
    };
    let mut rng = ChaCha20Rng::from_seed(prove_seed);
    let proof =
        Groth16::<Bn254>::prove(pk, circuit, &mut rng).map_err(|e| format!("prove failed: {e}"))?;
    let proof_words = proof_to_words(&proof)?;
    let public_inputs_dec =
        build_public_inputs(merkle_lo, merkle_hi, digest_lo, digest_hi, event_count);

    let mut proof_raw = Vec::new();
    proof
        .serialize_compressed(&mut proof_raw)
        .map_err(|e| format!("encode proof: {e}"))?;
    Ok(AttestationProofBundle {
        proof_hex: hex_encode(&proof_raw),
        proof_words,
        public_inputs: public_inputs_dec,
        public_input_schema_version: PUBLIC_INPUT_SCHEMA_VERSION,
    })
}

pub fn verify_attestation_proof(
    vk: &VerifyingKey<Bn254>,
    bundle: &AttestationProofBundle,
) -> Result<bool, String> {
    if bundle.public_input_schema_version != PUBLIC_INPUT_SCHEMA_VERSION {
        return Err(format!(
            "public input schema mismatch: expected {}, got {}",
            PUBLIC_INPUT_SCHEMA_VERSION, bundle.public_input_schema_version
        ));
    }
    if bundle.public_inputs.len() < REQUIRED_PUBLIC_INPUTS {
        return Err(format!(
            "public input count too small: expected at least {}, got {}",
            REQUIRED_PUBLIC_INPUTS,
            bundle.public_inputs.len()
        ));
    }
    let proof_raw = hex_decode(&bundle.proof_hex)?;
    let mut proof_slice = proof_raw.as_slice();
    let proof = Proof::<Bn254>::deserialize_compressed(&mut proof_slice)
        .map_err(|e| format!("decode proof: {e}"))?;
    let public_inputs = bundle
        .public_inputs
        .iter()
        .map(|s| {
            s.parse::<Fr>()
                .map_err(|_| format!("public input parse `{s}` failed"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let pvk = prepare_verifying_key(vk);
    Groth16::<Bn254>::verify_with_processed_vk(&pvk, &public_inputs, &proof)
        .map_err(|e| format!("verify failed: {e}"))
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn proof_words_json_array(bundle: &AttestationProofBundle) -> String {
    format!("[{}]", bundle.proof_words.join(","))
}

pub fn public_inputs_json_array(bundle: &AttestationProofBundle) -> String {
    format!("[{}]", bundle.public_inputs.join(","))
}

/// Split a hex hash into two u128 field elements for the Groth16 circuit.
/// For SHA-256 (64 hex chars → 32 bytes): used directly.
/// For SHA-512 (128 hex chars → 64 bytes): compressed to 32 bytes via SHA-256.
fn split_hash_u128(hex: &str) -> Result<(u128, u128), String> {
    let raw = hex_decode(hex)?;
    let bytes: [u8; 32] = if raw.len() == 32 {
        raw.try_into().unwrap()
    } else if raw.len() == 64 {
        // SHA-512 hash → compress to 32 bytes for circuit field element compatibility.
        digest_bytes("agenthalo.circuit.hash_compress.v1", &raw)
    } else {
        return Err(format!(
            "split_hash_u128: expected 32 or 64 bytes, got {}",
            raw.len()
        ));
    };
    let mut lo_bytes = [0u8; 16];
    let mut hi_bytes = [0u8; 16];
    lo_bytes.copy_from_slice(&bytes[..16]);
    hi_bytes.copy_from_slice(&bytes[16..]);
    Ok((u128::from_le_bytes(lo_bytes), u128::from_le_bytes(hi_bytes)))
}

fn proof_to_words(proof: &Proof<Bn254>) -> Result<[String; 8], String> {
    let ax = fq_to_decimal(&proof.a.x);
    let ay = fq_to_decimal(&proof.a.y);
    let bx0 = fq_to_decimal(&proof.b.x.c1);
    let bx1 = fq_to_decimal(&proof.b.x.c0);
    let by0 = fq_to_decimal(&proof.b.y.c1);
    let by1 = fq_to_decimal(&proof.b.y.c0);
    let cx = fq_to_decimal(&proof.c.x);
    let cy = fq_to_decimal(&proof.c.y);
    Ok([ax, ay, bx0, bx1, by0, by1, cx, cy])
}

fn fq_to_decimal(fq: &Fq) -> String {
    bigint_to_decimal(fq.into_bigint().to_bytes_le())
}

fn bigint_to_decimal(bytes_le: Vec<u8>) -> String {
    BigUint::from_bytes_le(&bytes_le).to_str_radix(10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::attest::AttestationResult;

    fn sample_attestation() -> AttestationResult {
        let merkle_root = hex_encode(&digest_bytes("test.merkle", b"root"));
        let attestation_digest = hex_encode(&digest_bytes("test.attest", b"digest"));
        AttestationResult {
            session_id: Some("sess-1".to_string()),
            blinded_session_ref: None,
            merkle_root,
            event_count: 3,
            content_hashes: vec!["00".repeat(32), "11".repeat(32), "22".repeat(32)],
            witness_algorithm: "ML-DSA-65".to_string(),
            attestation_digest,
            timestamp: 1,
            anonymous: false,
            proof_type: "merkle-sha256".to_string(),
            anonymous_membership_proof: None,
            groth16_proof: None,
            groth16_public_inputs: None,
            tx_hash: None,
            contract_address: None,
            block_number: None,
            chain: None,
        }
    }

    #[test]
    fn test_circuit_satisfies_constraints() {
        let att = sample_attestation();
        let (merkle_lo, merkle_hi) = split_hash_u128(&att.merkle_root).expect("split root");
        let (digest_lo, digest_hi) =
            split_hash_u128(&att.attestation_digest).expect("split digest");
        let circuit = AttestationCircuit {
            merkle_lo_public: Some(Fr::from(merkle_lo)),
            merkle_hi_public: Some(Fr::from(merkle_hi)),
            digest_lo_public: Some(Fr::from(digest_lo)),
            digest_hi_public: Some(Fr::from(digest_hi)),
            event_count_public: Some(Fr::from(att.event_count)),
            merkle_lo_witness: Some(Fr::from(merkle_lo)),
            merkle_hi_witness: Some(Fr::from(merkle_hi)),
            digest_lo_witness: Some(Fr::from(digest_lo)),
            digest_hi_witness: Some(Fr::from(digest_hi)),
            event_count_witness: Some(Fr::from(att.event_count)),
        };
        let cs = ark_relations::r1cs::ConstraintSystem::<Fr>::new_ref();
        circuit
            .generate_constraints(cs.clone())
            .expect("constraints");
        assert!(cs.is_satisfied().expect("satisfied"));
    }

    #[test]
    fn test_circuit_rejects_wrong_merkle_root() {
        let att = sample_attestation();
        let (merkle_lo, merkle_hi) = split_hash_u128(&att.merkle_root).expect("split root");
        let (digest_lo, digest_hi) =
            split_hash_u128(&att.attestation_digest).expect("split digest");
        let circuit = AttestationCircuit {
            merkle_lo_public: Some(Fr::from(merkle_lo + 1)),
            merkle_hi_public: Some(Fr::from(merkle_hi)),
            digest_lo_public: Some(Fr::from(digest_lo)),
            digest_hi_public: Some(Fr::from(digest_hi)),
            event_count_public: Some(Fr::from(att.event_count)),
            merkle_lo_witness: Some(Fr::from(merkle_lo)),
            merkle_hi_witness: Some(Fr::from(merkle_hi)),
            digest_lo_witness: Some(Fr::from(digest_lo)),
            digest_hi_witness: Some(Fr::from(digest_hi)),
            event_count_witness: Some(Fr::from(att.event_count)),
        };
        let cs = ark_relations::r1cs::ConstraintSystem::<Fr>::new_ref();
        circuit
            .generate_constraints(cs.clone())
            .expect("constraints");
        assert!(!cs.is_satisfied().expect("satisfied"));
    }

    #[test]
    fn test_circuit_rejects_wrong_event_count() {
        let att = sample_attestation();
        let (merkle_lo, merkle_hi) = split_hash_u128(&att.merkle_root).expect("split root");
        let (digest_lo, digest_hi) =
            split_hash_u128(&att.attestation_digest).expect("split digest");
        let circuit = AttestationCircuit {
            merkle_lo_public: Some(Fr::from(merkle_lo)),
            merkle_hi_public: Some(Fr::from(merkle_hi)),
            digest_lo_public: Some(Fr::from(digest_lo)),
            digest_hi_public: Some(Fr::from(digest_hi)),
            event_count_public: Some(Fr::from(att.event_count + 1)),
            merkle_lo_witness: Some(Fr::from(merkle_lo)),
            merkle_hi_witness: Some(Fr::from(merkle_hi)),
            digest_lo_witness: Some(Fr::from(digest_lo)),
            digest_hi_witness: Some(Fr::from(digest_hi)),
            event_count_witness: Some(Fr::from(att.event_count)),
        };
        let cs = ark_relations::r1cs::ConstraintSystem::<Fr>::new_ref();
        circuit
            .generate_constraints(cs.clone())
            .expect("constraints");
        assert!(!cs.is_satisfied().expect("satisfied"));
    }

    #[test]
    fn test_circuit_handles_padding() {
        let (pk, vk) = setup_attestation_circuit(256).expect("setup");
        let mut att = sample_attestation();
        att.event_count = 1;
        let bundle = prove_attestation(&pk, &att).expect("prove");
        assert!(verify_attestation_proof(&vk, &bundle).expect("verify"));
    }

    #[test]
    fn test_proof_generation_deterministic() {
        let (pk, _vk) = setup_attestation_circuit(256).expect("setup");
        let att = sample_attestation();
        let a = prove_attestation(&pk, &att).expect("prove a");
        let b = prove_attestation(&pk, &att).expect("prove b");
        assert_eq!(a.proof_hex, b.proof_hex);
        assert_eq!(a.proof_words, b.proof_words);
        assert_eq!(a.public_inputs, b.public_inputs);
    }

    #[test]
    fn test_proof_roundtrip() {
        let (pk, vk) = setup_attestation_circuit(64).expect("setup");
        let att = sample_attestation();
        let bundle = prove_attestation(&pk, &att).expect("prove");
        assert!(verify_attestation_proof(&vk, &bundle).expect("verify"));
    }

    #[test]
    fn test_production_policy_fails_without_crs() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_circuit_prod_fail_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let pk = dir.join("pk.bin");
        let vk = dir.join("vk.bin");
        let metadata = dir.join("metadata.json");
        let err = load_or_setup_attestation_keys_from_paths(
            64,
            CircuitPolicy::ProductionRequired,
            &pk,
            &vk,
            &metadata,
        )
        .expect_err("expected production fail-closed");
        assert!(err.contains("production circuit policy requires existing CRS artifacts"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dev_policy_generates_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_circuit_dev_meta_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let pk = dir.join("pk.bin");
        let vk = dir.join("vk.bin");
        let metadata = dir.join("metadata.json");
        let (_, _, info) = load_or_setup_attestation_keys_from_paths(
            64,
            CircuitPolicy::DevDeterministic,
            &pk,
            &vk,
            &metadata,
        )
        .expect("dev setup");
        assert!(std::path::Path::new(&info.metadata_path).exists());
        let loaded = load_metadata(std::path::Path::new(&info.metadata_path)).expect("load meta");
        assert_eq!(
            loaded.public_input_schema_version,
            PUBLIC_INPUT_SCHEMA_VERSION
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_dev_policy_persists_max_events_change() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_circuit_dev_max_events_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let pk = dir.join("pk.bin");
        let vk = dir.join("vk.bin");
        let metadata = dir.join("metadata.json");

        load_or_setup_attestation_keys_from_paths(
            128,
            CircuitPolicy::DevDeterministic,
            &pk,
            &vk,
            &metadata,
        )
        .expect("initial dev setup");

        load_or_setup_attestation_keys_from_paths(
            256,
            CircuitPolicy::DevDeterministic,
            &pk,
            &vk,
            &metadata,
        )
        .expect("reload with new max_events");

        let loaded = load_metadata(&metadata).expect("load metadata");
        assert_eq!(loaded.max_events, 256);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
