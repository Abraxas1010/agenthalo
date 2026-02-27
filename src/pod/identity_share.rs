//! Identity-to-POD sharing projection.
//!
//! This module organizes identity attributes into a stable keyspace so
//! existing POD grant patterns (`identity/*`, `identity/social/*`, etc.)
//! can selectively share identity data without exposing raw secrets.

use crate::halo::identity::IdentityConfig;
use crate::halo::identity_ledger::LedgerProjection;
use crate::halo::profile::UserProfile;
use crate::pod::acl::{key_pattern_matches, GrantStore};
use crate::transparency::ct6962::{hex_encode, leaf_hash, merkle_tree_hash};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const IDENTITY_POD_SHARE_DOMAIN: &str = "agenthalo.identity.pod_share.v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityShareRecord {
    pub key: String,
    pub value: Value,
    pub sensitivity: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityShareProofEnvelope {
    pub version: u32,
    pub domain: String,
    pub created_at: u64,
    pub payload_sha256: String,
    pub records_merkle_root: String,
    pub records_merkle_tree_size: usize,
    pub ledger_head_hash: Option<String>,
    pub ledger_total_entries: usize,
    pub ledger_chain_valid: bool,
    pub ledger_signed_entries: usize,
    pub ledger_unsigned_entries: usize,
    pub ledger_fully_signed: bool,
    pub patterns: Vec<String>,
    pub require_grants: bool,
    pub grantee_puf_hex: Option<String>,
    pub record_count: usize,
    pub provenance_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<crate::halo::pq::PqSignatureEnvelope>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityShareEnvelopeVerification {
    pub payload_hash_valid: bool,
    pub merkle_root_valid: bool,
    pub signature_present: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_valid: Option<bool>,
    pub signature_policy_ok: bool,
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

fn record<K: Into<String>, S: Into<String>>(
    key: K,
    value: Value,
    sensitivity: S,
) -> IdentityShareRecord {
    IdentityShareRecord {
        key: key.into(),
        value,
        sensitivity: sensitivity.into(),
    }
}

/// Default selector for identity POD sharing.
pub fn default_identity_patterns() -> Vec<String> {
    vec!["identity/*".to_string()]
}

/// Materialize identity data into POD-shareable records.
///
/// Keys are intentionally namespaced under `identity/*`.
/// Raw social tokens are never included.
pub fn materialize_identity_records(
    profile: &UserProfile,
    identity: &IdentityConfig,
    ledger: &LedgerProjection,
) -> Vec<IdentityShareRecord> {
    let mut out = vec![
        record(
            "identity/profile/display_name",
            serde_json::json!(profile.display_name),
            "public",
        ),
        record(
            "identity/profile/avatar_type",
            serde_json::json!(profile.avatar_type),
            "public",
        ),
        record(
            "identity/profile/has_name",
            serde_json::json!(profile.has_name()),
            "public",
        ),
        record(
            "identity/mode/anonymous",
            serde_json::json!(identity.anonymous_mode),
            "sensitive",
        ),
        record(
            "identity/mode/security_tier",
            serde_json::json!(identity.security_tier),
            "sensitive",
        ),
    ];

    if let Some(device) = identity.device.as_ref() {
        out.push(record(
            "identity/device/enabled",
            serde_json::json!(device.enabled),
            "sensitive",
        ));
        out.push(record(
            "identity/device/entropy_bits",
            serde_json::json!(device.entropy_bits),
            "sensitive",
        ));
        out.push(record(
            "identity/device/last_collected",
            serde_json::json!(device.last_collected),
            "sensitive",
        ));
        out.push(record(
            "identity/device/puf_fingerprint_hex",
            serde_json::json!(device.puf_fingerprint_hex),
            "high",
        ));
        out.push(record(
            "identity/device/puf_tier",
            serde_json::json!(device.puf_tier),
            "high",
        ));
    } else {
        out.push(record(
            "identity/device/enabled",
            serde_json::json!(false),
            "sensitive",
        ));
    }

    if let Some(network) = identity.network.as_ref() {
        out.push(record(
            "identity/network/share_local_ip",
            serde_json::json!(network.share_local_ip),
            "sensitive",
        ));
        out.push(record(
            "identity/network/share_public_ip",
            serde_json::json!(network.share_public_ip),
            "sensitive",
        ));
        out.push(record(
            "identity/network/share_mac",
            serde_json::json!(network.share_mac),
            "sensitive",
        ));
        out.push(record(
            "identity/network/mac_count",
            serde_json::json!(network.mac_addresses.len()),
            "sensitive",
        ));
    } else {
        out.push(record(
            "identity/network/share_local_ip",
            serde_json::json!(false),
            "sensitive",
        ));
        out.push(record(
            "identity/network/share_public_ip",
            serde_json::json!(false),
            "sensitive",
        ));
        out.push(record(
            "identity/network/share_mac",
            serde_json::json!(false),
            "sensitive",
        ));
    }

    let mut providers = BTreeSet::new();
    for p in identity.social.providers.keys() {
        providers.insert(p.clone());
    }
    for p in &ledger.providers {
        providers.insert(p.provider.clone());
    }
    for provider in providers {
        let cfg_state = identity
            .social
            .providers
            .get(&provider)
            .cloned()
            .unwrap_or_default();
        let projected = ledger.providers.iter().find(|p| p.provider == provider);
        out.push(record(
            format!("identity/social/{provider}/selected"),
            serde_json::json!(cfg_state.selected),
            "sensitive",
        ));
        out.push(record(
            format!("identity/social/{provider}/active"),
            serde_json::json!(projected.map(|p| p.active).unwrap_or(false)),
            "sensitive",
        ));
        out.push(record(
            format!("identity/social/{provider}/expired"),
            serde_json::json!(projected.map(|p| p.expired).unwrap_or(false)),
            "sensitive",
        ));
        out.push(record(
            format!("identity/social/{provider}/expires_at"),
            serde_json::json!(projected
                .and_then(|p| p.expires_at)
                .or(cfg_state.expires_at)),
            "sensitive",
        ));
        out.push(record(
            format!("identity/social/{provider}/last_status"),
            serde_json::json!(projected.and_then(|p| p.last_status.clone())),
            "sensitive",
        ));
    }

    out.push(record(
        "identity/super_secure/passkey_enabled",
        serde_json::json!(identity.super_secure.passkey_enabled),
        "high",
    ));
    out.push(record(
        "identity/super_secure/security_key_enabled",
        serde_json::json!(identity.super_secure.security_key_enabled),
        "high",
    ));
    out.push(record(
        "identity/super_secure/totp_enabled",
        serde_json::json!(identity.super_secure.totp_enabled),
        "high",
    ));
    out.push(record(
        "identity/super_secure/totp_label",
        serde_json::json!(identity.super_secure.totp_label),
        "high",
    ));
    out.push(record(
        "identity/super_secure/last_updated",
        serde_json::json!(identity.super_secure.last_updated),
        "high",
    ));

    out.push(record(
        "identity/ledger/total_entries",
        serde_json::json!(ledger.total_entries),
        "internal",
    ));
    out.push(record(
        "identity/ledger/head_hash",
        serde_json::json!(ledger.head_hash),
        "internal",
    ));
    out.push(record(
        "identity/ledger/chain_valid",
        serde_json::json!(ledger.chain_valid),
        "internal",
    ));
    out.push(record(
        "identity/ledger/signed_entries",
        serde_json::json!(ledger.signed_entries),
        "internal",
    ));
    out.push(record(
        "identity/ledger/unsigned_entries",
        serde_json::json!(ledger.unsigned_entries),
        "internal",
    ));
    out.push(record(
        "identity/ledger/fully_signed",
        serde_json::json!(ledger.fully_signed),
        "internal",
    ));

    out
}

/// Filter records by POD key patterns (`*` + trailing-glob semantics).
pub fn select_records_by_patterns(
    records: &[IdentityShareRecord],
    patterns: &[String],
) -> Vec<IdentityShareRecord> {
    let effective = if patterns.is_empty() {
        default_identity_patterns()
    } else {
        patterns.to_vec()
    };
    records
        .iter()
        .filter(|r| effective.iter().any(|p| key_pattern_matches(p, &r.key)))
        .cloned()
        .collect()
}

/// Enforce grants over identity records for a specific grantee.
///
/// Returns `(allowed_records, denied_keys)`.
pub fn filter_records_by_grants(
    records: &[IdentityShareRecord],
    grants: &GrantStore,
    grantee_puf: &[u8; 32],
) -> (Vec<IdentityShareRecord>, Vec<String>) {
    let mut allowed = Vec::new();
    let mut denied = Vec::new();
    for rec in records {
        if grants.can_read(grantee_puf, &rec.key) {
            allowed.push(rec.clone());
        } else {
            denied.push(rec.key.clone());
        }
    }
    (allowed, denied)
}

/// Convert records to a deterministic JSON map (`{ key: value }`).
pub fn records_to_json_map(records: &[IdentityShareRecord]) -> Value {
    let mut out = serde_json::Map::new();
    for rec in records {
        out.insert(rec.key.clone(), rec.value.clone());
    }
    Value::Object(out)
}

fn canonical_records(records: &[IdentityShareRecord]) -> Vec<IdentityShareRecord> {
    let mut ordered = records.to_vec();
    ordered.sort_by(|a, b| a.key.cmp(&b.key));
    ordered
}

fn canonical_payload(
    records: &[IdentityShareRecord],
    ledger: &LedgerProjection,
    patterns: &[String],
    require_grants: bool,
    grantee_puf_hex: Option<&str>,
) -> Value {
    let ordered = canonical_records(records);
    let ordered_patterns: Vec<String> = patterns.iter().map(|p| p.trim().to_string()).collect();
    serde_json::json!({
        "domain": IDENTITY_POD_SHARE_DOMAIN,
        "version": 1,
        "records": ordered,
        "patterns": ordered_patterns,
        "require_grants": require_grants,
        "grantee_puf_hex": grantee_puf_hex,
        "ledger_head_hash": ledger.head_hash,
        "ledger_total_entries": ledger.total_entries,
        "ledger_chain_valid": ledger.chain_valid,
        "ledger_signed_entries": ledger.signed_entries,
        "ledger_unsigned_entries": ledger.unsigned_entries,
        "ledger_fully_signed": ledger.fully_signed,
    })
}

fn payload_sha256_hex(payload: &Value) -> Result<String, String> {
    let raw = serde_json::to_vec(payload).map_err(|e| format!("serialize share payload: {e}"))?;
    let mut h = Sha256::new();
    h.update(raw);
    Ok(format!("sha256:{}", hex_encode(&h.finalize())))
}

fn records_merkle_root_hex(records: &[IdentityShareRecord]) -> Result<(String, usize), String> {
    let ordered = canonical_records(records);
    let mut leaves = Vec::with_capacity(ordered.len());
    for rec in &ordered {
        let leaf_payload = serde_json::to_vec(&serde_json::json!({
            "key": rec.key,
            "value": rec.value,
            "sensitivity": rec.sensitivity,
        }))
        .map_err(|e| format!("serialize identity share record leaf: {e}"))?;
        leaves.push(leaf_hash(&leaf_payload));
    }
    let root = merkle_tree_hash(&leaves);
    Ok((hex_encode(&root), leaves.len()))
}

fn compute_provenance_hash_hex(
    payload_sha256: &str,
    merkle_root: &str,
    ledger_head_hash: Option<&str>,
) -> String {
    let mut h = Sha256::new();
    h.update(IDENTITY_POD_SHARE_DOMAIN.as_bytes());
    h.update([0u8]);
    h.update(payload_sha256.as_bytes());
    h.update([0u8]);
    h.update(merkle_root.as_bytes());
    h.update([0u8]);
    h.update(ledger_head_hash.unwrap_or("").as_bytes());
    format!("sha256:{}", hex_encode(&h.finalize()))
}

pub fn build_share_envelope(
    records: &[IdentityShareRecord],
    ledger: &LedgerProjection,
    patterns: &[String],
    require_grants: bool,
    grantee_puf_hex: Option<&str>,
) -> Result<IdentityShareProofEnvelope, String> {
    let payload = canonical_payload(records, ledger, patterns, require_grants, grantee_puf_hex);
    let payload_sha256 = payload_sha256_hex(&payload)?;
    let (records_merkle_root, records_merkle_tree_size) = records_merkle_root_hex(records)?;
    let provenance_hash = compute_provenance_hash_hex(
        &payload_sha256,
        &records_merkle_root,
        ledger.head_hash.as_deref(),
    );

    let signature = if crate::halo::pq::has_wallet() {
        let sign_payload = serde_json::to_vec(&serde_json::json!({
            "domain": IDENTITY_POD_SHARE_DOMAIN,
            "payload_sha256": payload_sha256,
            "records_merkle_root": records_merkle_root,
            "ledger_head_hash": ledger.head_hash,
            "provenance_hash": provenance_hash,
        }))
        .map_err(|e| format!("serialize share signature payload: {e}"))?;
        match crate::halo::pq::sign_pq_payload(
            &sign_payload,
            "identity_pod_share_envelope",
            Some(provenance_hash.clone()),
        ) {
            Ok((env, _path)) => Some(env),
            Err(_) => None,
        }
    } else {
        None
    };

    Ok(IdentityShareProofEnvelope {
        version: 1,
        domain: IDENTITY_POD_SHARE_DOMAIN.to_string(),
        created_at: crate::pod::now_unix(),
        payload_sha256,
        records_merkle_root,
        records_merkle_tree_size,
        ledger_head_hash: ledger.head_hash.clone(),
        ledger_total_entries: ledger.total_entries,
        ledger_chain_valid: ledger.chain_valid,
        ledger_signed_entries: ledger.signed_entries,
        ledger_unsigned_entries: ledger.unsigned_entries,
        ledger_fully_signed: ledger.fully_signed,
        patterns: patterns.to_vec(),
        require_grants,
        grantee_puf_hex: grantee_puf_hex.map(str::to_string),
        record_count: records.len(),
        provenance_hash,
        signature,
    })
}

pub fn verify_share_envelope(
    envelope: &IdentityShareProofEnvelope,
    records: &[IdentityShareRecord],
    ledger: &LedgerProjection,
    patterns: &[String],
    require_grants: bool,
    grantee_puf_hex: Option<&str>,
) -> IdentityShareEnvelopeVerification {
    verify_share_envelope_with_policy(
        envelope,
        records,
        ledger,
        patterns,
        require_grants,
        grantee_puf_hex,
        false,
    )
}

pub fn verify_share_envelope_with_policy(
    envelope: &IdentityShareProofEnvelope,
    records: &[IdentityShareRecord],
    ledger: &LedgerProjection,
    patterns: &[String],
    require_grants: bool,
    grantee_puf_hex: Option<&str>,
    require_signature: bool,
) -> IdentityShareEnvelopeVerification {
    let payload = canonical_payload(records, ledger, patterns, require_grants, grantee_puf_hex);
    let payload_hash_valid = payload_sha256_hex(&payload)
        .map(|h| h == envelope.payload_sha256)
        .unwrap_or(false);
    let merkle_root_valid = records_merkle_root_hex(records)
        .map(|(root, size)| {
            root == envelope.records_merkle_root && size == envelope.records_merkle_tree_size
        })
        .unwrap_or(false);
    let signature_present = envelope.signature.is_some();
    let signature_valid = if let Some(sig) = envelope.signature.as_ref() {
        let sign_payload = serde_json::to_vec(&serde_json::json!({
            "domain": IDENTITY_POD_SHARE_DOMAIN,
            "payload_sha256": envelope.payload_sha256,
            "records_merkle_root": envelope.records_merkle_root,
            "ledger_head_hash": envelope.ledger_head_hash,
            "provenance_hash": envelope.provenance_hash,
        }))
        .ok();
        sign_payload
            .map(|bytes| {
                crate::halo::pq::verify_detached_signature(
                    &bytes,
                    &sig.public_key_hex,
                    &sig.signature_hex,
                )
                .unwrap_or(false)
            })
            .map(Some)
            .unwrap_or(Some(false))
    } else {
        None
    };
    let signature_policy_ok = if require_signature {
        signature_valid == Some(true)
    } else {
        signature_valid.unwrap_or(true)
    };
    let accepted = payload_hash_valid && merkle_root_valid && signature_policy_ok;
    let rejection_reason = if !payload_hash_valid {
        Some("identity share payload hash mismatch".to_string())
    } else if !merkle_root_valid {
        Some("identity share records merkle root mismatch".to_string())
    } else if require_signature && !signature_present {
        Some("identity share signature is required but missing".to_string())
    } else if signature_valid == Some(false) {
        Some("identity share signature verification failed".to_string())
    } else {
        None
    };
    IdentityShareEnvelopeVerification {
        payload_hash_valid,
        merkle_root_valid,
        signature_present,
        signature_valid,
        signature_policy_ok,
        accepted,
        rejection_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::identity::IdentityConfig;
    use crate::halo::identity_ledger::LedgerProjection;
    use crate::halo::profile::UserProfile;
    use crate::pod::acl::{GrantPermissions, GrantRequest};

    #[test]
    fn pattern_filter_keeps_identity_namespace() {
        let profile = UserProfile::default();
        let identity = IdentityConfig::default();
        let ledger = LedgerProjection {
            providers: vec![],
            total_entries: 0,
            head_hash: None,
            chain_valid: true,
            signed_entries: 0,
            unsigned_entries: 0,
            fully_signed: true,
        };
        let records = materialize_identity_records(&profile, &identity, &ledger);
        let selected = select_records_by_patterns(&records, &["identity/profile/*".to_string()]);
        assert!(!selected.is_empty());
        assert!(selected
            .iter()
            .all(|r| r.key.starts_with("identity/profile/")));
    }

    #[test]
    fn grant_filter_hides_ungranted_identity_keys() {
        let profile = UserProfile::default();
        let identity = IdentityConfig::default();
        let ledger = LedgerProjection {
            providers: vec![],
            total_entries: 0,
            head_hash: None,
            chain_valid: true,
            signed_entries: 0,
            unsigned_entries: 0,
            fully_signed: true,
        };
        let records = materialize_identity_records(&profile, &identity, &ledger);
        let mut store = GrantStore::new();
        let mut grantor = [0u8; 32];
        grantor[0] = 0xAA;
        let mut grantee = [0u8; 32];
        grantee[0] = 0xBB;
        store.create(GrantRequest {
            grantor_puf: grantor,
            grantee_puf: grantee,
            key_pattern: "identity/profile/*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: None,
        });
        let (allowed, denied) = filter_records_by_grants(&records, &store, &grantee);
        assert!(!allowed.is_empty());
        assert!(!denied.is_empty());
        assert!(allowed
            .iter()
            .all(|r| r.key.starts_with("identity/profile/")));
    }

    #[test]
    fn share_envelope_roundtrip_verifies() {
        let profile = UserProfile::default();
        let identity = IdentityConfig::default();
        let ledger = LedgerProjection {
            providers: vec![],
            total_entries: 0,
            head_hash: None,
            chain_valid: true,
            signed_entries: 0,
            unsigned_entries: 0,
            fully_signed: true,
        };
        let records = materialize_identity_records(&profile, &identity, &ledger);
        let patterns = vec!["identity/*".to_string()];
        let envelope =
            build_share_envelope(&records, &ledger, &patterns, false, None).expect("build env");
        let verify = verify_share_envelope(&envelope, &records, &ledger, &patterns, false, None);
        assert!(verify.accepted, "envelope should verify: {verify:?}");
    }
}
