use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Vault {
    path: PathBuf,
    master_key: [u8; 32],
    key_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultData {
    pub version: u8,
    pub key_id: String,
    pub providers: HashMap<String, ProviderKey>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderKey {
    pub provider: String,
    pub env_var: String,
    pub encrypted_key: Vec<u8>,
    pub nonce: [u8; 12],
    pub set_at: u64,
    pub tested: bool,
    pub tested_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct KeyStatus {
    pub provider: String,
    pub env_var: String,
    pub configured: bool,
    pub tested: bool,
    pub tested_at: Option<u64>,
    pub set_at: Option<u64>,
}

impl Vault {
    pub fn from_scope_key(scope_key: &[u8; 32], vault_path: &Path) -> Vault {
        let hk = Hkdf::<Sha256>::new(Some(b"nucleusdb-vault-v1"), scope_key);
        let mut master_key = [0u8; 32];
        hk.expand(b"aes-master", &mut master_key).unwrap();
        let key_id = format!(
            "scope-{:02x}{:02x}{:02x}{:02x}",
            scope_key[0], scope_key[1], scope_key[2], scope_key[3]
        );
        Vault {
            path: vault_path.to_path_buf(),
            master_key,
            key_id,
        }
    }

    pub fn set_key(&self, provider: &str, env_var: &str, raw_key: &str) -> Result<(), String> {
        let provider = normalize_provider(provider);
        if raw_key.trim().is_empty() {
            return Err("key must not be empty".to_string());
        }
        let mut data = self.load_data()?;
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| format!("vault cipher init: {e}"))?;
        let encrypted_key = cipher
            .encrypt(Nonce::from_slice(&nonce), raw_key.as_bytes())
            .map_err(|e| format!("encrypt provider key: {e}"))?;
        data.providers.insert(
            provider.clone(),
            ProviderKey {
                provider,
                env_var: env_var.to_string(),
                encrypted_key,
                nonce,
                set_at: crate::util::now_unix_secs(),
                tested: false,
                tested_at: None,
            },
        );
        self.save_data(&data)
    }

    pub fn get_key(&self, provider: &str) -> Result<String, String> {
        let provider = normalize_provider(provider);
        let data = self.load_data()?;
        let entry = data
            .providers
            .get(&provider)
            .ok_or_else(|| format!("no API key configured for {provider}"))?;
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| format!("vault cipher init: {e}"))?;
        let raw = cipher
            .decrypt(
                Nonce::from_slice(&entry.nonce),
                entry.encrypted_key.as_ref(),
            )
            .map_err(|e| format!("decrypt provider key: {e}"))?;
        String::from_utf8(raw).map_err(|e| format!("provider key UTF-8: {e}"))
    }

    pub fn delete_key(&self, provider: &str) -> Result<(), String> {
        let provider = normalize_provider(provider);
        let mut data = self.load_data()?;
        data.providers.remove(&provider);
        self.save_data(&data)
    }

    pub fn list_keys(&self) -> Result<Vec<KeyStatus>, String> {
        let data = self.load_data()?;
        let mut statuses: Vec<KeyStatus> = known_providers()
            .into_iter()
            .map(|provider| {
                if let Some(entry) = data.providers.get(*provider) {
                    KeyStatus {
                        provider: provider.to_string(),
                        env_var: entry.env_var.clone(),
                        configured: true,
                        tested: entry.tested,
                        tested_at: entry.tested_at,
                        set_at: Some(entry.set_at),
                    }
                } else {
                    KeyStatus {
                        provider: provider.to_string(),
                        env_var: provider_default_env_var(provider),
                        configured: false,
                        tested: false,
                        tested_at: None,
                        set_at: None,
                    }
                }
            })
            .collect();
        statuses.sort_by(|a, b| a.provider.cmp(&b.provider));
        Ok(statuses)
    }

    fn load_data(&self) -> Result<VaultData, String> {
        if !self.path.exists() {
            return Ok(VaultData {
                version: 1,
                key_id: self.key_id.clone(),
                providers: HashMap::new(),
            });
        }
        let raw = std::fs::read(&self.path)
            .map_err(|e| format!("read vault {}: {e}", self.path.display()))?;
        if raw.len() <= 12 {
            return Err(format!("vault {} is truncated", self.path.display()));
        }
        let nonce = Nonce::from_slice(&raw[..12]);
        let ciphertext = &raw[12..];
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| format!("vault cipher init: {e}"))?;
        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| format!("decrypt vault file {}: {e}", self.path.display()))?;
        let data: VaultData = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("parse vault JSON {}: {e}", self.path.display()))?;
        Ok(data)
    }

    fn save_data(&self, data: &VaultData) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create vault dir {}: {e}", parent.display()))?;
        }
        let plaintext =
            serde_json::to_vec(data).map_err(|e| format!("serialize vault JSON: {e}"))?;
        let mut file_nonce = [0u8; 12];
        OsRng.fill_bytes(&mut file_nonce);
        let cipher = Aes256Gcm::new_from_slice(&self.master_key)
            .map_err(|e| format!("vault cipher init: {e}"))?;
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&file_nonce), plaintext.as_ref())
            .map_err(|e| format!("encrypt vault file: {e}"))?;
        let mut out = Vec::with_capacity(12 + ciphertext.len());
        out.extend_from_slice(&file_nonce);
        out.extend_from_slice(&ciphertext);
        std::fs::write(&self.path, out)
            .map_err(|e| format!("write vault {}: {e}", self.path.display()))
    }
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_ascii_lowercase()
}
fn provider_default_env_var(provider: &str) -> String {
    format!("{}_API_KEY", provider.to_ascii_uppercase())
}
fn known_providers() -> &'static [&'static str] {
    &["anthropic", "openai", "google", "discord"]
}
