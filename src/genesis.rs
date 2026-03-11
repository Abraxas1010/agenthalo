mod seed_impl {
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
        crate::util::now_unix_secs()
    }

    fn load_wallet_seed_bytes(wallet_path: &std::path::Path) -> Result<Vec<u8>, String> {
        crate::pq::wallet_seed_bytes_from_path(wallet_path)
    }

    fn derive_seed_key(wallet_path: &std::path::Path) -> Result<[u8; 32], String> {
        let seed_bytes = load_wallet_seed_bytes(wallet_path)?;
        // Preserve the legacy HKDF domain so standalone NucleusDB can unwrap
        // genesis material created before the extraction.
        let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-genesis-seed-v1"), &seed_bytes);
        let mut out = [0u8; 32];
        hk.expand(b"seed-wrap", &mut out)
            .map_err(|_| "hkdf expand failed".to_string())?;
        Ok(out)
    }

    pub fn seed_exists() -> bool {
        crate::config::legacy_genesis_seed_path().exists()
            || crate::config::genesis_seed_v2_path().exists()
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
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
        let payload = StoredGenesisSeed {
            schema: "agenthalo.genesis.seed.v1".to_string(),
            created_at: now_unix(),
            combined_entropy_sha256: combined_entropy_sha256.to_string(),
            combined_entropy_hex: crate::util::hex_encode(seed),
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
        crate::config::ensure_nucleusdb_dir()?;
        let wallet_path = crate::config::wallet_seed_path();
        let seed_path = crate::config::legacy_genesis_seed_path();
        store_seed_once_with_paths(&wallet_path, &seed_path, seed, combined_entropy_sha256)
    }

    /// Store genesis seed using v2 encrypted file system (post-password-creation path).
    /// Uses the Genesis scope key instead of the legacy wallet-derived key.
    pub fn store_seed_once_v2(
        seed: &[u8; 64],
        combined_entropy_sha256: &str,
        genesis_scope_key: &[u8; 32],
    ) -> Result<(), String> {
        use crate::crypto_scope::CryptoScope;
        use crate::encrypted_file::EncryptedFileV2;
        crate::config::ensure_nucleusdb_dir()?;
        let v2_path = crate::config::genesis_seed_v2_path();
        if v2_path.exists() {
            return Err(format!(
                "genesis seed already initialized at {}",
                v2_path.display()
            ));
        }
        let header = crate::encrypted_file::load_header()?
            .ok_or_else(|| "v2 crypto header not found".to_string())?;
        let payload = StoredGenesisSeed {
            schema: "agenthalo.genesis.seed.v1".to_string(),
            created_at: now_unix(),
            combined_entropy_sha256: combined_entropy_sha256.to_string(),
            combined_entropy_hex: crate::util::hex_encode(seed),
        };
        let plaintext =
            serde_json::to_vec(&payload).map_err(|e| format!("serialize genesis seed: {e}"))?;
        let file = EncryptedFileV2::encrypt(
            &plaintext,
            genesis_scope_key,
            CryptoScope::Genesis,
            &header.kdf,
        )?;
        file.save(&v2_path)?;
        Ok(())
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
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
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
        let wallet_path = crate::config::wallet_seed_path();
        let seed_path = crate::config::legacy_genesis_seed_path();
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
        let cipher =
            Aes256Gcm::new_from_slice(&key).map_err(|e| format!("cipher init failed: {e}"))?;
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
        let bytes = crate::util::hex_decode(&payload.combined_entropy_hex)?;
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
        let wallet_path = crate::config::wallet_seed_path();
        let seed_path = crate::config::legacy_genesis_seed_path();
        load_seed_bytes_with_paths(&wallet_path, &seed_path)
    }

    /// Load genesis seed bytes via v2 encrypted file, using the provided Genesis scope key.
    /// This is the post-migration path: genesis_seed.v2.enc contains the seed payload
    /// encrypted with a password-derived scope key, bypassing the erased wrap key entirely.
    pub fn load_seed_bytes_v2(genesis_scope_key: &[u8; 32]) -> Result<Option<[u8; 64]>, String> {
        let v2_path = crate::config::genesis_seed_v2_path();
        if !v2_path.exists() {
            return Ok(None);
        }
        let file = crate::encrypted_file::EncryptedFileV2::load(&v2_path)?;
        let plaintext = file.decrypt(genesis_scope_key)?;
        let payload: StoredGenesisSeed = serde_json::from_slice(&plaintext)
            .map_err(|e| format!("parse v2 genesis seed: {e}"))?;
        if payload.schema != "agenthalo.genesis.seed.v1" {
            return Err(format!(
                "unsupported genesis seed schema {}",
                payload.schema
            ));
        }
        let bytes = crate::util::hex_decode(&payload.combined_entropy_hex)?;
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
        let v2_path = crate::config::genesis_seed_v2_path();
        if !v2_path.exists() {
            return Ok(None);
        }
        let file = crate::encrypted_file::EncryptedFileV2::load(&v2_path)?;
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

            let paths = crate::pq::PqStoragePaths {
                wallet_path: wallet_path.clone(),
                signatures_dir,
            };
            crate::pq::keygen_pq_with_paths(&paths, true).expect("create pq wallet");

            let mut seed = [0u8; 64];
            for (i, b) in seed.iter_mut().enumerate() {
                *b = i as u8;
            }
            let digest = "sha256:test_digest";
            store_seed_once_with_paths(&wallet_path, &seed_path, &seed, digest)
                .expect("store seed");
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

            let paths = crate::pq::PqStoragePaths {
                wallet_path: wallet_path.clone(),
                signatures_dir,
            };
            crate::pq::keygen_pq_with_paths(&paths, true).expect("create pq wallet");

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
}
mod entropy_impl {
    use rand_core::{OsRng, RngCore};
    use serde::{Deserialize, Serialize};
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256, Sha512};
    use std::time::{Duration, Instant};

    use crate::http_client;

    const CURBY_URL: &str = "https://random.colorado.edu/api/curbyq/round/latest/data";
    const CURBY_META_URL: &str = "https://random.colorado.edu/api/curbyq/round/latest";
    const NIST_URL: &str = "https://beacon.nist.gov/beacon/2.0/pulse/last";
    const DRAND_URL: &str = "https://api.drand.sh/public/latest";
    const ENTROPY_WIDTH: usize = 64;
    const SOURCE_MIN_SUCCESS: usize = 2;

    pub const ERR_CURBY_UNREACHABLE: &str = "CURBY_UNREACHABLE";
    pub const ERR_NIST_UNREACHABLE: &str = "NIST_UNREACHABLE";
    pub const ERR_DRAND_UNREACHABLE: &str = "DRAND_UNREACHABLE";
    pub const ERR_ALL_REMOTE_FAILED: &str = "ALL_REMOTE_FAILED";
    pub const ERR_INSUFFICIENT_ENTROPY: &str = "INSUFFICIENT_ENTROPY";
    pub const ERR_ENTROPY_QUALITY_FAILURE: &str = "ENTROPY_QUALITY_FAILURE";
    pub const ERR_UNKNOWN: &str = "UNKNOWN";

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum EntropySourceId {
        Curby,
        Nist,
        Drand,
        Os,
    }

    impl EntropySourceId {
        fn name(self) -> &'static str {
            match self {
                Self::Curby => "CURBy-Q",
                Self::Nist => "NIST-Beacon",
                Self::Drand => "drand",
                Self::Os => "OS-Entropy",
            }
        }

        fn tier(self) -> u8 {
            match self {
                Self::Curby => 2,
                Self::Nist => 3,
                Self::Drand => 4,
                Self::Os => 5,
            }
        }

        fn order(self) -> u8 {
            match self {
                Self::Curby => 0,
                Self::Nist => 1,
                Self::Drand => 2,
                Self::Os => 3,
            }
        }
    }

    #[derive(Clone, Debug)]
    struct SourceSample {
        id: EntropySourceId,
        bytes: [u8; ENTROPY_WIDTH],
        metadata: Value,
        detail: Option<String>,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct FailedSource {
        pub name: String,
        pub tier: u8,
        pub error: String,
        pub technical_detail: String,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct SourceRecord {
        pub name: String,
        pub tier: u8,
        pub status: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub detail: Option<String>,
        #[serde(default, skip_serializing_if = "value_is_null")]
        pub metadata: Value,
    }

    #[derive(Clone, Debug)]
    pub struct HarvestOutcome {
        pub combined_entropy: [u8; ENTROPY_WIDTH],
        pub combined_entropy_sha256: String,
        pub sources: Vec<SourceRecord>,
        pub failed_sources: Vec<FailedSource>,
        pub sources_count: usize,
        pub curby_pulse_id: Option<u64>,
        pub duration_ms: u64,
    }

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct GenesisError {
        pub error_code: String,
        pub message: String,
        #[serde(default)]
        pub failed_sources: Vec<FailedSource>,
    }

    impl GenesisError {
        fn new(
            error_code: &str,
            message: impl Into<String>,
            failed_sources: Vec<FailedSource>,
        ) -> Self {
            Self {
                error_code: error_code.to_string(),
                message: message.into(),
                failed_sources,
            }
        }
    }

    fn value_is_null(v: &Value) -> bool {
        v.is_null()
    }

    fn make_failed(id: EntropySourceId, message: String) -> FailedSource {
        FailedSource {
            name: id.name().to_string(),
            tier: id.tier(),
            error: message.clone(),
            technical_detail: message,
        }
    }

    fn parse_exact_64_hex(hex: &str, source: &str) -> Result<[u8; ENTROPY_WIDTH], String> {
        let bytes = crate::util::hex_decode(hex)?;
        if bytes.len() != ENTROPY_WIDTH {
            return Err(format!(
                "{source} returned {} bytes, expected {ENTROPY_WIDTH}",
                bytes.len()
            ));
        }
        let mut out = [0u8; ENTROPY_WIDTH];
        out.copy_from_slice(&bytes);
        Ok(out)
    }

    fn parse_u64_like(v: Option<&Value>) -> Option<u64> {
        match v {
            Some(Value::Number(n)) => n.as_u64(),
            Some(Value::String(s)) => s.trim().parse::<u64>().ok(),
            _ => None,
        }
    }

    fn extract_curby_meta_round(meta: &Value) -> Option<u64> {
        if let Some(arr0) = meta.as_array().and_then(|a| a.first()) {
            let payload = arr0
                .get("data")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.get("payload"));
            let content = arr0.get("data").and_then(|v| v.get("content"));
            parse_u64_like(payload.and_then(|p| p.get("round")))
                .or_else(|| parse_u64_like(content.and_then(|c| c.get("index"))))
        } else {
            parse_u64_like(meta.get("round"))
        }
    }

    fn extract_curby_meta_timestamp(meta: &Value) -> Option<String> {
        if let Some(arr0) = meta.as_array().and_then(|a| a.first()) {
            let payload = arr0
                .get("data")
                .and_then(|v| v.get("content"))
                .and_then(|v| v.get("payload"));
            payload
                .and_then(|p| p.get("timestamp"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        } else {
            meta.get("timestamp")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        }
    }

    fn fetch_curby_legacy_json_sample() -> Result<SourceSample, String> {
        let resp = http_client::get_with_timeout(CURBY_URL, Duration::from_secs(10))?
            .call()
            .map_err(|e| format!("curby request failed: {e}"))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|e| format!("curby parse failed: {e}"))?;
        let value_hex = body
            .get("value")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "curby response missing value".to_string())?;
        let bytes = parse_exact_64_hex(value_hex, "curby")?;
        let pulse_id = parse_u64_like(body.get("round"));
        let twine_hash = body
            .get("hash")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let timestamp = body.get("timestamp").and_then(|v| v.as_u64());
        Ok(SourceSample {
            id: EntropySourceId::Curby,
            bytes,
            metadata: json!({
                "pulse_id": pulse_id,
                "twine_hash": twine_hash,
                "timestamp": timestamp,
            }),
            detail: pulse_id.map(|id| format!("Pulse #{id}")),
        })
    }

    fn fetch_curby_chain_sample() -> Result<SourceSample, String> {
        let data_resp = http_client::get_with_timeout(CURBY_URL, Duration::from_secs(10))?
            .call()
            .map_err(|e| format!("curby data request failed: {e}"))?;
        let raw_data = data_resp
            .into_body()
            .read_to_vec()
            .map_err(|e| format!("curby data read failed: {e}"))?;
        if raw_data.is_empty() {
            return Err("curby data payload was empty".to_string());
        }

        let meta = http_client::get_with_timeout(CURBY_META_URL, Duration::from_secs(10))?
            .call()
            .ok()
            .and_then(|resp| resp.into_body().read_json::<Value>().ok());

        let mut hasher = Sha512::new();
        hasher.update(&raw_data);
        let mut bytes = [0u8; ENTROPY_WIDTH];
        bytes.copy_from_slice(&hasher.finalize());

        let pulse_id = meta.as_ref().and_then(extract_curby_meta_round);
        let timestamp = meta.as_ref().and_then(extract_curby_meta_timestamp);
        let detail = pulse_id
            .map(|id| format!("Pulse #{id}"))
            .or_else(|| Some("Quantum beacon connected".to_string()));

        Ok(SourceSample {
            id: EntropySourceId::Curby,
            bytes,
            metadata: json!({
                "pulse_id": pulse_id,
                "timestamp": timestamp,
                "normalization": "sha512",
                "input_bytes": raw_data.len(),
                "source_format": "curby_chain_payload",
            }),
            detail,
        })
    }

    fn fetch_curby_sample() -> Result<SourceSample, String> {
        match fetch_curby_legacy_json_sample() {
            Ok(sample) => Ok(sample),
            Err(legacy_err) => fetch_curby_chain_sample()
                .map_err(|chain_err| format!("{legacy_err}; fallback failed: {chain_err}")),
        }
    }

    fn fetch_nist_sample() -> Result<SourceSample, String> {
        let resp = http_client::get_with_timeout(NIST_URL, Duration::from_secs(10))?
            .call()
            .map_err(|e| format!("nist request failed: {e}"))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|e| format!("nist parse failed: {e}"))?;
        let pulse = body
            .get("pulse")
            .ok_or_else(|| "nist response missing pulse object".to_string())?;
        let output_hex = pulse
            .get("outputValue")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "nist response missing outputValue".to_string())?;
        let bytes = parse_exact_64_hex(output_hex, "nist")?;
        let pulse_index = pulse.get("pulseIndex").and_then(|v| v.as_u64());
        let time_stamp = pulse
            .get("timeStamp")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(SourceSample {
            id: EntropySourceId::Nist,
            bytes,
            metadata: json!({
                "pulse_index": pulse_index,
                "time_stamp": time_stamp,
            }),
            detail: pulse_index.map(|idx| format!("Pulse #{idx}")),
        })
    }

    fn fetch_drand_sample() -> Result<SourceSample, String> {
        let resp = http_client::get_with_timeout(DRAND_URL, Duration::from_secs(10))?
            .call()
            .map_err(|e| format!("drand request failed: {e}"))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|e| format!("drand parse failed: {e}"))?;
        let randomness_hex = body
            .get("randomness")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "drand response missing randomness".to_string())?;
        let raw = crate::util::hex_decode(randomness_hex)?;
        if raw.len() != 32 {
            return Err(format!("drand returned {} bytes, expected 32", raw.len()));
        }
        let mut h = Sha512::new();
        h.update(&raw);
        let digest = h.finalize();
        let mut bytes = [0u8; ENTROPY_WIDTH];
        bytes.copy_from_slice(&digest);
        let round = body.get("round").and_then(|v| v.as_u64());
        let signature = body.get("signature").and_then(|v| v.as_str()).unwrap_or("");
        Ok(SourceSample {
            id: EntropySourceId::Drand,
            bytes,
            metadata: json!({
                "round": round,
                "signature": signature,
                "normalized": true,
                "normalization": "sha512",
                "input_bytes": 32,
            }),
            detail: round.map(|r| format!("Round #{r}")),
        })
    }

    fn fetch_os_sample() -> Result<SourceSample, String> {
        let mut bytes = [0u8; ENTROPY_WIDTH];
        OsRng.fill_bytes(&mut bytes);
        Ok(SourceSample {
            id: EntropySourceId::Os,
            bytes,
            metadata: json!({
                "provider": "os_rng",
                "width_bytes": ENTROPY_WIDTH,
            }),
            detail: Some("CSPRNG available".to_string()),
        })
    }

    fn fixture_mode() -> Option<String> {
        std::env::var("NUCLEUSDB_GENESIS_TEST_MODE")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .filter(|v| !v.is_empty())
    }

    fn run_fixture(mode: &str) -> Result<HarvestOutcome, GenesisError> {
        let started = Instant::now();

        let mk = |id: EntropySourceId, fill: u8, detail: &str, metadata: Value| -> SourceSample {
            SourceSample {
                id,
                bytes: [fill; ENTROPY_WIDTH],
                metadata,
                detail: Some(detail.to_string()),
            }
        };

        let mut successes = Vec::<SourceSample>::new();
        let mut failures = Vec::<FailedSource>::new();
        match mode {
            "success" | "pass" => {
                successes.push(mk(
                    EntropySourceId::Curby,
                    0x11,
                    "Pulse #7523",
                    json!({"pulse_id": 7523, "twine_hash": "test"}),
                ));
                successes.push(mk(
                    EntropySourceId::Nist,
                    0x22,
                    "Pulse #42",
                    json!({"pulse_index": 42}),
                ));
                let drand_raw = [0x33u8; 32];
                let mut h = Sha512::new();
                h.update(drand_raw);
                let mut d64 = [0u8; ENTROPY_WIDTH];
                d64.copy_from_slice(&h.finalize());
                successes.push(SourceSample {
                    id: EntropySourceId::Drand,
                    bytes: d64,
                    metadata: json!({"round": 99, "normalized": true, "normalization": "sha512"}),
                    detail: Some("Round #99".to_string()),
                });
                successes.push(mk(
                    EntropySourceId::Os,
                    0x44,
                    "CSPRNG available",
                    json!({"provider": "os_rng"}),
                ));
            }
            "partial" => {
                failures.push(make_failed(
                    EntropySourceId::Curby,
                    "simulated fixture outage".to_string(),
                ));
                successes.push(mk(
                    EntropySourceId::Nist,
                    0x22,
                    "Pulse #42",
                    json!({"pulse_index": 42}),
                ));
                successes.push(mk(
                    EntropySourceId::Os,
                    0x44,
                    "CSPRNG available",
                    json!({"provider": "os_rng"}),
                ));
            }
            "all_remote_failed" => {
                failures.push(make_failed(
                    EntropySourceId::Curby,
                    "simulated fixture outage".to_string(),
                ));
                failures.push(make_failed(
                    EntropySourceId::Nist,
                    "simulated fixture outage".to_string(),
                ));
                failures.push(make_failed(
                    EntropySourceId::Drand,
                    "simulated fixture outage".to_string(),
                ));
                successes.push(mk(
                    EntropySourceId::Os,
                    0x44,
                    "CSPRNG available",
                    json!({"provider": "os_rng"}),
                ));
            }
            _ => {
                return Err(GenesisError::new(
                    ERR_UNKNOWN,
                    format!("unsupported NUCLEUSDB_GENESIS_TEST_MODE: {mode}"),
                    Vec::new(),
                ));
            }
        }

        finalize_harvest(successes, failures, started)
    }

    fn finalize_harvest(
        mut successes: Vec<SourceSample>,
        failures: Vec<FailedSource>,
        started: Instant,
    ) -> Result<HarvestOutcome, GenesisError> {
        let remote_successes = successes
            .iter()
            .filter(|s| s.id != EntropySourceId::Os)
            .count();

        if remote_successes == 0 {
            return Err(GenesisError::new(
                ERR_ALL_REMOTE_FAILED,
                "could not reach any remote entropy beacon (CURBy, NIST, drand)",
                failures,
            ));
        }

        if successes.len() < SOURCE_MIN_SUCCESS {
            return Err(GenesisError::new(
                ERR_INSUFFICIENT_ENTROPY,
                format!(
                    "only {} entropy source(s) succeeded; at least {SOURCE_MIN_SUCCESS} required",
                    successes.len()
                ),
                failures,
            ));
        }

        successes.sort_by_key(|s| s.id.order());
        let mut combined = [0u8; ENTROPY_WIDTH];
        for sample in &successes {
            for (idx, b) in sample.bytes.iter().enumerate() {
                combined[idx] ^= b;
            }
        }

        let mut h = Sha256::new();
        h.update(combined);
        let combined_entropy_sha256 = format!("sha256:{}", crate::util::hex_encode(&h.finalize()));

        let sources = successes
            .iter()
            .map(|s| SourceRecord {
                name: s.id.name().to_string(),
                tier: s.id.tier(),
                status: "ok".to_string(),
                detail: s.detail.clone(),
                metadata: s.metadata.clone(),
            })
            .collect::<Vec<_>>();

        let curby_pulse_id = successes
            .iter()
            .find(|s| s.id == EntropySourceId::Curby)
            .and_then(|s| s.metadata.get("pulse_id").and_then(|v| v.as_u64()));

        Ok(HarvestOutcome {
            combined_entropy: combined,
            combined_entropy_sha256,
            sources_count: sources.len(),
            sources,
            failed_sources: failures,
            curby_pulse_id,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }

    pub fn harvest_entropy() -> Result<HarvestOutcome, GenesisError> {
        if let Some(mode) = fixture_mode() {
            return run_fixture(&mode);
        }

        let started = Instant::now();
        let curby_handle = std::thread::spawn(fetch_curby_sample);
        let nist_handle = std::thread::spawn(fetch_nist_sample);
        let drand_handle = std::thread::spawn(fetch_drand_sample);

        let mut successes = Vec::<SourceSample>::new();
        let mut failures = Vec::<FailedSource>::new();

        let os = fetch_os_sample().map_err(|e| GenesisError::new(ERR_UNKNOWN, e, Vec::new()))?;
        successes.push(os);

        for (id, handle, mapped_code) in [
            (EntropySourceId::Curby, curby_handle, ERR_CURBY_UNREACHABLE),
            (EntropySourceId::Nist, nist_handle, ERR_NIST_UNREACHABLE),
            (EntropySourceId::Drand, drand_handle, ERR_DRAND_UNREACHABLE),
        ] {
            match handle.join() {
                Ok(Ok(sample)) => successes.push(sample),
                Ok(Err(err)) => failures.push(make_failed(id, err)),
                Err(_) => failures.push(make_failed(id, format!("{mapped_code}: thread panic"))),
            }
        }

        finalize_harvest(successes, failures, started)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::{Mutex, OnceLock};

        fn env_lock() -> &'static Mutex<()> {
            static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
            LOCK.get_or_init(|| Mutex::new(()))
        }

        fn lock_env() -> std::sync::MutexGuard<'static, ()> {
            let mutex = env_lock();
            let guard = mutex
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            mutex.clear_poison();
            guard
        }

        #[test]
        fn fixture_success_has_expected_shape() {
            let _guard = lock_env();
            std::env::set_var("NUCLEUSDB_GENESIS_TEST_MODE", "success");
            let out = harvest_entropy().expect("harvest success");
            std::env::remove_var("NUCLEUSDB_GENESIS_TEST_MODE");

            assert_eq!(out.sources_count, 4);
            assert_eq!(out.failed_sources.len(), 0);
            assert!(out.curby_pulse_id.is_some());
            assert!(out.combined_entropy_sha256.starts_with("sha256:"));
        }

        #[test]
        fn fixture_pass_alias_has_expected_shape() {
            let _guard = lock_env();
            std::env::set_var("NUCLEUSDB_GENESIS_TEST_MODE", "pass");
            let out = harvest_entropy().expect("harvest pass alias");
            std::env::remove_var("NUCLEUSDB_GENESIS_TEST_MODE");

            assert_eq!(out.sources_count, 4);
            assert_eq!(out.failed_sources.len(), 0);
            assert!(out.curby_pulse_id.is_some());
            assert!(out.combined_entropy_sha256.starts_with("sha256:"));
        }

        #[test]
        fn fixture_all_remote_failed_reports_expected_code() {
            let _guard = lock_env();
            std::env::set_var("NUCLEUSDB_GENESIS_TEST_MODE", "all_remote_failed");
            let err = harvest_entropy().expect_err("expected failure");
            std::env::remove_var("NUCLEUSDB_GENESIS_TEST_MODE");

            assert_eq!(err.error_code, ERR_ALL_REMOTE_FAILED);
            assert_eq!(err.failed_sources.len(), 3);
        }

        #[test]
        fn extract_curby_meta_round_accepts_numeric_and_string() {
            let numeric = serde_json::json!([{
                "data": {
                    "content": {
                        "index": 84644,
                        "payload": {
                            "round": 28297,
                            "timestamp": "2026-03-01T00:00:00.000Z"
                        }
                    }
                }
            }]);
            let stringy = serde_json::json!({
                "round": "7523",
                "timestamp": "2026-03-01T00:00:00.000Z"
            });

            assert_eq!(extract_curby_meta_round(&numeric), Some(28297));
            assert_eq!(
                extract_curby_meta_timestamp(&numeric).as_deref(),
                Some("2026-03-01T00:00:00.000Z")
            );
            assert_eq!(extract_curby_meta_round(&stringy), Some(7523));
            assert_eq!(
                extract_curby_meta_timestamp(&stringy).as_deref(),
                Some("2026-03-01T00:00:00.000Z")
            );
        }

        #[test]
        #[ignore]
        fn live_curby_sample_parses_current_api() {
            let _guard = lock_env();
            std::env::remove_var("NUCLEUSDB_GENESIS_TEST_MODE");
            let sample = fetch_curby_sample().expect("live curby fetch should parse");
            assert_eq!(sample.id, EntropySourceId::Curby);
            assert!(
                sample.detail.is_some(),
                "curby detail should include pulse or connected marker"
            );
            assert_eq!(
                sample
                    .metadata
                    .get("normalization")
                    .and_then(|v| v.as_str()),
                Some("sha512")
            );
            assert!(
                sample
                    .metadata
                    .get("input_bytes")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                    > 0
            );
        }
    }
}

pub use entropy_impl::*;
pub use seed_impl::*;
