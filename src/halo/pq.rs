use crate::halo::config;
use crate::halo::trace::now_unix_secs;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use ml_dsa::{
    EncodedSignature, EncodedVerifyingKey, KeyGen, MlDsa65, Signature as MlDsaSignature,
    VerifyingKey as MlDsaVerifyingKey,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use zeroize::Zeroize;

#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const PQ_CONTEXT: &[u8] = b"";

#[derive(Clone, Debug)]
pub struct PqStoragePaths {
    pub wallet_path: PathBuf,
    pub signatures_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqWallet {
    pub version: u8,
    pub algorithm: String,
    pub key_id: String,
    pub public_key_hex: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_seed_hex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_seed: Option<PqEncryptedSeed>,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqEncryptedSeed {
    pub schema: String,
    pub nonce_hex: String,
    pub ciphertext_hex: String,
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
    has_wallet_with_paths(&default_storage_paths())
}

pub fn has_wallet_with_paths(paths: &PqStoragePaths) -> bool {
    paths.wallet_path.exists()
}

pub fn keygen_pq(force: bool) -> Result<PqKeygenResult, String> {
    keygen_pq_with_paths(&default_storage_paths(), force)
}

pub fn keygen_pq_with_paths(paths: &PqStoragePaths, force: bool) -> Result<PqKeygenResult, String> {
    if let Some(parent) = paths.wallet_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create wallet parent {}: {e}", parent.display()))?;
    }
    std::fs::create_dir_all(&paths.signatures_dir).map_err(|e| {
        format!(
            "create signatures dir {}: {e}",
            paths.signatures_dir.display()
        )
    })?;

    let wallet_path = &paths.wallet_path;
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
    let encrypted_seed = encrypt_wallet_seed(paths, &seed_bytes)?;
    let wallet = PqWallet {
        version: 1,
        algorithm: "ml_dsa65".to_string(),
        key_id: key_id.clone(),
        public_key_hex: public_key_hex.clone(),
        secret_seed_hex: None,
        encrypted_seed: Some(encrypted_seed),
        created_at,
    };
    save_wallet(wallet_path, &wallet)?;

    Ok(PqKeygenResult {
        algorithm: wallet.algorithm,
        key_id,
        public_key_hex,
        created_at,
        wallet_path: paths.wallet_path.display().to_string(),
    })
}

pub fn sign_pq_payload(
    payload: &[u8],
    payload_kind: &str,
    payload_hint: Option<String>,
) -> Result<(PqSignatureEnvelope, PathBuf), String> {
    sign_pq_payload_with_paths(
        &default_storage_paths(),
        payload,
        payload_kind,
        payload_hint,
    )
}

pub fn sign_pq_payload_with_paths(
    paths: &PqStoragePaths,
    payload: &[u8],
    payload_kind: &str,
    payload_hint: Option<String>,
) -> Result<(PqSignatureEnvelope, PathBuf), String> {
    let wallet = load_wallet(&paths.wallet_path)?;
    let kp = keypair_from_wallet(&paths.wallet_path, &wallet)?;
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

    let save_path = save_signature(&paths.signatures_dir, &envelope)?;
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

pub fn key_id_for_public_key(public_key_hex: &str) -> String {
    key_id_from_public_key(public_key_hex)
}

pub fn wallet_key_identity() -> Result<Option<(String, String)>, String> {
    let paths = default_storage_paths();
    if !paths.wallet_path.exists() {
        return Ok(None);
    }
    let wallet = load_wallet(&paths.wallet_path)?;
    Ok(Some((wallet.key_id, wallet.public_key_hex)))
}

pub fn wallet_seed_bytes() -> Result<Option<Vec<u8>>, String> {
    let paths = default_storage_paths();
    if !paths.wallet_path.exists() {
        return Ok(None);
    }
    let wallet = load_wallet(&paths.wallet_path)?;
    Ok(Some(extract_wallet_seed_bytes(
        &paths.wallet_path,
        &wallet,
    )?))
}

pub fn wallet_seed_bytes_from_path(wallet_path: &Path) -> Result<Vec<u8>, String> {
    let wallet = load_wallet(wallet_path)?;
    extract_wallet_seed_bytes(wallet_path, &wallet)
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

fn keypair_from_wallet(
    wallet_path: &Path,
    wallet: &PqWallet,
) -> Result<ml_dsa::KeyPair<MlDsa65>, String> {
    let mut seed_vec = extract_wallet_seed_bytes(wallet_path, wallet)?;
    if seed_vec.len() != 32 {
        return Err(format!(
            "wallet seed must be 32 bytes, got {}",
            seed_vec.len()
        ));
    }
    let mut seed_arr = [0u8; 32];
    seed_arr.copy_from_slice(&seed_vec);
    seed_vec.zeroize();
    let seed_bytes = seed_arr;
    let seed = ml_dsa::Seed::try_from(seed_bytes.as_slice())
        .map_err(|_| "wallet seed must be 32 bytes".to_string())?;
    let kp = MlDsa65::from_seed(&seed);
    seed_arr.zeroize();
    let actual_pub = hex_encode(kp.verifying_key().encode().as_slice());
    if !eq_case_insensitive(&actual_pub, &wallet.public_key_hex) {
        return Err("wallet public key does not match stored seed".to_string());
    }
    Ok(kp)
}

fn extract_wallet_seed_bytes(wallet_path: &Path, wallet: &PqWallet) -> Result<Vec<u8>, String> {
    if let Some(enc) = &wallet.encrypted_seed {
        return decrypt_wallet_seed(wallet_path, enc);
    }
    if let Some(seed_hex) = &wallet.secret_seed_hex {
        let seed = hex_decode_exact::<32>(seed_hex)?.to_vec();
        let migrated = PqWallet {
            version: wallet.version,
            algorithm: wallet.algorithm.clone(),
            key_id: wallet.key_id.clone(),
            public_key_hex: wallet.public_key_hex.clone(),
            secret_seed_hex: None,
            encrypted_seed: Some(encrypt_wallet_seed_from_existing(wallet_path, &seed)?),
            created_at: wallet.created_at,
        };
        let _ = save_wallet(wallet_path, &migrated);
        return Ok(seed);
    }
    Err("wallet missing encrypted_seed and legacy secret_seed_hex".to_string())
}

fn wallet_wrap_key_path(wallet_path: &Path) -> PathBuf {
    wallet_path.with_extension("seed.key")
}

fn load_or_create_wallet_wrap_key(wallet_path: &Path) -> Result<[u8; 32], String> {
    let key_path = wallet_wrap_key_path(wallet_path);
    if key_path.exists() {
        let raw = std::fs::read(&key_path)
            .map_err(|e| format!("read wallet wrap key {}: {e}", key_path.display()))?;
        if raw.len() != 32 {
            return Err(format!(
                "wallet wrap key {} must be 32 bytes, got {}",
                key_path.display(),
                raw.len()
            ));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&raw);
        return Ok(out);
    }
    if let Some(parent) = key_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create wrap-key dir {}: {e}", parent.display()))?;
    }
    let key = random_seed_32();
    #[cfg(unix)]
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(0o600);
        let mut file = opts
            .open(&key_path)
            .map_err(|e| format!("open wallet wrap key {}: {e}", key_path.display()))?;
        file.write_all(&key)
            .map_err(|e| format!("write wallet wrap key {}: {e}", key_path.display()))?;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod wallet wrap key {}: {e}", key_path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&key_path, &key)
            .map_err(|e| format!("write wallet wrap key {}: {e}", key_path.display()))?;
    }
    Ok(key)
}

fn encrypt_wallet_seed(
    paths: &PqStoragePaths,
    seed_bytes: &[u8; 32],
) -> Result<PqEncryptedSeed, String> {
    encrypt_wallet_seed_from_existing(&paths.wallet_path, seed_bytes)
}

fn encrypt_wallet_seed_from_existing(
    wallet_path: &Path,
    seed_bytes: &[u8],
) -> Result<PqEncryptedSeed, String> {
    let key = load_or_create_wallet_wrap_key(wallet_path)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("seed-wrap cipher init: {e}"))?;
    let mut nonce = [0u8; 12];
    nonce[..].copy_from_slice(&random_seed_32()[..12]);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), seed_bytes)
        .map_err(|e| format!("seed-wrap encrypt failed: {e}"))?;
    Ok(PqEncryptedSeed {
        schema: "agenthalo.pq.seedwrap.v1".to_string(),
        nonce_hex: hex_encode(&nonce),
        ciphertext_hex: hex_encode(&ciphertext),
    })
}

fn decrypt_wallet_seed(wallet_path: &Path, encrypted: &PqEncryptedSeed) -> Result<Vec<u8>, String> {
    if encrypted.schema != "agenthalo.pq.seedwrap.v1" {
        return Err(format!(
            "unsupported wallet encrypted seed schema {}",
            encrypted.schema
        ));
    }
    let key = load_or_create_wallet_wrap_key(wallet_path)?;
    let nonce = hex_decode_exact::<12>(&encrypted.nonce_hex)?;
    let ciphertext = hex_decode_dynamic(&encrypted.ciphertext_hex)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| format!("seed-wrap cipher init: {e}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "seed-wrap decryption failed".to_string())?;
    Ok(plaintext)
}

fn save_signature(
    signatures_dir: &Path,
    envelope: &PqSignatureEnvelope,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(signatures_dir)
        .map_err(|e| format!("create signatures dir {}: {e}", signatures_dir.display()))?;
    let short_key = envelope.key_id.chars().take(16).collect::<String>();
    let nonce = uuid::Uuid::new_v4();
    let path = signatures_dir.join(format!(
        "{}_{}_{}.json",
        short_key, envelope.created_at, nonce
    ));
    let raw = serde_json::to_vec_pretty(envelope)
        .map_err(|e| format!("serialize signature envelope: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write signature {}: {e}", path.display()))?;
    Ok(path)
}

fn default_storage_paths() -> PqStoragePaths {
    PqStoragePaths {
        wallet_path: config::pq_wallet_path(),
        signatures_dir: config::signatures_dir(),
    }
}

fn random_seed_32() -> [u8; 32] {
    let mut out = [0u8; 32];
    getrandom::getrandom(&mut out).expect("OS entropy source unavailable");
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
    if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
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

    fn temp_paths(tag: &str) -> (PqStoragePaths, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "agenthalo_pq_{tag}_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("create temp home");
        let paths = PqStoragePaths {
            wallet_path: root.join("pq_wallet.json"),
            signatures_dir: root.join("signatures"),
        };
        (paths, root)
    }

    #[test]
    fn keygen_and_sign_roundtrip() {
        let (paths, root) = temp_paths("roundtrip");
        let key = keygen_pq_with_paths(&paths, false).expect("keygen");
        assert_eq!(key.algorithm, "ml_dsa65");
        assert!(has_wallet_with_paths(&paths));

        let message = b"phase2 signing test";
        let (env, _path) =
            sign_pq_payload_with_paths(&paths, message, "message", Some("inline".to_string()))
                .expect("sign");
        assert_eq!(env.algorithm, "ml_dsa65");
        assert_eq!(env.key_id, key.key_id);
        assert!(
            verify_detached_signature(message, &env.public_key_hex, &env.signature_hex)
                .expect("verify")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keygen_requires_force_for_rotation() {
        let (paths, root) = temp_paths("rotate");
        let _ = keygen_pq_with_paths(&paths, false).expect("first key");
        let err = keygen_pq_with_paths(&paths, false).expect_err("second key should require force");
        assert!(err.contains("already exists"));
        let _ = keygen_pq_with_paths(&paths, true).expect("forced rotation");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn signature_paths_are_unique_per_sign_call() {
        let (paths, root) = temp_paths("sigpath");
        let _ = keygen_pq_with_paths(&paths, false).expect("keygen");
        let (_sig1, path1) =
            sign_pq_payload_with_paths(&paths, b"m1", "message", None).expect("sign 1");
        let (_sig2, path2) =
            sign_pq_payload_with_paths(&paths, b"m2", "message", None).expect("sign 2");
        assert_ne!(path1, path2);
        assert!(path1.exists());
        assert!(path2.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn keygen_wallet_no_longer_persists_plain_seed_hex() {
        let (paths, root) = temp_paths("no_plain_seed");
        let _ = keygen_pq_with_paths(&paths, false).expect("keygen");
        let raw = std::fs::read_to_string(&paths.wallet_path).expect("read wallet");
        let wallet: PqWallet = serde_json::from_str(&raw).expect("parse wallet");
        assert!(wallet.secret_seed_hex.is_none());
        assert!(wallet.encrypted_seed.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }
}
