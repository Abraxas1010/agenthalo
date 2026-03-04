use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::hybrid_kem;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use hkdf::Hkdf;
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::Zeroize;

const HKDF_AUTHCRYPT_INFO: &[u8] = b"agenthalo-didcomm-authcrypt-v1";
const HKDF_ANONCRYPT_INFO: &[u8] = b"agenthalo-didcomm-anoncrypt-v1";
const X25519_PREFIX: &[u8] = &[0xec, 0x01];
const TYPE_MLKEM768: &str = "MlKem768KeyAgreementKey2025";

pub mod message_types {
    pub const AGENT_CARD_REQUEST: &str = "https://agenthalo.dev/didcomm/agent-card-request/1.0";
    pub const AGENT_CARD_RESPONSE: &str = "https://agenthalo.dev/didcomm/agent-card-response/1.0";
    pub const TASK_SEND: &str = "https://agenthalo.dev/didcomm/task-send/1.0";
    pub const TASK_STATUS: &str = "https://agenthalo.dev/didcomm/task-status/1.0";
    pub const TASK_ARTIFACT: &str = "https://agenthalo.dev/didcomm/task-artifact/1.0";
    pub const TASK_CANCEL: &str = "https://agenthalo.dev/didcomm/task-cancel/1.0";
    pub const PING: &str = "https://agenthalo.dev/didcomm/ping/1.0";
    pub const ACK: &str = "https://agenthalo.dev/didcomm/ack/1.0";
    pub const ERROR: &str = "https://agenthalo.dev/didcomm/error/1.0";
    pub const CREDENTIAL_OFFER: &str = "https://agenthalo.dev/didcomm/credential-offer/1.0";
    pub const CREDENTIAL_REQUEST: &str = "https://agenthalo.dev/didcomm/credential-request/1.0";
    pub const CREDENTIAL_ISSUE: &str = "https://agenthalo.dev/didcomm/credential-issue/1.0";
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DIDCommMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    pub to: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_time: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_time: Option<u64>,
    pub body: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<DIDCommAttachment>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DIDCommAttachment {
    pub id: String,
    pub media_type: String,
    pub data: AttachmentData,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AttachmentData {
    Json { json: serde_json::Value },
    Base64 { base64: String },
    Links { links: Vec<String> },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AuthcryptProtected {
    alg: String,
    enc: String,
    sender_did: String,
    sender_x25519_public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sender_evm_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "sender_binding_proof_sha256")]
    sender_binding_proof_hash: Option<String>,
    /// Post-quantum KEM algorithm identifier (e.g. "ML-KEM-768").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pq_kem: Option<String>,
    /// ML-KEM-768 ciphertext, base64-encoded. Present when hybrid KEM is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pq_ct: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AuthcryptEnvelope {
    kind: String,
    protected: AuthcryptProtected,
    nonce: String,
    ciphertext: String,
    ed25519_signature: String,
    mldsa65_signature: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AnoncryptProtected {
    alg: String,
    enc: String,
    epk: String,
    /// Post-quantum KEM algorithm identifier (e.g. "ML-KEM-768").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pq_kem: Option<String>,
    /// ML-KEM-768 ciphertext, base64-encoded. Present when hybrid KEM is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pq_ct: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AnoncryptEnvelope {
    kind: String,
    protected: AnoncryptProtected,
    nonce: String,
    ciphertext: String,
}

impl DIDCommMessage {
    pub fn new(type_: &str, from: Option<&str>, to: Vec<String>, body: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            type_: type_.to_string(),
            from: from.map(|value| value.to_string()),
            to,
            created_time: Some(now_unix_secs()),
            expires_time: None,
            body,
            attachments: None,
        }
    }

    pub fn with_expiry(mut self, ttl_secs: u64) -> Self {
        self.expires_time = Some(now_unix_secs().saturating_add(ttl_secs));
        self
    }

    pub fn with_attachment(mut self, attachment: DIDCommAttachment) -> Self {
        self.attachments
            .get_or_insert_with(Vec::new)
            .push(attachment);
        self
    }

    pub fn is_expired(&self) -> bool {
        self.expires_time
            .map(|expires| now_unix_secs() > expires)
            .unwrap_or(false)
    }
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

// T21: rust_authcrypt_refines_protocol (lean/NucleusDB/Security/DIDCommRefinement.lean)
pub(crate) fn authcrypt_gate_accepts(
    ed25519_sig_valid: bool,
    mldsa65_sig_valid: bool,
    decrypt_ok: bool,
    is_expired: bool,
) -> bool {
    ed25519_sig_valid && mldsa65_sig_valid && decrypt_ok && !is_expired
}

// T22: rust_anoncrypt_refines_protocol (lean/NucleusDB/Security/DIDCommRefinement.lean)
pub(crate) fn anoncrypt_gate_accepts(decrypt_ok: bool, is_expired: bool) -> bool {
    decrypt_ok && !is_expired
}

fn random_nonce() -> Result<[u8; 12], String> {
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce)
        .map_err(|e| format!("OS entropy unavailable for DIDComm nonce: {e}"))?;
    Ok(nonce)
}

fn derive_key(shared_secret: &[u8], info: &[u8]) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(None, shared_secret);
    let mut out = [0u8; 32];
    hk.expand(info, &mut out)
        .map_err(|_| "HKDF expand failed for DIDComm key derivation".to_string())?;
    Ok(out)
}

fn encrypt_with_key(key: &[u8; 32], plaintext: &[u8], nonce: &[u8; 12]) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init failed: {e}"))?;
    cipher
        .encrypt(Nonce::from_slice(nonce), plaintext)
        .map_err(|e| format!("DIDComm encrypt failed: {e}"))
}

fn decrypt_with_key(
    key: &[u8; 32],
    ciphertext: &[u8],
    nonce: &[u8; 12],
) -> Result<Vec<u8>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init failed: {e}"))?;
    cipher
        .decrypt(Nonce::from_slice(nonce), ciphertext)
        .map_err(|e| format!("DIDComm decrypt failed: {e}"))
}

#[allow(dead_code)]
fn body_contains_envelope_kind(body: &serde_json::Value) -> bool {
    let direct_kind = body
        .get("kind")
        .and_then(|value| value.as_str())
        .map(|kind| {
            kind == "authcrypt" || kind == "anoncrypt" || kind.contains('(') || kind.contains(')')
        })
        .unwrap_or(false);
    if direct_kind {
        return true;
    }
    serde_json::to_string(body)
        .map(|serialized| {
            serialized.contains("\"kind\":\"authcrypt\"")
                || serialized.contains("\"kind\":\"anoncrypt\"")
                || serialized.contains("anoncrypt(authcrypt)")
                || serialized.contains("authcrypt(anoncrypt)")
        })
        .unwrap_or(false)
}

pub fn extract_x25519_public_key_from_doc(doc: &DIDDocument) -> Result<[u8; 32], String> {
    let method = doc
        .key_agreement
        .iter()
        .find(|candidate| candidate.type_ == "X25519KeyAgreementKey2020")
        .ok_or_else(|| "DID document missing X25519 key agreement method".to_string())?;
    let (_, decoded) = multibase::decode(&method.public_key_multibase)
        .map_err(|e| format!("decode X25519 key from DID document: {e}"))?;
    if decoded.len() < X25519_PREFIX.len() || !decoded.starts_with(X25519_PREFIX) {
        return Err("DID document X25519 key has unexpected multicodec prefix".to_string());
    }
    let raw = &decoded[X25519_PREFIX.len()..];
    raw.try_into()
        .map_err(|_| "DID document X25519 key must be 32 bytes".to_string())
}

/// Extract the ML-KEM-768 encapsulation key from a DID document's keyAgreement section.
/// Returns None if the document has no ML-KEM-768 key (classical-only peer).
pub fn extract_mlkem_encapsulation_key_from_doc(
    doc: &DIDDocument,
) -> Option<hybrid_kem::MlKem768EncapsulationKey> {
    let method = doc
        .key_agreement
        .iter()
        .find(|candidate| candidate.type_ == TYPE_MLKEM768)?;
    // ML-KEM keys are stored without multicodec prefix (untyped encoding).
    let (_, decoded) = multibase::decode(&method.public_key_multibase).ok()?;
    hybrid_kem::mlkem768_ek_from_bytes(&decoded).ok()
}

/// Sender enrichment for sovereign identity binding.
#[derive(Clone, Debug, Default)]
pub struct SenderEnrichment {
    pub evm_address: Option<String>,
    pub binding_proof_hash: Option<String>,
}

pub fn pack_authcrypt(
    message: &DIDCommMessage,
    sender: &DIDIdentity,
    recipient_x25519_public_key: &[u8; 32],
) -> Result<Vec<u8>, String> {
    pack_authcrypt_inner(
        message,
        sender,
        recipient_x25519_public_key,
        None,
        None,
    )
}

/// Pack authcrypt with hybrid KEM when the recipient's DID document contains
/// an ML-KEM-768 key. Falls back to classical X25519 if no PQ key is present.
pub fn pack_authcrypt_hybrid(
    message: &DIDCommMessage,
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    enrichment: Option<&SenderEnrichment>,
) -> Result<Vec<u8>, String> {
    let recipient_x25519 = extract_x25519_public_key_from_doc(recipient_doc)?;
    let recipient_mlkem = extract_mlkem_encapsulation_key_from_doc(recipient_doc);
    pack_authcrypt_inner(
        message,
        sender,
        &recipient_x25519,
        recipient_mlkem.as_ref(),
        enrichment,
    )
}

pub fn pack_authcrypt_enriched(
    message: &DIDCommMessage,
    sender: &DIDIdentity,
    recipient_x25519_public_key: &[u8; 32],
    enrichment: Option<&SenderEnrichment>,
) -> Result<Vec<u8>, String> {
    pack_authcrypt_inner(
        message,
        sender,
        recipient_x25519_public_key,
        None,
        enrichment,
    )
}

fn pack_authcrypt_inner(
    message: &DIDCommMessage,
    sender: &DIDIdentity,
    recipient_x25519_public_key: &[u8; 32],
    recipient_mlkem_ek: Option<&hybrid_kem::MlKem768EncapsulationKey>,
    enrichment: Option<&SenderEnrichment>,
) -> Result<Vec<u8>, String> {
    let plaintext =
        serde_json::to_vec(message).map_err(|e| format!("serialize DIDComm message: {e}"))?;

    // Two paths: hybrid (ephemeral X25519 + ML-KEM-768) or classical (static X25519).
    // When pq_kem is absent, the recipient expects static-static ECDH for backward
    // compatibility. When pq_kem is present, the envelope carries an ephemeral X25519
    // key + ML-KEM ciphertext, and the recipient uses hybrid_decap.
    let (mut key, sender_x25519_pk_bytes, pq_kem, pq_ct) = if let Some(ek) = recipient_mlkem_ek {
        let recipient_x25519_pk = X25519PublicKey::from(*recipient_x25519_public_key);
        let encap = hybrid_kem::hybrid_encap(&recipient_x25519_pk, Some(ek), HKDF_AUTHCRYPT_INFO)
            .map_err(|e| format!("hybrid KEM encap failed: {e}"))?;
        let ct = encap
            .mlkem_ciphertext
            .as_ref()
            .expect("ML-KEM ciphertext must be present when ek is Some");
        (
            encap.shared_secret,
            encap.x25519_ephemeral_pk,
            Some("ML-KEM-768".to_string()),
            Some(B64.encode(ct)),
        )
    } else {
        // Classical path: static-static ECDH (backward compatible with old protocol).
        let shared_secret = sender
            .x25519_agreement_key
            .diffie_hellman(&X25519PublicKey::from(*recipient_x25519_public_key));
        let mut key = derive_key(shared_secret.as_bytes(), HKDF_AUTHCRYPT_INFO)?;
        let sender_pk = X25519PublicKey::from(&sender.x25519_agreement_key);
        let result = (key, sender_pk.to_bytes(), None, None);
        key.zeroize();
        result
    };

    let nonce = random_nonce()?;
    let ciphertext = encrypt_with_key(&key, &plaintext, &nonce)?;
    let (ed_sig, pq_sig) = dual_sign(sender, &ciphertext)?;

    let envelope = AuthcryptEnvelope {
        kind: "authcrypt".to_string(),
        protected: AuthcryptProtected {
            alg: "ECDH-ES+A256KW".to_string(),
            enc: "A256GCM".to_string(),
            sender_did: sender.did.clone(),
            sender_x25519_public_key: B64.encode(sender_x25519_pk_bytes),
            sender_evm_address: enrichment.and_then(|e| e.evm_address.clone()),
            sender_binding_proof_hash: enrichment.and_then(|e| e.binding_proof_hash.clone()),
            pq_kem,
            pq_ct,
        },
        nonce: B64.encode(nonce),
        ciphertext: B64.encode(&ciphertext),
        ed25519_signature: B64.encode(ed_sig),
        mldsa65_signature: B64.encode(pq_sig),
    };

    key.zeroize();
    serde_json::to_vec(&envelope).map_err(|e| format!("serialize authcrypt envelope: {e}"))
}

/// Pack a DIDComm message with sender-anonymous encryption.
///
/// # Composition restriction
/// This function MUST NOT be used to wrap the output of `pack_authcrypt`
/// or `pack_authcrypt_enriched`. The combined anoncrypt(authcrypt()) mode
/// requires AES-CBC-HMAC for key-commitment safety (IOG eprint 2024/1361).
/// AgentHALO achieves authenticated anonymity via authcrypt + Nym transport,
/// not via envelope composition.
///
/// See: `lean/NucleusDB/Comms/Protocol/COMPOSITION_POLICY.md`
///
/// ```compile_fail
/// use nucleusdb::halo::didcomm::pack_anoncrypt;
/// # fn main() {}
/// ```
#[allow(dead_code)]
pub(crate) fn pack_anoncrypt(
    message: &DIDCommMessage,
    recipient_x25519_public_key: &[u8; 32],
) -> Result<Vec<u8>, String> {
    pack_anoncrypt_inner(message, recipient_x25519_public_key, None)
}

/// Pack anoncrypt with hybrid KEM when the recipient's DID document contains
/// an ML-KEM-768 key. Falls back to classical ephemeral X25519 if no PQ key.
#[allow(dead_code)]
pub(crate) fn pack_anoncrypt_hybrid(
    message: &DIDCommMessage,
    recipient_doc: &DIDDocument,
) -> Result<Vec<u8>, String> {
    let recipient_x25519 = extract_x25519_public_key_from_doc(recipient_doc)?;
    let recipient_mlkem = extract_mlkem_encapsulation_key_from_doc(recipient_doc);
    pack_anoncrypt_inner(message, &recipient_x25519, recipient_mlkem.as_ref())
}

fn pack_anoncrypt_inner(
    message: &DIDCommMessage,
    recipient_x25519_public_key: &[u8; 32],
    recipient_mlkem_ek: Option<&hybrid_kem::MlKem768EncapsulationKey>,
) -> Result<Vec<u8>, String> {
    let nested = body_contains_envelope_kind(&message.body);
    debug_assert!(
        !nested,
        "BUG: attempting anoncrypt composition over nested envelope body — violates composition policy"
    );
    if nested {
        return Err(
            "DIDComm anoncrypt composition over nested envelope body is not supported".to_string(),
        );
    }

    let plaintext =
        serde_json::to_vec(message).map_err(|e| format!("serialize DIDComm message: {e}"))?;

    // Two paths: hybrid (ephemeral X25519 + ML-KEM-768) or classical (ephemeral X25519 only).
    // Classical path uses old HKDF construction (salt=None) for backward compatibility
    // with recipients running pre-PQ code.
    let (mut key, epk_bytes, pq_kem, pq_ct) = if let Some(ek) = recipient_mlkem_ek {
        let recipient_pk = X25519PublicKey::from(*recipient_x25519_public_key);
        let encap =
            hybrid_kem::hybrid_encap(&recipient_pk, Some(ek), HKDF_ANONCRYPT_INFO)
                .map_err(|e| format!("hybrid KEM encap failed: {e}"))?;
        let ct = encap
            .mlkem_ciphertext
            .as_ref()
            .expect("ML-KEM ciphertext must be present when ek is Some");
        (
            encap.shared_secret,
            encap.x25519_ephemeral_pk,
            Some("ML-KEM-768".to_string()),
            Some(B64.encode(ct)),
        )
    } else {
        // Classical path: ephemeral X25519 with old HKDF (backward compatible).
        let ephemeral_secret = X25519StaticSecret::random_from_rng(OsRng);
        let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);
        let shared_secret = ephemeral_secret
            .diffie_hellman(&X25519PublicKey::from(*recipient_x25519_public_key));
        let key = derive_key(shared_secret.as_bytes(), HKDF_ANONCRYPT_INFO)?;
        (key, ephemeral_public.to_bytes(), None, None)
    };

    let nonce = random_nonce()?;
    let ciphertext = encrypt_with_key(&key, &plaintext, &nonce)?;

    let envelope = AnoncryptEnvelope {
        kind: "anoncrypt".to_string(),
        protected: AnoncryptProtected {
            alg: "ECDH-ES+A256KW".to_string(),
            enc: "A256GCM".to_string(),
            epk: B64.encode(epk_bytes),
            pq_kem,
            pq_ct,
        },
        nonce: B64.encode(nonce),
        ciphertext: B64.encode(&ciphertext),
    };

    key.zeroize();
    serde_json::to_vec(&envelope).map_err(|e| format!("serialize anoncrypt envelope: {e}"))
}

pub fn unpack_with_resolver<F>(
    packed: &[u8],
    recipient: &DIDIdentity,
    resolve_document: F,
) -> Result<(DIDCommMessage, Option<String>), String>
where
    F: Fn(&str) -> Option<DIDDocument>,
{
    let value: serde_json::Value =
        serde_json::from_slice(packed).map_err(|e| format!("parse DIDComm envelope: {e}"))?;
    let kind = value
        .get("kind")
        .and_then(|k| k.as_str())
        .ok_or_else(|| "DIDComm envelope missing `kind`".to_string())?;
    // Composition policy: anoncrypt(authcrypt(...)) and authcrypt(anoncrypt(...))
    // are not supported in this runtime profile.
    if kind.contains('(') || kind.contains(')') {
        return Err("DIDComm nested authcrypt/anoncrypt composition is not supported".to_string());
    }

    match kind {
        "authcrypt" => {
            let envelope: AuthcryptEnvelope = serde_json::from_value(value)
                .map_err(|e| format!("parse authcrypt envelope: {e}"))?;
            let sender_did = envelope.protected.sender_did.clone();
            let sender_doc = resolve_document(&sender_did).ok_or_else(|| {
                format!("unable to resolve DID document for sender `{sender_did}`")
            })?;

            let sender_x25519_public_key_bytes = B64
                .decode(envelope.protected.sender_x25519_public_key)
                .map_err(|e| format!("decode sender X25519 public key: {e}"))?;
            let sender_x25519_public_key: [u8; 32] = sender_x25519_public_key_bytes
                .as_slice()
                .try_into()
                .map_err(|_| "sender X25519 key must be 32 bytes".to_string())?;

            let ciphertext = B64
                .decode(envelope.ciphertext)
                .map_err(|e| format!("decode authcrypt ciphertext: {e}"))?;
            let ed_sig = B64
                .decode(envelope.ed25519_signature)
                .map_err(|e| format!("decode authcrypt Ed25519 signature: {e}"))?;
            let pq_sig = B64
                .decode(envelope.mldsa65_signature)
                .map_err(|e| format!("decode authcrypt ML-DSA signature: {e}"))?;

            let signatures_ok = dual_verify(&sender_doc, &ciphertext, &ed_sig, &pq_sig)?;
            if !signatures_ok {
                return Err("DIDComm authcrypt signatures failed verification".to_string());
            }

            let nonce_bytes = B64
                .decode(envelope.nonce)
                .map_err(|e| format!("decode authcrypt nonce: {e}"))?;
            let nonce: [u8; 12] = nonce_bytes
                .as_slice()
                .try_into()
                .map_err(|_| "authcrypt nonce must be 12 bytes".to_string())?;

            // Derive decryption key: hybrid (ephemeral + ML-KEM) or classical (static ECDH).
            let mut key = if envelope.protected.pq_kem.is_some() {
                let pq_ct_b64 = envelope
                    .protected
                    .pq_ct
                    .as_deref()
                    .ok_or("pq_kem present but pq_ct missing")?;
                let pq_ct = B64
                    .decode(pq_ct_b64)
                    .map_err(|e| format!("decode ML-KEM ciphertext: {e}"))?;
                let decap = hybrid_kem::hybrid_decap(
                    &sender_x25519_public_key,
                    Some(&pq_ct),
                    &recipient.x25519_agreement_key,
                    Some(&recipient.mlkem768_decapsulation_key),
                    HKDF_AUTHCRYPT_INFO,
                )
                .map_err(|e| format!("hybrid KEM decap failed: {e}"))?;
                decap.shared_secret
            } else {
                // Classical path: static-static ECDH (backward compatible).
                let shared_secret = recipient
                    .x25519_agreement_key
                    .diffie_hellman(&X25519PublicKey::from(sender_x25519_public_key));
                derive_key(shared_secret.as_bytes(), HKDF_AUTHCRYPT_INFO)?
            };

            let plaintext = decrypt_with_key(&key, &ciphertext, &nonce)?;
            key.zeroize();

            let message: DIDCommMessage = serde_json::from_slice(&plaintext)
                .map_err(|e| format!("decode authcrypt plaintext message: {e}"))?;
            if !authcrypt_gate_accepts(true, true, true, message.is_expired()) {
                return Err("DIDComm message expired".to_string());
            }
            Ok((message, Some(sender_did)))
        }
        "anoncrypt" => {
            let envelope: AnoncryptEnvelope = serde_json::from_value(value)
                .map_err(|e| format!("parse anoncrypt envelope: {e}"))?;
            let epk_bytes = B64
                .decode(envelope.protected.epk)
                .map_err(|e| format!("decode anoncrypt ephemeral key: {e}"))?;
            let epk: [u8; 32] = epk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| "anoncrypt ephemeral key must be 32 bytes".to_string())?;
            let nonce_bytes = B64
                .decode(envelope.nonce)
                .map_err(|e| format!("decode anoncrypt nonce: {e}"))?;
            let nonce: [u8; 12] = nonce_bytes
                .as_slice()
                .try_into()
                .map_err(|_| "anoncrypt nonce must be 12 bytes".to_string())?;
            let ciphertext = B64
                .decode(envelope.ciphertext)
                .map_err(|e| format!("decode anoncrypt ciphertext: {e}"))?;

            // Derive decryption key: hybrid or classical.
            let mut key = if envelope.protected.pq_kem.is_some() {
                let pq_ct_b64 = envelope
                    .protected
                    .pq_ct
                    .as_deref()
                    .ok_or("pq_kem present but pq_ct missing")?;
                let pq_ct = B64
                    .decode(pq_ct_b64)
                    .map_err(|e| format!("decode ML-KEM ciphertext: {e}"))?;
                let decap = hybrid_kem::hybrid_decap(
                    &epk,
                    Some(&pq_ct),
                    &recipient.x25519_agreement_key,
                    Some(&recipient.mlkem768_decapsulation_key),
                    HKDF_ANONCRYPT_INFO,
                )
                .map_err(|e| format!("hybrid KEM decap failed: {e}"))?;
                decap.shared_secret
            } else {
                // Classical path: ephemeral ECDH with old HKDF (backward compatible).
                let shared_secret = recipient
                    .x25519_agreement_key
                    .diffie_hellman(&X25519PublicKey::from(epk));
                derive_key(shared_secret.as_bytes(), HKDF_ANONCRYPT_INFO)?
            };

            let plaintext = decrypt_with_key(&key, &ciphertext, &nonce)?;
            key.zeroize();

            let message: DIDCommMessage = serde_json::from_slice(&plaintext)
                .map_err(|e| format!("decode anoncrypt plaintext message: {e}"))?;
            if !anoncrypt_gate_accepts(true, message.is_expired()) {
                return Err("DIDComm message expired".to_string());
            }
            Ok((message, None))
        }
        other => Err(format!("unsupported DIDComm envelope kind `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    #[test]
    fn authcrypt_roundtrip() {
        let sender = crate::halo::did::did_from_genesis_seed(&seed(0x01)).expect("sender");
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x02)).expect("recipient");
        let recipient_key =
            extract_x25519_public_key_from_doc(&recipient.did_document).expect("recipient key");

        let message = DIDCommMessage::new(
            message_types::PING,
            Some(&sender.did),
            vec![recipient.did.clone()],
            serde_json::json!({ "msg": "hello" }),
        );

        let packed = pack_authcrypt(&message, &sender, &recipient_key).expect("pack authcrypt");
        let (decoded, resolved_sender) = unpack_with_resolver(&packed, &recipient, |did| {
            if did == sender.did {
                Some(sender.did_document.clone())
            } else {
                None
            }
        })
        .expect("unpack authcrypt");

        assert_eq!(resolved_sender.as_deref(), Some(sender.did.as_str()));
        assert_eq!(decoded.type_, message_types::PING);
        assert_eq!(decoded.body["msg"], "hello");
    }

    #[test]
    fn anoncrypt_roundtrip() {
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x03)).expect("recipient");
        let recipient_key =
            extract_x25519_public_key_from_doc(&recipient.did_document).expect("recipient key");

        let message = DIDCommMessage::new(
            message_types::TASK_SEND,
            None,
            vec![recipient.did.clone()],
            serde_json::json!({ "task": "compute" }),
        );

        let packed = pack_anoncrypt(&message, &recipient_key).expect("pack anoncrypt");
        let (decoded, sender) =
            unpack_with_resolver(&packed, &recipient, |_| None).expect("unpack anoncrypt");

        assert!(sender.is_none());
        assert_eq!(decoded.type_, message_types::TASK_SEND);
        assert_eq!(decoded.body["task"], "compute");
    }

    #[test]
    fn authcrypt_gate_truth_table_matches_spec() {
        for ed_ok in [false, true] {
            for pq_ok in [false, true] {
                for decrypt_ok in [false, true] {
                    for expired in [false, true] {
                        let got = authcrypt_gate_accepts(ed_ok, pq_ok, decrypt_ok, expired);
                        let expected = ed_ok && pq_ok && decrypt_ok && !expired;
                        assert_eq!(
                            got, expected,
                            "mismatch for ed={ed_ok} pq={pq_ok} decrypt={decrypt_ok} expired={expired}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn anoncrypt_gate_truth_table_matches_spec() {
        for decrypt_ok in [false, true] {
            for expired in [false, true] {
                let got = anoncrypt_gate_accepts(decrypt_ok, expired);
                let expected = decrypt_ok && !expired;
                assert_eq!(
                    got, expected,
                    "mismatch for decrypt={decrypt_ok} expired={expired}"
                );
            }
        }
    }

    #[test]
    fn nested_composition_kind_rejected() {
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x04)).expect("recipient");
        let packed = serde_json::json!({
            "kind": "anoncrypt(authcrypt)",
            "ciphertext": "",
        });
        let err = unpack_with_resolver(
            &serde_json::to_vec(&packed).expect("serialize"),
            &recipient,
            |_| None,
        )
        .expect_err("nested composition must reject");
        assert!(err.contains("nested authcrypt/anoncrypt composition is not supported"));
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn anoncrypt_body_level_composition_rejected() {
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x05)).expect("recipient");
        let recipient_key =
            extract_x25519_public_key_from_doc(&recipient.did_document).expect("recipient key");
        let message = DIDCommMessage::new(
            message_types::TASK_SEND,
            None,
            vec![recipient.did.clone()],
            serde_json::json!({
                "kind": "authcrypt",
                "ciphertext": "deadbeef"
            }),
        );
        let err = pack_anoncrypt(&message, &recipient_key)
            .expect_err("body-level authcrypt composition must reject");
        assert!(err.contains("composition"));
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "BUG: attempting anoncrypt composition")]
    fn anoncrypt_debug_assert_fires_on_nested_envelope_body() {
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x06)).expect("recipient");
        let recipient_key =
            extract_x25519_public_key_from_doc(&recipient.did_document).expect("recipient key");
        let message = DIDCommMessage::new(
            message_types::TASK_SEND,
            None,
            vec![recipient.did.clone()],
            serde_json::json!({
                "kind": "authcrypt",
                "ciphertext": "deadbeef"
            }),
        );

        let _ = pack_anoncrypt(&message, &recipient_key);
    }

    // --- Hybrid KEM integration tests ---

    #[test]
    fn authcrypt_hybrid_roundtrip() {
        let sender = crate::halo::did::did_from_genesis_seed(&seed(0x10)).expect("sender");
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x11)).expect("recipient");

        let message = DIDCommMessage::new(
            message_types::PING,
            Some(&sender.did),
            vec![recipient.did.clone()],
            serde_json::json!({ "hybrid": true }),
        );

        let packed = pack_authcrypt_hybrid(&message, &sender, &recipient.did_document, None)
            .expect("pack hybrid authcrypt");
        let (decoded, resolved_sender) = unpack_with_resolver(&packed, &recipient, |did| {
            if did == sender.did {
                Some(sender.did_document.clone())
            } else {
                None
            }
        })
        .expect("unpack hybrid authcrypt");

        assert_eq!(resolved_sender.as_deref(), Some(sender.did.as_str()));
        assert_eq!(decoded.type_, message_types::PING);
        assert_eq!(decoded.body["hybrid"], true);
    }

    #[test]
    fn authcrypt_hybrid_envelope_carries_pq_fields() {
        let sender = crate::halo::did::did_from_genesis_seed(&seed(0x12)).expect("sender");
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x13)).expect("recipient");

        let message = DIDCommMessage::new(
            message_types::PING,
            Some(&sender.did),
            vec![recipient.did.clone()],
            serde_json::json!({}),
        );

        let packed = pack_authcrypt_hybrid(&message, &sender, &recipient.did_document, None)
            .expect("pack");
        let envelope: serde_json::Value =
            serde_json::from_slice(&packed).expect("parse envelope");

        assert_eq!(
            envelope["protected"]["pq_kem"].as_str(),
            Some("ML-KEM-768")
        );
        assert!(envelope["protected"]["pq_ct"].as_str().is_some());
        // ML-KEM-768 ciphertext is 1088 bytes → ~1451 base64 chars
        let pq_ct = envelope["protected"]["pq_ct"].as_str().unwrap();
        assert!(pq_ct.len() > 1000, "pq_ct should be base64 of 1088 bytes");
    }

    #[test]
    fn authcrypt_classical_has_no_pq_fields() {
        let sender = crate::halo::did::did_from_genesis_seed(&seed(0x14)).expect("sender");
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x15)).expect("recipient");
        let recipient_key =
            extract_x25519_public_key_from_doc(&recipient.did_document).expect("key");

        let message = DIDCommMessage::new(
            message_types::PING,
            Some(&sender.did),
            vec![recipient.did.clone()],
            serde_json::json!({}),
        );

        let packed = pack_authcrypt(&message, &sender, &recipient_key).expect("pack");
        let envelope: serde_json::Value =
            serde_json::from_slice(&packed).expect("parse envelope");

        assert!(envelope["protected"]["pq_kem"].is_null());
        assert!(envelope["protected"]["pq_ct"].is_null());
    }

    #[test]
    fn authcrypt_hybrid_tampered_mlkem_ct_rejected() {
        let sender = crate::halo::did::did_from_genesis_seed(&seed(0x16)).expect("sender");
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x17)).expect("recipient");

        let message = DIDCommMessage::new(
            message_types::PING,
            Some(&sender.did),
            vec![recipient.did.clone()],
            serde_json::json!({}),
        );

        let packed = pack_authcrypt_hybrid(&message, &sender, &recipient.did_document, None)
            .expect("pack");
        let mut envelope: serde_json::Value =
            serde_json::from_slice(&packed).expect("parse envelope");

        // Tamper with the ML-KEM ciphertext
        let pq_ct = envelope["protected"]["pq_ct"].as_str().unwrap().to_string();
        let mut ct_bytes = B64.decode(&pq_ct).unwrap();
        ct_bytes[0] ^= 0xFF;
        envelope["protected"]["pq_ct"] = serde_json::Value::String(B64.encode(&ct_bytes));

        let tampered = serde_json::to_vec(&envelope).expect("reserialize");
        let result = unpack_with_resolver(&tampered, &recipient, |did| {
            if did == sender.did {
                Some(sender.did_document.clone())
            } else {
                None
            }
        });

        // Tampered ciphertext should cause decryption failure (wrong key)
        assert!(result.is_err(), "tampered pq_ct must cause unpack failure");
    }

    #[test]
    fn anoncrypt_hybrid_roundtrip() {
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x18)).expect("recipient");

        let message = DIDCommMessage::new(
            message_types::TASK_SEND,
            None,
            vec![recipient.did.clone()],
            serde_json::json!({ "hybrid_anon": true }),
        );

        let packed = pack_anoncrypt_hybrid(&message, &recipient.did_document)
            .expect("pack hybrid anoncrypt");
        let (decoded, sender) =
            unpack_with_resolver(&packed, &recipient, |_| None).expect("unpack");

        assert!(sender.is_none());
        assert_eq!(decoded.type_, message_types::TASK_SEND);
        assert_eq!(decoded.body["hybrid_anon"], true);
    }

    #[test]
    fn anoncrypt_hybrid_envelope_carries_pq_fields() {
        let recipient = crate::halo::did::did_from_genesis_seed(&seed(0x19)).expect("recipient");

        let message = DIDCommMessage::new(
            message_types::TASK_SEND,
            None,
            vec![recipient.did.clone()],
            serde_json::json!({}),
        );

        let packed = pack_anoncrypt_hybrid(&message, &recipient.did_document).expect("pack");
        let envelope: serde_json::Value =
            serde_json::from_slice(&packed).expect("parse envelope");

        assert_eq!(
            envelope["protected"]["pq_kem"].as_str(),
            Some("ML-KEM-768")
        );
        assert!(envelope["protected"]["pq_ct"].as_str().is_some());
    }

    #[test]
    fn extract_mlkem_key_from_did_document() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x20)).expect("identity");
        let ek = extract_mlkem_encapsulation_key_from_doc(&identity.did_document);
        assert!(ek.is_some(), "DID document should contain ML-KEM-768 key");
    }
}
