//! Container-level access policies with ACP-style deny-overrides semantics.

use crate::pod::acl::key_pattern_matches;
use crate::pod::capability::AccessMode;
use serde::{Deserialize, Serialize};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AccessPolicy {
    pub policy_id: String,
    pub matcher: AgentMatcher,
    pub resource_patterns: Vec<String>,
    pub allowed_modes: Vec<AccessMode>,
    pub denied_modes: Vec<AccessMode>,
    pub effective_from: Option<u64>,
    pub effective_until: Option<u64>,
    pub active: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMatcher {
    AnyAgent,
    AnyAuthenticated,
    ByDID { did_uris: Vec<String> },
    ByTier { min_tier: u8 },
    ByPUF { puf_fingerprints: Vec<[u8; 32]> },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessDecision {
    Allow { policy_id: String },
    Deny { policy_id: String, reason: String },
    NoMatch,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PolicyStore {
    pub policies: Vec<AccessPolicy>,
}

#[derive(Clone, Debug)]
pub struct AccessContext<'a> {
    pub agent_did: Option<&'a str>,
    pub agent_tier: Option<u8>,
    pub agent_puf: Option<&'a [u8; 32]>,
    pub resource_key: &'a str,
    pub mode: AccessMode,
    pub now: u64,
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

fn mode_in(modes: &[AccessMode], mode: AccessMode) -> bool {
    modes.contains(&mode)
}

fn matches_time_window(policy: &AccessPolicy, now: u64) -> bool {
    if !policy.active {
        return false;
    }
    if let Some(start) = policy.effective_from {
        if now < start {
            return false;
        }
    }
    if let Some(end) = policy.effective_until {
        if now >= end {
            return false;
        }
    }
    true
}

fn matches_resource(policy: &AccessPolicy, key: &str) -> bool {
    let pats = normalize_patterns(&policy.resource_patterns);
    !pats.is_empty() && pats.iter().any(|p| key_pattern_matches(p, key))
}

fn matches_agent(policy: &AccessPolicy, ctx: &AccessContext<'_>) -> bool {
    match &policy.matcher {
        AgentMatcher::AnyAgent => true,
        AgentMatcher::AnyAuthenticated => ctx
            .agent_did
            .map(|d| !d.trim().is_empty() && d.starts_with("did:"))
            .unwrap_or(false),
        AgentMatcher::ByDID { did_uris } => {
            let did = match ctx.agent_did {
                Some(v) if !v.trim().is_empty() => v,
                _ => return false,
            };
            did_uris.iter().any(|d| d == did)
        }
        AgentMatcher::ByTier { min_tier } => {
            ctx.agent_tier.map(|t| t >= *min_tier).unwrap_or(false)
        }
        AgentMatcher::ByPUF { puf_fingerprints } => {
            let puf = match ctx.agent_puf {
                Some(v) => v,
                None => return false,
            };
            puf_fingerprints.iter().any(|fp| fp == puf)
        }
    }
}

impl PolicyStore {
    pub fn new() -> Self {
        Self { policies: vec![] }
    }

    pub fn load_or_default(path: &std::path::Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let raw = std::fs::read(path)
            .map_err(|e| format!("read policy store {}: {e}", path.display()))?;
        serde_json::from_slice(&raw)
            .map_err(|e| format!("parse policy store {}: {e}", path.display()))
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create policy dir {}: {e}", parent.display()))?;
        }
        let raw = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("serialize policy store {}: {e}", path.display()))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &raw)
            .map_err(|e| format!("write temp policy store {}: {e}", tmp.display()))?;
        #[cfg(unix)]
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod temp policy store {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            format!(
                "rename policy store {} -> {}: {e}",
                tmp.display(),
                path.display()
            )
        })?;
        #[cfg(unix)]
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("chmod policy store {}: {e}", path.display()))?;
        Ok(())
    }

    pub fn add(&mut self, policy: AccessPolicy) {
        self.policies.retain(|p| p.policy_id != policy.policy_id);
        self.policies.push(policy);
    }

    pub fn remove(&mut self, policy_id: &str) -> bool {
        let before = self.policies.len();
        self.policies.retain(|p| p.policy_id != policy_id);
        self.policies.len() != before
    }

    pub fn list(&self) -> &[AccessPolicy] {
        &self.policies
    }

    /// ACP-style evaluation: deny overrides allow.
    pub fn evaluate(&self, ctx: AccessContext<'_>) -> AccessDecision {
        let applicable: Vec<&AccessPolicy> = self
            .policies
            .iter()
            .filter(|p| matches_time_window(p, ctx.now))
            .filter(|p| matches_agent(p, &ctx))
            .filter(|p| matches_resource(p, ctx.resource_key))
            .collect();

        if let Some(policy) = applicable
            .iter()
            .find(|p| mode_in(&p.denied_modes, ctx.mode))
            .copied()
        {
            return AccessDecision::Deny {
                policy_id: policy.policy_id.clone(),
                reason: "deny rule matched".to_string(),
            };
        }

        if let Some(policy) = applicable
            .iter()
            .find(|p| mode_in(&p.allowed_modes, ctx.mode))
            .copied()
        {
            return AccessDecision::Allow {
                policy_id: policy.policy_id.clone(),
            };
        }

        AccessDecision::NoMatch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_policy(id: &str) -> AccessPolicy {
        AccessPolicy {
            policy_id: id.to_string(),
            matcher: AgentMatcher::AnyAuthenticated,
            resource_patterns: vec!["results/*".to_string()],
            allowed_modes: vec![AccessMode::Read],
            denied_modes: vec![],
            effective_from: None,
            effective_until: None,
            active: true,
        }
    }

    fn ctx(mode: AccessMode) -> AccessContext<'static> {
        AccessContext {
            agent_did: Some("did:key:z6MkExample"),
            agent_tier: Some(3),
            agent_puf: None,
            resource_key: "results/theorem_42",
            mode,
            now: 1_700_000_000,
        }
    }

    #[test]
    fn deny_overrides_allow() {
        let mut store = PolicyStore::new();
        let mut allow = base_policy("allow-read");
        allow.allowed_modes = vec![AccessMode::Read];
        let mut deny = base_policy("deny-read");
        deny.denied_modes = vec![AccessMode::Read];
        store.add(allow);
        store.add(deny);
        let decision = store.evaluate(ctx(AccessMode::Read));
        match decision {
            AccessDecision::Deny { policy_id, .. } => assert_eq!(policy_id, "deny-read"),
            _ => panic!("expected deny"),
        }
    }

    #[test]
    fn allow_when_no_deny_matches() {
        let mut store = PolicyStore::new();
        store.add(base_policy("allow-read"));
        let decision = store.evaluate(ctx(AccessMode::Read));
        match decision {
            AccessDecision::Allow { policy_id } => assert_eq!(policy_id, "allow-read"),
            _ => panic!("expected allow"),
        }
    }

    #[test]
    fn no_match_defaults_to_deny() {
        let mut store = PolicyStore::new();
        let mut policy = base_policy("allow-read");
        policy.allowed_modes = vec![AccessMode::Write];
        store.add(policy);
        assert_eq!(
            store.evaluate(ctx(AccessMode::Read)),
            AccessDecision::NoMatch
        );
    }

    #[test]
    fn by_did_matcher_works() {
        let mut store = PolicyStore::new();
        let mut policy = base_policy("by-did");
        policy.matcher = AgentMatcher::ByDID {
            did_uris: vec!["did:key:z6MkExample".to_string()],
        };
        store.add(policy);
        matches!(
            store.evaluate(ctx(AccessMode::Read)),
            AccessDecision::Allow { .. }
        );
    }

    #[test]
    fn by_tier_matcher_works() {
        let mut store = PolicyStore::new();
        let mut policy = base_policy("by-tier");
        policy.matcher = AgentMatcher::ByTier { min_tier: 2 };
        store.add(policy);
        assert!(matches!(
            store.evaluate(ctx(AccessMode::Read)),
            AccessDecision::Allow { .. }
        ));
    }

    #[test]
    fn by_puf_matcher_works() {
        let mut store = PolicyStore::new();
        let mut p = [0u8; 32];
        p[0] = 0xAA;
        let mut policy = base_policy("by-puf");
        policy.matcher = AgentMatcher::ByPUF {
            puf_fingerprints: vec![p],
        };
        store.add(policy);
        let decision = store.evaluate(AccessContext {
            agent_did: Some("did:key:z6MkExample"),
            agent_tier: None,
            agent_puf: Some(&p),
            resource_key: "results/theorem_42",
            mode: AccessMode::Read,
            now: 1_700_000_000,
        });
        assert!(matches!(decision, AccessDecision::Allow { .. }));
    }

    #[test]
    fn inactive_or_out_of_window_policies_do_not_apply() {
        let mut store = PolicyStore::new();
        let mut inactive = base_policy("inactive");
        inactive.active = false;
        store.add(inactive);
        assert_eq!(
            store.evaluate(ctx(AccessMode::Read)),
            AccessDecision::NoMatch
        );

        let mut timed = base_policy("timed");
        timed.active = true;
        timed.effective_from = Some(1_700_000_100);
        timed.effective_until = Some(1_700_000_200);
        store.add(timed);
        assert_eq!(
            store.evaluate(ctx(AccessMode::Read)),
            AccessDecision::NoMatch
        );
    }
}
