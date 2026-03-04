use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use bip39::{Language, Mnemonic};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroize;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredGenesisSeed {
    schema: String,
    created_at: u64,
    combined_entropy_sha256: String,
    combined_entropy_hex: String,
}

fn now_unix() -> u64 {
    crate::halo::util::now_unix_secs()
}

fn load_wallet_seed_bytes(wallet_path: &std::path::Path) -> Result<Vec<u8>, String> {
    crate::halo::pq::wallet_seed_bytes_from_path(wallet_path)
}

fn derive_seed_key(wallet_path: &std::path::Path) -> Result<[u8; 32], String> {
    let seed_bytes = load_wallet_seed_bytes(wallet_path)?;
    let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-genesis-seed-v1"), &seed_bytes);
    let mut out = [0u8; 32];
    hk.expand(b"seed-wrap", &mut out)
        .map_err(|_| "hkdf expand failed".to_string())?;
    Ok(out)
}

pub fn seed_exists() -> bool {
    crate::halo::config::genesis_seed_path().exists()
}

fn store_seed_once_with_paths(
    wallet_path: &std::path::Path,
    seed_path: &std::path::Path,
    seed: &[u8; 64],
    combined_entropy_sha256: &str,
) -> Result<(), String> {
    if seed_path.exists() {
        return Err(format!(
            "genesis seed already initialized at {}",
            seed_path.display()
        ));
    }

    let mut key = derive_seed_key(wallet_path)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
    let payload = StoredGenesisSeed {
        schema: "agenthalo.genesis.seed.v1".to_string(),
        created_at: now_unix(),
        combined_entropy_sha256: combined_entropy_sha256.to_string(),
        combined_entropy_hex: crate::halo::util::hex_encode(seed),
    };
    let plaintext =
        serde_json::to_vec(&payload).map_err(|e| format!("serialize genesis seed: {e}"))?;

    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|e| format!("encrypt genesis seed: {e}"))?;

    let mut out = Vec::with_capacity(12 + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);

    let tmp = seed_path.with_extension("enc.tmp");
    std::fs::write(&tmp, out)
        .map_err(|e| format!("write temp genesis seed {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod temp genesis seed {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, seed_path).map_err(|e| {
        format!(
            "rename genesis seed {} -> {}: {e}",
            tmp.display(),
            seed_path.display()
        )
    })?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(seed_path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod genesis seed {}: {e}", seed_path.display()))?;
    }
    key.zeroize();
    Ok(())
}

pub fn store_seed_once(seed: &[u8; 64], combined_entropy_sha256: &str) -> Result<(), String> {
    crate::halo::config::ensure_halo_dir()?;
    let wallet_path = crate::halo::config::pq_wallet_path();
    let seed_path = crate::halo::config::genesis_seed_path();
    store_seed_once_with_paths(&wallet_path, &seed_path, seed, combined_entropy_sha256)
}

fn load_seed_payload_with_paths(
    wallet_path: &std::path::Path,
    seed_path: &std::path::Path,
) -> Result<Option<StoredGenesisSeed>, String> {
    if !seed_path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(seed_path)
        .map_err(|e| format!("read genesis seed {}: {e}", seed_path.display()))?;
    if raw.len() <= 12 {
        return Err(format!("genesis seed {} is truncated", seed_path.display()));
    }
    let mut key = derive_seed_key(wallet_path)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&raw[..12]), &raw[12..])
        .map_err(|e| format!("decrypt genesis seed {}: {e}", seed_path.display()))?;
    let payload: StoredGenesisSeed = serde_json::from_slice(&plaintext)
        .map_err(|e| format!("parse genesis seed {}: {e}", seed_path.display()))?;
    if payload.schema != "agenthalo.genesis.seed.v1" {
        return Err(format!(
            "unsupported genesis seed schema {}",
            payload.schema
        ));
    }
    key.zeroize();
    Ok(Some(payload))
}

fn load_seed_sha256_with_paths(
    wallet_path: &std::path::Path,
    seed_path: &std::path::Path,
) -> Result<Option<String>, String> {
    Ok(load_seed_payload_with_paths(wallet_path, seed_path)?
        .map(|payload| payload.combined_entropy_sha256))
}

pub fn load_seed_sha256() -> Result<Option<String>, String> {
    let wallet_path = crate::halo::config::pq_wallet_path();
    let seed_path = crate::halo::config::genesis_seed_path();
    load_seed_sha256_with_paths(&wallet_path, &seed_path)
}

pub fn decrypt_legacy_seed_payload(
    wallet_path: &std::path::Path,
    seed_path: &std::path::Path,
) -> Result<Vec<u8>, String> {
    let raw = std::fs::read(seed_path)
        .map_err(|e| format!("read genesis seed {}: {e}", seed_path.display()))?;
    if raw.len() <= 12 {
        return Err(format!("genesis seed {} is truncated", seed_path.display()));
    }
    let mut key = derive_seed_key(wallet_path)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&raw[..12]), &raw[12..])
        .map_err(|e| format!("decrypt genesis seed {}: {e}", seed_path.display()))?;
    key.zeroize();
    Ok(plaintext)
}

fn load_seed_bytes_with_paths(
    wallet_path: &std::path::Path,
    seed_path: &std::path::Path,
) -> Result<Option<[u8; 64]>, String> {
    let Some(payload) = load_seed_payload_with_paths(wallet_path, seed_path)? else {
        return Ok(None);
    };
    let bytes = crate::halo::util::hex_decode(&payload.combined_entropy_hex)?;
    if bytes.len() != 64 {
        return Err(format!(
            "genesis seed payload has invalid byte length: expected 64, got {}",
            bytes.len()
        ));
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(&bytes);
    Ok(Some(out))
}

pub fn load_seed_bytes() -> Result<Option<[u8; 64]>, String> {
    let wallet_path = crate::halo::config::pq_wallet_path();
    let seed_path = crate::halo::config::genesis_seed_path();
    load_seed_bytes_with_paths(&wallet_path, &seed_path)
}

/// Load genesis seed bytes via v2 encrypted file, using the provided Genesis scope key.
/// This is the post-migration path: genesis_seed.v2.enc contains the seed payload
/// encrypted with a password-derived scope key, bypassing the erased wrap key entirely.
pub fn load_seed_bytes_v2(genesis_scope_key: &[u8; 32]) -> Result<Option<[u8; 64]>, String> {
    let v2_path = crate::halo::config::genesis_seed_v2_path();
    if !v2_path.exists() {
        return Ok(None);
    }
    let file = crate::halo::encrypted_file::EncryptedFileV2::load(&v2_path)?;
    let plaintext = file.decrypt(genesis_scope_key)?;
    let payload: StoredGenesisSeed = serde_json::from_slice(&plaintext)
        .map_err(|e| format!("parse v2 genesis seed: {e}"))?;
    if payload.schema != "agenthalo.genesis.seed.v1" {
        return Err(format!(
            "unsupported genesis seed schema {}",
            payload.schema
        ));
    }
    let bytes = crate::halo::util::hex_decode(&payload.combined_entropy_hex)?;
    if bytes.len() != 64 {
        return Err(format!(
            "v2 genesis seed has invalid byte length: expected 64, got {}",
            bytes.len()
        ));
    }
    let mut out = [0u8; 64];
    out.copy_from_slice(&bytes);
    Ok(Some(out))
}

/// Load genesis seed SHA-256 hash via v2 encrypted file.
pub fn load_seed_sha256_v2(genesis_scope_key: &[u8; 32]) -> Result<Option<String>, String> {
    let v2_path = crate::halo::config::genesis_seed_v2_path();
    if !v2_path.exists() {
        return Ok(None);
    }
    let file = crate::halo::encrypted_file::EncryptedFileV2::load(&v2_path)?;
    let plaintext = file.decrypt(genesis_scope_key)?;
    let payload: StoredGenesisSeed = serde_json::from_slice(&plaintext)
        .map_err(|e| format!("parse v2 genesis seed: {e}"))?;
    Ok(Some(payload.combined_entropy_sha256))
}

/// Derive wallet entropy via v2 path.
pub fn derive_wallet_entropy32_v2(
    genesis_scope_key: &[u8; 32],
) -> Result<Option<[u8; 32]>, String> {
    let Some(seed) = load_seed_bytes_v2(genesis_scope_key)? else {
        return Ok(None);
    };
    Ok(Some(derive_wallet_entropy32_from_seed(&seed)?))
}

/// Derive wallet mnemonic via v2 path.
pub fn derive_wallet_mnemonic_v2(
    genesis_scope_key: &[u8; 32],
) -> Result<Option<String>, String> {
    let Some(entropy) = derive_wallet_entropy32_v2(genesis_scope_key)? else {
        return Ok(None);
    };
    let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
        .map_err(|e| format!("derive wallet mnemonic from v2 genesis: {e}"))?;
    Ok(Some(mnemonic.to_string()))
}

/// Try v2 path first (with scope key), fall back to v1 path.
/// This is the preferred entry point post-E1-fix.
pub fn derive_wallet_mnemonic_prefer_v2(
    genesis_scope_key: Option<&[u8; 32]>,
) -> Result<Option<String>, String> {
    if let Some(key) = genesis_scope_key {
        if let Some(mnemonic) = derive_wallet_mnemonic_v2(key)? {
            return Ok(Some(mnemonic));
        }
    }
    derive_wallet_mnemonic()
}

/// Try v2 path first for seed SHA-256 hash.
pub fn load_seed_sha256_prefer_v2(
    genesis_scope_key: Option<&[u8; 32]>,
) -> Result<Option<String>, String> {
    if let Some(key) = genesis_scope_key {
        if let Some(hash) = load_seed_sha256_v2(key)? {
            return Ok(Some(hash));
        }
    }
    load_seed_sha256()
}

fn derive_wallet_entropy32_from_seed(seed: &[u8; 64]) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(
        Some(b"agenthalo-genesis-wallet-entropy-v1"),
        seed.as_slice(),
    );
    let mut out = [0u8; 32];
    hk.expand(b"bip39-entropy-32", &mut out)
        .map_err(|_| "wallet entropy HKDF expand failed".to_string())?;
    Ok(out)
}

/// Public entry point for wallet entropy derivation from a raw 64-byte genesis seed.
/// Used by the sovereign binding ceremony which already holds the seed in memory.
pub fn derive_wallet_entropy32_from_seed_public(seed: &[u8; 64]) -> Result<[u8; 32], String> {
    derive_wallet_entropy32_from_seed(seed)
}

/// Derive deterministic identity key material from the genesis seed.
/// The output is suitable as Ed25519 secret-key bytes.
/// T5: genesis_derivation_deterministic
pub fn derive_p2p_identity(seed: &[u8; 64]) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-genesis-identity-v1"), seed.as_slice());
    let mut out = [0u8; 32];
    hk.expand(b"agenthalo-p2p-identity-v1", &mut out)
        .expect("HKDF expand should succeed for fixed 32-byte output");
    out
}

/// Derive deterministic DIDComm agreement material from the genesis seed.
/// Returns `(x25519_secret_bytes, mlkem768_seed_bytes)`.
pub fn derive_did_agreement_keys(seed: &[u8; 64]) -> ([u8; 32], [u8; 64]) {
    let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-genesis-identity-v1"), seed.as_slice());
    let mut x25519 = [0u8; 32];
    hk.expand(b"agenthalo-didcomm-x25519-v1", &mut x25519)
        .expect("HKDF expand should succeed for fixed 32-byte output");
    let mut mlkem768 = [0u8; 64];
    hk.expand(b"agenthalo-didcomm-mlkem768-v1", &mut mlkem768)
        .expect("HKDF expand should succeed for fixed 64-byte output");
    (x25519, mlkem768)
}

pub fn derive_wallet_entropy32() -> Result<Option<[u8; 32]>, String> {
    let Some(seed) = load_seed_bytes()? else {
        return Ok(None);
    };
    Ok(Some(derive_wallet_entropy32_from_seed(&seed)?))
}

pub fn derive_wallet_mnemonic() -> Result<Option<String>, String> {
    let Some(entropy) = derive_wallet_entropy32()? else {
        return Ok(None);
    };
    let mnemonic = Mnemonic::from_entropy_in(Language::English, &entropy)
        .map_err(|e| format!("derive wallet mnemonic from genesis entropy: {e}"))?;
    Ok(Some(mnemonic.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn lock() -> &'static Mutex<()> {
        static L: OnceLock<Mutex<()>> = OnceLock::new();
        L.get_or_init(|| Mutex::new(()))
    }

    fn make_tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "genesis_seed_paths_{}_{}_{}",
            tag,
            std::process::id(),
            now_unix()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp home");
        dir
    }

    #[test]
    fn store_then_load_seed_sha256() {
        let _g = lock().lock().expect("lock");
        let dir = make_tmp_dir("roundtrip");
        let wallet_path = dir.join("pq_wallet.json");
        let signatures_dir = dir.join("signatures");
        let seed_path = dir.join("genesis_seed.enc");

        let paths = crate::halo::pq::PqStoragePaths {
            wallet_path: wallet_path.clone(),
            signatures_dir,
        };
        crate::halo::pq::keygen_pq_with_paths(&paths, true).expect("create pq wallet");

        let mut seed = [0u8; 64];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = i as u8;
        }
        let digest = "sha256:test_digest";
        store_seed_once_with_paths(&wallet_path, &seed_path, &seed, digest).expect("store seed");
        let got = load_seed_sha256_with_paths(&wallet_path, &seed_path).expect("load seed");
        assert_eq!(got.as_deref(), Some(digest));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn derive_wallet_entropy_and_mnemonic_are_stable() {
        let _g = lock().lock().expect("lock");
        let dir = make_tmp_dir("wallet_entropy");
        let wallet_path = dir.join("pq_wallet.json");
        let signatures_dir = dir.join("signatures");
        let seed_path = dir.join("genesis_seed.enc");

        let paths = crate::halo::pq::PqStoragePaths {
            wallet_path: wallet_path.clone(),
            signatures_dir,
        };
        crate::halo::pq::keygen_pq_with_paths(&paths, true).expect("create pq wallet");

        let mut seed = [0u8; 64];
        for (i, b) in seed.iter_mut().enumerate() {
            *b = (255 - i) as u8;
        }
        store_seed_once_with_paths(&wallet_path, &seed_path, &seed, "sha256:seed")
            .expect("store seed");

        let e1 = derive_wallet_entropy32_from_seed(&seed).expect("derive entropy");
        let e2 = derive_wallet_entropy32_from_seed(&seed).expect("derive entropy repeat");
        assert_eq!(e1, e2, "wallet entropy derivation must be deterministic");

        let stored = load_seed_bytes_with_paths(&wallet_path, &seed_path)
            .expect("load stored seed")
            .expect("seed exists");
        assert_eq!(stored, seed);

        let mnemonic =
            Mnemonic::from_entropy_in(Language::English, &e1).expect("mnemonic conversion");
        let phrase = mnemonic.to_string();
        assert_eq!(phrase.split_whitespace().count(), 24);

        let _ = std::fs::remove_dir_all(dir);
    }
}
