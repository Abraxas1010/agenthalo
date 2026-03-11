//! DID <-> ACL bridge for capability-based sharing.

use crate::halo::did::DIDIdentity;
use crate::pod::acl::{AccessGrant, GrantStore};
use crate::pod::capability::{
    capability_to_grants, create_capability, verify_capability, AccessMode, AgentClass,
    CapabilityStore, CapabilityToken,
};

pub fn grant_access_to_agent(
    local_identity: &DIDIdentity,
    remote_agent_did: &str,
    patterns: &[String],
    modes: &[AccessMode],
    duration_secs: u64,
) -> Result<CapabilityToken, String> {
    if duration_secs == 0 {
        return Err("duration_secs must be > 0".to_string());
    }
    let now = crate::halo::util::now_unix_secs();
    create_capability(
        local_identity,
        remote_agent_did,
        AgentClass::Specific {
            did_uri: remote_agent_did.to_string(),
        },
        patterns,
        modes,
        now,
        now.saturating_add(duration_secs),
        false,
    )
}

pub fn accept_capability(
    token: &CapabilityToken,
    local_store: &mut GrantStore,
    local_puf: &[u8; 32],
    remote_puf: &[u8; 32],
    now: u64,
) -> Result<Vec<AccessGrant>, String> {
    verify_capability(token, now)?;
    let requests = capability_to_grants(token, local_puf, remote_puf)?;
    let mut grants = Vec::with_capacity(requests.len());
    for req in requests {
        grants.push(local_store.create(req));
    }
    Ok(grants)
}

pub fn load_capability_store() -> Result<CapabilityStore, String> {
    CapabilityStore::load_or_default(&crate::halo::config::capability_store_path())
}

pub fn save_capability_store(store: &CapabilityStore) -> Result<(), String> {
    store.save(&crate::halo::config::capability_store_path())
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

    #[test]
    fn grant_and_accept_capability_roundtrip() {
        let owner = did(0x01);
        let grantee = did(0x02);
        let token = grant_access_to_agent(
            &owner,
            &grantee.did,
            &["results/*".to_string()],
            &[AccessMode::Read],
            3600,
        )
        .expect("grant capability");

        let mut store = GrantStore::new();
        let grants = accept_capability(
            &token,
            &mut store,
            &puf(0xAA),
            &puf(0xBB),
            crate::halo::util::now_unix_secs(),
        )
        .expect("accept capability");
        assert_eq!(grants.len(), 1);
        assert!(store.can_read(&puf(0xAA), "results/theorem_1"));
    }
}
