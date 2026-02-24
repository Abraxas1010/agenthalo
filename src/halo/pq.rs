use crate::halo::config;
use crate::halo::trace::now_unix_secs;
use ml_dsa::{
    EncodedSignature, EncodedVerifyingKey, KeyGen, MlDsa65, Signature as MlDsaSignature,
    VerifyingKey as MlDsaVerifyingKey,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const PQ_CONTEXT: &[u8] = b"";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqWallet {
    pub version: u8,
    pub algorithm: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub secret_seed_hex: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PqKeygenResult {
    pub algorithm: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub created_at: u64,
    pub wallet_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PqSignatureEnvelope {
    pub algorithm: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub payload_sha256: String,
    pub signature_hex: String,
    pub signature_digest: String,
    pub created_at: u64,
    pub payload_kind: String,
    pub payload_hint: Option<String>,
}

pub fn has_wallet() -> bool {
    config::pq_wallet_path().exists()
}

pub fn keygen_pq(force: bool) -> Result<PqKeygenResult, String> {
    config::ensure_halo_dir()?;
    let wallet_path = config::pq_wallet_path();
    if wallet_path.exists() && !force {
        return Err(format!(
            "PQ wallet already exists at {} (use --force to rotate)",
            wallet_path.display()
        ));
    }

    let seed_bytes = random_seed_32();
    let seed = ml_dsa::Seed::try_from(seed_bytes.as_slice())
        .map_err(|_| "failed to construct ML-DSA seed".to_string())?;
    let kp = MlDsa65::from_seed(&seed);
    let public_key_hex = hex_encode(kp.verifying_key().encode().as_slice());
    let key_id = key_id_from_public_key(&public_key_hex);
    let created_at = now_unix_secs();
    let wallet = PqWallet {
        version: 1,
        algorithm: "ml_dsa65".to_string(),
        key_id: key_id.clone(),
        public_key_hex: public_key_hex.clone(),
        secret_seed_hex: hex_encode(&seed_bytes),
        created_at,
    };
    save_wallet(&wallet_path, &wallet)?;

    Ok(PqKeygenResult {
        algorithm: wallet.algorithm,
        key_id,
        public_key_hex,
        created_at,
        wallet_path: wallet_path.display().to_string(),
    })
}

pub fn sign_pq_payload(
    payload: &[u8],
    payload_kind: &str,
    payload_hint: Option<String>,
) -> Result<(PqSignatureEnvelope, PathBuf), String> {
    let wallet_path = config::pq_wallet_path();
    let wallet = load_wallet(&wallet_path)?;
    let kp = keypair_from_wallet(&wallet)?;
    let sig = kp
        .signing_key()
        .sign_deterministic(payload, PQ_CONTEXT)
        .map_err(|_| "ML-DSA signing failed".to_string())?;
    if !kp
        .verifying_key()
        .verify_with_context(payload, PQ_CONTEXT, &sig)
    {
        return Err("ML-DSA self-verification failed".to_string());
    }

    let signature_hex = hex_encode(sig.encode().as_slice());
    let payload_sha256 = hex_encode(Sha256::digest(payload).as_slice());
    let signature_digest = hex_encode(
        Sha256::digest(
            format!(
                "agenthalo.sign.pq.v1:{}:{}:{}",
                wallet.key_id, payload_sha256, signature_hex
            )
            .as_bytes(),
        )
        .as_slice(),
    );
    let envelope = PqSignatureEnvelope {
        algorithm: "ml_dsa65".to_string(),
        key_id: wallet.key_id.clone(),
        public_key_hex: wallet.public_key_hex,
        payload_sha256,
        signature_hex,
        signature_digest,
        created_at: now_unix_secs(),
        payload_kind: payload_kind.to_string(),
        payload_hint,
    };

    let save_path = save_signature(&envelope)?;
    Ok((envelope, save_path))
}

pub fn verify_detached_signature(
    payload: &[u8],
    public_key_hex: &str,
    signature_hex: &str,
) -> Result<bool, String> {
    let vk_bytes = hex_decode_dynamic(public_key_hex)?;
    let sig_bytes = hex_decode_dynamic(signature_hex)?;
    let enc_vk = EncodedVerifyingKey::<MlDsa65>::try_from(vk_bytes.as_slice())
        .map_err(|_| "invalid ML-DSA public key encoding".to_string())?;
    let enc_sig = EncodedSignature::<MlDsa65>::try_from(sig_bytes.as_slice())
        .map_err(|_| "invalid ML-DSA signature encoding".to_string())?;
    let vk = MlDsaVerifyingKey::<MlDsa65>::decode(&enc_vk);
    let sig = MlDsaSignature::<MlDsa65>::decode(&enc_sig)
        .ok_or_else(|| "invalid ML-DSA signature payload".to_string())?;
    Ok(vk.verify_with_context(payload, PQ_CONTEXT, &sig))
}

fn save_wallet(path: &Path, wallet: &PqWallet) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(wallet).map_err(|e| format!("serialize wallet: {e}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create wallet dir {}: {e}", parent.display()))?;
    }

    #[cfg(unix)]
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut file = opts
            .open(path)
            .map_err(|e| format!("open wallet {}: {e}", path.display()))?;
        file.write_all(raw.as_bytes())
            .map_err(|e| format!("write wallet {}: {e}", path.display()))?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod wallet {} to 0600: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, raw).map_err(|e| format!("write wallet {}: {e}", path.display()))?;
    }

    Ok(())
}

fn load_wallet(path: &Path) -> Result<PqWallet, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read wallet {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse wallet {}: {e}", path.display()))
}

fn keypair_from_wallet(wallet: &PqWallet) -> Result<ml_dsa::KeyPair<MlDsa65>, String> {
    let seed_bytes = hex_decode_exact::<32>(&wallet.secret_seed_hex)?;
    let seed = ml_dsa::Seed::try_from(seed_bytes.as_slice())
        .map_err(|_| "wallet seed must be 32 bytes".to_string())?;
    let kp = MlDsa65::from_seed(&seed);
    let actual_pub = hex_encode(kp.verifying_key().encode().as_slice());
    if !eq_case_insensitive(&actual_pub, &wallet.public_key_hex) {
        return Err("wallet public key does not match stored seed".to_string());
    }
    Ok(kp)
}

fn save_signature(envelope: &PqSignatureEnvelope) -> Result<PathBuf, String> {
    config::ensure_halo_dir()?;
    config::ensure_signatures_dir()?;
    let short_key = envelope.key_id.chars().take(16).collect::<String>();
    let path = config::signatures_dir().join(format!("{}_{}.json", short_key, envelope.created_at));
    let raw = serde_json::to_vec_pretty(envelope)
        .map_err(|e| format!("serialize signature envelope: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write signature {}: {e}", path.display()))?;
    Ok(path)
}

fn random_seed_32() -> [u8; 32] {
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    out[16..].copy_from_slice(uuid::Uuid::new_v4().as_bytes());
    out
}

fn key_id_from_public_key(public_key_hex: &str) -> String {
    hex_encode(
        Sha256::digest(format!("agenthalo.pq.keyid.v1:{public_key_hex}").as_bytes()).as_slice(),
    )
}

fn eq_case_insensitive(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

fn hex_decode_dynamic(input: &str) -> Result<Vec<u8>, String> {
    let bytes = input.as_bytes();
    if bytes.is_empty() || bytes.len() % 2 != 0 {
        return Err("hex string must have even length".to_string());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| "invalid hex".to_string())?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| "invalid hex".to_string())?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_decode_exact<const N: usize>(input: &str) -> Result<[u8; N], String> {
    if input.len() != N * 2 {
        return Err(format!("expected {} hex chars, got {}", N * 2, input.len()));
    }
    let mut out = [0u8; N];
    for (i, pair) in input.as_bytes().chunks_exact(2).enumerate() {
        let hi = hex_nibble(pair[0]).ok_or_else(|| "invalid hex".to_string())?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| "invalid hex".to_string())?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<T>(tag: &str, f: impl FnOnce() -> T) -> T {
        let old_home = std::env::var("AGENTHALO_HOME").ok();
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_pq_{tag}_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &dir);
        let out = f();
        if let Some(v) = old_home {
            std::env::set_var("AGENTHALO_HOME", v);
        } else {
            std::env::remove_var("AGENTHALO_HOME");
        }
        let _ = std::fs::remove_dir_all(&dir);
        out
    }

    #[test]
    fn keygen_and_sign_roundtrip() {
        with_temp_home("roundtrip", || {
            let key = keygen_pq(false).expect("keygen");
            assert_eq!(key.algorithm, "ml_dsa65");
            assert!(has_wallet());

            let message = b"phase2 signing test";
            let (env, _path) =
                sign_pq_payload(message, "message", Some("inline".to_string())).expect("sign");
            assert_eq!(env.algorithm, "ml_dsa65");
            assert_eq!(env.key_id, key.key_id);
            assert!(
                verify_detached_signature(message, &env.public_key_hex, &env.signature_hex)
                    .expect("verify")
            );
        });
    }

    #[test]
    fn keygen_requires_force_for_rotation() {
        with_temp_home("rotate", || {
            let _ = keygen_pq(false).expect("first key");
            let err = keygen_pq(false).expect_err("second key should require force");
            assert!(err.contains("already exists"));
            let _ = keygen_pq(true).expect("forced rotation");
        });
    }
}
