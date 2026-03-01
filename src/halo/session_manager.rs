use crate::halo::crypto_scope::{CryptoScope, ScopeKey};
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use sha2::Sha256;
use std::collections::HashMap;
use zeroize::{Zeroize, ZeroizeOnDrop};

const ARGON2_MEMORY_KIB: u32 = 128 * 1024;
const ARGON2_ITERATIONS: u32 = 4;
const ARGON2_PARALLELISM: u32 = 1;

#[derive(Debug, Zeroize, ZeroizeOnDrop)]
struct EncryptedMasterCache {
    session_key: [u8; 32],
    encrypted_master: Vec<u8>,
    nonce: [u8; 12],
}

#[derive(Debug)]
pub struct SessionManager {
    cache: Option<EncryptedMasterCache>,
    scope_keys: HashMap<CryptoScope, ScopeKey>,
    failed_attempts: u32,
    locked_until_unix: u64,
    session_started: Option<u64>,
    max_session_secs: u64,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            cache: None,
            scope_keys: HashMap::new(),
            failed_attempts: 0,
            locked_until_unix: 0,
            session_started: None,
            max_session_secs: 8 * 3600,
        }
    }

    pub fn unlock_with_password(&mut self, password: &str, salt: &[u8; 32]) -> Result<(), String> {
        if password.trim().is_empty() {
            return Err("password must not be empty".to_string());
        }
        let now = now_unix();
        self.check_throttle(now)
            .map_err(|(until, _)| format!("unlock throttled until unix={until}"))?;

        let master = Self::derive_master_key(password, salt)?;
        self.unlock_with_master_key(master)
    }

    pub fn unlock_with_master_key(&mut self, mut master_key: [u8; 32]) -> Result<(), String> {
        let now = now_unix();
        let cache = match Self::encrypt_master(&master_key) {
            Ok(c) => c,
            Err(e) => {
                master_key.zeroize();
                return Err(e);
            }
        };
        master_key.zeroize();
        self.cache = Some(cache);
        // `ScopeKey` implements `ZeroizeOnDrop`; dropping entries here zeroizes key bytes.
        self.scope_keys.clear();
        self.session_started = Some(now);
        self.reset_throttle();
        Ok(())
    }

    pub fn is_unlocked(&self) -> bool {
        if self.cache.is_none() && self.scope_keys.is_empty() {
            return false;
        }
        if let Some(started) = self.session_started {
            let now = now_unix();
            if now.saturating_sub(started) > self.max_session_secs {
                return false;
            }
        }
        true
    }

    pub fn get_scope_key(&mut self, scope: CryptoScope) -> Result<&ScopeKey, String> {
        if scope == CryptoScope::Admin {
            return Err("admin is not a concrete decrypt scope".to_string());
        }
        if !self.is_unlocked() {
            return Err("session locked".to_string());
        }

        let now = now_unix();
        let expired = self
            .scope_keys
            .get(&scope)
            .map(|k| k.is_expired(now))
            .unwrap_or(false);

        if expired || !self.scope_keys.contains_key(&scope) {
            let Some(cache) = self.cache.as_ref() else {
                return Err(format!("scope unavailable: {}", scope.as_str()));
            };
            let mut master = Self::decrypt_master(cache)?;
            let scoped = Self::derive_scope_key(&master, scope, now)?;
            master.zeroize();
            self.scope_keys.insert(scope, scoped);
        }

        if let Some(k) = self.scope_keys.get_mut(&scope) {
            k.touch(now);
        }

        self.scope_keys
            .get(&scope)
            .ok_or_else(|| "failed to load scope key".to_string())
    }

    pub fn lock(&mut self) {
        // `ScopeKey` implements `ZeroizeOnDrop`; dropping entries here zeroizes key bytes.
        self.scope_keys.clear();
        self.cache = None;
        self.session_started = None;
    }

    pub fn reap_expired(&mut self) {
        let now = now_unix();
        self.scope_keys.retain(|_, key| !key.is_expired(now));
        if let Some(started) = self.session_started {
            if now.saturating_sub(started) > self.max_session_secs {
                self.lock();
            }
        }
    }

    pub fn check_throttle(&self, now: u64) -> Result<(), (u64, u32)> {
        if self.locked_until_unix > now {
            return Err((self.locked_until_unix, self.failed_attempts));
        }
        Ok(())
    }

    pub fn record_failed_attempt(&mut self, now: u64) {
        self.failed_attempts = self.failed_attempts.saturating_add(1);
        let delay = unlock_delay_secs(self.failed_attempts);
        self.locked_until_unix = now.saturating_add(delay);
    }

    pub fn reset_throttle(&mut self) {
        self.failed_attempts = 0;
        self.locked_until_unix = 0;
    }

    pub fn active_scopes(&self) -> Vec<CryptoScope> {
        let now = now_unix();
        self.scope_keys
            .iter()
            .filter_map(|(scope, key)| {
                if key.is_expired(now) {
                    None
                } else {
                    Some(*scope)
                }
            })
            .collect()
    }

    pub fn insert_scope_key(&mut self, scope_key: ScopeKey) {
        if self.session_started.is_none() {
            self.session_started = Some(now_unix());
        }
        self.scope_keys.insert(scope_key.scope, scope_key);
    }

    pub fn failed_attempts(&self) -> u32 {
        self.failed_attempts
    }

    pub fn locked_until_unix(&self) -> u64 {
        self.locked_until_unix
    }

    pub fn set_max_session_secs_for_test(&mut self, secs: u64) {
        self.max_session_secs = secs;
    }

    fn derive_master_key(password: &str, salt: &[u8]) -> Result<[u8; 32], String> {
        let params = Params::new(
            ARGON2_MEMORY_KIB,
            ARGON2_ITERATIONS,
            ARGON2_PARALLELISM,
            Some(32),
        )
        .map_err(|e| format!("argon2 params: {e}"))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut out = [0u8; 32];
        argon2
            .hash_password_into(password.as_bytes(), salt, &mut out)
            .map_err(|e| format!("argon2 derive: {e}"))?;
        Ok(out)
    }

    fn encrypt_master(master_key: &[u8; 32]) -> Result<EncryptedMasterCache, String> {
        let mut session_key = [0u8; 32];
        getrandom::getrandom(&mut session_key).map_err(|e| format!("rng: {e}"))?;
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|e| format!("rng: {e}"))?;
        let cipher =
            Aes256Gcm::new_from_slice(&session_key).map_err(|e| format!("session cipher: {e}"))?;
        let encrypted = cipher
            .encrypt(Nonce::from_slice(&nonce), master_key.as_slice())
            .map_err(|e| format!("encrypt master: {e}"))?;
        Ok(EncryptedMasterCache {
            session_key,
            encrypted_master: encrypted,
            nonce,
        })
    }

    fn decrypt_master(cache: &EncryptedMasterCache) -> Result<[u8; 32], String> {
        let cipher = Aes256Gcm::new_from_slice(&cache.session_key)
            .map_err(|e| format!("session cipher: {e}"))?;
        let mut raw = cipher
            .decrypt(
                Nonce::from_slice(&cache.nonce),
                cache.encrypted_master.as_ref(),
            )
            .map_err(|_| "session decrypt failed".to_string())?;
        if raw.len() != 32 {
            return Err("invalid master key length".to_string());
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&raw);
        raw.zeroize();
        Ok(out)
    }

    fn derive_scope_key(
        master_key: &[u8; 32],
        scope: CryptoScope,
        now: u64,
    ) -> Result<ScopeKey, String> {
        let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-scope-v2"), master_key);
        let mut out = [0u8; 32];
        hk.expand(scope.hkdf_info(), &mut out)
            .map_err(|_| "hkdf expand failed".to_string())?;
        Ok(ScopeKey::new(out, scope, now))
    }
}

fn unlock_delay_secs(failed_attempts: u32) -> u64 {
    let delay = match failed_attempts {
        0 => 0,
        1 => 2,
        2 => 4,
        3 => 8,
        4 => 16,
        5 => 32,
        6 => 64,
        7 => 128,
        _ => 256,
    };
    delay.clamp(2, 300)
}

fn now_unix() -> u64 {
    crate::halo::util::now_unix_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_and_scope_derivation_work() {
        let mut sm = SessionManager::new();
        let salt = [0x11u8; 32];
        sm.unlock_with_password("correct horse battery staple", &salt)
            .expect("unlock");
        assert!(sm.is_unlocked());

        let sign = *sm
            .get_scope_key(CryptoScope::Sign)
            .expect("sign")
            .key_bytes();
        let vault = *sm
            .get_scope_key(CryptoScope::Vault)
            .expect("vault")
            .key_bytes();
        assert_ne!(sign, vault);
    }

    #[test]
    fn lock_clears_session() {
        let mut sm = SessionManager::new();
        let salt = [0x22u8; 32];
        sm.unlock_with_password("pw12345678", &salt)
            .expect("unlock");
        sm.get_scope_key(CryptoScope::Sign).expect("sign");
        sm.lock();
        assert!(!sm.is_unlocked());
        assert!(sm.get_scope_key(CryptoScope::Sign).is_err());
    }

    #[test]
    fn throttle_progresses() {
        let mut sm = SessionManager::new();
        let now = now_unix();
        sm.record_failed_attempt(now);
        assert!(sm.locked_until_unix() >= now + 1);
        sm.record_failed_attempt(now + 2);
        assert!(sm.failed_attempts() >= 2);
    }
}
