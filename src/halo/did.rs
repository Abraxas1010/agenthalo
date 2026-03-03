#[cfg(feature = "zk-compute")]
use crate::halo::zk_compute::ComputeReceipt;
use crate::halo::zk_credential::CredentialProofBundle;
use ed25519_dalek::{
    Signature as Ed25519Signature, Signer as Ed25519Signer, SigningKey as Ed25519SigningKey,
    Verifier as Ed25519Verifier, VerifyingKey as Ed25519VerifyingKey,
};
use hkdf::Hkdf;
use ml_dsa::{
    EncodedSignature as MlDsaEncodedSignature, EncodedVerifyingKey as MlDsaEncodedVerifyingKey,
    KeyGen, KeyPair as MlDsaKeyPair, MlDsa65, Signature as MlDsaSignature,
    VerifyingKey as MlDsaVerifyingKey,
};
use ml_kem::{EncodedSizeUser, KemCore, MlKem768};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::Zeroizing;

const HKDF_IDENTITY_SALT: &[u8] = b"agenthalo-genesis-identity-v1";
const HKDF_DID_PQ_SIGNING_INFO: &[u8] = b"agenthalo-did-pq-signing-v1";
const MLDSA65_CONTEXT: &[u8] = b"agenthalo-did-v1";

const MULTICODEC_ED25519_PUB: &[u8] = &[0xed, 0x01];
const MULTICODEC_X25519_PUB: &[u8] = &[0xec, 0x01];

const TYPE_ED25519: &str = "Ed25519VerificationKey2020";
const TYPE_MLDSA65: &str = "MlDsa65VerificationKey2025";
const TYPE_X25519: &str = "X25519KeyAgreementKey2020";
const TYPE_MLKEM768: &str = "MlKem768KeyAgreementKey2025";

type MlKem768DecapsulationKey = <MlKem768 as KemCore>::DecapsulationKey;

pub struct DIDIdentity {
    pub did: String,
    pub did_document: DIDDocument,
    pub ed25519_signing_key: Ed25519SigningKey,
    pub mldsa65_signing_key: MlDsaKeyPair<MlDsa65>,
    pub x25519_agreement_key: X25519StaticSecret,
    pub mlkem768_decapsulation_key: MlKem768DecapsulationKey,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DIDDocument {
    pub id: String,
    #[serde(
        default,
        rename = "alsoKnownAs",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub also_known_as: Vec<String>,
    #[serde(rename = "verificationMethod")]
    pub verification_method: Vec<VerificationMethod>,
    #[serde(rename = "keyAgreement")]
    pub key_agreement: Vec<KeyAgreementMethod>,
    #[serde(rename = "authentication")]
    pub authentication: Vec<String>,
    #[serde(rename = "assertionMethod")]
    pub assertion_method: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationMethod {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub controller: String,
    #[serde(rename = "publicKeyMultibase")]
    pub public_key_multibase: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyAgreementMethod {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub controller: String,
    #[serde(rename = "publicKeyMultibase")]
    pub public_key_multibase: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DIDCommCredentialAttachment {
    pub proof_bundle: CredentialProofBundle,
    pub resource_uri: String,
    pub requested_action: String,
}

#[cfg(feature = "zk-compute")]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DIDCommComputeAttachment {
    pub receipt: ComputeReceipt,
    pub result_summary: String,
}

fn hkdf_expand<const N: usize>(seed: &[u8; 64], info: &[u8]) -> Result<[u8; N], String> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_IDENTITY_SALT), seed.as_slice());
    let mut out = [0u8; N];
    hk.expand(info, &mut out).map_err(|_| {
        format!(
            "HKDF expand failed for info {}",
            String::from_utf8_lossy(info)
        )
    })?;
    Ok(out)
}

fn encode_multibase_key(prefix: &[u8], public_key: &[u8]) -> String {
    let mut payload = Vec::with_capacity(prefix.len() + public_key.len());
    payload.extend_from_slice(prefix);
    payload.extend_from_slice(public_key);
    multibase::encode(multibase::Base::Base58Btc, payload)
}

/// ML-DSA/ML-KEM do not currently have stable did:key multicodec assignments in this stack.
/// We therefore emit raw key bytes in multibase and rely on `verificationMethod.type` as key type.
fn encode_multibase_untyped_key(public_key: &[u8]) -> String {
    encode_multibase_key(&[], public_key)
}

fn decode_multibase_key(encoded: &str, expected_prefix: &[u8]) -> Result<Vec<u8>, String> {
    let (_, decoded) = multibase::decode(encoded)
        .map_err(|e| format!("multibase decode failed for key material: {e}"))?;
    if decoded.len() < expected_prefix.len() {
        return Err("decoded multibase key is shorter than expected prefix".to_string());
    }
    if !decoded.starts_with(expected_prefix) {
        return Err("decoded multibase key has unexpected multicodec prefix".to_string());
    }
    Ok(decoded[expected_prefix.len()..].to_vec())
}

/// Counterpart of `encode_multibase_untyped_key` for PQ verification materials.
fn decode_multibase_untyped_key(encoded: &str) -> Result<Vec<u8>, String> {
    let (_, decoded) = multibase::decode(encoded)
        .map_err(|e| format!("multibase decode failed for untyped key material: {e}"))?;
    Ok(decoded)
}

fn did_from_ed25519_public_key(public_key: &[u8; 32]) -> String {
    let encoded = encode_multibase_key(MULTICODEC_ED25519_PUB, public_key);
    format!("did:key:{encoded}")
}

fn did_fragment(did: &str, suffix: &str) -> String {
    format!("{did}#{suffix}")
}

fn build_did_document_from_parts(
    did: &str,
    ed25519_public_key: &[u8; 32],
    mldsa65_public_key: &[u8],
    x25519_public_key: &[u8; 32],
    mlkem768_public_key: &[u8],
) -> DIDDocument {
    // T7: did_document_wellformed
    let ed_key_id = did_fragment(did, "key-ed25519-1");
    let pq_key_id = did_fragment(did, "key-mldsa65-1");
    let x25519_key_id = did_fragment(did, "key-x25519-1");
    let mlkem_key_id = did_fragment(did, "key-mlkem768-1");

    DIDDocument {
        id: did.to_string(),
        also_known_as: Vec::new(),
        verification_method: vec![
            VerificationMethod {
                id: ed_key_id.clone(),
                type_: TYPE_ED25519.to_string(),
                controller: did.to_string(),
                public_key_multibase: encode_multibase_key(
                    MULTICODEC_ED25519_PUB,
                    ed25519_public_key,
                ),
            },
            VerificationMethod {
                id: pq_key_id.clone(),
                type_: TYPE_MLDSA65.to_string(),
                controller: did.to_string(),
                public_key_multibase: encode_multibase_untyped_key(mldsa65_public_key),
            },
        ],
        key_agreement: vec![
            KeyAgreementMethod {
                id: x25519_key_id,
                type_: TYPE_X25519.to_string(),
                controller: did.to_string(),
                public_key_multibase: encode_multibase_key(
                    MULTICODEC_X25519_PUB,
                    x25519_public_key,
                ),
            },
            KeyAgreementMethod {
                id: mlkem_key_id,
                type_: TYPE_MLKEM768.to_string(),
                controller: did.to_string(),
                public_key_multibase: encode_multibase_untyped_key(mlkem768_public_key),
            },
        ],
        authentication: vec![ed_key_id.clone(), pq_key_id.clone()],
        assertion_method: vec![ed_key_id, pq_key_id],
    }
}

pub fn did_from_genesis_seed(seed: &[u8; 64]) -> Result<DIDIdentity, String> {
    let ed25519_seed = Zeroizing::new(crate::halo::genesis_seed::derive_p2p_identity(seed));
    let ed25519_signing_key = Ed25519SigningKey::from_bytes(&ed25519_seed);
    let ed25519_public_key = ed25519_signing_key.verifying_key().to_bytes();

    let mldsa65_seed_bytes = Zeroizing::new(hkdf_expand::<32>(seed, HKDF_DID_PQ_SIGNING_INFO)?);
    let mldsa65_seed = ml_dsa::Seed::try_from(mldsa65_seed_bytes.as_slice())
        .map_err(|_| "failed to build ML-DSA-65 seed from HKDF output".to_string())?;
    let mldsa65_keypair = MlDsa65::from_seed(&mldsa65_seed);

    let (x25519_secret_bytes_raw, mlkem_seed_bytes_raw) =
        crate::halo::genesis_seed::derive_did_agreement_keys(seed);
    let x25519_secret_bytes = Zeroizing::new(x25519_secret_bytes_raw);
    let mlkem_seed_bytes = Zeroizing::new(mlkem_seed_bytes_raw);
    let x25519_agreement_key = X25519StaticSecret::from(*x25519_secret_bytes);

    let mut chacha_seed = [0u8; 32];
    chacha_seed.copy_from_slice(&mlkem_seed_bytes[..32]);
    let chacha_seed = Zeroizing::new(chacha_seed);
    let mut mlkem_rng = ChaCha20Rng::from_seed(*chacha_seed);
    let (mlkem768_decapsulation_key, _) = MlKem768::generate(&mut mlkem_rng);

    let did = did_from_ed25519_public_key(&ed25519_public_key);
    let x25519_public_key = X25519PublicKey::from(&x25519_agreement_key).to_bytes();
    let mlkem_public_key = mlkem768_decapsulation_key.encapsulation_key();
    let did_document = build_did_document_from_parts(
        &did,
        &ed25519_public_key,
        mldsa65_keypair.verifying_key().encode().as_slice(),
        &x25519_public_key,
        mlkem_public_key.as_bytes().as_slice(),
    );

    Ok(DIDIdentity {
        did,
        did_document,
        ed25519_signing_key,
        mldsa65_signing_key: mldsa65_keypair,
        x25519_agreement_key,
        mlkem768_decapsulation_key,
    })
}

pub fn build_did_document(identity: &DIDIdentity) -> DIDDocument {
    let ed25519_public_key = identity.ed25519_signing_key.verifying_key().to_bytes();
    let x25519_public_key = X25519PublicKey::from(&identity.x25519_agreement_key).to_bytes();
    let mlkem_public_key = identity.mlkem768_decapsulation_key.encapsulation_key();
    build_did_document_from_parts(
        &identity.did,
        &ed25519_public_key,
        identity
            .mldsa65_signing_key
            .verifying_key()
            .encode()
            .as_slice(),
        &x25519_public_key,
        mlkem_public_key.as_bytes().as_slice(),
    )
}

pub fn did_document_to_json(doc: &DIDDocument) -> serde_json::Value {
    serde_json::to_value(doc).expect("DID document should always serialize")
}

/// Bind an EVM address to a DID document via `alsoKnownAs` using the did:pkh specification.
/// Returns true if the binding was added (false if already present).
pub fn bind_evm_address(doc: &mut DIDDocument, evm_address: &str) -> bool {
    let did_pkh = format!("did:pkh:eip155:1:{}", evm_address.to_lowercase());
    if doc.also_known_as.contains(&did_pkh) {
        return false;
    }
    doc.also_known_as.push(did_pkh);
    true
}

pub fn dual_sign(identity: &DIDIdentity, message: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let ed_signature = identity
        .ed25519_signing_key
        .sign(message)
        .to_bytes()
        .to_vec();
    let pq_signature = identity
        .mldsa65_signing_key
        .signing_key()
        .sign_deterministic(message, MLDSA65_CONTEXT)
        .map_err(|e| format!("ML-DSA signing failed: {e}"))?
        .encode()
        .as_slice()
        .to_vec();
    Ok((ed_signature, pq_signature))
}

pub fn dual_verify(
    doc: &DIDDocument,
    message: &[u8],
    ed_sig: &[u8],
    pq_sig: &[u8],
) -> Result<bool, String> {
    let ed_method = doc
        .verification_method
        .iter()
        .find(|m| m.type_ == TYPE_ED25519)
        .ok_or_else(|| "DID document missing Ed25519 verification method".to_string())?;
    let pq_method = doc
        .verification_method
        .iter()
        .find(|m| m.type_ == TYPE_MLDSA65)
        .ok_or_else(|| "DID document missing ML-DSA-65 verification method".to_string())?;

    let ed_public_key_raw =
        decode_multibase_key(&ed_method.public_key_multibase, MULTICODEC_ED25519_PUB)?;
    let ed_public_key_bytes: [u8; 32] = ed_public_key_raw
        .as_slice()
        .try_into()
        .map_err(|_| "Ed25519 key length must be exactly 32 bytes".to_string())?;
    let ed_vk = Ed25519VerifyingKey::from_bytes(&ed_public_key_bytes)
        .map_err(|e| format!("invalid Ed25519 key in DID document: {e}"))?;
    let ed_signature = Ed25519Signature::from_slice(ed_sig)
        .map_err(|e| format!("invalid Ed25519 signature bytes: {e}"))?;
    let ed_ok = ed_vk.verify(message, &ed_signature).is_ok();

    let pq_public_key_raw = decode_multibase_untyped_key(&pq_method.public_key_multibase)?;
    let pq_encoded_vk = MlDsaEncodedVerifyingKey::<MlDsa65>::try_from(pq_public_key_raw.as_slice())
        .map_err(|_| "invalid ML-DSA verifying key encoding in DID document".to_string())?;
    let pq_vk = MlDsaVerifyingKey::<MlDsa65>::decode(&pq_encoded_vk);
    let pq_encoded_sig = MlDsaEncodedSignature::<MlDsa65>::try_from(pq_sig)
        .map_err(|_| "invalid ML-DSA signature encoding".to_string())?;
    let pq_signature = MlDsaSignature::<MlDsa65>::decode(&pq_encoded_sig)
        .ok_or_else(|| "invalid ML-DSA signature payload".to_string())?;
    let pq_ok = pq_vk.verify_with_context(message, MLDSA65_CONTEXT, &pq_signature);

    Ok(ed_ok && pq_ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_from_byte(b: u8) -> [u8; 64] {
        [b; 64]
    }

    #[test]
    fn deterministic_derivation() {
        let seed = seed_from_byte(0x11);
        let a = did_from_genesis_seed(&seed).expect("identity a");
        let b = did_from_genesis_seed(&seed).expect("identity b");

        assert_eq!(a.did, b.did);
        assert_eq!(a.did_document, b.did_document);
        assert_eq!(
            a.ed25519_signing_key.to_bytes(),
            b.ed25519_signing_key.to_bytes()
        );
        assert_eq!(
            a.x25519_agreement_key.to_bytes(),
            b.x25519_agreement_key.to_bytes()
        );
        assert_eq!(
            a.mlkem768_decapsulation_key.as_bytes().as_slice(),
            b.mlkem768_decapsulation_key.as_bytes().as_slice()
        );
        assert_eq!(
            a.mldsa65_signing_key.verifying_key().encode().as_slice(),
            b.mldsa65_signing_key.verifying_key().encode().as_slice()
        );
    }

    #[test]
    fn different_seeds_different_dids() {
        let a = did_from_genesis_seed(&seed_from_byte(0x01)).expect("identity a");
        let b = did_from_genesis_seed(&seed_from_byte(0x02)).expect("identity b");
        assert_ne!(a.did, b.did);
    }

    #[test]
    fn did_format_valid() {
        let identity = did_from_genesis_seed(&seed_from_byte(0x22)).expect("identity");
        assert!(identity.did.starts_with("did:key:z6Mk"));
        assert!(identity.did.len() > "did:key:z6Mk".len());
    }

    #[test]
    fn did_document_structure() {
        let identity = did_from_genesis_seed(&seed_from_byte(0x33)).expect("identity");
        let doc = &identity.did_document;

        assert_eq!(doc.verification_method.len(), 2);
        assert_eq!(doc.key_agreement.len(), 2);
        assert_eq!(doc.authentication.len(), 2);
        assert_eq!(doc.assertion_method.len(), 2);

        assert!(doc
            .verification_method
            .iter()
            .any(|vm| vm.type_ == TYPE_ED25519 && vm.controller == identity.did));
        assert!(doc
            .verification_method
            .iter()
            .any(|vm| vm.type_ == TYPE_MLDSA65 && vm.controller == identity.did));
        assert!(doc
            .key_agreement
            .iter()
            .any(|ka| ka.type_ == TYPE_X25519 && ka.controller == identity.did));
        assert!(doc
            .key_agreement
            .iter()
            .any(|ka| ka.type_ == TYPE_MLKEM768 && ka.controller == identity.did));
    }

    #[test]
    fn dual_sign_verify_roundtrip() {
        let identity = did_from_genesis_seed(&seed_from_byte(0x44)).expect("identity");
        let message = b"agenthalo did roundtrip";
        let (ed_sig, pq_sig) = dual_sign(&identity, message).expect("dual sign should run");
        let verified = dual_verify(&identity.did_document, message, &ed_sig, &pq_sig)
            .expect("dual verify should run");
        assert!(verified);
    }

    #[test]
    fn dual_verify_rejects_wrong_message() {
        let identity = did_from_genesis_seed(&seed_from_byte(0x55)).expect("identity");
        let (ed_sig, pq_sig) = dual_sign(&identity, b"msg A").expect("dual sign should run");
        let verified = dual_verify(&identity.did_document, b"msg B", &ed_sig, &pq_sig)
            .expect("dual verify should run");
        assert!(!verified);
    }

    #[test]
    fn dual_verify_rejects_wrong_key() {
        let signer = did_from_genesis_seed(&seed_from_byte(0x66)).expect("signer");
        let verifier = did_from_genesis_seed(&seed_from_byte(0x67)).expect("verifier");
        let message = b"binding test";
        let (ed_sig, pq_sig) = dual_sign(&signer, message).expect("dual sign should run");
        let verified = dual_verify(&verifier.did_document, message, &ed_sig, &pq_sig)
            .expect("dual verify should run");
        assert!(!verified);
    }

    #[test]
    fn did_document_json_roundtrip() {
        let identity = did_from_genesis_seed(&seed_from_byte(0x77)).expect("identity");
        let value = did_document_to_json(&identity.did_document);
        assert_eq!(value["id"], serde_json::Value::String(identity.did.clone()));
        assert_eq!(
            value["verificationMethod"].as_array().map(|v| v.len()),
            Some(2)
        );
        assert_eq!(value["keyAgreement"].as_array().map(|v| v.len()), Some(2));

        let parsed: DIDDocument = serde_json::from_value(value).expect("parse did document json");
        assert_eq!(parsed, identity.did_document);
    }
}
