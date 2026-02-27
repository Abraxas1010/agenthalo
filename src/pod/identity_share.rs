//! Identity-to-POD sharing projection.
//!
//! This module organizes identity attributes into a stable keyspace so
//! existing POD grant patterns (`identity/*`, `identity/social/*`, etc.)
//! can selectively share identity data without exposing raw secrets.

use crate::halo::identity::IdentityConfig;
use crate::halo::identity_ledger::LedgerProjection;
use crate::halo::profile::UserProfile;
use crate::pod::acl::{key_pattern_matches, GrantStore};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IdentityShareRecord {
    pub key: String,
    pub value: Value,
    pub sensitivity: String,
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
}
