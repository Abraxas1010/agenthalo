use crate::halo::config;
use crate::halo::crypto_scope::{CryptoScope, ScopeKey};
use crate::halo::session_manager::SessionManager;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{Encoded, EncodedSizeUser, KemCore, MlKem1024};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const AGENT_CRED_SCHEMA: &str = "agenthalo.agent-credential.v1";
pub const AGENT_SK_SCHEMA: &str = "agenthalo.agent-sk.v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentCredential {
    pub schema: String,
    pub agent_id: String,
    pub label: String,
    pub algorithm: String,
    pub public_key_hex: String,
    pub scopes: HashMap<String, ScopeEncapsulation>,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScopeEncapsulation {
    pub kem_ciphertext_hex: String,
    pub wrapped_key_nonce_hex: String,
    pub wrapped_key_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct AgentSecretKey {
    #[zeroize(skip)]
    pub schema: String,
    #[zeroize(skip)]
    pub agent_id: String,
    #[zeroize(skip)]
    pub algorithm: String,
    pub secret_key_hex: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentCredentialSummary {
    pub agent_id: String,
    pub label: String,
    pub scopes: Vec<String>,
    pub created_at: u64,
    pub expires_at: Option<u64>,
}

pub fn authorize_agent(
    session: &mut SessionManager,
    label: &str,
    scopes: &[CryptoScope],
    expires_days: Option<u64>,
) -> Result<(AgentCredential, AgentSecretKey), String> {
    if !session.is_unlocked() {
        return Err("session locked".to_string());
    }
    if scopes.is_empty() {
        return Err("at least one scope is required".to_string());
    }

    config::ensure_agent_credentials_dir()?;
    let mut rng = OsRng;
    let (dk, ek) = MlKem1024::generate(&mut rng);
    let agent_id = format!("agent_{}", uuid::Uuid::new_v4().simple());
    let now = now_unix();
    let expires_at = expires_days.map(|d| now.saturating_add(d.saturating_mul(86_400)));

    let mut encapsulations = HashMap::new();
    for scope in scopes.iter().copied().filter(|s| *s != CryptoScope::Admin) {
        let (ct, shared) = ek
            .encapsulate(&mut rng)
            .map_err(|_| "ml-kem encapsulate failed".to_string())?;
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|e| format!("rng: {e}"))?;
        let cipher = Aes256Gcm::new_from_slice(shared.as_slice())
            .map_err(|e| format!("cipher init: {e}"))?;
        let scope_key = session
            .get_scope_key(scope)
            .map_err(|_| format!("scope unavailable: {}", scope.as_str()))?;
        let wrapped = cipher
            .encrypt(Nonce::from_slice(&nonce), scope_key.key_bytes().as_slice())
            .map_err(|e| format!("wrap scope key: {e}"))?;

        encapsulations.insert(
            scope.as_str().to_string(),
            ScopeEncapsulation {
                kem_ciphertext_hex: hex::encode(ct.as_slice()),
                wrapped_key_nonce_hex: hex::encode(nonce),
                wrapped_key_hex: hex::encode(wrapped),
            },
        );
    }

    let cred = AgentCredential {
        schema: AGENT_CRED_SCHEMA.to_string(),
        agent_id: agent_id.clone(),
        label: label.trim().to_string(),
        algorithm: "ml-kem-1024".to_string(),
        public_key_hex: hex::encode(ek.as_bytes().as_slice()),
        scopes: encapsulations,
        created_at: now,
        expires_at,
    };

    save_credential(&cred)?;

    let secret = AgentSecretKey {
        schema: AGENT_SK_SCHEMA.to_string(),
        agent_id,
        algorithm: "ml-kem-1024".to_string(),
        secret_key_hex: hex::encode(dk.as_bytes().as_slice()),
    };

    Ok((cred, secret))
}

pub fn revoke_agent(agent_id: &str) -> Result<(), String> {
    let path = credential_path(agent_id);
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("delete {}: {e}", path.display()))?;
    }
    Ok(())
}

pub fn list_agents() -> Result<Vec<AgentCredentialSummary>, String> {
    let mut out = Vec::new();
    let dir = config::agent_credentials_dir();
    if !dir.exists() {
        return Ok(out);
    }
    for ent in std::fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let ent = ent.map_err(|e| format!("read entry: {e}"))?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let raw = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let cred: AgentCredential =
            serde_json::from_slice(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let mut scopes = cred.scopes.keys().cloned().collect::<Vec<_>>();
        scopes.sort();
        out.push(AgentCredentialSummary {
            agent_id: cred.agent_id,
            label: cred.label,
            scopes,
            created_at: cred.created_at,
            expires_at: cred.expires_at,
        });
    }
    out.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    Ok(out)
}

pub fn agent_unlock_scope(
    agent_id: &str,
    agent_sk_hex: &str,
    scope: CryptoScope,
) -> Result<ScopeKey, String> {
    if scope == CryptoScope::Admin {
        return Err("admin scope cannot be unlocked directly".to_string());
    }
    let cred = load_credential(agent_id)?;
    if let Some(expires_at) = cred.expires_at {
        if now_unix() >= expires_at {
            return Err("agent credential expired".to_string());
        }
    }

    let sk_raw = hex::decode(agent_sk_hex).map_err(|e| format!("agent SK hex decode: {e}"))?;
    let sk_arr: Encoded<<MlKem1024 as KemCore>::DecapsulationKey> = sk_raw
        .as_slice()
        .try_into()
        .map_err(|_| "agent SK has invalid length".to_string())?;
    let dk = <MlKem1024 as KemCore>::DecapsulationKey::from_bytes(&sk_arr);

    let expected_public = hex::encode(dk.encapsulation_key().as_bytes().as_slice());
    if !expected_public.eq_ignore_ascii_case(&cred.public_key_hex) {
        return Err("agent secret key does not match credential public key".to_string());
    }

    let scope_name = scope.as_str();
    let scoped = cred
        .scopes
        .get(scope_name)
        .ok_or_else(|| format!("scope not authorized: {scope_name}"))?;

    let ct_raw = hex::decode(&scoped.kem_ciphertext_hex)
        .map_err(|e| format!("ciphertext hex decode: {e}"))?;
    let ct_arr: ml_kem::Ciphertext<MlKem1024> = ct_raw
        .as_slice()
        .try_into()
        .map_err(|_| "ML-KEM ciphertext has invalid length".to_string())?;
    let shared = dk
        .decapsulate(&ct_arr)
        .map_err(|_| "ml-kem decapsulate failed".to_string())?;

    let nonce =
        hex::decode(&scoped.wrapped_key_nonce_hex).map_err(|e| format!("nonce decode: {e}"))?;
    let wrapped =
        hex::decode(&scoped.wrapped_key_hex).map_err(|e| format!("wrapped decode: {e}"))?;
    let cipher =
        Aes256Gcm::new_from_slice(shared.as_slice()).map_err(|e| format!("cipher init: {e}"))?;
    let key_raw = cipher
        .decrypt(Nonce::from_slice(&nonce), wrapped.as_ref())
        .map_err(|_| "wrapped scope key decrypt failed".to_string())?;
    if key_raw.len() != 32 {
        return Err("scope key has invalid length".to_string());
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&key_raw);
    Ok(ScopeKey::new(key, scope, now_unix()))
}

pub fn reencapsulate_all_agents(session: &mut SessionManager) -> Result<usize, String> {
    if !session.is_unlocked() {
        return Err("session locked".to_string());
    }
    let dir = config::agent_credentials_dir();
    if !dir.exists() {
        return Ok(0);
    }
    let mut rng = OsRng;
    let mut updated = 0usize;

    for ent in std::fs::read_dir(&dir).map_err(|e| format!("read {}: {e}", dir.display()))? {
        let ent = ent.map_err(|e| format!("read entry: {e}"))?;
        let path = ent.path();
        if !path.is_file() {
            continue;
        }
        let raw = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let mut cred: AgentCredential =
            serde_json::from_slice(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;

        let ek_raw =
            hex::decode(&cred.public_key_hex).map_err(|e| format!("public key decode: {e}"))?;
        let ek_arr: Encoded<<MlKem1024 as KemCore>::EncapsulationKey> = ek_raw
            .as_slice()
            .try_into()
            .map_err(|_| "public key has invalid length".to_string())?;
        let ek = <MlKem1024 as KemCore>::EncapsulationKey::from_bytes(&ek_arr);

        let mut new_scopes = HashMap::new();
        for scope_name in cred.scopes.keys().cloned().collect::<Vec<_>>() {
            let scope = CryptoScope::parse(&scope_name)
                .ok_or_else(|| format!("unknown stored scope: {}", scope_name))?;
            let (ct, shared) = ek
                .encapsulate(&mut rng)
                .map_err(|_| "ml-kem encapsulate failed".to_string())?;
            let mut nonce = [0u8; 12];
            getrandom::getrandom(&mut nonce).map_err(|e| format!("rng: {e}"))?;
            let cipher = Aes256Gcm::new_from_slice(shared.as_slice())
                .map_err(|e| format!("cipher init: {e}"))?;
            let scope_key = session
                .get_scope_key(scope)
                .map_err(|_| format!("scope unavailable during re-encapsulation: {scope_name}"))?;
            let wrapped = cipher
                .encrypt(Nonce::from_slice(&nonce), scope_key.key_bytes().as_slice())
                .map_err(|e| format!("wrap scope key: {e}"))?;

            new_scopes.insert(
                scope_name,
                ScopeEncapsulation {
                    kem_ciphertext_hex: hex::encode(ct.as_slice()),
                    wrapped_key_nonce_hex: hex::encode(nonce),
                    wrapped_key_hex: hex::encode(wrapped),
                },
            );
        }

        cred.scopes = new_scopes;
        write_credential_at_path(&path, &cred)?;
        updated = updated.saturating_add(1);
    }

    Ok(updated)
}

fn credential_path(agent_id: &str) -> std::path::PathBuf {
    config::agent_credentials_dir().join(format!("{}.kem", agent_id))
}

fn save_credential(cred: &AgentCredential) -> Result<(), String> {
    config::ensure_agent_credentials_dir()?;
    let path = credential_path(&cred.agent_id);
    write_credential_at_path(&path, cred)
}

fn write_credential_at_path(path: &Path, cred: &AgentCredential) -> Result<(), String> {
    let raw = serde_json::to_vec_pretty(cred).map_err(|e| format!("serialize credential: {e}"))?;
    let tmp = path.with_extension("kem.tmp");
    std::fs::write(&tmp, &raw).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod 600 {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .map_err(|e| format!("rename {} -> {}: {e}", tmp.display(), path.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| format!("chmod 600 {}: {e}", path.display()))?;
    Ok(())
}

fn load_credential(agent_id: &str) -> Result<AgentCredential, String> {
    let path = credential_path(agent_id);
    let raw = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let cred: AgentCredential =
        serde_json::from_slice(&raw).map_err(|e| format!("parse {}: {e}", path.display()))?;
    if cred.schema != AGENT_CRED_SCHEMA {
        return Err(format!("unsupported credential schema {}", cred.schema));
    }
    Ok(cred)
}

fn now_unix() -> u64 {
    crate::halo::util::now_unix_secs()
}
