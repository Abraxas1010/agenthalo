use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};

const LEDGER_VERSION: u8 = 1;
const HASH_DOMAIN: &str = "agenthalo.identity.ledger.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityLedgerKind {
    ProfileUpdated,
    DeviceUpdated,
    NetworkUpdated,
    AnonymousModeUpdated,
    SafetyTierApplied,
    SocialTokenConnected,
    SocialTokenRevoked,
    SuperSecureUpdated,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerSignatureRef {
    pub algorithm: String,
    pub key_id: String,
    pub payload_sha256: String,
    pub signature_hex: String,
    pub signature_digest: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityLedgerEntry {
    pub version: u8,
    pub seq: u64,
    pub timestamp: u64,
    pub kind: IdentityLedgerKind,
    pub provider: Option<String>,
    pub token_ref_sha256: Option<String>,
    pub expires_at: Option<u64>,
    pub status: String,
    #[serde(default)]
    pub payload: Value,
    pub prev_hash: Option<String>,
    pub entry_hash: String,
    pub signature: Option<LedgerSignatureRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SocialProviderProjection {
    pub provider: String,
    pub active: bool,
    pub expired: bool,
    pub most_recent_seq: Option<u64>,
    pub most_recent_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub active_token_ref_sha256: Option<String>,
    pub last_status: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LedgerProjection {
    pub providers: Vec<SocialProviderProjection>,
    pub total_entries: usize,
    pub head_hash: Option<String>,
    pub chain_valid: bool,
    pub signed_entries: usize,
    pub unsigned_entries: usize,
    pub fully_signed: bool,
}

#[derive(Clone, Debug)]
pub struct SocialConnectInput<'a> {
    pub provider: &'a str,
    pub token: &'a str,
    pub expires_at: Option<u64>,
    pub source: &'a str,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn normalize_social_provider(provider: &str) -> String {
    provider.trim().to_ascii_lowercase().replace(' ', "_")
}

pub fn social_token_ref_sha256(provider: &str, token: &str) -> String {
    let provider = normalize_social_provider(provider);
    let mut h = Sha256::new();
    h.update(
        format!(
            "agenthalo.identity.social.token.v1:{provider}:{}",
            token.trim()
        )
        .as_bytes(),
    );
    format!("sha256:{}", crate::halo::util::hex_encode(&h.finalize()))
}

fn entry_payload_for_hash(entry: &IdentityLedgerEntry) -> String {
    let payload_json = serde_json::to_string(&entry.payload)
        .unwrap_or_else(|_| "{\"error\":\"payload\"}".to_string());
    format!(
        "{HASH_DOMAIN}|v={}|seq={}|ts={}|kind={:?}|provider={}|token_ref={}|expires_at={}|status={}|payload={}|prev_hash={}",
        entry.version,
        entry.seq,
        entry.timestamp,
        entry.kind,
        entry.provider.as_deref().unwrap_or(""),
        entry.token_ref_sha256.as_deref().unwrap_or(""),
        entry
            .expires_at
            .map(|v| v.to_string())
            .unwrap_or_else(String::new),
        entry.status,
        payload_json,
        entry.prev_hash.as_deref().unwrap_or(""),
    )
}

fn compute_entry_hash(entry: &IdentityLedgerEntry) -> String {
    let mut h = Sha256::new();
    h.update(entry_payload_for_hash(entry).as_bytes());
    crate::halo::util::hex_encode(&h.finalize())
}

fn ledger_path() -> std::path::PathBuf {
    crate::halo::config::identity_social_ledger_path()
}

pub fn load_entries() -> Result<Vec<IdentityLedgerEntry>, String> {
    let path = ledger_path();
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = std::fs::File::open(&path)
        .map_err(|e| format!("open identity social ledger {}: {e}", path.display()))?;
    let reader = BufReader::new(file);
    let mut out = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let raw = line.map_err(|e| format!("read identity social ledger line {}: {e}", idx + 1))?;
        if raw.trim().is_empty() {
            continue;
        }
        let entry: IdentityLedgerEntry = serde_json::from_str(&raw).map_err(|e| {
            format!(
                "parse identity social ledger line {} at {}: {e}",
                idx + 1,
                path.display()
            )
        })?;
        out.push(entry);
    }
    Ok(out)
}

pub fn verify_chain(entries: &[IdentityLedgerEntry]) -> Result<(), String> {
    let mut last_hash: Option<String> = None;
    let mut expected_seq = 1u64;
    for (idx, entry) in entries.iter().enumerate() {
        if entry.version != LEDGER_VERSION {
            return Err(format!(
                "ledger version mismatch at entry {}: expected {}, got {}",
                idx + 1,
                LEDGER_VERSION,
                entry.version
            ));
        }
        if entry.seq != expected_seq {
            return Err(format!(
                "ledger sequence mismatch at entry {}: expected {}, got {}",
                idx + 1,
                expected_seq,
                entry.seq
            ));
        }
        if entry.prev_hash != last_hash {
            return Err(format!(
                "ledger prev_hash mismatch at entry {}: expected {:?}, got {:?}",
                idx + 1,
                last_hash,
                entry.prev_hash
            ));
        }
        let computed = compute_entry_hash(entry);
        if computed != entry.entry_hash {
            return Err(format!(
                "ledger entry hash mismatch at seq {}: expected {}, got {}",
                entry.seq, computed, entry.entry_hash
            ));
        }
        last_hash = Some(entry.entry_hash.clone());
        expected_seq = expected_seq.saturating_add(1);
    }
    Ok(())
}

fn try_sign_entry_hash(entry_hash: &str) -> Option<LedgerSignatureRef> {
    if !crate::halo::pq::has_wallet() {
        return None;
    }
    let payload = format!("agenthalo.identity.ledger.entry_hash.v1:{entry_hash}");
    let signed = crate::halo::pq::sign_pq_payload(
        payload.as_bytes(),
        "identity_social_ledger_entry",
        Some(entry_hash.to_string()),
    )
    .ok()?;
    let env = signed.0;
    Some(LedgerSignatureRef {
        algorithm: env.algorithm,
        key_id: env.key_id,
        payload_sha256: env.payload_sha256,
        signature_hex: env.signature_hex,
        signature_digest: env.signature_digest,
        created_at: env.created_at,
    })
}

fn append_entry(mut entry: IdentityLedgerEntry) -> Result<IdentityLedgerEntry, String> {
    crate::halo::config::ensure_halo_dir()?;
    let path = ledger_path();
    let mut entries = load_entries()?;
    verify_chain(&entries)?;

    let next_seq = entries.last().map(|e| e.seq.saturating_add(1)).unwrap_or(1);
    let prev_hash = entries.last().map(|e| e.entry_hash.clone());

    entry.seq = next_seq;
    entry.prev_hash = prev_hash;
    entry.entry_hash = compute_entry_hash(&entry);
    entry.signature = try_sign_entry_hash(&entry.entry_hash);

    let line = serde_json::to_string(&entry)
        .map_err(|e| format!("serialize identity ledger entry: {e}"))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open identity social ledger {}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| format!("append identity social ledger: {e}"))?;
    entries.push(entry.clone());
    verify_chain(&entries)?;
    Ok(entry)
}

pub fn append_social_connect(input: SocialConnectInput<'_>) -> Result<IdentityLedgerEntry, String> {
    let provider = normalize_social_provider(input.provider);
    let token_ref = social_token_ref_sha256(&provider, input.token);
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::SocialTokenConnected,
        provider: Some(provider),
        token_ref_sha256: Some(token_ref),
        expires_at: input.expires_at,
        status: "active".to_string(),
        payload: serde_json::json!({
            "source": input.source,
            "token_ref": "stored_in_vault_or_external_secret_store",
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_social_revoke(
    provider: &str,
    reason: Option<&str>,
) -> Result<IdentityLedgerEntry, String> {
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::SocialTokenRevoked,
        provider: Some(normalize_social_provider(provider)),
        token_ref_sha256: None,
        expires_at: None,
        status: "revoked".to_string(),
        payload: serde_json::json!({
            "reason": reason.unwrap_or("operator_requested"),
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_super_secure_update(
    option_key: &str,
    enabled: bool,
    metadata: Value,
) -> Result<IdentityLedgerEntry, String> {
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::SuperSecureUpdated,
        provider: None,
        token_ref_sha256: None,
        expires_at: None,
        status: if enabled { "enabled" } else { "disabled" }.to_string(),
        payload: serde_json::json!({
            "option": option_key,
            "enabled": enabled,
            "metadata": metadata,
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_profile_update(
    display_name: Option<&str>,
    avatar_type: Option<&str>,
    name_locked: bool,
    name_revision: u64,
) -> Result<IdentityLedgerEntry, String> {
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::ProfileUpdated,
        provider: None,
        token_ref_sha256: None,
        expires_at: None,
        status: "updated".to_string(),
        payload: serde_json::json!({
            "display_name_set": display_name.map(|v| !v.trim().is_empty()).unwrap_or(false),
            "display_name_preview": display_name.map(|v| v.chars().take(2).collect::<String>()),
            "avatar_type": avatar_type,
            "name_locked": name_locked,
            "name_revision": name_revision,
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_device_update(
    enabled: bool,
    entropy_bits: u32,
    component_count: usize,
    has_browser_fingerprint: bool,
    puf_fingerprint_hex: Option<&str>,
    puf_tier: Option<&str>,
) -> Result<IdentityLedgerEntry, String> {
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::DeviceUpdated,
        provider: None,
        token_ref_sha256: None,
        expires_at: None,
        status: if enabled { "enabled" } else { "disabled" }.to_string(),
        payload: serde_json::json!({
            "enabled": enabled,
            "entropy_bits": entropy_bits,
            "component_count": component_count,
            "has_browser_fingerprint": has_browser_fingerprint,
            "has_puf_binding": puf_fingerprint_hex.map(|v| !v.trim().is_empty()).unwrap_or(false),
            "puf_tier": puf_tier,
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_network_update(
    share_local_ip: bool,
    share_public_ip: bool,
    share_mac: bool,
    local_ip_hash_present: bool,
    public_ip_hash_present: bool,
    mac_count: usize,
) -> Result<IdentityLedgerEntry, String> {
    let configured = share_local_ip
        || share_public_ip
        || share_mac
        || local_ip_hash_present
        || public_ip_hash_present
        || mac_count > 0;
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::NetworkUpdated,
        provider: None,
        token_ref_sha256: None,
        expires_at: None,
        status: if configured {
            "configured".to_string()
        } else {
            "cleared".to_string()
        },
        payload: serde_json::json!({
            "share_local_ip": share_local_ip,
            "share_public_ip": share_public_ip,
            "share_mac": share_mac,
            "local_ip_hash_present": local_ip_hash_present,
            "public_ip_hash_present": public_ip_hash_present,
            "mac_count": mac_count,
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_anonymous_mode_update(
    enabled: bool,
    cleared_device: bool,
    cleared_network: bool,
) -> Result<IdentityLedgerEntry, String> {
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::AnonymousModeUpdated,
        provider: None,
        token_ref_sha256: None,
        expires_at: None,
        status: if enabled { "enabled" } else { "disabled" }.to_string(),
        payload: serde_json::json!({
            "enabled": enabled,
            "cleared_device": cleared_device,
            "cleared_network": cleared_network,
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

pub fn append_safety_tier_applied(
    tier: &str,
    applied_by: &str,
    step_failures: usize,
) -> Result<IdentityLedgerEntry, String> {
    let normalized = tier.trim().to_ascii_lowercase();
    let entry = IdentityLedgerEntry {
        version: LEDGER_VERSION,
        seq: 0,
        timestamp: now_unix(),
        kind: IdentityLedgerKind::SafetyTierApplied,
        provider: None,
        token_ref_sha256: None,
        expires_at: None,
        status: "applied".to_string(),
        payload: serde_json::json!({
            "tier": normalized,
            "applied_by": applied_by,
            "step_failures": step_failures,
        }),
        prev_hash: None,
        entry_hash: String::new(),
        signature: None,
    };
    append_entry(entry)
}

/// Build the current immutable-ledger projection:
/// per-provider social activity plus global chain/signing status.
pub fn project_ledger_status(now: u64) -> Result<LedgerProjection, String> {
    let entries = load_entries()?;
    let chain_valid = verify_chain(&entries).is_ok();
    let signed_entries = entries.iter().filter(|e| e.signature.is_some()).count();
    let unsigned_entries = entries.len().saturating_sub(signed_entries);
    let mut map: BTreeMap<String, SocialProviderProjection> = BTreeMap::new();

    for entry in &entries {
        let Some(provider) = entry.provider.as_deref() else {
            continue;
        };
        let state = map
            .entry(provider.to_string())
            .or_insert(SocialProviderProjection {
                provider: provider.to_string(),
                active: false,
                expired: false,
                most_recent_seq: None,
                most_recent_at: None,
                expires_at: None,
                active_token_ref_sha256: None,
                last_status: None,
            });

        state.most_recent_seq = Some(entry.seq);
        state.most_recent_at = Some(entry.timestamp);
        state.last_status = Some(entry.status.clone());

        match entry.kind {
            IdentityLedgerKind::ProfileUpdated
            | IdentityLedgerKind::DeviceUpdated
            | IdentityLedgerKind::NetworkUpdated
            | IdentityLedgerKind::AnonymousModeUpdated
            | IdentityLedgerKind::SafetyTierApplied => {}
            IdentityLedgerKind::SocialTokenConnected => {
                let expired = entry.expires_at.map(|exp| exp <= now).unwrap_or(false);
                state.expired = expired;
                state.active = !expired;
                state.expires_at = entry.expires_at;
                state.active_token_ref_sha256 = if expired {
                    None
                } else {
                    entry.token_ref_sha256.clone()
                };
            }
            IdentityLedgerKind::SocialTokenRevoked => {
                state.active = false;
                state.expires_at = None;
                state.active_token_ref_sha256 = None;
                state.expired = false;
            }
            IdentityLedgerKind::SuperSecureUpdated => {}
        }
    }

    Ok(LedgerProjection {
        providers: map.into_values().collect(),
        total_entries: entries.len(),
        head_hash: entries.last().map(|e| e.entry_hash.clone()),
        chain_valid,
        signed_entries,
        unsigned_entries,
        fully_signed: entries.is_empty() || unsigned_entries == 0,
    })
}

pub fn project_social_status(now: u64) -> Result<LedgerProjection, String> {
    project_ledger_status(now)
}

pub fn encode_oauth_state(provider: &str, expires_at: u64, secret: &str) -> String {
    let payload = serde_json::json!({
        "provider": normalize_social_provider(provider),
        "expires_at": expires_at,
    });
    let payload_raw = serde_json::to_vec(&payload).unwrap_or_default();
    let payload_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_raw);
    let mut h = Sha256::new();
    h.update(format!("agenthalo.identity.oauth.v1:{payload_b64}:{secret}").as_bytes());
    let sig = crate::halo::util::hex_encode(&h.finalize());
    format!("{payload_b64}.{sig}")
}

pub fn decode_oauth_state(
    raw: &str,
    expected_provider: &str,
    now: u64,
    secret: &str,
) -> Result<(), String> {
    let (payload_b64, sig) = raw
        .split_once('.')
        .ok_or_else(|| "invalid oauth state format".to_string())?;
    let mut h = Sha256::new();
    h.update(format!("agenthalo.identity.oauth.v1:{payload_b64}:{secret}").as_bytes());
    let expected_sig = crate::halo::util::hex_encode(&h.finalize());
    if expected_sig != sig {
        return Err("oauth state signature mismatch".to_string());
    }
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| format!("decode oauth state payload: {e}"))?;
    let val: Value =
        serde_json::from_slice(&decoded).map_err(|e| format!("parse oauth state payload: {e}"))?;
    let provider = val
        .get("provider")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "oauth state missing provider".to_string())?;
    if normalize_social_provider(provider) != normalize_social_provider(expected_provider) {
        return Err("oauth state provider mismatch".to_string());
    }
    let expires_at = val
        .get("expires_at")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "oauth state missing expires_at".to_string())?;
    if expires_at < now {
        return Err("oauth state expired".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct TmpHomeGuard {
        path: std::path::PathBuf,
        previous_home: Option<String>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl TmpHomeGuard {
        fn new(tag: &str) -> Self {
            let guard = env_lock().lock().expect("env lock");
            let previous_home = std::env::var("AGENTHALO_HOME").ok();
            let path = std::env::temp_dir().join(format!(
                "identity_ledger_{}_{}_{}",
                tag,
                std::process::id(),
                now_unix()
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("create tmp");
            std::env::set_var("AGENTHALO_HOME", &path);
            Self {
                path,
                previous_home,
                _guard: guard,
            }
        }
    }

    impl Drop for TmpHomeGuard {
        fn drop(&mut self) {
            if let Some(prev) = self.previous_home.as_deref() {
                std::env::set_var("AGENTHALO_HOME", prev);
            } else {
                std::env::remove_var("AGENTHALO_HOME");
            }
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn set_tmp_home(tag: &str) -> TmpHomeGuard {
        TmpHomeGuard::new(tag)
    }

    #[test]
    fn append_and_verify_social_chain() {
        let _home = set_tmp_home("chain");
        let _ = append_social_connect(SocialConnectInput {
            provider: "google",
            token: "tok-1",
            expires_at: Some(now_unix() + 3600),
            source: "test",
        })
        .expect("append connect");
        let _ = append_social_revoke("google", Some("test revoke")).expect("append revoke");
        let entries = load_entries().expect("load entries");
        assert_eq!(entries.len(), 2);
        verify_chain(&entries).expect("chain should verify");
    }

    #[test]
    fn projection_marks_expired_token_inactive() {
        let _home = set_tmp_home("projection");
        let _ = append_social_connect(SocialConnectInput {
            provider: "google",
            token: "tok-expired",
            expires_at: Some(now_unix().saturating_sub(5)),
            source: "test",
        })
        .expect("append connect");
        let proj = project_ledger_status(now_unix()).expect("project");
        assert_eq!(proj.providers.len(), 1);
        assert!(!proj.providers[0].active);
        assert!(proj.providers[0].expired);
    }

    #[test]
    fn oauth_state_roundtrip() {
        let secret = "test-secret";
        let raw = encode_oauth_state("google", now_unix() + 60, secret);
        decode_oauth_state(&raw, "google", now_unix(), secret).expect("state must validate");
    }

    #[test]
    fn appends_all_identity_mutation_kinds() {
        let _home = set_tmp_home("all_kinds");
        append_profile_update(Some("Hal"), Some("initials"), true, 0).expect("profile");
        append_device_update(true, 64, 3, true, Some("sha256:abc"), Some("consumer"))
            .expect("device");
        append_network_update(true, false, true, true, false, 1).expect("network");
        append_anonymous_mode_update(false, false, false).expect("anon");
        append_safety_tier_applied("max-safe", "test", 0).expect("tier");
        append_super_secure_update("totp", true, serde_json::json!({"label":"T"}))
            .expect("super secure");

        let entries = load_entries().expect("load entries");
        assert_eq!(entries.len(), 6);
        verify_chain(&entries).expect("chain should verify");
    }
}
