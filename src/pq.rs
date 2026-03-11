use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct PqStoragePaths {
    pub wallet_path: PathBuf,
    pub signatures_dir: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PqKeygenResult {
    pub algorithm: String,
    pub key_id: String,
    pub public_key_hex: String,
    pub created_at: u64,
    pub wallet_path: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct PqWallet {
    #[serde(default)]
    secret_seed_hex: Option<String>,
    #[serde(default)]
    encrypted_seed: Option<PqEncryptedSeed>,
}

#[derive(Debug, Deserialize, Serialize)]
struct PqEncryptedSeed {
    nonce_hex: String,
    ciphertext_hex: String,
}

pub fn wallet_seed_bytes_from_path(wallet_path: &Path) -> Result<Vec<u8>, String> {
    let raw = std::fs::read_to_string(wallet_path)
        .map_err(|e| format!("read wallet {}: {e}", wallet_path.display()))?;
    let wallet: PqWallet = serde_json::from_str(&raw)
        .map_err(|e| format!("parse wallet {}: {e}", wallet_path.display()))?;

    if let Some(seed_hex) = wallet.secret_seed_hex {
        return hex::decode(seed_hex).map_err(|e| format!("decode wallet seed hex: {e}"));
    }
    if let Some(enc) = wallet.encrypted_seed {
        return decrypt_wallet_seed(wallet_path, &enc);
    }
    Err("wallet missing secret_seed_hex and encrypted_seed".to_string())
}

pub fn keygen_pq_with_paths(paths: &PqStoragePaths, force: bool) -> Result<PqKeygenResult, String> {
    if let Some(parent) = paths.wallet_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create wallet dir {}: {e}", parent.display()))?;
    }
    std::fs::create_dir_all(&paths.signatures_dir).map_err(|e| {
        format!(
            "create signatures dir {}: {e}",
            paths.signatures_dir.display()
        )
    })?;
    if paths.wallet_path.exists() && !force {
        return Err(format!(
            "PQ wallet already exists at {} (use force to rotate)",
            paths.wallet_path.display()
        ));
    }
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let public_key_hex = hex::encode(Sha256::digest(seed));
    let key_id = format!("pq-{}", &public_key_hex[..16]);
    let wallet = PqWallet {
        secret_seed_hex: Some(hex::encode(seed)),
        encrypted_seed: None,
    };
    let raw =
        serde_json::to_string_pretty(&wallet).map_err(|e| format!("serialize wallet: {e}"))?;
    std::fs::write(&paths.wallet_path, raw)
        .map_err(|e| format!("write wallet {}: {e}", paths.wallet_path.display()))?;
    Ok(PqKeygenResult {
        algorithm: "ml_dsa65".to_string(),
        key_id,
        public_key_hex,
        created_at: crate::util::now_unix_secs(),
        wallet_path: paths.wallet_path.display().to_string(),
    })
}

fn decrypt_wallet_seed(wallet_path: &Path, enc: &PqEncryptedSeed) -> Result<Vec<u8>, String> {
    let key_path = wallet_wrap_key_path(wallet_path);
    let key = std::fs::read(&key_path)
        .map_err(|e| format!("read wallet wrap key {}: {e}", key_path.display()))?;
    if key.len() != 32 {
        return Err(format!(
            "wallet wrap key {} must be 32 bytes, got {}",
            key_path.display(),
            key.len()
        ));
    }
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init: {e}"))?;
    let nonce = hex::decode(&enc.nonce_hex).map_err(|e| format!("decode wallet nonce: {e}"))?;
    let ciphertext =
        hex::decode(&enc.ciphertext_hex).map_err(|e| format!("decode wallet ciphertext: {e}"))?;
    cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|e| format!("decrypt wallet seed: {e}"))
}

fn wallet_wrap_key_path(wallet_path: &Path) -> PathBuf {
    wallet_path.with_extension("seed.key")
}

pub fn wallet_seed_fingerprint(wallet_path: &Path) -> Result<String, String> {
    let seed = wallet_seed_bytes_from_path(wallet_path)?;
    Ok(hex::encode(Sha256::digest(seed)))
}
