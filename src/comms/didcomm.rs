//! DIDComm v2 envelope construction and verification for mesh communication.
//!
//! Implements the DIDComm Messaging v2 encrypted envelope format using
//! existing crypto primitives (X25519 ECDH-ES + AES-256-GCM for classical,
//! ML-KEM-768 + AES-256-GCM for post-quantum forward secrecy).
//!
//! All crates used here are already in Cargo.toml — no new dependencies.

use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::didcomm::{extract_mlkem_encapsulation_key_from_doc, extract_x25519_public_key_from_doc};
use crate::halo::hybrid_kem;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64URL, Engine as _};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::Zeroize;

const HKDF_MESH_DIDCOMM_INFO: &[u8] = b"agenthalo-mesh-didcomm-v2";
const CONTENT_TYPE: &str = "application/didcomm-encrypted+json";

/// DIDComm v2 message types for AgentHALO mesh communication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// MCP tool call request.
    McpToolCall,
    /// MCP tool call response.
    McpToolResponse,
    /// ProofEnvelope exchange.
    EnvelopeExchange,
    /// Capability token grant.
    CapabilityGrant,
    /// Capability token acceptance.
    CapabilityAccept,
    /// Peer discovery announcement.
    PeerAnnounce,
    /// Heartbeat / keepalive.
    Heartbeat,
}

/// Plaintext DIDComm message (before encryption).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DIDCommMessage {
    pub id: String,
    #[serde(rename = "type")]
    pub type_: MessageType,
    pub from: String,
    pub to: Vec<String>,
    pub created_time: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_time: Option<u64>,
    pub body: serde_json::Value,
    /// Thread ID for request/response correlation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thid: Option<String>,
    /// Parent thread ID for nested conversations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pthid: Option<String>,
}

impl DIDCommMessage {
    pub fn is_expired(&self) -> bool {
        self.expires_time
            .map(|expires| crate::pod::now_unix() > expires)
            .unwrap_or(false)
    }
}

/// Encrypted DIDComm envelope (JWE-like structure).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DIDCommEnvelope {
    /// Content type: "application/didcomm-encrypted+json"
    pub typ: String,
    /// Ephemeral X25519 public key (base64url).
    pub epk_x25519: String,
    /// AES-256-GCM nonce (base64url).
    pub nonce: String,
    /// Encrypted message body (base64url).
    pub ciphertext: String,
    /// AES-256-GCM authentication tag (base64url).
    pub tag: String,
    /// Sender DID URI (for key resolution).
    pub sender_did: String,
    /// Ed25519 signature over (ciphertext || tag || nonce), base64url.
    pub signature_ed25519: String,
    /// ML-DSA-65 signature over (ciphertext || tag || nonce), base64url.
    pub signature_mldsa65: String,
    /// Post-quantum KEM algorithm identifier (e.g. "ML-KEM-768").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pq_kem: Option<String>,
    /// ML-KEM-768 ciphertext, base64url-encoded. Present when hybrid KEM is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pq_ct: Option<String>,
}

fn random_nonce() -> Result<[u8; 12], String> {
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce)
        .map_err(|e| format!("OS entropy unavailable for DIDComm nonce: {e}"))?;
    Ok(nonce)
}

fn derive_key(shared_secret: &[u8]) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(None, shared_secret);
    let mut out = [0u8; 32];
    hk.expand(HKDF_MESH_DIDCOMM_INFO, &mut out)
        .map_err(|_| "HKDF expand failed for mesh DIDComm key derivation".to_string())?;
    Ok(out)
}

/// Build the signed data: ciphertext || tag || nonce (all raw bytes).
fn signature_input(ciphertext: &[u8], tag: &[u8], nonce: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(ciphertext.len() + tag.len() + nonce.len());
    input.extend_from_slice(ciphertext);
    input.extend_from_slice(tag);
    input.extend_from_slice(nonce);
    input
}

/// Encrypt a DIDComm message for a recipient.
///
/// Uses hybrid KEM (X25519 + ML-KEM-768) when the recipient's DID document
/// contains an ML-KEM-768 key. Falls back to classical ephemeral X25519 ECDH
/// when the recipient lacks PQ keys (backward compatible with pre-PQ peers).
///
/// 1. Generate ephemeral X25519 keypair (+ ML-KEM-768 encapsulation if available).
/// 2. ECDH with recipient's X25519 public key from their DID Document.
/// 3. HKDF-SHA256 to derive AES-256-GCM key (hybrid or classical).
/// 4. Encrypt plaintext message body.
/// 5. Dual-sign (ciphertext || tag || nonce) with sender's Ed25519 + ML-DSA-65.
pub fn encrypt_message(
    sender: &DIDIdentity,
    recipient_doc: &DIDDocument,
    message: &DIDCommMessage,
) -> Result<DIDCommEnvelope, String> {
    let recipient_x25519 = extract_x25519_public_key_from_doc(recipient_doc)?;
    let recipient_mlkem = extract_mlkem_encapsulation_key_from_doc(recipient_doc);
    let plaintext =
        serde_json::to_vec(message).map_err(|e| format!("serialize DIDComm message: {e}"))?;

    // Derive encryption key: hybrid (ephemeral X25519 + ML-KEM-768) or classical.
    let (mut key, epk_bytes, pq_kem, pq_ct) = if let Some(ref ek) = recipient_mlkem {
        let recipient_pk = X25519PublicKey::from(recipient_x25519);
        let encap =
            hybrid_kem::hybrid_encap(&recipient_pk, Some(ek), HKDF_MESH_DIDCOMM_INFO)
                .map_err(|e| format!("hybrid KEM encap failed: {e}"))?;
        let ct = encap
            .mlkem_ciphertext
            .as_ref()
            .expect("ML-KEM ciphertext must be present when ek is Some");
        (
            encap.shared_secret,
            encap.x25519_ephemeral_pk,
            Some("ML-KEM-768".to_string()),
            Some(B64URL.encode(ct)),
        )
    } else {
        // Classical path: ephemeral X25519 with old HKDF (backward compatible).
        let ephemeral_secret = X25519StaticSecret::random_from_rng(rand_core::OsRng);
        let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);
        let shared_secret =
            ephemeral_secret.diffie_hellman(&X25519PublicKey::from(recipient_x25519));
        let key = derive_key(shared_secret.as_bytes())?;
        (key, ephemeral_public.to_bytes(), None, None)
    };

    let nonce = random_nonce()?;

    // AES-256-GCM encrypt. The tag is appended to ciphertext by aes-gcm.
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
    let ciphertext_with_tag = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|e| format!("DIDComm encrypt failed: {e}"))?;
    key.zeroize();

    // aes-gcm appends a 16-byte tag to the ciphertext.
    let tag_offset = ciphertext_with_tag.len().saturating_sub(16);
    let ciphertext_bytes = &ciphertext_with_tag[..tag_offset];
    let tag_bytes = &ciphertext_with_tag[tag_offset..];

    // Dual-sign over (ciphertext || tag || nonce).
    let sig_input = signature_input(ciphertext_bytes, tag_bytes, &nonce);
    let (ed_sig, pq_sig) = dual_sign(sender, &sig_input)?;

    Ok(DIDCommEnvelope {
        typ: CONTENT_TYPE.to_string(),
        epk_x25519: B64URL.encode(epk_bytes),
        nonce: B64URL.encode(nonce),
        ciphertext: B64URL.encode(ciphertext_bytes),
        tag: B64URL.encode(tag_bytes),
        sender_did: sender.did.clone(),
        signature_ed25519: B64URL.encode(ed_sig),
        signature_mldsa65: B64URL.encode(pq_sig),
        pq_kem,
        pq_ct,
    })
}

/// Decrypt a DIDComm envelope.
///
/// 1. Verify dual signature.
/// 2. ECDH with ephemeral key and local X25519 secret.
/// 3. HKDF to derive decryption key.
/// 4. AES-256-GCM decrypt.
/// 5. Deserialize plaintext DIDComm message.
pub fn decrypt_message(
    recipient: &DIDIdentity,
    sender_doc: &DIDDocument,
    envelope: &DIDCommEnvelope,
) -> Result<DIDCommMessage, String> {
    let ciphertext_bytes = B64URL
        .decode(&envelope.ciphertext)
        .map_err(|e| format!("decode ciphertext: {e}"))?;
    let tag_bytes = B64URL
        .decode(&envelope.tag)
        .map_err(|e| format!("decode tag: {e}"))?;
    let nonce_bytes = B64URL
        .decode(&envelope.nonce)
        .map_err(|e| format!("decode nonce: {e}"))?;
    let nonce: [u8; 12] = nonce_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "DIDComm nonce must be 12 bytes".to_string())?;

    // Verify dual signature over (ciphertext || tag || nonce).
    let sig_input = signature_input(&ciphertext_bytes, &tag_bytes, &nonce_bytes);
    let ed_sig = B64URL
        .decode(&envelope.signature_ed25519)
        .map_err(|e| format!("decode Ed25519 signature: {e}"))?;
    let pq_sig = B64URL
        .decode(&envelope.signature_mldsa65)
        .map_err(|e| format!("decode ML-DSA signature: {e}"))?;
    let sig_ok = dual_verify(sender_doc, &sig_input, &ed_sig, &pq_sig)?;
    if !sig_ok {
        return Err("DIDComm envelope signature verification failed".to_string());
    }

    // Derive decryption key: hybrid or classical.
    let epk_bytes = B64URL
        .decode(&envelope.epk_x25519)
        .map_err(|e| format!("decode ephemeral key: {e}"))?;
    let epk: [u8; 32] = epk_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "ephemeral X25519 key must be 32 bytes".to_string())?;

    let mut key = if envelope.pq_kem.is_some() {
        let pq_ct_b64 = envelope
            .pq_ct
            .as_deref()
            .ok_or("pq_kem present but pq_ct missing")?;
        let pq_ct = B64URL
            .decode(pq_ct_b64)
            .map_err(|e| format!("decode ML-KEM ciphertext: {e}"))?;
        let decap = hybrid_kem::hybrid_decap(
            &epk,
            Some(&pq_ct),
            &recipient.x25519_agreement_key,
            Some(&recipient.mlkem768_decapsulation_key),
            HKDF_MESH_DIDCOMM_INFO,
        )
        .map_err(|e| format!("hybrid KEM decap failed: {e}"))?;
        decap.shared_secret
    } else {
        // Classical path: ephemeral ECDH with old HKDF (backward compatible).
        let shared_secret = recipient
            .x25519_agreement_key
            .diffie_hellman(&X25519PublicKey::from(epk));
        derive_key(shared_secret.as_bytes())?
    };

    // Reconstruct ciphertext+tag for aes-gcm (it expects them concatenated).
    let mut ct_with_tag = Vec::with_capacity(ciphertext_bytes.len() + tag_bytes.len());
    ct_with_tag.extend_from_slice(&ciphertext_bytes);
    ct_with_tag.extend_from_slice(&tag_bytes);

    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ct_with_tag.as_ref())
        .map_err(|e| format!("DIDComm decrypt failed: {e}"))?;
    key.zeroize();

    let message: DIDCommMessage =
        serde_json::from_slice(&plaintext).map_err(|e| format!("decode DIDComm plaintext: {e}"))?;
    Ok(message)
}

/// Verify the signature on an envelope without decrypting.
pub fn verify_envelope_signature(
    sender_doc: &DIDDocument,
    envelope: &DIDCommEnvelope,
) -> Result<bool, String> {
    let ciphertext_bytes = B64URL
        .decode(&envelope.ciphertext)
        .map_err(|e| format!("decode ciphertext: {e}"))?;
    let tag_bytes = B64URL
        .decode(&envelope.tag)
        .map_err(|e| format!("decode tag: {e}"))?;
    let nonce_bytes = B64URL
        .decode(&envelope.nonce)
        .map_err(|e| format!("decode nonce: {e}"))?;
    let sig_input = signature_input(&ciphertext_bytes, &tag_bytes, &nonce_bytes);
    let ed_sig = B64URL
        .decode(&envelope.signature_ed25519)
        .map_err(|e| format!("decode Ed25519 signature: {e}"))?;
    let pq_sig = B64URL
        .decode(&envelope.signature_mldsa65)
        .map_err(|e| format!("decode ML-DSA signature: {e}"))?;
    dual_verify(sender_doc, &sig_input, &ed_sig, &pq_sig)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_identity(byte: u8) -> DIDIdentity {
        let seed = [byte; 64];
        crate::halo::did::did_from_genesis_seed(&seed).unwrap()
    }

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let alice = test_identity(0xA1);
        let bob = test_identity(0xA2);
        let msg = DIDCommMessage {
            id: "test-roundtrip-1".into(),
            type_: MessageType::Heartbeat,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 1000,
            expires_time: None,
            body: serde_json::json!({"ping": true}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        assert_eq!(envelope.typ, CONTENT_TYPE);
        assert_eq!(envelope.sender_did, alice.did);

        let decrypted = decrypt_message(&bob, &alice.did_document, &envelope).unwrap();
        assert_eq!(decrypted.id, "test-roundtrip-1");
        assert_eq!(decrypted.type_, MessageType::Heartbeat);
        assert_eq!(decrypted.body["ping"], true);
    }

    #[test]
    fn wrong_recipient_cannot_decrypt() {
        let alice = test_identity(0xA3);
        let bob = test_identity(0xA4);
        let charlie = test_identity(0xA5);
        let msg = DIDCommMessage {
            id: "test-wrong-recip".into(),
            type_: MessageType::Heartbeat,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 1000,
            expires_time: None,
            body: serde_json::json!({"secret": "for bob only"}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        // Charlie cannot decrypt (different X25519 secret → different shared secret → AEAD fail).
        assert!(decrypt_message(&charlie, &alice.did_document, &envelope).is_err());
    }

    #[test]
    fn tampered_ciphertext_fails_verification() {
        let alice = test_identity(0xA6);
        let bob = test_identity(0xA7);
        let msg = DIDCommMessage {
            id: "test-tamper".into(),
            type_: MessageType::McpToolCall,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 1000,
            expires_time: None,
            body: serde_json::json!({"tool": "query"}),
            thid: None,
            pthid: None,
        };
        let mut envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        // Tamper with ciphertext.
        envelope.ciphertext = B64URL.encode(b"tampered-data");
        // Signature verification fails before decryption.
        assert!(decrypt_message(&bob, &alice.did_document, &envelope).is_err());
    }

    #[test]
    fn verify_signature_without_decrypt() {
        let alice = test_identity(0xA8);
        let bob = test_identity(0xA9);
        let msg = DIDCommMessage {
            id: "test-sig-only".into(),
            type_: MessageType::PeerAnnounce,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 1000,
            expires_time: None,
            body: serde_json::json!({"announce": true}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        assert!(verify_envelope_signature(&alice.did_document, &envelope).unwrap());
        // Different sender doc → signature mismatch.
        assert!(!verify_envelope_signature(&bob.did_document, &envelope).unwrap());
    }

    #[test]
    fn envelope_content_type_is_correct() {
        let alice = test_identity(0xAA);
        let bob = test_identity(0xAB);
        let msg = DIDCommMessage {
            id: "ct-check".into(),
            type_: MessageType::CapabilityGrant,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 2000,
            expires_time: Some(crate::pod::now_unix() + 3600),
            body: serde_json::json!({"grant": "test"}),
            thid: Some("thread-1".into()),
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        assert_eq!(envelope.typ, "application/didcomm-encrypted+json");
        let decrypted = decrypt_message(&bob, &alice.did_document, &envelope).unwrap();
        assert_eq!(decrypted.thid.as_deref(), Some("thread-1"));
        assert!(!decrypted.is_expired());
    }

    #[test]
    fn message_type_serialization() {
        let json = serde_json::to_string(&MessageType::McpToolCall).unwrap();
        assert_eq!(json, "\"mcp_tool_call\"");
        let parsed: MessageType = serde_json::from_str("\"envelope_exchange\"").unwrap();
        assert_eq!(parsed, MessageType::EnvelopeExchange);
    }

    // ── Hybrid KEM integration tests ──────────────────────────────────

    #[test]
    fn mesh_hybrid_kem_roundtrip() {
        // Both alice and bob have ML-KEM-768 keys (from did_from_genesis_seed).
        // encrypt_message should pick the hybrid path automatically.
        let alice = test_identity(0xB1);
        let bob = test_identity(0xB2);
        let msg = DIDCommMessage {
            id: "mesh-hybrid-1".into(),
            type_: MessageType::McpToolCall,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 3000,
            expires_time: None,
            body: serde_json::json!({"tool": "heyting_prove_assist", "args": {"goal": "P -> P"}}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        // Hybrid path sets pq fields.
        assert_eq!(envelope.pq_kem.as_deref(), Some("ML-KEM-768"));
        assert!(envelope.pq_ct.is_some());

        let decrypted = decrypt_message(&bob, &alice.did_document, &envelope).unwrap();
        assert_eq!(decrypted.id, "mesh-hybrid-1");
        assert_eq!(decrypted.body["tool"], "heyting_prove_assist");
    }

    #[test]
    fn mesh_hybrid_wrong_recipient_rejected() {
        let alice = test_identity(0xB3);
        let bob = test_identity(0xB4);
        let charlie = test_identity(0xB5);
        let msg = DIDCommMessage {
            id: "mesh-hybrid-wrong".into(),
            type_: MessageType::EnvelopeExchange,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 3000,
            expires_time: None,
            body: serde_json::json!({"proof": "secret"}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        assert!(envelope.pq_kem.is_some());
        // Charlie's ML-KEM dk + X25519 sk are wrong → decap fails.
        assert!(decrypt_message(&charlie, &alice.did_document, &envelope).is_err());
    }

    #[test]
    fn mesh_hybrid_tampered_pq_ct_rejected() {
        let alice = test_identity(0xB6);
        let bob = test_identity(0xB7);
        let msg = DIDCommMessage {
            id: "mesh-hybrid-tamper".into(),
            type_: MessageType::Heartbeat,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 3000,
            expires_time: None,
            body: serde_json::json!({"ping": true}),
            thid: None,
            pthid: None,
        };
        let mut envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        // Tamper with ML-KEM ciphertext (flip bytes).
        if let Some(ref ct) = envelope.pq_ct {
            let mut raw = B64URL.decode(ct).unwrap();
            for b in raw.iter_mut().take(16) {
                *b ^= 0xFF;
            }
            envelope.pq_ct = Some(B64URL.encode(&raw));
        }
        // Tampered ML-KEM ct → wrong shared secret → AEAD decrypt fails.
        // (Signature still valid because sig covers ciphertext+tag+nonce, not pq_ct.)
        assert!(decrypt_message(&bob, &alice.did_document, &envelope).is_err());
    }

    #[test]
    fn mesh_hybrid_verify_signature_independent_of_kem() {
        let alice = test_identity(0xB8);
        let bob = test_identity(0xB9);
        let msg = DIDCommMessage {
            id: "mesh-hybrid-sigcheck".into(),
            type_: MessageType::PeerAnnounce,
            from: alice.did.clone(),
            to: vec![bob.did.clone()],
            created_time: 3000,
            expires_time: None,
            body: serde_json::json!({"announce": true}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();
        assert!(envelope.pq_kem.is_some());
        // Signature verification works regardless of KEM type.
        assert!(verify_envelope_signature(&alice.did_document, &envelope).unwrap());
        assert!(!verify_envelope_signature(&bob.did_document, &envelope).unwrap());
    }
}
