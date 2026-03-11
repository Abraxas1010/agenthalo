use ed25519_dalek::SigningKey as Ed25519SigningKey;
use hkdf::Hkdf;
use ml_dsa::{KeyGen, KeyPair as MlDsaKeyPair, MlDsa65};
use ml_kem::{EncodedSizeUser, KemCore};
use rand_chacha::ChaCha20Rng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};

const HKDF_IDENTITY_SALT: &[u8] = b"nucleusdb-genesis-identity-v1";
const HKDF_DID_PQ_SIGNING_INFO: &[u8] = b"nucleusdb-did-pq-signing-v1";
const MULTICODEC_ED25519_PUB: &[u8; 2] = &[0xed, 0x01];
const MULTICODEC_X25519_PUB: &[u8; 2] = &[0xec, 0x01];
const TYPE_ED25519: &str = "Ed25519VerificationKey2020";
const TYPE_MLDSA65: &str = "MlDsa65VerificationKey2025";
const TYPE_X25519: &str = "X25519KeyAgreementKey2020";
const TYPE_MLKEM768: &str = "MlKem768KeyAgreementKey2025";

type MlKem768DecapsulationKey = <ml_kem::MlKem768 as ml_kem::KemCore>::DecapsulationKey;

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

fn hkdf_expand<const N: usize>(seed: &[u8; 64], info: &[u8]) -> Result<[u8; N], String> {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_IDENTITY_SALT), seed.as_slice());
    let mut out = [0u8; N];
    hk.expand(info, &mut out)
        .map_err(|_| "hkdf expand failed".to_string())?;
    Ok(out)
}

fn encode_multibase_key(prefix: &[u8], public_key: &[u8]) -> String {
    let mut payload = Vec::with_capacity(prefix.len() + public_key.len());
    payload.extend_from_slice(prefix);
    payload.extend_from_slice(public_key);
    multibase::encode(multibase::Base::Base58Btc, payload)
}

fn encode_multibase_untyped_key(public_key: &[u8]) -> String {
    encode_multibase_key(&[], public_key)
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
    let ed_key_id = did_fragment(did, "key-ed25519-1");
    let pq_key_id = did_fragment(did, "key-mldsa65-1");
    let x25519_key_id = did_fragment(did, "key-x25519-1");
    let mlkem_key_id = did_fragment(did, "key-mlkem768-1");
    DIDDocument {
        id: did.to_string(),
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

pub fn did_uri_from_genesis_seed(seed: &[u8; 64]) -> String {
    let ed25519_seed = crate::genesis::derive_p2p_identity(seed);
    let signing_key = Ed25519SigningKey::from_bytes(&ed25519_seed);
    let public_key = signing_key.verifying_key().to_bytes();
    did_from_ed25519_public_key(&public_key)
}

pub fn did_from_genesis_seed(seed: &[u8; 64]) -> Result<DIDIdentity, String> {
    let ed25519_seed = crate::genesis::derive_p2p_identity(seed);
    let ed25519_signing_key = Ed25519SigningKey::from_bytes(&ed25519_seed);
    let ed25519_public_key = ed25519_signing_key.verifying_key().to_bytes();

    let mldsa65_seed_bytes = hkdf_expand::<32>(seed, HKDF_DID_PQ_SIGNING_INFO)?;
    let mldsa65_seed = ml_dsa::Seed::try_from(mldsa65_seed_bytes.as_slice())
        .map_err(|_| "failed to build ML-DSA-65 seed".to_string())?;
    let mldsa65_keypair = MlDsa65::from_seed(&mldsa65_seed);

    let (x25519_secret_bytes, mlkem_seed_bytes) = crate::genesis::derive_did_agreement_keys(seed);
    let x25519_agreement_key = X25519StaticSecret::from(x25519_secret_bytes);
    let mut chacha_seed = [0u8; 32];
    chacha_seed.copy_from_slice(&mlkem_seed_bytes[..32]);
    let mut mlkem_rng = ChaCha20Rng::from_seed(chacha_seed);
    let (mlkem768_decapsulation_key, _) = ml_kem::MlKem768::generate(&mut mlkem_rng);

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
