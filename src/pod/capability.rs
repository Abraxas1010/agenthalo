//! Capability tokens for DID-authenticated agent access control.
//!
//! Inspired by Solid WAC/ACP with time-constrained capability tokens.

use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::util::{hex_decode, hex_encode};
use crate::pod::acl::{key_pattern_matches, GrantPermissions, GrantRequest, GrantStore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Access modes matching Solid WAC plus Control.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    Read,
    Write,
    Append,
    Control,
}

impl AccessMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Append => "append",
            Self::Control => "control",
        }
    }

    fn sort_rank(&self) -> u8 {
        match self {
            Self::Read => 0,
            Self::Write => 1,
            Self::Append => 2,
            Self::Control => 3,
        }
    }
}

/// Agent class for policy defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentClass {
    Public,
    Authenticated,
    Verified { min_tier: u8 },
    Specific { did_uri: String },
}

/// Time-constrained capability token.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub token_id: [u8; 32],
    pub grantor_did: String,
    pub grantor_did_document: DIDDocument,
    pub grantee_did: String,
    pub agent_class: AgentClass,
    pub resource_patterns: Vec<String>,
    pub modes: Vec<AccessMode>,
    pub not_before: u64,
    pub expires_at: u64,
    pub delegatable: bool,
    pub delegation_chain: Vec<String>,
    pub signature_ed25519_hex: String,
    pub signature_mldsa65_hex: String,
    pub created_at: u64,
    pub revoked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CapabilityCanonical {
    grantor_did: String,
    grantee_did: String,
    agent_class: AgentClass,
    resource_patterns: Vec<String>,
    modes: Vec<AccessMode>,
    not_before: u64,
    expires_at: u64,
    delegatable: bool,
    delegation_chain: Vec<String>,
    created_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapabilityStore {
    pub tokens: Vec<CapabilityToken>,
}

fn now_unix() -> u64 {
    crate::halo::util::now_unix_secs()
}

fn normalize_patterns(patterns: &[String]) -> Vec<String> {
    let mut out: Vec<String> = patterns
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    out.sort();
    out.dedup();
    out
}

fn normalize_modes(modes: &[AccessMode]) -> Vec<AccessMode> {
    let mut out = modes.to_vec();
    out.sort_by_key(|m| m.sort_rank());
    out.dedup();
    out
}

fn canonical_from_token(token: &CapabilityToken) -> CapabilityCanonical {
    CapabilityCanonical {
        grantor_did: token.grantor_did.clone(),
        grantee_did: token.grantee_did.clone(),
        agent_class: token.agent_class.clone(),
        resource_patterns: normalize_patterns(&token.resource_patterns),
        modes: normalize_modes(&token.modes),
        not_before: token.not_before,
        expires_at: token.expires_at,
        delegatable: token.delegatable,
        delegation_chain: token.delegation_chain.clone(),
        created_at: token.created_at,
    }
}

fn canonical_bytes(c: &CapabilityCanonical) -> Result<Vec<u8>, String> {
    serde_json::to_vec(c).map_err(|e| format!("serialize capability canonical payload: {e}"))
}

fn compute_token_id(c: &CapabilityCanonical) -> Result<[u8; 32], String> {
    let mut h = Sha256::new();
    h.update(b"agenthalo.capability.v1|");
    h.update(canonical_bytes(c)?);
    Ok(h.finalize().into())
}

fn delegation_chain_valid(token: &CapabilityToken) -> bool {
    if token.delegation_chain.is_empty() {
        return true;
    }
    if !token.delegatable {
        return false;
    }
    if token
        .delegation_chain
        .iter()
        .any(|did| did.trim().is_empty() || !did.starts_with("did:"))
    {
        return false;
    }
    token.delegation_chain.first() == Some(&token.grantor_did)
        && token.delegation_chain.last() == Some(&token.grantee_did)
}

/// Create a capability token signed by the grantor DID identity.
pub fn create_capability(
    grantor: &DIDIdentity,
    grantee_did: &str,
    agent_class: AgentClass,
    resource_patterns: &[String],
    modes: &[AccessMode],
    not_before: u64,
    expires_at: u64,
    delegatable: bool,
) -> Result<CapabilityToken, String> {
    let created_at = now_unix();
    if grantee_did.trim().is_empty() {
        return Err("grantee_did must not be empty".to_string());
    }
    if !grantee_did.starts_with("did:") {
        return Err("grantee_did must be a DID URI".to_string());
    }
    if expires_at <= not_before {
        return Err("expires_at must be greater than not_before".to_string());
    }

    let resource_patterns = normalize_patterns(resource_patterns);
    if resource_patterns.is_empty() {
        return Err("at least one resource pattern is required".to_string());
    }
    let modes = normalize_modes(modes);
    if modes.is_empty() {
        return Err("at least one access mode is required".to_string());
    }

    let canonical = CapabilityCanonical {
        grantor_did: grantor.did.clone(),
        grantee_did: grantee_did.to_string(),
        agent_class,
        resource_patterns,
        modes,
        not_before,
        expires_at,
        delegatable,
        delegation_chain: vec![],
        created_at,
    };
    let token_id = compute_token_id(&canonical)?;
    let message = canonical_bytes(&canonical)?;
    let (ed_sig, pq_sig) = dual_sign(grantor, &message)?;

    Ok(CapabilityToken {
        token_id,
        grantor_did: canonical.grantor_did,
        grantor_did_document: grantor.did_document.clone(),
        grantee_did: canonical.grantee_did,
        agent_class: canonical.agent_class,
        resource_patterns: canonical.resource_patterns,
        modes: canonical.modes,
        not_before: canonical.not_before,
        expires_at: canonical.expires_at,
        delegatable: canonical.delegatable,
        delegation_chain: canonical.delegation_chain,
        signature_ed25519_hex: hex_encode(&ed_sig),
        signature_mldsa65_hex: hex_encode(&pq_sig),
        created_at: canonical.created_at,
        revoked: false,
    })
}

/// Verify capability signatures and time constraints.
pub fn verify_capability(token: &CapabilityToken, now: u64) -> Result<(), String> {
    if token.grantor_did_document.id != token.grantor_did {
        return Err("grantor DID does not match DID document id".to_string());
    }
    if token.revoked {
        return Err("capability token is revoked".to_string());
    }
    if now < token.not_before {
        return Err("capability token not yet valid".to_string());
    }
    if now >= token.expires_at {
        return Err("capability token expired".to_string());
    }
    if !delegation_chain_valid(token) {
        return Err("invalid delegation chain".to_string());
    }
    let canonical = canonical_from_token(token);
    let expected_id = compute_token_id(&canonical)?;
    if expected_id != token.token_id {
        return Err("capability token id mismatch".to_string());
    }

    let ed_sig = hex_decode(&token.signature_ed25519_hex)
        .map_err(|e| format!("decode Ed25519 signature: {e}"))?;
    let pq_sig = hex_decode(&token.signature_mldsa65_hex)
        .map_err(|e| format!("decode ML-DSA signature: {e}"))?;
    if ed_sig.is_empty() || pq_sig.is_empty() {
        return Err("both Ed25519 and ML-DSA signatures are required".to_string());
    }

    let message = canonical_bytes(&canonical)?;
    let ok = dual_verify(&token.grantor_did_document, &message, &ed_sig, &pq_sig)?;
    if !ok {
        return Err("capability signature verification failed".to_string());
    }
    Ok(())
}

/// Check whether the token authorizes mode on resource key at time `now`.
pub fn capability_authorizes(
    token: &CapabilityToken,
    resource_key: &str,
    mode: AccessMode,
    now: u64,
) -> bool {
    verify_capability(token, now).is_ok()
        && token.modes.contains(&mode)
        && token
            .resource_patterns
            .iter()
            .any(|p| key_pattern_matches(p, resource_key))
}

fn permissions_from_modes(modes: &[AccessMode]) -> GrantPermissions {
    GrantPermissions {
        read: modes.contains(&AccessMode::Read),
        write: modes.contains(&AccessMode::Write),
        append: modes.contains(&AccessMode::Append),
        control: modes.contains(&AccessMode::Control),
    }
}

/// Convert a capability token into one or more ACL grant requests.
pub fn capability_to_grants(
    token: &CapabilityToken,
    grantee_puf: &[u8; 32],
    grantor_puf: &[u8; 32],
) -> Result<Vec<GrantRequest>, String> {
    if token.resource_patterns.is_empty() {
        return Err("capability token has no resource patterns".to_string());
    }
    let permissions = permissions_from_modes(&token.modes);
    if !permissions.read && !permissions.write && !permissions.append && !permissions.control {
        return Err("capability token has no grantable permissions".to_string());
    }
    Ok(token
        .resource_patterns
        .iter()
        .map(|pattern| GrantRequest {
            grantor_puf: *grantor_puf,
            grantee_puf: *grantee_puf,
            key_pattern: pattern.clone(),
            permissions,
            expires_at: Some(token.expires_at),
        })
        .collect())
}

/// Backward-compatible single-grant conversion helper.
pub fn capability_to_grant(
    token: &CapabilityToken,
    grantee_puf: &[u8; 32],
    grantor_puf: &[u8; 32],
) -> Result<GrantRequest, String> {
    capability_to_grants(token, grantee_puf, grantor_puf)?
        .into_iter()
        .next()
        .ok_or_else(|| "capability token has no resource patterns".to_string())
}

impl CapabilityStore {
    pub fn new() -> Self {
        Self { tokens: vec![] }
    }

    pub fn load_or_default(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let raw = std::fs::read(path)
            .map_err(|e| format!("read capability store {}: {e}", path.display()))?;
        serde_json::from_slice(&raw)
            .map_err(|e| format!("parse capability store {}: {e}", path.display()))
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create capability store dir {}: {e}", parent.display()))?;
        }
        let raw = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("serialize capability store {}: {e}", path.display()))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &raw)
            .map_err(|e| format!("write temp capability store {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod temp capability store {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            format!(
                "rename capability store {} -> {}: {e}",
                tmp.display(),
                path.display()
            )
        })?;
        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod capability store {}: {e}", path.display()))?;
        Ok(())
    }

    pub fn create(&mut self, token: CapabilityToken) -> CapabilityToken {
        self.tokens.push(token.clone());
        token
    }

    pub fn revoke(&mut self, token_id: &[u8; 32]) -> bool {
        for token in &mut self.tokens {
            if &token.token_id == token_id {
                token.revoked = true;
                return true;
            }
        }
        false
    }

    pub fn list_all(&self) -> &[CapabilityToken] {
        &self.tokens
    }

    pub fn list_active(&self, now: u64) -> Vec<&CapabilityToken> {
        self.tokens
            .iter()
            .filter(|t| verify_capability(t, now).is_ok())
            .collect()
    }

    pub fn apply_to_grant_store(
        &self,
        grants: &mut GrantStore,
        grantee_puf: &[u8; 32],
        grantor_puf: &[u8; 32],
        now: u64,
    ) -> usize {
        let mut applied = 0usize;
        for token in self
            .tokens
            .iter()
            .filter(|t| verify_capability(t, now).is_ok())
        {
            if let Ok(reqs) = capability_to_grants(token, grantee_puf, grantor_puf) {
                for req in reqs {
                    grants.create(req);
                    applied = applied.saturating_add(1);
                }
            }
        }
        applied
    }
}

pub fn revoke_capability(store: &mut CapabilityStore, token_id: &[u8; 32]) -> bool {
    store.revoke(token_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_from_byte(b: u8) -> [u8; 64] {
        [b; 64]
    }

    fn did(byte: u8) -> DIDIdentity {
        crate::halo::did::did_from_genesis_seed(&seed_from_byte(byte)).expect("did")
    }

    fn puf(byte: u8) -> [u8; 32] {
        let mut out = [0u8; 32];
        out[0] = byte;
        out
    }

    fn sample_token(now: u64) -> CapabilityToken {
        create_capability(
            &did(0x10),
            &did(0x20).did,
            AgentClass::Authenticated,
            &["results/*".to_string()],
            &[AccessMode::Read, AccessMode::Write],
            now.saturating_sub(10),
            now.saturating_add(300),
            false,
        )
        .expect("create capability")
    }

    #[test]
    fn create_and_verify_capability() {
        let now = now_unix();
        let token = sample_token(now);
        verify_capability(&token, now).expect("verify token");
    }

    #[test]
    fn expired_capability_rejected() {
        let now = now_unix();
        let mut token = sample_token(now);
        token.expires_at = now.saturating_sub(1);
        assert!(verify_capability(&token, now).is_err());
    }

    #[test]
    fn not_before_enforced() {
        let now = now_unix();
        let mut token = sample_token(now);
        token.not_before = now.saturating_add(120);
        assert!(verify_capability(&token, now).is_err());
    }

    #[test]
    fn revoked_capability_rejected() {
        let now = now_unix();
        let mut token = sample_token(now);
        token.revoked = true;
        assert!(verify_capability(&token, now).is_err());
    }

    #[test]
    fn capability_to_grant_bridge() {
        let now = now_unix();
        let token = sample_token(now);
        let reqs = capability_to_grants(&token, &puf(0xAA), &puf(0xBB)).expect("to grants");
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].key_pattern, "results/*");
        assert!(reqs[0].permissions.read);
        assert!(reqs[0].permissions.write);
        assert!(!reqs[0].permissions.append);
        assert!(!reqs[0].permissions.control);
    }

    #[test]
    fn dual_signature_required() {
        let now = now_unix();
        let mut token = sample_token(now);
        token.signature_mldsa65_hex.clear();
        assert!(verify_capability(&token, now).is_err());
    }

    #[test]
    fn delegation_chain_verified() {
        let now = now_unix();
        let mut token = create_capability(
            &did(0x11),
            &did(0x22).did,
            AgentClass::Authenticated,
            &["results/*".to_string()],
            &[AccessMode::Read],
            now.saturating_sub(10),
            now.saturating_add(30),
            true,
        )
        .expect("create delegatable token");
        token.delegation_chain = vec![token.grantor_did.clone(), token.grantee_did.clone()];
        // Signatures are for empty delegation chain; changing the chain must invalidate the token.
        assert!(verify_capability(&token, now).is_err());
    }

    #[test]
    fn control_permission_maps_to_acl_control() {
        let now = now_unix();
        let token = create_capability(
            &did(0x31),
            &did(0x32).did,
            AgentClass::Authenticated,
            &["acl/*".to_string()],
            &[AccessMode::Control],
            now.saturating_sub(1),
            now.saturating_add(20),
            false,
        )
        .expect("create control token");
        let req = capability_to_grant(&token, &puf(0x01), &puf(0x02)).expect("bridge");
        assert!(!req.permissions.read);
        assert!(!req.permissions.write);
        assert!(!req.permissions.append);
        assert!(req.permissions.control);
    }

    #[test]
    fn capability_authorizes_resource_and_mode() {
        let now = now_unix();
        let token = sample_token(now);
        assert!(capability_authorizes(
            &token,
            "results/theorem_42",
            AccessMode::Read,
            now
        ));
        assert!(!capability_authorizes(
            &token,
            "private/key",
            AccessMode::Read,
            now
        ));
    }
}
