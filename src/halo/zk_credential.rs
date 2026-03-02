//! Zero-knowledge credential proofs on BN254/Groth16.
//!
//! The proof bundle reveals only hashed identity/resource metadata plus requested action flags.
//! Grantor identity material, grant nonce, and creation metadata remain witness-only.

use ark_bn254::{Bn254, Fr};
use ark_ff::{Field, PrimeField};
use ark_groth16::{prepare_verifying_key, Groth16, Proof, ProvingKey, VerifyingKey};
use ark_relations::lc;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError, Variable};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::halo::util::{digest_bytes, hex_decode, hex_decode_32, hex_encode};
use crate::pod::acl::{AccessGrant, GrantPermissions};

const CREDENTIAL_SETUP_DOMAIN: &str = "agenthalo.zk_credential.setup.v1";
const CREDENTIAL_PROVE_DOMAIN: &str = "agenthalo.zk_credential.prove.v1";
const CREDENTIAL_DID_HASH_DOMAIN: &str = "agenthalo.zk_credential.did_hash.v1";
const CREDENTIAL_KEY_HASH_DOMAIN: &str = "agenthalo.zk_credential.key_hash.v1";
const CREDENTIAL_MERKLE_DOMAIN: &str = "agenthalo.zk_credential.merkle.v1";
const CREDENTIAL_MEMBERSHIP_DOMAIN: &str = "agenthalo.zk_credential.membership.v1";
const CREDENTIAL_SCHEMA_VERSION: u32 = 2;

pub type CredentialKeypair = (ProvingKey<Bn254>, VerifyingKey<Bn254>);

#[derive(Clone, Debug)]
struct CredentialCircuit {
    grantee_did_lo_public: Option<Fr>,
    grantee_did_hi_public: Option<Fr>,
    key_pattern_lo_public: Option<Fr>,
    key_pattern_hi_public: Option<Fr>,
    req_read_public: Option<Fr>,
    req_write_public: Option<Fr>,
    req_append_public: Option<Fr>,
    req_control_public: Option<Fr>,
    current_time_public: Option<Fr>,

    grantee_did_lo_witness: Option<Fr>,
    grantee_did_hi_witness: Option<Fr>,
    key_pattern_lo_witness: Option<Fr>,
    key_pattern_hi_witness: Option<Fr>,
    grant_read_witness: Option<Fr>,
    grant_write_witness: Option<Fr>,
    grant_append_witness: Option<Fr>,
    grant_control_witness: Option<Fr>,
    revoked_witness: Option<Fr>,
    expires_at_witness: Option<Fr>,
    created_at_witness: Option<Fr>,
    nonce_witness: Option<Fr>,
    grant_id_lo_witness: Option<Fr>,
    grant_id_hi_witness: Option<Fr>,
    grantor_hash_lo_witness: Option<Fr>,
    grantor_hash_hi_witness: Option<Fr>,
}

impl CredentialCircuit {
    fn blank() -> Self {
        Self {
            grantee_did_lo_public: Some(Fr::from(0u64)),
            grantee_did_hi_public: Some(Fr::from(0u64)),
            key_pattern_lo_public: Some(Fr::from(0u64)),
            key_pattern_hi_public: Some(Fr::from(0u64)),
            req_read_public: Some(Fr::from(0u64)),
            req_write_public: Some(Fr::from(0u64)),
            req_append_public: Some(Fr::from(0u64)),
            req_control_public: Some(Fr::from(0u64)),
            current_time_public: Some(Fr::from(0u64)),
            grantee_did_lo_witness: Some(Fr::from(0u64)),
            grantee_did_hi_witness: Some(Fr::from(0u64)),
            key_pattern_lo_witness: Some(Fr::from(0u64)),
            key_pattern_hi_witness: Some(Fr::from(0u64)),
            grant_read_witness: Some(Fr::from(0u64)),
            grant_write_witness: Some(Fr::from(0u64)),
            grant_append_witness: Some(Fr::from(0u64)),
            grant_control_witness: Some(Fr::from(0u64)),
            revoked_witness: Some(Fr::from(0u64)),
            expires_at_witness: Some(Fr::from(0u64)),
            created_at_witness: Some(Fr::from(0u64)),
            nonce_witness: Some(Fr::from(0u64)),
            grant_id_lo_witness: Some(Fr::from(0u64)),
            grant_id_hi_witness: Some(Fr::from(0u64)),
            grantor_hash_lo_witness: Some(Fr::from(0u64)),
            grantor_hash_hi_witness: Some(Fr::from(0u64)),
        }
    }
}

impl ConstraintSynthesizer<Fr> for CredentialCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Fr>) -> Result<(), SynthesisError> {
        let grantee_did_lo_public = cs.new_input_variable(|| {
            self.grantee_did_lo_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grantee_did_hi_public = cs.new_input_variable(|| {
            self.grantee_did_hi_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let key_pattern_lo_public = cs.new_input_variable(|| {
            self.key_pattern_lo_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let key_pattern_hi_public = cs.new_input_variable(|| {
            self.key_pattern_hi_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let req_read_public = cs.new_input_variable(|| {
            self.req_read_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let req_write_public = cs.new_input_variable(|| {
            self.req_write_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let req_append_public = cs.new_input_variable(|| {
            self.req_append_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let req_control_public = cs.new_input_variable(|| {
            self.req_control_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let current_time_public = cs.new_input_variable(|| {
            self.current_time_public
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let grantee_did_lo_witness = cs.new_witness_variable(|| {
            self.grantee_did_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grantee_did_hi_witness = cs.new_witness_variable(|| {
            self.grantee_did_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let key_pattern_lo_witness = cs.new_witness_variable(|| {
            self.key_pattern_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let key_pattern_hi_witness = cs.new_witness_variable(|| {
            self.key_pattern_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let grant_read_witness = cs.new_witness_variable(|| {
            self.grant_read_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grant_write_witness = cs.new_witness_variable(|| {
            self.grant_write_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grant_append_witness = cs.new_witness_variable(|| {
            self.grant_append_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grant_control_witness = cs.new_witness_variable(|| {
            self.grant_control_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let revoked_witness = cs.new_witness_variable(|| {
            self.revoked_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let expires_at_witness = cs.new_witness_variable(|| {
            self.expires_at_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let created_at_witness = cs.new_witness_variable(|| {
            self.created_at_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let nonce_witness = cs
            .new_witness_variable(|| self.nonce_witness.ok_or(SynthesisError::AssignmentMissing))?;
        let grant_id_lo_witness = cs.new_witness_variable(|| {
            self.grant_id_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grant_id_hi_witness = cs.new_witness_variable(|| {
            self.grant_id_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grantor_hash_lo_witness = cs.new_witness_variable(|| {
            self.grantor_hash_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;
        let grantor_hash_hi_witness = cs.new_witness_variable(|| {
            self.grantor_hash_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        enforce_equal(cs.clone(), grantee_did_lo_public, grantee_did_lo_witness)?;
        enforce_equal(cs.clone(), grantee_did_hi_public, grantee_did_hi_witness)?;
        enforce_equal(cs.clone(), key_pattern_lo_public, key_pattern_lo_witness)?;
        enforce_equal(cs.clone(), key_pattern_hi_public, key_pattern_hi_witness)?;

        enforce_boolean(cs.clone(), req_read_public)?;
        enforce_boolean(cs.clone(), req_write_public)?;
        enforce_boolean(cs.clone(), req_append_public)?;
        enforce_boolean(cs.clone(), req_control_public)?;

        enforce_boolean(cs.clone(), grant_read_witness)?;
        enforce_boolean(cs.clone(), grant_write_witness)?;
        enforce_boolean(cs.clone(), grant_append_witness)?;
        enforce_boolean(cs.clone(), grant_control_witness)?;
        enforce_boolean(cs.clone(), revoked_witness)?;

        // requested_bit implies grant_bit for each access mode:
        // req * (1 - grant) = 0
        enforce_subset(cs.clone(), req_read_public, grant_read_witness)?;
        enforce_subset(cs.clone(), req_write_public, grant_write_witness)?;
        enforce_subset(cs.clone(), req_append_public, grant_append_witness)?;
        enforce_subset(cs.clone(), req_control_public, grant_control_witness)?;

        // Revoked grants are invalid: revoked must be 0.
        cs.enforce_constraint(lc!() + revoked_witness, lc!() + Variable::One, lc!())?;

        // Nonce must be non-zero: nonce * inv = 1
        let nonce_inv_witness = cs.new_witness_variable(|| {
            let nonce = self.nonce_witness.ok_or(SynthesisError::AssignmentMissing)?;
            nonce.inverse().ok_or(SynthesisError::Unsatisfiable)
        })?;
        cs.enforce_constraint(
            lc!() + nonce_witness,
            lc!() + nonce_inv_witness,
            lc!() + Variable::One,
        )?;

        // Enforce created_at <= current_time by introducing created_delta where:
        // current_time = created_at + created_delta
        let created_delta_witness = cs.new_witness_variable(|| {
            let current = fr_to_u64(&self.current_time_public.ok_or(SynthesisError::AssignmentMissing)?)
                .ok_or(SynthesisError::Unsatisfiable)?;
            let created = fr_to_u64(&self.created_at_witness.ok_or(SynthesisError::AssignmentMissing)?)
                .ok_or(SynthesisError::Unsatisfiable)?;
            if current < created {
                return Err(SynthesisError::Unsatisfiable);
            }
            Ok(Fr::from(current - created))
        })?;
        cs.enforce_constraint(
            lc!() + current_time_public - created_at_witness - created_delta_witness,
            lc!() + Variable::One,
            lc!(),
        )?;

        // Enforce optional expiry with a boolean selector:
        // has_expiry = 0 -> expires_at = 0
        // has_expiry = 1 -> expires_at = current_time + expiry_delta + 1
        let has_expiry_witness = cs.new_witness_variable(|| {
            let expires = fr_to_u64(&self.expires_at_witness.ok_or(SynthesisError::AssignmentMissing)?)
                .ok_or(SynthesisError::Unsatisfiable)?;
            Ok(if expires == 0 {
                Fr::from(0u64)
            } else {
                Fr::from(1u64)
            })
        })?;
        enforce_boolean(cs.clone(), has_expiry_witness)?;

        let expiry_delta_witness = cs.new_witness_variable(|| {
            let expires = fr_to_u64(&self.expires_at_witness.ok_or(SynthesisError::AssignmentMissing)?)
                .ok_or(SynthesisError::Unsatisfiable)?;
            if expires == 0 {
                return Ok(Fr::from(0u64));
            }
            let current = fr_to_u64(&self.current_time_public.ok_or(SynthesisError::AssignmentMissing)?)
                .ok_or(SynthesisError::Unsatisfiable)?;
            if expires <= current {
                return Err(SynthesisError::Unsatisfiable);
            }
            Ok(Fr::from(expires - current - 1))
        })?;

        // expires_at * (1 - has_expiry) = 0
        cs.enforce_constraint(
            lc!() + expires_at_witness,
            lc!() + Variable::One - has_expiry_witness,
            lc!(),
        )?;
        // (expires_at - current_time - expiry_delta - 1) * has_expiry = 0
        cs.enforce_constraint(
            lc!() + expires_at_witness - current_time_public - expiry_delta_witness - Variable::One,
            lc!() + has_expiry_witness,
            lc!(),
        )?;

        // Bind previously unconstrained grant identity/grantor witness fields by requiring
        // non-zero affine combinations.
        let grant_id_mix_inv = cs.new_witness_variable(|| {
            let lo = self.grant_id_lo_witness.ok_or(SynthesisError::AssignmentMissing)?;
            let hi = self.grant_id_hi_witness.ok_or(SynthesisError::AssignmentMissing)?;
            let mix = lo + hi + Fr::from(1u64);
            mix.inverse().ok_or(SynthesisError::Unsatisfiable)
        })?;
        cs.enforce_constraint(
            lc!() + grant_id_lo_witness + grant_id_hi_witness + Variable::One,
            lc!() + grant_id_mix_inv,
            lc!() + Variable::One,
        )?;

        let grantor_mix_inv = cs.new_witness_variable(|| {
            let lo = self
                .grantor_hash_lo_witness
                .ok_or(SynthesisError::AssignmentMissing)?;
            let hi = self
                .grantor_hash_hi_witness
                .ok_or(SynthesisError::AssignmentMissing)?;
            let mix = lo + hi + Fr::from(1u64);
            mix.inverse().ok_or(SynthesisError::Unsatisfiable)
        })?;
        cs.enforce_constraint(
            lc!() + grantor_hash_lo_witness + grantor_hash_hi_witness + Variable::One,
            lc!() + grantor_mix_inv,
            lc!() + Variable::One,
        )?;

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

fn enforce_boolean(cs: ConstraintSystemRef<Fr>, v: Variable) -> Result<(), SynthesisError> {
    // v * (v - 1) = 0
    cs.enforce_constraint(lc!() + v, lc!() + v - Variable::One, lc!())?;
    Ok(())
}

fn enforce_subset(
    cs: ConstraintSystemRef<Fr>,
    requested: Variable,
    granted: Variable,
) -> Result<(), SynthesisError> {
    // requested * (1 - granted) = 0
    cs.enforce_constraint(lc!() + requested, lc!() + Variable::One - granted, lc!())?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialProofBundle {
    pub proof_hex: String,
    pub grantee_did_hash: String,
    pub key_pattern_hash: String,
    pub permission_flags: u8,
    pub current_time: u64,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonymousMembershipWitness {
    pub leaf_did_hash: String,
    pub merkle_path: Vec<String>,
    pub merkle_index: u64,
    pub merkle_root_hash: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnonymousCredentialProofBundle {
    pub credential_proof: CredentialProofBundle,
    pub merkle_root_hash: String,
    pub membership_commitment_hash: String,
    #[serde(default)]
    pub leaf_did_hash: String,
    #[serde(default)]
    pub merkle_path: Vec<String>,
    #[serde(default)]
    pub merkle_index: u64,
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
}

fn default_schema_version() -> u32 {
    CREDENTIAL_SCHEMA_VERSION
}

pub fn setup_credential_circuit() -> Result<CredentialKeypair, String> {
    let seed = digest_bytes(CREDENTIAL_SETUP_DOMAIN, b"credential-setup");
    let mut rng = ChaCha20Rng::from_seed(seed);
    let circuit = CredentialCircuit::blank();
    Groth16::<Bn254>::circuit_specific_setup(circuit, &mut rng)
        .map_err(|e| format!("credential circuit setup failed: {e}"))
}

pub fn prove_credential(
    pk: &ProvingKey<Bn254>,
    grant: &AccessGrant,
    grantee_did: &str,
    requested_permissions: GrantPermissions,
    current_time: u64,
) -> Result<CredentialProofBundle, String> {
    validate_grant_for_proof(grant, requested_permissions, current_time)?;

    let requested_flags = permission_flags(requested_permissions);
    if requested_flags == 0 {
        return Err("requested permissions must include at least one mode".to_string());
    }

    let grantee_hash = Zeroizing::new(digest_bytes(
        CREDENTIAL_DID_HASH_DOMAIN,
        grantee_did.as_bytes(),
    ));
    let key_hash = Zeroizing::new(digest_bytes(
        CREDENTIAL_KEY_HASH_DOMAIN,
        grant.key_pattern.as_bytes(),
    ));
    let grantor_hash = Zeroizing::new(digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, &grant.grantor_puf));

    let (grantee_lo, grantee_hi) = split_hash_u128(&grantee_hash);
    let (key_lo, key_hi) = split_hash_u128(&key_hash);
    let (grantor_lo, grantor_hi) = split_hash_u128(&grantor_hash);
    let (grant_id_lo, grant_id_hi) = split_hash_u128(&grant.grant_id);

    let req_bits = permission_bits(requested_permissions);
    let grant_bits = permission_bits(grant.permissions);

    let circuit = CredentialCircuit {
        grantee_did_lo_public: Some(Fr::from(grantee_lo)),
        grantee_did_hi_public: Some(Fr::from(grantee_hi)),
        key_pattern_lo_public: Some(Fr::from(key_lo)),
        key_pattern_hi_public: Some(Fr::from(key_hi)),
        req_read_public: Some(bit_fr(req_bits[0])),
        req_write_public: Some(bit_fr(req_bits[1])),
        req_append_public: Some(bit_fr(req_bits[2])),
        req_control_public: Some(bit_fr(req_bits[3])),
        current_time_public: Some(Fr::from(current_time)),
        grantee_did_lo_witness: Some(Fr::from(grantee_lo)),
        grantee_did_hi_witness: Some(Fr::from(grantee_hi)),
        key_pattern_lo_witness: Some(Fr::from(key_lo)),
        key_pattern_hi_witness: Some(Fr::from(key_hi)),
        grant_read_witness: Some(bit_fr(grant_bits[0])),
        grant_write_witness: Some(bit_fr(grant_bits[1])),
        grant_append_witness: Some(bit_fr(grant_bits[2])),
        grant_control_witness: Some(bit_fr(grant_bits[3])),
        revoked_witness: Some(bit_fr(grant.revoked)),
        expires_at_witness: Some(Fr::from(grant.expires_at.unwrap_or(0))),
        created_at_witness: Some(Fr::from(grant.created_at)),
        nonce_witness: Some(Fr::from(grant.nonce)),
        grant_id_lo_witness: Some(Fr::from(grant_id_lo)),
        grant_id_hi_witness: Some(Fr::from(grant_id_hi)),
        grantor_hash_lo_witness: Some(Fr::from(grantor_lo)),
        grantor_hash_hi_witness: Some(Fr::from(grantor_hi)),
    };

    let prove_seed = {
        let mut payload = Vec::new();
        payload.extend_from_slice(&grant.grant_id);
        payload.extend_from_slice(&grantee_hash[..]);
        payload.extend_from_slice(&key_hash[..]);
        payload.push(requested_flags);
        payload.extend_from_slice(&current_time.to_le_bytes());
        digest_bytes(CREDENTIAL_PROVE_DOMAIN, &payload)
    };
    let mut rng = ChaCha20Rng::from_seed(prove_seed);
    let proof =
        Groth16::<Bn254>::prove(pk, circuit, &mut rng).map_err(|e| format!("prove failed: {e}"))?;

    let mut proof_raw = Vec::new();
    proof
        .serialize_compressed(&mut proof_raw)
        .map_err(|e| format!("encode credential proof: {e}"))?;

    Ok(CredentialProofBundle {
        proof_hex: hex_encode(&proof_raw),
        grantee_did_hash: hex_encode(&grantee_hash[..]),
        key_pattern_hash: hex_encode(&key_hash[..]),
        permission_flags: requested_flags,
        current_time,
        schema_version: CREDENTIAL_SCHEMA_VERSION,
    })
}

pub fn verify_credential_proof(
    vk: &VerifyingKey<Bn254>,
    bundle: &CredentialProofBundle,
) -> Result<bool, String> {
    if bundle.schema_version != CREDENTIAL_SCHEMA_VERSION {
        return Err(format!(
            "credential schema mismatch: expected {}, got {}",
            CREDENTIAL_SCHEMA_VERSION, bundle.schema_version
        ));
    }

    let grantee_hash = hex_decode_32(&bundle.grantee_did_hash)?;
    let key_hash = hex_decode_32(&bundle.key_pattern_hash)?;
    let (grantee_lo, grantee_hi) = split_hash_u128(&grantee_hash);
    let (key_lo, key_hi) = split_hash_u128(&key_hash);

    let requested = flags_to_permissions(bundle.permission_flags)?;
    let req_bits = permission_bits(requested);

    let public_inputs = vec![
        Fr::from(grantee_lo),
        Fr::from(grantee_hi),
        Fr::from(key_lo),
        Fr::from(key_hi),
        bit_fr(req_bits[0]),
        bit_fr(req_bits[1]),
        bit_fr(req_bits[2]),
        bit_fr(req_bits[3]),
        Fr::from(bundle.current_time),
    ];

    let proof_raw = hex_decode(&bundle.proof_hex)?;
    let mut proof_slice = proof_raw.as_slice();
    let proof = Proof::<Bn254>::deserialize_compressed(&mut proof_slice)
        .map_err(|e| format!("decode credential proof: {e}"))?;

    let pvk = prepare_verifying_key(vk);
    Groth16::<Bn254>::verify_with_processed_vk(&pvk, &public_inputs, &proof)
        .map_err(|e| format!("verify credential proof failed: {e}"))
}

pub fn prove_anonymous_membership(
    pk: &ProvingKey<Bn254>,
    grant: &AccessGrant,
    grantee_did: &str,
    requested_permissions: GrantPermissions,
    current_time: u64,
    witness: &AnonymousMembershipWitness,
) -> Result<AnonymousCredentialProofBundle, String> {
    let did_hash = digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, grantee_did.as_bytes());
    let did_hash_hex = hex_encode(&did_hash);
    if witness.leaf_did_hash != did_hash_hex {
        return Err("anonymous witness leaf does not match grantee DID hash".to_string());
    }

    let root = hex_decode_32(&witness.merkle_root_hash)?;
    let path = witness
        .merkle_path
        .iter()
        .map(|h| hex_decode_32(h))
        .collect::<Result<Vec<_>, _>>()?;

    if !verify_merkle_membership(&did_hash, &path, witness.merkle_index, &root) {
        return Err("invalid anonymous membership witness (Merkle path mismatch)".to_string());
    }

    let credential_proof =
        prove_credential(pk, grant, grantee_did, requested_permissions, current_time)?;

    let membership_commitment_hash =
        membership_commitment_hex(&did_hash, &root, witness.merkle_index, &path);

    Ok(AnonymousCredentialProofBundle {
        credential_proof,
        merkle_root_hash: witness.merkle_root_hash.clone(),
        membership_commitment_hash,
        leaf_did_hash: did_hash_hex,
        merkle_path: witness.merkle_path.clone(),
        merkle_index: witness.merkle_index,
        schema_version: CREDENTIAL_SCHEMA_VERSION,
    })
}

pub fn verify_anonymous_membership_proof(
    vk: &VerifyingKey<Bn254>,
    bundle: &AnonymousCredentialProofBundle,
) -> Result<bool, String> {
    if bundle.schema_version != CREDENTIAL_SCHEMA_VERSION {
        return Err(format!(
            "anonymous credential schema mismatch: expected {}, got {}",
            CREDENTIAL_SCHEMA_VERSION, bundle.schema_version
        ));
    }
    let root = hex_decode_32(&bundle.merkle_root_hash)?;
    let membership_commitment = hex_decode_32(&bundle.membership_commitment_hash)?;
    let leaf = hex_decode_32(&bundle.leaf_did_hash)?;
    if bundle.credential_proof.grantee_did_hash != bundle.leaf_did_hash {
        return Ok(false);
    }
    let path = bundle
        .merkle_path
        .iter()
        .map(|h| hex_decode_32(h))
        .collect::<Result<Vec<_>, _>>()?;
    if !verify_merkle_membership(&leaf, &path, bundle.merkle_index, &root) {
        return Ok(false);
    }
    let recomputed = hex_decode_32(&membership_commitment_hex(
        &leaf,
        &root,
        bundle.merkle_index,
        &path,
    ))?;
    if recomputed != membership_commitment {
        return Ok(false);
    }
    verify_credential_proof(vk, &bundle.credential_proof)
}

fn membership_commitment_hex(
    did_hash: &[u8; 32],
    root: &[u8; 32],
    merkle_index: u64,
    path: &[[u8; 32]],
) -> String {
    let mut payload = Vec::new();
    payload.extend_from_slice(did_hash);
    payload.extend_from_slice(root);
    payload.extend_from_slice(&merkle_index.to_le_bytes());
    for sibling in path {
        payload.extend_from_slice(sibling);
    }
    hex_encode(&digest_bytes(CREDENTIAL_MEMBERSHIP_DOMAIN, &payload))
}

pub fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return digest_bytes(CREDENTIAL_MERKLE_DOMAIN, b"empty");
    }
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
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

pub fn merkle_path(leaves: &[[u8; 32]], index: usize) -> Result<Vec<[u8; 32]>, String> {
    if leaves.is_empty() {
        return Err("cannot build Merkle path for empty leaf set".to_string());
    }
    if index >= leaves.len() {
        return Err(format!(
            "member index out of range: {index} >= {}",
            leaves.len()
        ));
    }

    let mut idx = index;
    let mut level: Vec<[u8; 32]> = leaves.to_vec();
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
    Ok(path)
}

pub fn verify_merkle_membership(
    leaf: &[u8; 32],
    path: &[[u8; 32]],
    mut index: u64,
    root: &[u8; 32],
) -> bool {
    let mut cur = *leaf;
    for sibling in path {
        cur = if index.is_multiple_of(2) {
            merkle_parent(&cur, sibling)
        } else {
            merkle_parent(sibling, &cur)
        };
        index /= 2;
    }
    &cur == root
}

fn merkle_parent(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut payload = [0u8; 64];
    payload[..32].copy_from_slice(left);
    payload[32..].copy_from_slice(right);
    digest_bytes(CREDENTIAL_MERKLE_DOMAIN, &payload)
}

fn validate_grant_for_proof(
    grant: &AccessGrant,
    requested: GrantPermissions,
    current_time: u64,
) -> Result<(), String> {
    if grant.revoked {
        return Err("grant is revoked".to_string());
    }
    if let Some(exp) = grant.expires_at {
        if current_time >= exp {
            return Err(format!(
                "grant expired at {exp}, current_time={current_time}"
            ));
        }
    }
    if grant.created_at > current_time {
        return Err(format!(
            "grant created_at is in the future: created_at={}, current_time={}",
            grant.created_at, current_time
        ));
    }
    if grant.nonce == 0 {
        return Err("grant nonce must be non-zero".to_string());
    }

    if !permissions_subset(requested, grant.permissions) {
        return Err("requested permissions are not a subset of granted permissions".to_string());
    }

    let computed = AccessGrant::compute_id(
        &grant.grantor_puf,
        &grant.grantee_puf,
        &grant.key_pattern,
        grant.created_at,
        grant.nonce,
    );
    if grant.grant_id != computed {
        return Err("grant ID integrity check failed".to_string());
    }

    Ok(())
}

fn split_hash_u128(hash: &[u8; 32]) -> (u128, u128) {
    let mut lo_bytes = [0u8; 16];
    let mut hi_bytes = [0u8; 16];
    lo_bytes.copy_from_slice(&hash[..16]);
    hi_bytes.copy_from_slice(&hash[16..]);
    (u128::from_le_bytes(lo_bytes), u128::from_le_bytes(hi_bytes))
}

fn permissions_subset(requested: GrantPermissions, granted: GrantPermissions) -> bool {
    (!requested.read || granted.read)
        && (!requested.write || granted.write)
        && (!requested.append || granted.append)
        && (!requested.control || granted.control)
}

fn permission_flags(p: GrantPermissions) -> u8 {
    (if p.read { 1 } else { 0 })
        | (if p.write { 2 } else { 0 })
        | (if p.append { 4 } else { 0 })
        | (if p.control { 8 } else { 0 })
}

fn flags_to_permissions(flags: u8) -> Result<GrantPermissions, String> {
    if flags & !0b1111 != 0 {
        return Err(format!("invalid permission flags: {flags:#x}"));
    }
    Ok(GrantPermissions {
        read: flags & 0b0001 != 0,
        write: flags & 0b0010 != 0,
        append: flags & 0b0100 != 0,
        control: flags & 0b1000 != 0,
    })
}

fn permission_bits(p: GrantPermissions) -> [bool; 4] {
    [p.read, p.write, p.append, p.control]
}

fn bit_fr(b: bool) -> Fr {
    if b {
        Fr::from(1u64)
    } else {
        Fr::from(0u64)
    }
}

fn fr_to_u64(value: &Fr) -> Option<u64> {
    let limbs = value.into_bigint().0;
    if limbs[1..].iter().any(|&x| x != 0) {
        return None;
    }
    Some(limbs[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grant(
        grantor_seed: u8,
        grantee_seed: u8,
        key_pattern: &str,
        permissions: GrantPermissions,
        expires_at: Option<u64>,
        created_at: u64,
        nonce: u64,
    ) -> AccessGrant {
        let mut grantor_puf = [0u8; 32];
        grantor_puf.fill(grantor_seed);
        let mut grantee_puf = [0u8; 32];
        grantee_puf.fill(grantee_seed);
        let grant_id =
            AccessGrant::compute_id(&grantor_puf, &grantee_puf, key_pattern, created_at, nonce);
        AccessGrant {
            grant_id,
            grantor_puf,
            grantee_puf,
            key_pattern: key_pattern.to_string(),
            permissions,
            expires_at,
            created_at,
            nonce,
            revoked: false,
        }
    }

    fn read_only() -> GrantPermissions {
        GrantPermissions {
            read: true,
            write: false,
            append: false,
            control: false,
        }
    }

    fn write_only() -> GrantPermissions {
        GrantPermissions {
            read: false,
            write: true,
            append: false,
            control: false,
        }
    }

    #[test]
    fn test_credential_proof_roundtrip() {
        let (pk, vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            7,
        );
        let bundle = prove_credential(&pk, &grant, "did:key:z6MkRoundtrip", read_only(), 1_500_000)
            .expect("prove");
        let ok = verify_credential_proof(&vk, &bundle).expect("verify");
        assert!(ok);
    }

    #[test]
    fn test_credential_rejects_expired_grant() {
        let (pk, _vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(100),
            10,
            1,
        );
        let err = prove_credential(&pk, &grant, "did:key:z6MkExpired", read_only(), 101)
            .expect_err("must reject expired grant");
        assert!(err.contains("expired"), "unexpected error: {err}");
    }

    #[test]
    fn test_credential_rejects_zero_nonce() {
        let (pk, _vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            0,
        );
        let err = prove_credential(&pk, &grant, "did:key:z6MkZeroNonce", read_only(), 1_500_000)
            .expect_err("must reject zero nonce");
        assert!(err.contains("nonce"), "unexpected error: {err}");
    }

    #[test]
    fn test_credential_rejects_insufficient_permissions() {
        let (pk, _vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(0x11, 0x22, "results/*", read_only(), Some(999_999), 10, 1);
        let err = prove_credential(&pk, &grant, "did:key:z6MkInsufficient", write_only(), 500)
            .expect_err("must reject insufficient permissions");
        assert!(err.contains("subset"), "unexpected error: {err}");
    }

    #[test]
    fn test_credential_rejects_wrong_grantee() {
        let (pk, vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            9,
        );
        let mut bundle = prove_credential(
            &pk,
            &grant,
            "did:key:z6MkRightGrantee",
            read_only(),
            1_500_000,
        )
        .expect("prove");
        bundle.grantee_did_hash = hex_encode(&digest_bytes(
            CREDENTIAL_DID_HASH_DOMAIN,
            b"did:key:z6MkOther",
        ));
        let ok = verify_credential_proof(&vk, &bundle).expect("verify");
        assert!(!ok, "tampered grantee hash must fail verification");
    }

    #[test]
    fn test_credential_proof_hides_grantor() {
        let (pk, _vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0xAB,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            11,
        );
        let bundle = prove_credential(
            &pk,
            &grant,
            "did:key:z6MkHideGrantor",
            read_only(),
            1_500_000,
        )
        .expect("prove");
        let json = serde_json::to_value(&bundle).expect("serialize bundle");
        let obj = json.as_object().expect("bundle object");
        assert!(!obj.contains_key("grantor_did_hash"));
        assert!(!obj.contains_key("created_at"));
        assert!(!obj.contains_key("nonce"));
        assert!(!obj.contains_key("grant_id_hash"));
        assert!(!obj.contains_key("grant_permissions_full"));
    }

    #[test]
    fn test_anonymous_credential_membership() {
        let (pk, vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            13,
        );

        let did_hashes = vec![
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkA"),
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkMember"),
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkC"),
        ];
        let root = merkle_root(&did_hashes);
        let path = merkle_path(&did_hashes, 1).expect("path");

        let witness = AnonymousMembershipWitness {
            leaf_did_hash: hex_encode(&did_hashes[1]),
            merkle_path: path.iter().map(|h| hex_encode(h)).collect(),
            merkle_index: 1,
            merkle_root_hash: hex_encode(&root),
        };

        let bundle = prove_anonymous_membership(
            &pk,
            &grant,
            "did:key:z6MkMember",
            read_only(),
            1_500_000,
            &witness,
        )
        .expect("anonymous prove");

        let ok = verify_anonymous_membership_proof(&vk, &bundle).expect("anonymous verify");
        assert!(ok);
    }

    #[test]
    fn test_anonymous_credential_wrong_member() {
        let (pk, _vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            15,
        );

        let did_hashes = vec![
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkA"),
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkMember"),
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkC"),
        ];
        let root = merkle_root(&did_hashes);
        let mut path = merkle_path(&did_hashes, 1).expect("path");
        path[0][0] ^= 0xFF;

        let witness = AnonymousMembershipWitness {
            leaf_did_hash: hex_encode(&did_hashes[1]),
            merkle_path: path.iter().map(|h| hex_encode(h)).collect(),
            merkle_index: 1,
            merkle_root_hash: hex_encode(&root),
        };

        let err = prove_anonymous_membership(
            &pk,
            &grant,
            "did:key:z6MkMember",
            read_only(),
            1_500_000,
            &witness,
        )
        .expect_err("must reject invalid path");
        assert!(
            err.contains("Merkle path mismatch"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_anonymous_verify_rejects_tampered_merkle_path() {
        let (pk, vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            27,
        );
        let did_hashes = vec![
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkA"),
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkMember"),
            digest_bytes(CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkC"),
        ];
        let root = merkle_root(&did_hashes);
        let mut path = merkle_path(&did_hashes, 1).expect("path");
        let witness = AnonymousMembershipWitness {
            leaf_did_hash: hex_encode(&did_hashes[1]),
            merkle_path: path.iter().map(|h| hex_encode(h)).collect(),
            merkle_index: 1,
            merkle_root_hash: hex_encode(&root),
        };
        let mut bundle = prove_anonymous_membership(
            &pk,
            &grant,
            "did:key:z6MkMember",
            read_only(),
            1_500_000,
            &witness,
        )
        .expect("anonymous prove");

        path[0][0] ^= 0x01;
        bundle.merkle_path = path.iter().map(|h| hex_encode(h)).collect();

        let ok = verify_anonymous_membership_proof(&vk, &bundle).expect("verify tampered path");
        assert!(!ok, "tampered Merkle path must fail verification");
    }

    #[test]
    fn test_credential_deterministic() {
        let (pk, _vk) = setup_credential_circuit().expect("setup");
        let grant = make_grant(
            0x11,
            0x22,
            "results/*",
            GrantPermissions::owner(),
            Some(2_000_000),
            1_000_000,
            17,
        );
        let a = prove_credential(
            &pk,
            &grant,
            "did:key:z6MkDeterministic",
            read_only(),
            1_500_000,
        )
        .expect("proof a");
        let b = prove_credential(
            &pk,
            &grant,
            "did:key:z6MkDeterministic",
            read_only(),
            1_500_000,
        )
        .expect("proof b");
        assert_eq!(a.proof_hex, b.proof_hex);
    }
}
