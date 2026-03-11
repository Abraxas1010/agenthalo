use crate::halo::config;
use crate::halo::crypto_scope::CryptoScope;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub const SCHEMA_V2: &str = "agenthalo.encrypted.v2";
pub const CRYPTO_HEADER_SCHEMA: &str = "agenthalo.crypto-header.v1";

const ARGON2_MEMORY_KIB: u32 = 128 * 1024;
const ARGON2_ITERATIONS: u32 = 4;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EncryptedFileV2 {
    pub schema: String,
    pub scope: String,
    pub kdf: KdfParams,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_credentials: Vec<AgentEncapsulation>,
    pub nonce_hex: String,
    pub ciphertext_hex: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KdfParams {
    pub algorithm: String,
    pub salt_hex: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentEncapsulation {
    pub agent_id: String,
    pub label: String,
    pub algorithm: String,
    pub encapsulated_key_hex: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CryptoHeader {
    pub schema: String,
    pub kdf: KdfParams,
    pub created_at: u64,
    pub password_protected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_verifier_hex: Option<String>,
}

impl KdfParams {
    pub fn random_v2() -> Self {
        let mut salt = [0u8; 32];
        OsRng.fill_bytes(&mut salt);
        Self {
            algorithm: "argon2id-v2".to_string(),
            salt_hex: hex::encode(salt),
            memory_kib: ARGON2_MEMORY_KIB,
            iterations: ARGON2_ITERATIONS,
            parallelism: ARGON2_PARALLELISM,
        }
    }

    pub fn derive_master_key(&self, password: &str) -> Result<[u8; 32], String> {
        if self.algorithm != "argon2id-v2" {
            return Err(format!("unsupported KDF algorithm: {}", self.algorithm));
        }
        let salt = hex::decode(&self.salt_hex).map_err(|e| format!("kdf salt decode: {e}"))?;
        let params = Params::new(self.memory_kib, self.iterations, self.parallelism, Some(32))
            .map_err(|e| format!("argon2 params: {e}"))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = [0u8; 32];
        argon2
            .hash_password_into(password.as_bytes(), &salt, &mut out)
            .map_err(|e| format!("argon2 derive: {e}"))?;
        Ok(out)
    }
}

impl EncryptedFileV2 {
    pub fn encrypt(
        plaintext: &[u8],
        scope_key: &[u8; 32],
        scope: CryptoScope,
        kdf_params: &KdfParams,
    ) -> Result<Self, String> {
        let now = now_unix();
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let cipher =
            Aes256Gcm::new_from_slice(scope_key).map_err(|e| format!("cipher init: {e}"))?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce), plaintext)
            .map_err(|e| format!("encrypt {}: {e}", scope.as_str()))?;
        Ok(Self {
            schema: SCHEMA_V2.to_string(),
            scope: scope.as_str().to_string(),
            kdf: kdf_params.clone(),
            agent_credentials: Vec::new(),
            nonce_hex: hex::encode(nonce),
            ciphertext_hex: hex::encode(ciphertext),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn decrypt(&self, scope_key: &[u8; 32]) -> Result<Vec<u8>, String> {
        if self.schema != SCHEMA_V2 {
            return Err(format!("unsupported encrypted schema: {}", self.schema));
        }
        let nonce = hex::decode(&self.nonce_hex).map_err(|e| format!("nonce decode: {e}"))?;
        let ciphertext =
            hex::decode(&self.ciphertext_hex).map_err(|e| format!("ciphertext decode: {e}"))?;
        let cipher =
            Aes256Gcm::new_from_slice(scope_key).map_err(|e| format!("cipher init: {e}"))?;
        cipher
            .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
            .map_err(|_| "decrypt failed — wrong key or corrupted file".to_string())
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let raw = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        serde_json::from_slice(&raw).map_err(|e| format!("parse {}: {e}", path.display()))
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
        }
        let tmp = path.with_extension("tmp");
        let raw = serde_json::to_vec_pretty(self).map_err(|e| format!("serialize: {e}"))?;
        std::fs::write(&tmp, raw).map_err(|e| format!("write temp {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        {
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod temp {}: {e}", tmp.display()))?;
        }
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
        #[cfg(unix)]
        {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| format!("chmod {}: {e}", path.display()))?;
        }
        Ok(())
    }

    pub fn is_v2(path: &std::path::Path) -> bool {
        let Ok(raw) = std::fs::read(path) else {
            return false;
        };
        let Ok(v) = serde_json::from_slice::<serde_json::Value>(&raw) else {
            return false;
        };
        v.get("schema")
            .and_then(|s| s.as_str())
            .map(|s| s == SCHEMA_V2)
            .unwrap_or(false)
    }
}

pub fn header_exists() -> bool {
    config::crypto_header_path().exists()
}

pub fn load_header() -> Result<Option<CryptoHeader>, String> {
    let path = config::crypto_header_path();
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let header: CryptoHeader =
        serde_json::from_slice(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if header.schema != CRYPTO_HEADER_SCHEMA {
        return Err(format!(
            "unsupported crypto header schema {}",
            header.schema
        ));
    }
    Ok(Some(header))
}

pub fn save_header(header: &CryptoHeader) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let path = config::crypto_header_path();
    let tmp = path.with_extension("tmp");
    let raw = serde_json::to_vec_pretty(header).map_err(|e| format!("serialize header: {e}"))?;
    std::fs::write(&tmp, raw).map_err(|e| format!("write temp header {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod temp header {}: {e}", tmp.display()))?;
    }
    std::fs::rename(&tmp, &path)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    #[cfg(unix)]
    {
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod header {}: {e}", path.display()))?;
    }
    Ok(())
}

pub fn create_header_if_missing() -> Result<CryptoHeader, String> {
    if let Some(existing) = load_header()? {
        return Ok(existing);
    }
    let header = CryptoHeader {
        schema: CRYPTO_HEADER_SCHEMA.to_string(),
        kdf: KdfParams::random_v2(),
        created_at: now_unix(),
        password_protected: true,
        password_verifier_hex: None,
    };
    save_header(&header)?;
    Ok(header)
}

pub fn password_verifier_hex(master_key: &[u8; 32]) -> String {
    let mut h = Sha256::new();
    h.update(b"agenthalo.password.verifier.v1");
    h.update([0u8]);
    h.update(master_key);
    hex::encode(h.finalize())
}

pub fn verify_password_with_header(header: &CryptoHeader, master_key: &[u8; 32]) -> bool {
    match header.password_verifier_hex.as_deref() {
        Some(stored) => {
            let Ok(stored_bytes) = hex::decode(stored.trim()) else {
                return false;
            };
            if stored_bytes.len() != 32 {
                return false;
            }
            let expected = password_verifier_hex(master_key);
            let Ok(expected_bytes) = hex::decode(expected) else {
                return false;
            };
            stored_bytes
                .as_slice()
                .ct_eq(expected_bytes.as_slice())
                .into()
        }
        None => false,
    }
}

fn now_unix() -> u64 {
    crate::halo::util::now_unix_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt() {
        let kdf = KdfParams::random_v2();
        let key = [0x42u8; 32];
        let f = EncryptedFileV2::encrypt(b"hello", &key, CryptoScope::Vault, &kdf).expect("enc");
        let p = f.decrypt(&key).expect("dec");
        assert_eq!(p, b"hello");
    }

    #[test]
    fn decrypt_fails_with_wrong_key() {
        let kdf = KdfParams::random_v2();
        let key = [0x42u8; 32];
        let wrong = [0x24u8; 32];
        let f = EncryptedFileV2::encrypt(b"hello", &key, CryptoScope::Vault, &kdf).expect("enc");
        assert!(f.decrypt(&wrong).is_err());
    }
}
