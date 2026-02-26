use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub fn open(pq_wallet_path: &Path, vault_path: &Path) -> Result<Vault, String> {
        let (key_id, master_key) = derive_master_key(pq_wallet_path)?;
        Ok(Vault {
            path: vault_path.to_path_buf(),
            master_key,
            key_id,
        })
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
                set_at: now_unix(),
                tested: false,
                tested_at: None,
            },
        );

        self.save_data(&data)
    }

    pub fn set_test_result(&self, provider: &str, tested: bool) -> Result<(), String> {
        let provider = normalize_provider(provider);
        let mut data = self.load_data()?;
        let entry = data
            .providers
            .get_mut(&provider)
            .ok_or_else(|| format!("no API key configured for {provider}"))?;
        entry.tested = tested;
        entry.tested_at = Some(now_unix());
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
                if let Some(entry) = data.providers.get(provider) {
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

    pub fn env_vars_for_providers(
        &self,
        providers: &[&str],
    ) -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        for provider in providers {
            let provider_norm = normalize_provider(provider);
            let key = self.get_key(&provider_norm)?;
            let env_var = self
                .load_data()?
                .providers
                .get(&provider_norm)
                .map(|p| p.env_var.clone())
                .unwrap_or_else(|| provider_default_env_var(&provider_norm));
            out.push((env_var, key));
        }
        Ok(out)
    }

    pub fn path(&self) -> &Path {
        &self.path
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

        if data.key_id != self.key_id {
            return Err(format!(
                "vault key_id mismatch (wallet key rotated?): vault={}, wallet={}",
                data.key_id, self.key_id
            ));
        }
        if data.version != 1 {
            return Err(format!("unsupported vault version {}", data.version));
        }
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

        let mut payload = Vec::with_capacity(12 + ciphertext.len());
        payload.extend_from_slice(&file_nonce);
        payload.extend_from_slice(&ciphertext);

        let tmp = self.path.with_extension("enc.tmp");
        std::fs::write(&tmp, payload)
            .map_err(|e| format!("write temp vault {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            format!(
                "rename vault {} -> {}: {e}",
                tmp.display(),
                self.path.display()
            )
        })
    }
}

fn derive_master_key(pq_wallet_path: &Path) -> Result<(String, [u8; 32]), String> {
    let raw = std::fs::read_to_string(pq_wallet_path)
        .map_err(|e| format!("read wallet {}: {e}", pq_wallet_path.display()))?;
    let wallet: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| format!("parse wallet {}: {e}", pq_wallet_path.display()))?;

    let key_id = wallet
        .get("key_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "wallet missing key_id".to_string())?
        .to_string();
    let seed_hex = wallet
        .get("secret_seed_hex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "wallet missing secret_seed_hex".to_string())?;

    let seed_bytes = hex_decode(seed_hex)?;
    let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-vault-v1"), &seed_bytes);
    let mut out = [0u8; 32];
    hk.expand(b"aes-master", &mut out)
        .map_err(|_| "hkdf expand failed".to_string())?;
    Ok((key_id, out))
}

pub fn provider_default_env_var(provider: &str) -> String {
    match normalize_provider(provider).as_str() {
        "anthropic" => "ANTHROPIC_API_KEY".to_string(),
        "openai" => "OPENAI_API_KEY".to_string(),
        "google" => "GOOGLE_API_KEY".to_string(),
        "openclaw" => "OPENAI_API_KEY".to_string(),
        "custom_1" => "CUSTOM_1_API_KEY".to_string(),
        "custom_2" => "CUSTOM_2_API_KEY".to_string(),
        "custom_3" => "CUSTOM_3_API_KEY".to_string(),
        "custom_4" => "CUSTOM_4_API_KEY".to_string(),
        "custom_5" => "CUSTOM_5_API_KEY".to_string(),
        other => format!("{}_API_KEY", other.to_ascii_uppercase()),
    }
}

fn known_providers() -> Vec<&'static str> {
    vec![
        "anthropic",
        "openai",
        "google",
        "openclaw",
        "custom_1",
        "custom_2",
        "custom_3",
        "custom_4",
        "custom_5",
    ]
}

fn normalize_provider(provider: &str) -> String {
    provider.trim().to_ascii_lowercase()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn hex_decode(input: &str) -> Result<Vec<u8>, String> {
    let s = input.trim();
    if s.is_empty() || !s.len().is_multiple_of(2) {
        return Err("hex string must have even length".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for pair in s.as_bytes().chunks_exact(2) {
        let hi = hex_nibble(pair[0]).ok_or_else(|| "invalid hex".to_string())?;
        let lo = hex_nibble(pair[1]).ok_or_else(|| "invalid hex".to_string())?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "vault_test_{}_{}_{}",
            tag,
            std::process::id(),
            now_unix()
        ))
    }

    fn make_wallet(path: &Path, key_id: &str, seed_hex: &str) {
        let wallet = serde_json::json!({
            "version": 1,
            "algorithm": "ml_dsa65",
            "key_id": key_id,
            "public_key_hex": "00",
            "secret_seed_hex": seed_hex,
            "created_at": now_unix(),
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, serde_json::to_vec_pretty(&wallet).unwrap()).unwrap();
    }

    #[test]
    fn roundtrip_set_get_delete_key() {
        let wallet_path = temp_path("wallet1.json");
        let vault_path = temp_path("vault1.enc");
        make_wallet(
            &wallet_path,
            "kid-1",
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        );

        let vault = Vault::open(&wallet_path, &vault_path).expect("open vault");
        vault
            .set_key("openai", "OPENAI_API_KEY", "sk-test-123")
            .expect("set key");

        let got = vault.get_key("openai").expect("get key");
        assert_eq!(got, "sk-test-123");

        let listed = vault.list_keys().expect("list keys");
        let openai = listed
            .iter()
            .find(|s| s.provider == "openai")
            .expect("openai status exists");
        assert!(openai.configured);
        assert_eq!(openai.env_var, "OPENAI_API_KEY");

        vault.delete_key("openai").expect("delete key");
        assert!(vault.get_key("openai").is_err());

        let _ = std::fs::remove_file(wallet_path);
        let _ = std::fs::remove_file(vault_path);
    }

    #[test]
    fn bad_master_key_fails_decrypt() {
        let wallet_a = temp_path("wallet-a.json");
        let wallet_b = temp_path("wallet-b.json");
        let vault_path = temp_path("vault2.enc");

        make_wallet(
            &wallet_a,
            "same-key-id",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        make_wallet(
            &wallet_b,
            "same-key-id",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );

        let v1 = Vault::open(&wallet_a, &vault_path).expect("open v1");
        v1.set_key("anthropic", "ANTHROPIC_API_KEY", "sk-ant-test")
            .expect("set key v1");

        let v2 = Vault::open(&wallet_b, &vault_path).expect("open v2");
        let err = v2.list_keys().expect_err("decrypt should fail");
        assert!(err.contains("decrypt vault file"));

        let _ = std::fs::remove_file(wallet_a);
        let _ = std::fs::remove_file(wallet_b);
        let _ = std::fs::remove_file(vault_path);
    }

    #[test]
    fn wallet_rotation_detected_by_key_id() {
        let wallet_a = temp_path("wallet-c.json");
        let wallet_b = temp_path("wallet-d.json");
        let vault_path = temp_path("vault3.enc");

        make_wallet(
            &wallet_a,
            "kid-a",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        );
        make_wallet(
            &wallet_b,
            "kid-b",
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        );

        let v1 = Vault::open(&wallet_a, &vault_path).expect("open v1");
        v1.set_key("google", "GOOGLE_API_KEY", "g-test")
            .expect("set key v1");

        let v2 = Vault::open(&wallet_b, &vault_path).expect("open v2");
        let err = v2.list_keys().expect_err("key_id mismatch expected");
        assert!(err.contains("vault key_id mismatch"));

        let _ = std::fs::remove_file(wallet_a);
        let _ = std::fs::remove_file(wallet_b);
        let _ = std::fs::remove_file(vault_path);
    }
}
