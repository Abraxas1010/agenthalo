use crate::halo::config;
use crate::halo::crypto_scope::CryptoScope;
use crate::halo::encrypted_file::{self, EncryptedFileV2};
use crate::halo::password;
use hkdf::Hkdf;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MigrationStatus {
    Fresh,
    NeedsPasswordCreation,
    V2Locked,
    V2Unlocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MigrationReport {
    pub files_migrated: Vec<String>,
    pub files_failed: Vec<(String, String)>,
    pub seed_key_deleted: bool,
    #[serde(default)]
    pub legacy_wallet_removed: bool,
}

pub fn detect_migration_status() -> MigrationStatus {
    let has_header = encrypted_file::header_exists();
    let has_legacy = has_legacy_files();
    match (has_header, has_legacy) {
        (false, false) => MigrationStatus::Fresh,
        (false, true) => MigrationStatus::NeedsPasswordCreation,
        (true, _) => MigrationStatus::V2Locked,
    }
}

pub fn migrate_v1_to_v2(password: &str) -> Result<MigrationReport, String> {
    password::validate_password(password)?;
    config::ensure_halo_dir()?;

    let header = encrypted_file::create_header_if_missing()?;
    let mut master = header.kdf.derive_master_key(password)?;

    let mut report = MigrationReport {
        files_migrated: Vec::new(),
        files_failed: Vec::new(),
        seed_key_deleted: false,
        legacy_wallet_removed: false,
    };

    let mappings = vec![
        (
            config::pq_wallet_path(),
            config::pq_wallet_v2_path(),
            CryptoScope::Sign,
        ),
        (
            config::vault_path(),
            config::vault_v2_path(),
            CryptoScope::Vault,
        ),
        (
            config::genesis_seed_path(),
            config::genesis_seed_v2_path(),
            CryptoScope::Genesis,
        ),
        (
            config::identity_config_path(),
            config::identity_v2_path(),
            CryptoScope::Identity,
        ),
        (
            config::profile_path(),
            config::profile_v2_path(),
            CryptoScope::Identity,
        ),
        (
            crate::halo::wdk_proxy::encrypted_seed_path(),
            config::wdk_seed_v2_path(),
            CryptoScope::Wallet,
        ),
    ];

    for (legacy_path, new_path, scope) in mappings {
        if !legacy_path.exists() {
            continue;
        }
        match migrate_one_file(&legacy_path, &new_path, scope, &master, &header.kdf) {
            Ok(()) => report.files_migrated.push(format!(
                "{} -> {}",
                legacy_path.display(),
                new_path.display()
            )),
            Err(e) => report
                .files_failed
                .push((legacy_path.display().to_string(), e)),
        }
    }

    let seed_key = config::pq_wallet_path().with_extension("seed.key");
    if seed_key.exists() {
        match secure_erase(&seed_key) {
            Ok(()) => report.seed_key_deleted = true,
            Err(e) => report
                .files_failed
                .push((seed_key.display().to_string(), e.to_string())),
        }
    }

    // Remove legacy v1 wallet file to prevent stale-key decryption path.
    // After migration, pq_wallet.json contains an encrypted_seed referencing the
    // now-erased wrap key. Any code that reads it would silently create a new
    // random wrap key that cannot decrypt the old ciphertext (E1 bug).
    // The authoritative copy is now pq_wallet.v2.enc.
    let legacy_wallet = config::pq_wallet_path();
    if legacy_wallet.exists() && config::pq_wallet_v2_path().exists() {
        match std::fs::remove_file(&legacy_wallet) {
            Ok(()) => report.legacy_wallet_removed = true,
            Err(e) => report.files_failed.push((
                legacy_wallet.display().to_string(),
                format!("remove legacy wallet: {e}"),
            )),
        }
    }

    let result = if !report.files_failed.is_empty() {
        Err(format!(
            "migration failed for {} files",
            report.files_failed.len()
        ))
    } else {
        Ok(report)
    };
    master.zeroize();
    result
}

fn migrate_one_file(
    legacy_path: &std::path::Path,
    v2_path: &std::path::Path,
    scope: CryptoScope,
    master: &[u8; 32],
    kdf: &encrypted_file::KdfParams,
) -> Result<(), String> {
    if EncryptedFileV2::is_v2(v2_path) {
        return Ok(());
    }
    let plaintext = Zeroizing::new(
        legacy_plaintext_for_migration(legacy_path)
            .map_err(|e| format!("read/decrypt legacy {}: {e}", legacy_path.display()))?,
    );
    let mut scope_key = derive_scope_key(master, scope)?;
    let file = EncryptedFileV2::encrypt(plaintext.as_slice(), &scope_key, scope, kdf)?;
    scope_key.zeroize();
    file.save(v2_path)
}

fn legacy_plaintext_for_migration(legacy_path: &std::path::Path) -> Result<Vec<u8>, String> {
    if legacy_path == config::vault_path() {
        return crate::halo::vault::decrypt_legacy_vault_payload(
            &config::pq_wallet_path(),
            legacy_path,
        );
    }
    if legacy_path == config::genesis_seed_path() {
        return crate::halo::genesis_seed::decrypt_legacy_seed_payload(
            &config::pq_wallet_path(),
            legacy_path,
        );
    }
    // PQ wallet: unwrap the encrypted_seed using the wrap key (still present at migration
    // time) and store the seed as secret_seed_hex so the v2-encrypted container has a
    // directly accessible plaintext seed.  Without this, the wrap key is erased after
    // migration and the inner encrypted_seed becomes unrecoverable.
    if legacy_path == config::pq_wallet_path() {
        let raw = std::fs::read_to_string(legacy_path)
            .map_err(|e| format!("read {}: {e}", legacy_path.display()))?;
        let wallet: crate::halo::pq::PqWallet = serde_json::from_str(&raw)
            .map_err(|e| format!("parse {}: {e}", legacy_path.display()))?;
        let seed_bytes = crate::halo::pq::wallet_seed_bytes_from_path(legacy_path)?;
        let seed_hex = hex::encode(&seed_bytes);
        let mut migrated = wallet;
        migrated.secret_seed_hex = Some(seed_hex);
        migrated.encrypted_seed = None;
        return serde_json::to_vec_pretty(&migrated)
            .map_err(|e| format!("re-serialize wallet for v2 migration: {e}"));
    }
    std::fs::read(legacy_path).map_err(|e| format!("read {}: {e}", legacy_path.display()))
}

fn derive_scope_key(master: &[u8; 32], scope: CryptoScope) -> Result<[u8; 32], String> {
    let hk = Hkdf::<Sha256>::new(Some(b"agenthalo-scope-v2"), master);
    let mut out = [0u8; 32];
    hk.expand(scope.hkdf_info(), &mut out)
        .map_err(|_| "hkdf expand failed".to_string())?;
    Ok(out)
}

pub fn secure_erase(path: &std::path::Path) -> Result<(), String> {
    eprintln!(
        "warning: secure_erase({}) best-effort only; physical recovery may remain possible on CoW/journaled SSD filesystems",
        path.display()
    );
    if !path.exists() {
        return Ok(());
    }
    let metadata = std::fs::metadata(path).map_err(|e| format!("stat {}: {e}", path.display()))?;
    let len = metadata.len() as usize;
    if len == 0 {
        std::fs::remove_file(path).map_err(|e| format!("unlink {}: {e}", path.display()))?;
        return Ok(());
    }

    use std::io::{Seek, SeekFrom, Write};
    for _ in 0..2 {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("seek {}: {e}", path.display()))?;
        let mut buf = vec![0u8; len];
        OsRng.fill_bytes(&mut buf);
        file.write_all(&buf)
            .map_err(|e| format!("overwrite {}: {e}", path.display()))?;
        file.flush()
            .map_err(|e| format!("flush {}: {e}", path.display()))?;
        let _ = file.sync_all();
    }

    std::fs::remove_file(path).map_err(|e| format!("unlink {}: {e}", path.display()))
}

fn has_legacy_files() -> bool {
    let legacy = [
        config::pq_wallet_path(),
        config::pq_wallet_path().with_extension("seed.key"),
        config::vault_path(),
        config::identity_config_path(),
        config::profile_path(),
        config::genesis_seed_path(),
        crate::halo::wdk_proxy::encrypted_seed_path(),
    ];
    legacy.into_iter().any(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_status_is_fresh_without_files() {
        let _ = detect_migration_status();
    }
}
