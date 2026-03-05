use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

const WDK_PORT: u16 = 7321;
const WDK_SIDECAR_DIR: &str = "wdk-sidecar";
const WDK_SIDECAR_DIR_ENV: &str = "WDK_SIDECAR_DIR";
const WDK_AUTH_TOKEN_ENV: &str = "WDK_AUTH_TOKEN";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedSeed {
    pub nonce: String,
    pub ciphertext: String,
    pub salt: String,
    #[serde(default)]
    pub kdf: Option<String>,
    #[serde(default)]
    pub kdf_memory_kib: Option<u32>,
    #[serde(default)]
    pub kdf_iterations: Option<u32>,
    #[serde(default)]
    pub kdf_parallelism: Option<u32>,
    pub created_at: String,
    pub chains: Vec<String>,
}

pub struct WdkManager {
    child: Option<Child>,
    port: u16,
    auth_token: String,
}

impl WdkManager {
    pub fn new() -> Self {
        let auth_token = std::env::var(WDK_AUTH_TOKEN_ENV)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                let mut token = [0u8; 32];
                OsRng.fill_bytes(&mut token);
                hex::encode(token)
            });
        Self {
            child: None,
            port: WDK_PORT,
            auth_token,
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
        if let Ok(raw) = std::env::var(WDK_SIDECAR_DIR_ENV) {
            let candidate = PathBuf::from(raw.trim());
            if candidate.join("index.mjs").exists() {
                return candidate;
            }
        }
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

    fn auth_header_name() -> &'static str {
        "x-agenthalo-wdk-token"
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        if self.child.is_none() && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
            return Err(
                "WDK sidecar is already bound but authentication token mismatched; set WDK_AUTH_TOKEN to the sidecar token".to_string(),
            );
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
            .env("WDK_AUTH_TOKEN", &self.auth_token)
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
        ureq::get(&self.api_url("/status"))
            .header(Self::auth_header_name(), &self.auth_token)
            .call()
            .is_ok()
    }

    pub fn get(&self, path: &str) -> Result<Value, String> {
        let mut resp = ureq::get(&self.api_url(path))
            .header(Self::auth_header_name(), &self.auth_token)
            .call()
            .map_err(|e| format!("WDK GET {} failed: {}", path, sanitize_error(&e)))?;
        resp.body_mut()
            .read_json::<Value>()
            .map_err(|e| format!("WDK GET {} parse JSON: {e}", path))
    }

    pub fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        let mut resp = ureq::post(&self.api_url(path))
            .header(Self::auth_header_name(), &self.auth_token)
            .send_json(body)
            .map_err(|e| format!("WDK POST {} failed: {}", path, sanitize_error(&e)))?;
        resp.body_mut()
            .read_json::<Value>()
            .map_err(|e| format!("WDK POST {} parse JSON: {e}", path))
    }
}

impl Default for WdkManager {
    fn default() -> Self {
        Self::new()
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

const ARGON2_MEMORY_KIB: u32 = 64 * 1024;
const ARGON2_ITERATIONS: u32 = 3;
const ARGON2_PARALLELISM: u32 = 1;
const WDK_KDF_ARGON2ID_V1: &str = "argon2id-v1";
const WDK_KDF_HKDF_LEGACY: &str = "hkdf-v1";

fn derive_key_argon2(passphrase: &[u8], salt: &[u8]) -> Result<[u8; 32], String> {
    let params = Params::new(
        ARGON2_MEMORY_KIB,
        ARGON2_ITERATIONS,
        ARGON2_PARALLELISM,
        Some(32),
    )
    .map_err(|e| format!("argon2 params: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(passphrase, salt, &mut key)
        .map_err(|e| format!("argon2 derive failed: {e}"))?;
    Ok(key)
}

fn derive_key_hkdf_legacy(passphrase: &[u8], salt: &[u8]) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(Some(salt), passphrase);
    let mut key = [0u8; 32];
    hk.expand(b"agenthalo.wdk.seed.v1", &mut key)
        .map_err(|e| format!("legacy HKDF expand failed: {e}"))?;
    Ok(key)
}

fn try_decrypt_with_key(
    key: &[u8; 32],
    nonce_bytes: &[u8],
    ciphertext: &[u8],
) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| format!("cipher init: {e}"))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "decryption failed — wrong passphrase or corrupted wallet".to_string())?;
    String::from_utf8(plaintext).map_err(|e| format!("seed utf8 decode: {e}"))
}

pub fn encrypt_seed(seed: &str, passphrase: &str) -> Result<EncryptedSeed, String> {
    if passphrase.len() < 8 {
        return Err("passphrase must be at least 8 characters".to_string());
    }
    let mut salt = [0u8; 32];
    OsRng.fill_bytes(&mut salt);
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);

    let mut key = derive_key_argon2(passphrase.as_bytes(), &salt)?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, seed.as_bytes())
        .map_err(|e| format!("seed encryption failed: {e}"))?;

    let out = EncryptedSeed {
        nonce: hex::encode(nonce_bytes),
        ciphertext: hex::encode(ciphertext),
        salt: hex::encode(salt),
        kdf: Some(WDK_KDF_ARGON2ID_V1.to_string()),
        kdf_memory_kib: Some(ARGON2_MEMORY_KIB),
        kdf_iterations: Some(ARGON2_ITERATIONS),
        kdf_parallelism: Some(ARGON2_PARALLELISM),
        created_at: chrono::Utc::now().to_rfc3339(),
        chains: vec![
            "bitcoin".to_string(),
            "ethereum".to_string(),
            "polygon".to_string(),
            "arbitrum".to_string(),
        ],
    };
    key.zeroize();
    Ok(out)
}

pub fn decrypt_seed(encrypted: &EncryptedSeed, passphrase: &str) -> Result<String, String> {
    let salt = hex::decode(&encrypted.salt).map_err(|e| format!("salt decode: {e}"))?;
    let nonce_bytes = hex::decode(&encrypted.nonce).map_err(|e| format!("nonce decode: {e}"))?;
    let ciphertext =
        hex::decode(&encrypted.ciphertext).map_err(|e| format!("ciphertext decode: {e}"))?;

    // Default to Argon2id for current files; keep HKDF fallback for legacy wallets.
    let use_legacy_first = encrypted.kdf.as_deref() == Some(WDK_KDF_HKDF_LEGACY);
    let mut attempts: Vec<[u8; 32]> = Vec::new();
    if use_legacy_first {
        attempts.push(derive_key_hkdf_legacy(passphrase.as_bytes(), &salt)?);
        attempts.push(derive_key_argon2(passphrase.as_bytes(), &salt)?);
    } else {
        attempts.push(derive_key_argon2(passphrase.as_bytes(), &salt)?);
        attempts.push(derive_key_hkdf_legacy(passphrase.as_bytes(), &salt)?);
    }

    let mut last_err = None;
    for mut key in attempts {
        match try_decrypt_with_key(&key, &nonce_bytes, &ciphertext) {
            Ok(seed) => {
                key.zeroize();
                return Ok(seed);
            }
            Err(e) => {
                key.zeroize();
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| "decryption failed".to_string()))
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

    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&tmp)
        .map_err(|e| format!("open encrypted seed tmp {}: {e}", tmp.display()))?;
    file.write_all(&json)
        .map_err(|e| format!("write encrypted seed tmp: {e}"))?;
    file.flush()
        .map_err(|e| format!("flush encrypted seed tmp: {e}"))?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("set encrypted seed tmp permissions: {e}"))?;
    }

    std::fs::rename(&tmp, &path).map_err(|e| format!("commit encrypted seed file: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("set encrypted seed permissions: {e}"))?;
    }
    Ok(())
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
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

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

    #[test]
    fn legacy_hkdf_decrypt_still_supported() {
        let mut salt = [0u8; 32];
        OsRng.fill_bytes(&mut salt);
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let key = derive_key_hkdf_legacy(b"passphrase123", &salt).expect("legacy key");
        let cipher = Aes256Gcm::new_from_slice(&key).expect("cipher");
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(
                nonce,
                b"abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".as_slice(),
            )
            .expect("encrypt");
        let legacy = EncryptedSeed {
            nonce: hex::encode(nonce_bytes),
            ciphertext: hex::encode(ciphertext),
            salt: hex::encode(salt),
            kdf: Some(WDK_KDF_HKDF_LEGACY.to_string()),
            kdf_memory_kib: None,
            kdf_iterations: None,
            kdf_parallelism: None,
            created_at: chrono::Utc::now().to_rfc3339(),
            chains: vec!["bitcoin".to_string()],
        };
        let dec = decrypt_seed(&legacy, "passphrase123").expect("legacy decrypt");
        assert!(dec.starts_with("abandon abandon"));
    }

    #[test]
    fn new_uses_wdk_auth_token_env_when_set() {
        let _guard = env_lock().lock().expect("lock env");
        std::env::set_var(WDK_AUTH_TOKEN_ENV, "container-shared-token");
        let mgr = WdkManager::new();
        assert_eq!(mgr.auth_token, "container-shared-token");
        std::env::remove_var(WDK_AUTH_TOKEN_ENV);
    }

    #[test]
    fn sidecar_dir_prefers_env_override() {
        let _guard = env_lock().lock().expect("lock env");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("wdk-sidecar-test-{stamp}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("node_modules")).expect("create sidecar dir");
        std::fs::write(path.join("index.mjs"), "console.log('ok');").expect("write index");
        std::env::set_var(WDK_SIDECAR_DIR_ENV, path.display().to_string());
        let resolved: PathBuf = WdkManager::sidecar_dir();
        assert_eq!(resolved, path);
        std::env::remove_var(WDK_SIDECAR_DIR_ENV);
        let _ = std::fs::remove_dir_all(path);
    }
}
