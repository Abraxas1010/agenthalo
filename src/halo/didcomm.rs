use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
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

fn random_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    getrandom::getrandom(&mut nonce).expect("OS entropy unavailable for DIDComm nonce");
    nonce
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

pub fn pack_authcrypt(
    message: &DIDCommMessage,
    sender: &DIDIdentity,
    recipient_x25519_public_key: &[u8; 32],
) -> Result<Vec<u8>, String> {
    let plaintext =
        serde_json::to_vec(message).map_err(|e| format!("serialize DIDComm message: {e}"))?;
    let shared_secret = sender
        .x25519_agreement_key
        .diffie_hellman(&X25519PublicKey::from(*recipient_x25519_public_key));

    let mut key = derive_key(shared_secret.as_bytes(), HKDF_AUTHCRYPT_INFO)?;
    let nonce = random_nonce();
    let ciphertext = encrypt_with_key(&key, &plaintext, &nonce)?;
    let (ed_sig, pq_sig) = dual_sign(sender, &ciphertext)?;

    let sender_x25519_public_key = X25519PublicKey::from(&sender.x25519_agreement_key);
    let envelope = AuthcryptEnvelope {
        kind: "authcrypt".to_string(),
        protected: AuthcryptProtected {
            alg: "ECDH-ES+A256KW".to_string(),
            enc: "A256GCM".to_string(),
            sender_did: sender.did.clone(),
            sender_x25519_public_key: B64.encode(sender_x25519_public_key.as_bytes()),
        },
        nonce: B64.encode(nonce),
        ciphertext: B64.encode(&ciphertext),
        ed25519_signature: B64.encode(ed_sig),
        mldsa65_signature: B64.encode(pq_sig),
    };

    key.zeroize();
    serde_json::to_vec(&envelope).map_err(|e| format!("serialize authcrypt envelope: {e}"))
}

pub fn pack_anoncrypt(
    message: &DIDCommMessage,
    recipient_x25519_public_key: &[u8; 32],
) -> Result<Vec<u8>, String> {
    let plaintext =
        serde_json::to_vec(message).map_err(|e| format!("serialize DIDComm message: {e}"))?;
    let ephemeral_secret = X25519StaticSecret::random_from_rng(OsRng);
    let ephemeral_public = X25519PublicKey::from(&ephemeral_secret);
    let shared_secret =
        ephemeral_secret.diffie_hellman(&X25519PublicKey::from(*recipient_x25519_public_key));

    let mut key = derive_key(shared_secret.as_bytes(), HKDF_ANONCRYPT_INFO)?;
    let nonce = random_nonce();
    let ciphertext = encrypt_with_key(&key, &plaintext, &nonce)?;

    let envelope = AnoncryptEnvelope {
        kind: "anoncrypt".to_string(),
        protected: AnoncryptProtected {
            alg: "ECDH-ES+A256KW".to_string(),
            enc: "A256GCM".to_string(),
            epk: B64.encode(ephemeral_public.as_bytes()),
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

            let shared_secret = recipient
                .x25519_agreement_key
                .diffie_hellman(&X25519PublicKey::from(sender_x25519_public_key));
            let mut key = derive_key(shared_secret.as_bytes(), HKDF_AUTHCRYPT_INFO)?;
            let plaintext = decrypt_with_key(&key, &ciphertext, &nonce)?;
            key.zeroize();

            let message: DIDCommMessage = serde_json::from_slice(&plaintext)
                .map_err(|e| format!("decode authcrypt plaintext message: {e}"))?;
            if message.is_expired() {
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

            let shared_secret = recipient
                .x25519_agreement_key
                .diffie_hellman(&X25519PublicKey::from(epk));
            let mut key = derive_key(shared_secret.as_bytes(), HKDF_ANONCRYPT_INFO)?;
            let plaintext = decrypt_with_key(&key, &ciphertext, &nonce)?;
            key.zeroize();

            let message: DIDCommMessage = serde_json::from_slice(&plaintext)
                .map_err(|e| format!("decode anoncrypt plaintext message: {e}"))?;
            if message.is_expired() {
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
}
