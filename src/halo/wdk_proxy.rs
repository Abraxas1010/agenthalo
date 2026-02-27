use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const WDK_PORT: u16 = 7321;
const WDK_SIDECAR_DIR: &str = "wdk-sidecar";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSeed {
    pub nonce: String,
    pub ciphertext: String,
    pub salt: String,
    pub created_at: String,
    pub chains: Vec<String>,
}

pub struct WdkManager {
    child: Option<Child>,
    port: u16,
}

impl WdkManager {
    pub fn new() -> Self {
        Self {
            child: None,
            port: WDK_PORT,
        }
    }

    pub fn is_available() -> bool {
        let node_ok = Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !node_ok {
            return false;
        }

        let sidecar = Self::sidecar_dir();
        sidecar.join("index.mjs").exists() && sidecar.join("node_modules").exists()
    }

    fn sidecar_dir() -> PathBuf {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
        if let Some(dir) = exe_dir {
            let candidate = dir.join("..").join(WDK_SIDECAR_DIR);
            if candidate.join("index.mjs").exists() {
                return candidate;
            }
            let candidate = dir.join(WDK_SIDECAR_DIR);
            if candidate.join("index.mjs").exists() {
                return candidate;
            }
        }
        PathBuf::from(WDK_SIDECAR_DIR)
    }

    fn api_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        if self.child.is_some() {
            self.stop();
        }
        let sidecar_dir = Self::sidecar_dir();
        if !sidecar_dir.join("index.mjs").exists() {
            return Err(format!(
                "WDK sidecar missing at {} (run: cd wdk-sidecar && npm install)",
                sidecar_dir.display()
            ));
        }
        let child = Command::new("node")
            .arg("index.mjs")
            .current_dir(&sidecar_dir)
            .env("WDK_PORT", self.port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("start WDK sidecar: {e}"))?;
        self.child = Some(child);

        let started = Instant::now();
        let timeout = Duration::from_secs(10);
        while started.elapsed() < timeout {
            if self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        self.stop();
        Err("WDK sidecar did not become ready within 10s".to_string())
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn is_running(&self) -> bool {
        ureq::get(&self.api_url("/status")).call().is_ok()
    }

    pub fn get(&self, path: &str) -> Result<Value, String> {
        let mut resp = ureq::get(&self.api_url(path))
            .call()
            .map_err(|e| format!("WDK GET {} failed: {}", path, sanitize_error(&e)))?;
        resp.body_mut()
            .read_json::<Value>()
            .map_err(|e| format!("WDK GET {} parse JSON: {e}", path))
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let mut resp = ureq::post(&self.api_url(path))
            .send_json(body)
            .map_err(|e| format!("WDK POST {} failed: {}", path, sanitize_error(&e)))?;
        resp.body_mut()
            .read_json::<Value>()
            .map_err(|e| format!("WDK POST {} parse JSON: {e}", path))
    }
}

impl Drop for WdkManager {
    fn drop(&mut self) {
        self.stop();
    }
}

fn sanitize_error(err: &ureq::Error) -> String {
    let mut msg = err.to_string();
    if let Some((_, rest)) = msg.split_once('?') {
        let suffix = rest
            .split('&')
            .map(|part| {
                if part.to_ascii_lowercase().starts_with("key=") {
                    "key=<redacted>"
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join("&");
        msg = format!("{}?{}", msg.split('?').next().unwrap_or_default(), suffix);
    }
    msg
}

fn derive_key(passphrase: &[u8], salt: &[u8]) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(Some(salt), passphrase);
    let mut key = [0u8; 32];
    hk.expand(b"agenthalo.wdk.seed.v1", &mut key)
        .map_err(|e| format!("HKDF expand failed: {e}"))?;
    Ok(key)
}

pub fn encrypt_seed(seed: &str, passphrase: &str) -> Result<EncryptedSeed, String> {
    if passphrase.len() < 8 {
        return Err("passphrase must be at least 8 characters".to_string());
    }
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);

    let key = derive_key(passphrase.as_bytes(), &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, seed.as_bytes())
        .map_err(|e| format!("seed encryption failed: {e}"))?;

    Ok(EncryptedSeed {
        nonce: hex::encode(nonce_bytes),
        ciphertext: hex::encode(ciphertext),
        salt: hex::encode(salt),
        created_at: chrono::Utc::now().to_rfc3339(),
        chains: vec![
            "bitcoin".to_string(),
            "ethereum".to_string(),
            "polygon".to_string(),
            "arbitrum".to_string(),
        ],
    })
}

pub fn decrypt_seed(encrypted: &EncryptedSeed, passphrase: &str) -> Result<String, String> {
    let salt = hex::decode(&encrypted.salt).map_err(|e| format!("salt decode: {e}"))?;
    let nonce_bytes = hex::decode(&encrypted.nonce).map_err(|e| format!("nonce decode: {e}"))?;
    let ciphertext =
        hex::decode(&encrypted.ciphertext).map_err(|e| format!("ciphertext decode: {e}"))?;
    let key = derive_key(passphrase.as_bytes(), &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_slice())
        .map_err(|_| "decryption failed — wrong passphrase or corrupted wallet".to_string())?;
    String::from_utf8(plaintext).map_err(|e| format!("seed utf8 decode: {e}"))
}

pub fn encrypted_seed_path() -> PathBuf {
    crate::halo::config::halo_dir().join("wdk_seed.json")
}

pub fn save_encrypted_seed(enc: &EncryptedSeed) -> Result<(), String> {
    crate::halo::config::ensure_halo_dir()?;
    let path = encrypted_seed_path();
    let tmp = path.with_extension("tmp");
    let json =
        serde_json::to_vec_pretty(enc).map_err(|e| format!("serialize encrypted seed: {e}"))?;
    std::fs::write(&tmp, json).map_err(|e| format!("write encrypted seed tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("commit encrypted seed file: {e}"))
}

pub fn load_encrypted_seed() -> Option<EncryptedSeed> {
    let path = encrypted_seed_path();
    if !path.exists() {
        return None;
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn wallet_exists() -> bool {
    encrypted_seed_path().exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_encrypt_decrypt_roundtrip() {
        let enc = encrypt_seed(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "passphrase123",
        )
        .expect("encrypt");
        let dec = decrypt_seed(&enc, "passphrase123").expect("decrypt");
        assert!(dec.starts_with("abandon abandon"));
    }

    #[test]
    fn seed_wrong_passphrase_fails() {
        let enc = encrypt_seed(
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about",
            "passphrase123",
        )
        .expect("encrypt");
        let err = decrypt_seed(&enc, "wrong-pass").expect_err("expected decrypt failure");
        assert!(err.to_ascii_lowercase().contains("decryption failed"));
    }
}
