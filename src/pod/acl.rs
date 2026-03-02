//! Per-key access control grants for NucleusPOD.
//!
//! Grants allow fine-grained sharing: a POD owner can grant read access to
//! specific keys (or key patterns) to specific agents, with optional expiry.
//!
//! Grants are stored as NucleusDB records themselves — the access control
//! metadata is committed with the same Merkle proofs as the data.

use crate::pod::{now_unix, now_unix_nanos};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Permission flags for a grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantPermissions {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    #[serde(default)]
    pub control: bool,
}

impl GrantPermissions {
    pub fn read_only() -> Self {
        Self {
            read: true,
            write: false,
            append: false,
            control: false,
        }
    }

    pub fn read_write() -> Self {
        Self {
            read: true,
            write: true,
            append: false,
            control: false,
        }
    }

    pub fn all() -> Self {
        Self {
            read: true,
            write: true,
            append: true,
            control: false,
        }
    }

    pub fn owner() -> Self {
        Self {
            read: true,
            write: true,
            append: true,
            control: true,
        }
    }
}

/// A per-key access grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessGrant {
    /// Content-hash identifier for this grant.
    pub grant_id: [u8; 32],
    /// PUF fingerprint of the grantor (POD owner).
    pub grantor_puf: [u8; 32],
    /// PUF fingerprint of the grantee (consumer agent).
    pub grantee_puf: [u8; 32],
    /// Key pattern: exact key or glob with trailing `*`.
    /// Examples: `"results/theorem_42"`, `"results/*"`, `"*"` (all keys).
    pub key_pattern: String,
    /// Permissions granted.
    pub permissions: GrantPermissions,
    /// Optional expiry timestamp (Unix seconds). `None` = no expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
    /// Creation timestamp.
    pub created_at: u64,
    /// Nonce to prevent same-second grant ID collisions under automation.
    #[serde(default)]
    pub nonce: u64,
    /// Whether this grant has been revoked.
    pub revoked: bool,
}

/// Input for creating a new grant.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrantRequest {
    pub grantor_puf: [u8; 32],
    pub grantee_puf: [u8; 32],
    pub key_pattern: String,
    pub permissions: GrantPermissions,
    pub expires_at: Option<u64>,
}

/// In-memory grant store (backed by a Vec for simplicity; can be persisted via NucleusDB records).
#[derive(Clone, Debug, Default)]
pub struct GrantStore {
    grants: Vec<AccessGrant>,
}

/// Thread-safe shared grant store for HTTP handler state.
pub type SharedGrantStore = Arc<RwLock<GrantStore>>;

impl AccessGrant {
    /// Compute the grant ID as SHA-256(grantor | grantee | pattern | created_at | nonce).
    fn compute_id(
        grantor_puf: &[u8; 32],
        grantee_puf: &[u8; 32],
        key_pattern: &str,
        created_at: u64,
        nonce: u64,
    ) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"nucleusdb.pod.grant.v1|");
        h.update(grantor_puf);
        h.update(grantee_puf);
        h.update(key_pattern.as_bytes());
        h.update(created_at.to_le_bytes());
        h.update(nonce.to_le_bytes());
        h.finalize().into()
    }

    /// Check if this grant matches a given key.
    pub fn matches_key(&self, key: &str) -> bool {
        key_pattern_matches(&self.key_pattern, key)
    }

    /// Check if this grant is currently active (not revoked, not expired).
    pub fn is_active(&self) -> bool {
        if self.revoked {
            return false;
        }
        if let Some(expires_at) = self.expires_at {
            return now_unix() < expires_at;
        }
        true
    }
}

/// Match a key against a pattern.
///
/// - Exact match: `"results/theorem_42"` matches `"results/theorem_42"` only.
/// - Prefix glob: `"results/*"` matches any key starting with `"results/"`.
/// - Wildcard: `"*"` matches everything.
pub fn key_pattern_matches(pattern: &str, key: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return key.starts_with(prefix);
    }
    pattern == key
}

impl GrantStore {
    pub fn new() -> Self {
        Self { grants: vec![] }
    }

    pub fn from_grants(grants: Vec<AccessGrant>) -> Self {
        Self { grants }
    }

    pub fn shared() -> SharedGrantStore {
        Arc::new(RwLock::new(Self::new()))
    }

    pub fn replace_all(&mut self, grants: Vec<AccessGrant>) {
        self.grants = grants;
    }

    /// Create a new grant and return it.
    pub fn create(&mut self, request: GrantRequest) -> AccessGrant {
        let created_at = now_unix();
        let nonce = next_grant_nonce();
        let grant_id = AccessGrant::compute_id(
            &request.grantor_puf,
            &request.grantee_puf,
            &request.key_pattern,
            created_at,
            nonce,
        );
        let grant = AccessGrant {
            grant_id,
            grantor_puf: request.grantor_puf,
            grantee_puf: request.grantee_puf,
            key_pattern: request.key_pattern,
            permissions: request.permissions,
            expires_at: request.expires_at,
            created_at,
            nonce,
            revoked: false,
        };
        self.grants.push(grant.clone());
        grant
    }

    /// Revoke a grant by ID. Returns true if found and revoked.
    pub fn revoke(&mut self, grant_id: &[u8; 32]) -> bool {
        for g in &mut self.grants {
            if &g.grant_id == grant_id {
                g.revoked = true;
                return true;
            }
        }
        false
    }

    /// Find a grant by ID.
    pub fn get(&self, grant_id: &[u8; 32]) -> Option<&AccessGrant> {
        self.grants.iter().find(|g| &g.grant_id == grant_id)
    }

    /// List all grants (including revoked/expired).
    pub fn list_all(&self) -> &[AccessGrant] {
        &self.grants
    }

    /// List active grants for a given grantee.
    pub fn active_for_grantee(&self, grantee_puf: &[u8; 32]) -> Vec<&AccessGrant> {
        self.grants
            .iter()
            .filter(|g| &g.grantee_puf == grantee_puf && g.is_active())
            .collect()
    }

    /// Check if a grantee has read access to a specific key.
    pub fn can_read(&self, grantee_puf: &[u8; 32], key: &str) -> bool {
        self.grants.iter().any(|g| {
            &g.grantee_puf == grantee_puf
                && g.is_active()
                && g.permissions.read
                && g.matches_key(key)
        })
    }

    /// Check if a grantee has write access to a specific key.
    pub fn can_write(&self, grantee_puf: &[u8; 32], key: &str) -> bool {
        self.grants.iter().any(|g| {
            &g.grantee_puf == grantee_puf
                && g.is_active()
                && g.permissions.write
                && g.matches_key(key)
        })
    }

    /// Check if a grantee has control access to a specific key.
    pub fn can_control(&self, grantee_puf: &[u8; 32], key: &str) -> bool {
        self.grants.iter().any(|g| {
            &g.grantee_puf == grantee_puf
                && g.is_active()
                && g.permissions.control
                && g.matches_key(key)
        })
    }
}

static GRANT_NONCE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_grant_nonce() -> u64 {
    let ts_mix = now_unix_nanos() as u64;
    let ctr = GRANT_NONCE_COUNTER.fetch_add(1, Ordering::Relaxed);
    ts_mix ^ ctr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn puf_a() -> [u8; 32] {
        let mut p = [0u8; 32];
        p[0] = 0xAA;
        p
    }

    fn puf_b() -> [u8; 32] {
        let mut p = [0u8; 32];
        p[0] = 0xBB;
        p
    }

    #[test]
    fn pattern_exact_match() {
        assert!(key_pattern_matches(
            "results/theorem_42",
            "results/theorem_42"
        ));
        assert!(!key_pattern_matches(
            "results/theorem_42",
            "results/theorem_43"
        ));
    }

    #[test]
    fn pattern_glob_match() {
        assert!(key_pattern_matches("results/*", "results/theorem_42"));
        assert!(key_pattern_matches("results/*", "results/foo/bar"));
        assert!(!key_pattern_matches("results/*", "other/key"));
    }

    #[test]
    fn pattern_wildcard_match() {
        assert!(key_pattern_matches("*", "anything"));
        assert!(key_pattern_matches("*", "results/theorem_42"));
    }

    #[test]
    fn grant_create_and_read_check() {
        let mut store = GrantStore::new();
        store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "results/*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: None,
        });

        assert!(store.can_read(&puf_b(), "results/theorem_42"));
        assert!(!store.can_read(&puf_b(), "private/secret"));
        assert!(!store.can_write(&puf_b(), "results/theorem_42"));
    }

    #[test]
    fn grant_revocation() {
        let mut store = GrantStore::new();
        let grant = store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "*".to_string(),
            permissions: GrantPermissions::all(),
            expires_at: None,
        });

        assert!(store.can_read(&puf_b(), "anything"));

        store.revoke(&grant.grant_id);
        assert!(!store.can_read(&puf_b(), "anything"));
    }

    #[test]
    fn grant_expiry() {
        let mut store = GrantStore::new();
        store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: Some(1), // Expired in 1970.
        });

        assert!(!store.can_read(&puf_b(), "anything"));
    }

    #[test]
    fn grant_list_active() {
        let mut store = GrantStore::new();
        store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "a/*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: None,
        });
        let g2 = store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "b/*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: None,
        });
        store.revoke(&g2.grant_id);

        let active = store.active_for_grantee(&puf_b());
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].key_pattern, "a/*");
    }

    #[test]
    fn grant_id_is_deterministic() {
        let id1 = AccessGrant::compute_id(&puf_a(), &puf_b(), "test/*", 12345, 7);
        let id2 = AccessGrant::compute_id(&puf_a(), &puf_b(), "test/*", 12345, 7);
        assert_eq!(id1, id2);

        let id3 = AccessGrant::compute_id(&puf_a(), &puf_b(), "other/*", 12345, 7);
        assert_ne!(id1, id3);
    }

    #[test]
    fn grant_id_changes_with_nonce_for_same_second() {
        let id1 = AccessGrant::compute_id(&puf_a(), &puf_b(), "test/*", 12345, 1);
        let id2 = AccessGrant::compute_id(&puf_a(), &puf_b(), "test/*", 12345, 2);
        assert_ne!(id1, id2);
    }

    #[test]
    fn store_create_uses_distinct_nonces() {
        let mut store = GrantStore::new();
        let g1 = store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "same/*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: None,
        });
        let g2 = store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "same/*".to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at: None,
        });

        assert_ne!(g1.nonce, g2.nonce);
        assert_ne!(g1.grant_id, g2.grant_id);
    }

    #[test]
    fn serde_roundtrip() {
        let mut store = GrantStore::new();
        let grant = store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "results/*".to_string(),
            permissions: GrantPermissions::read_write(),
            expires_at: Some(9999999999),
        });

        let json = serde_json::to_string(&grant).expect("serialize");
        let deserialized: AccessGrant = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.grant_id, grant.grant_id);
        assert_eq!(deserialized.key_pattern, "results/*");
        assert_eq!(deserialized.nonce, grant.nonce);
        assert!(deserialized.permissions.read);
        assert!(deserialized.permissions.write);
        assert!(!deserialized.permissions.append);
        assert!(!deserialized.permissions.control);
    }

    #[test]
    fn shared_store_is_constructible() {
        let shared = GrantStore::shared();
        let guard = shared.read().expect("rwlock read");
        assert_eq!(guard.list_all().len(), 0);
    }

    #[test]
    fn control_permission_is_enforced() {
        let mut store = GrantStore::new();
        store.create(GrantRequest {
            grantor_puf: puf_a(),
            grantee_puf: puf_b(),
            key_pattern: "acl/*".to_string(),
            permissions: GrantPermissions::owner(),
            expires_at: None,
        });
        assert!(store.can_control(&puf_b(), "acl/ruleset"));
        assert!(!store.can_control(&puf_b(), "other/ruleset"));
    }
}
