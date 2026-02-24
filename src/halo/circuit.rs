use crate::halo::attest::AttestationResult;
use crate::halo::config;
use crate::halo::util::{digest_bytes, hex_decode_32, hex_encode};
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CircuitKeyInfo {
    pub pk_path: String,
    pub vk_path: String,
    pub max_events: usize,
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
    config::ensure_halo_dir()?;
    config::ensure_circuit_dir()?;
    let max_events = max_events.unwrap_or(MAX_EVENTS_DEFAULT);
    let pk_path = config::circuit_pk_path();
    let vk_path = config::circuit_vk_path();

    if pk_path.exists() && vk_path.exists() {
        let pk_raw = std::fs::read(&pk_path)
            .map_err(|e| format!("read proving key {}: {e}", pk_path.display()))?;
        let vk_raw = std::fs::read(&vk_path)
            .map_err(|e| format!("read verifying key {}: {e}", vk_path.display()))?;
        let mut pk_slice = pk_raw.as_slice();
        let mut vk_slice = vk_raw.as_slice();
        let pk = ProvingKey::<Bn254>::deserialize_compressed(&mut pk_slice)
            .map_err(|e| format!("decode proving key: {e}"))?;
        let vk = VerifyingKey::<Bn254>::deserialize_compressed(&mut vk_slice)
            .map_err(|e| format!("decode verifying key: {e}"))?;
        return Ok((
            pk,
            vk,
            CircuitKeyInfo {
                pk_path: pk_path.display().to_string(),
                vk_path: vk_path.display().to_string(),
                max_events,
            },
        ));
    }

    let (pk, vk) = setup_attestation_circuit(max_events)?;
    let mut pk_raw = Vec::new();
    let mut vk_raw = Vec::new();
    pk.serialize_compressed(&mut pk_raw)
        .map_err(|e| format!("encode proving key: {e}"))?;
    vk.serialize_compressed(&mut vk_raw)
        .map_err(|e| format!("encode verifying key: {e}"))?;
    std::fs::write(&pk_path, pk_raw)
        .map_err(|e| format!("write proving key {}: {e}", pk_path.display()))?;
    std::fs::write(&vk_path, vk_raw)
        .map_err(|e| format!("write verifying key {}: {e}", vk_path.display()))?;

    Ok((
        pk,
        vk,
        CircuitKeyInfo {
            pk_path: pk_path.display().to_string(),
            vk_path: vk_path.display().to_string(),
            max_events,
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
        let digest = hex_decode_32(&attestation.attestation_digest)?;
        digest_bytes(CIRCUIT_PROVE_DOMAIN, &digest)
    };
    let mut rng = ChaCha20Rng::from_seed(prove_seed);
    let proof =
        Groth16::<Bn254>::prove(pk, circuit, &mut rng).map_err(|e| format!("prove failed: {e}"))?;
    let proof_words = proof_to_words(&proof)?;
    let public_inputs = vec![
        Fr::from(merkle_lo),
        Fr::from(merkle_hi),
        Fr::from(digest_lo),
        Fr::from(digest_hi),
        Fr::from(event_count),
    ];
    let public_inputs_dec = public_inputs.iter().map(fr_to_decimal).collect::<Vec<_>>();

    let mut proof_raw = Vec::new();
    proof
        .serialize_compressed(&mut proof_raw)
        .map_err(|e| format!("encode proof: {e}"))?;
    Ok(AttestationProofBundle {
        proof_hex: hex_encode(&proof_raw),
        proof_words,
        public_inputs: public_inputs_dec,
    })
}

pub fn verify_attestation_proof(
    vk: &VerifyingKey<Bn254>,
    bundle: &AttestationProofBundle,
) -> Result<bool, String> {
    let proof_raw = hex_decode_bytes(&bundle.proof_hex)?;
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

pub fn proof_words_json_array(bundle: &AttestationProofBundle) -> String {
    format!("[{}]", bundle.proof_words.join(","))
}

pub fn public_inputs_json_array(bundle: &AttestationProofBundle) -> String {
    format!("[{}]", bundle.public_inputs.join(","))
}

fn split_hash_u128(hex: &str) -> Result<(u128, u128), String> {
    let bytes = hex_decode_32(hex)?;
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

fn fr_to_decimal(fr: &Fr) -> String {
    bigint_to_decimal(fr.into_bigint().to_bytes_le())
}

fn fq_to_decimal(fq: &Fq) -> String {
    bigint_to_decimal(fq.into_bigint().to_bytes_le())
}

fn bigint_to_decimal(bytes_le: Vec<u8>) -> String {
    BigUint::from_bytes_le(&bytes_le).to_str_radix(10)
}

fn hex_decode_bytes(hex: &str) -> Result<Vec<u8>, String> {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    if s.len() % 2 != 0 {
        return Err("hex input must have even length".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks_exact(2) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| "invalid hex".to_string())?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| "invalid hex".to_string())?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
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
}
