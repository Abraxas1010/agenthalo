//! Proof gate for tool-level theorem requirements.

use super::checker::{has_theorem, verify_export, TrustTier, VerificationResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofRequirement {
    pub tool_name: String,
    pub required_theorem: String,
    pub description: String,
    pub enforced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_statement_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_commit_hash: Option<String>,
    #[serde(default)]
    pub require_signature: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_trust_tier: Option<TrustTier>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofGateConfig {
    pub certificate_dir: PathBuf,
    pub requirements: HashMap<String, Vec<ProofRequirement>>,
    pub enabled: bool,
}

impl Default for ProofGateConfig {
    fn default() -> Self {
        Self {
            certificate_dir: crate::halo::config::proof_certificates_dir(),
            requirements: HashMap::new(),
            enabled: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequirementCheck {
    pub theorem_name: String,
    pub found: bool,
    pub verified: bool,
    pub trust_tier: Option<TrustTier>,
    pub statement_hash_match: Option<bool>,
    pub commit_hash_match: Option<bool>,
    pub signature_valid: Option<bool>,
    pub certificate_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateResult {
    pub tool_name: String,
    pub passed: bool,
    pub requirements_checked: usize,
    pub requirements_met: usize,
    pub achieved_trust_tier: Option<TrustTier>,
    pub verification_results: Vec<RequirementCheck>,
    pub elapsed_ms: u64,
}

fn tier_rank(tier: TrustTier) -> u8 {
    match tier {
        TrustTier::Untrusted => 0,
        TrustTier::Legacy => 1,
        TrustTier::CryptoExtended => 2,
        TrustTier::Standard => 3,
    }
}

fn weakest_tier(current: Option<TrustTier>, candidate: TrustTier) -> TrustTier {
    match current {
        Some(cur) => {
            if tier_rank(candidate) < tier_rank(cur) {
                candidate
            } else {
                cur
            }
        }
        None => candidate,
    }
}

fn trust_tier_meets(actual: TrustTier, required: TrustTier) -> bool {
    match required {
        TrustTier::Standard => actual == TrustTier::Standard,
        TrustTier::CryptoExtended => {
            actual == TrustTier::Standard || actual == TrustTier::CryptoExtended
        }
        TrustTier::Legacy => actual != TrustTier::Untrusted,
        TrustTier::Untrusted => true,
    }
}

impl ProofGateConfig {
    pub fn load(path: &Path) -> Result<Self, String> {
        let raw = std::fs::read_to_string(path)
            .map_err(|e| format!("read proof gate config {}: {e}", path.display()))?;
        let mut cfg: Self =
            serde_json::from_str(&raw).map_err(|e| format!("parse proof gate config: {e}"))?;
        if let Some(s) = cfg.certificate_dir.to_str() {
            if let Some(rest) = s.strip_prefix("~/") {
                if let Some(home) = dirs::home_dir() {
                    cfg.certificate_dir = home.join(rest);
                }
            }
        }
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create proof gate config dir {}: {e}", parent.display()))?;
        }
        let raw = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("serialize proof gate config {}: {e}", path.display()))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &raw)
            .map_err(|e| format!("write proof gate config {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            format!(
                "rename proof gate config {} -> {}: {e}",
                tmp.display(),
                path.display()
            )
        })
    }

    pub fn has_requirements(&self, tool_name: &str) -> bool {
        self.enabled && self.requirements.contains_key(tool_name)
    }

    pub fn requirements_for_tool(&self, tool_name: Option<&str>) -> Vec<ProofRequirement> {
        match tool_name {
            Some(name) => self.requirements.get(name).cloned().unwrap_or_default(),
            None => self
                .requirements
                .values()
                .flat_map(|v| v.iter().cloned())
                .collect(),
        }
    }

    pub fn evaluate(&self, tool_name: &str) -> GateResult {
        let start = std::time::Instant::now();
        if !self.enabled {
            return GateResult {
                tool_name: tool_name.to_string(),
                passed: true,
                requirements_checked: 0,
                requirements_met: 0,
                achieved_trust_tier: None,
                verification_results: vec![],
                elapsed_ms: 0,
            };
        }

        let reqs = match self.requirements.get(tool_name) {
            Some(v) => v,
            None => {
                return GateResult {
                    tool_name: tool_name.to_string(),
                    passed: true,
                    requirements_checked: 0,
                    requirements_met: 0,
                    achieved_trust_tier: None,
                    verification_results: vec![],
                    elapsed_ms: 0,
                };
            }
        };

        let mut checks = Vec::with_capacity(reqs.len());
        let mut met = 0usize;
        let mut enforced_total = 0usize;
        let mut enforced_met = 0usize;
        let mut achieved_tier: Option<TrustTier> = None;
        for req in reqs {
            if req.enforced {
                enforced_total = enforced_total.saturating_add(1);
            }
            let check = self.check_requirement(req);
            if check.found && check.verified {
                met = met.saturating_add(1);
                if req.enforced {
                    enforced_met = enforced_met.saturating_add(1);
                }
                if let Some(t) = check.trust_tier {
                    achieved_tier = Some(weakest_tier(achieved_tier, t));
                }
            }
            checks.push(check);
        }

        GateResult {
            tool_name: tool_name.to_string(),
            passed: enforced_met == enforced_total,
            requirements_checked: reqs.len(),
            requirements_met: met,
            achieved_trust_tier: achieved_tier,
            verification_results: checks,
            elapsed_ms: start.elapsed().as_millis() as u64,
        }
    }

    fn check_requirement(&self, req: &ProofRequirement) -> RequirementCheck {
        if !self.certificate_dir.exists() {
            return RequirementCheck {
                theorem_name: req.required_theorem.clone(),
                found: false,
                verified: false,
                trust_tier: None,
                statement_hash_match: None,
                commit_hash_match: None,
                signature_valid: None,
                certificate_path: None,
                error: Some("certificate directory does not exist".to_string()),
            };
        }

        let entries = match std::fs::read_dir(&self.certificate_dir) {
            Ok(v) => v,
            Err(e) => {
                return RequirementCheck {
                    theorem_name: req.required_theorem.clone(),
                    found: false,
                    verified: false,
                    trust_tier: None,
                    statement_hash_match: None,
                    commit_hash_match: None,
                    signature_valid: None,
                    certificate_path: None,
                    error: Some(format!("read certificate directory: {e}")),
                };
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("lean4export") {
                continue;
            }
            let result = match verify_export(&path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if !has_theorem(&result, &req.required_theorem) {
                continue;
            }

            let statement_hash_match = req.expected_statement_hash.as_ref().map(|expected| {
                result
                    .metadata
                    .theorem_statement_sha256
                    .as_deref()
                    .map(|actual| actual == expected)
                    .unwrap_or(false)
            });
            let commit_hash_match = req.expected_commit_hash.as_ref().map(|expected| {
                result
                    .metadata
                    .commit_hash
                    .as_deref()
                    .map(|actual| actual == expected)
                    .unwrap_or(false)
            });
            let signature_valid = result.metadata.signature_valid;

            let statement_ok = statement_hash_match.unwrap_or(true);
            let commit_ok = commit_hash_match.unwrap_or(true);
            let signature_ok = if req.require_signature {
                signature_valid.unwrap_or(false)
            } else {
                signature_valid.unwrap_or(true)
            };
            let tier_ok = req
                .min_trust_tier
                .map(|required| trust_tier_meets(result.trust_tier, required))
                .unwrap_or(true);

            let verified = result.all_checked
                && result.axioms_trusted
                && statement_ok
                && commit_ok
                && signature_ok
                && tier_ok;

            let error = if verified {
                None
            } else if !result.all_checked {
                Some("certificate parse/metadata checks failed".to_string())
            } else if !result.axioms_trusted {
                Some("untrusted axioms used".to_string())
            } else if !statement_ok {
                Some("theorem statement hash mismatch".to_string())
            } else if !commit_ok {
                Some("certificate commit hash mismatch".to_string())
            } else if !signature_ok {
                Some("certificate signature missing or invalid".to_string())
            } else if !tier_ok {
                Some("certificate trust tier does not satisfy requirement".to_string())
            } else {
                Some("unknown gate verification failure".to_string())
            };

            return RequirementCheck {
                theorem_name: req.required_theorem.clone(),
                found: true,
                verified,
                trust_tier: Some(result.trust_tier),
                statement_hash_match,
                commit_hash_match,
                signature_valid,
                certificate_path: Some(path.display().to_string()),
                error,
            };
        }

        RequirementCheck {
            theorem_name: req.required_theorem.clone(),
            found: false,
            verified: false,
            trust_tier: None,
            statement_hash_match: None,
            commit_hash_match: None,
            signature_valid: None,
            certificate_path: None,
            error: Some("no certificate found containing this theorem".to_string()),
        }
    }
}

fn local_did() -> Option<String> {
    let seed = crate::halo::genesis_seed::load_seed_bytes()
        .ok()
        .flatten()?;
    crate::halo::did::did_from_genesis_seed(&seed)
        .ok()
        .map(|id| id.did)
}

pub fn load_gate_config() -> Result<ProofGateConfig, String> {
    let env_path = std::env::var("AGENTHALO_PROOF_GATE_CONFIG")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(crate::halo::config::proof_gate_config_path);
    if env_path.exists() {
        return ProofGateConfig::load(&env_path);
    }

    let repo_default = PathBuf::from("configs/proof_gate.json");
    if repo_default.exists() {
        return ProofGateConfig::load(&repo_default);
    }

    Ok(ProofGateConfig::default())
}

pub fn verify_certificate(path: &Path) -> Result<VerificationResult, String> {
    verify_export(path)
}

pub fn submit_certificate(path: &Path) -> Result<PathBuf, String> {
    if !path.exists() {
        return Err(format!("certificate {} does not exist", path.display()));
    }

    let verification = verify_export(path)?;
    if !verification.all_checked {
        return Err(format!(
            "certificate {} failed metadata checks: {}",
            path.display(),
            verification.errors.join("; ")
        ));
    }

    if verification.metadata.signature_ed25519.is_some()
        || verification.metadata.signing_key_multibase.is_some()
    {
        if verification.metadata.signature_valid != Some(true) {
            return Err(format!(
                "certificate {} has invalid Ed25519 signature metadata",
                path.display()
            ));
        }
    }

    if let (Some(cert_did), Some(local)) = (
        verification.metadata.signing_did.as_deref(),
        local_did().as_deref(),
    ) {
        if cert_did != local {
            return Err(format!(
                "certificate signer DID mismatch: cert `{cert_did}` != local `{local}`"
            ));
        }
    }

    crate::halo::config::ensure_proof_certificates_dir()?;
    let base = path
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("certificate.lean4export");
    let dest = crate::halo::config::proof_certificates_dir().join(base);
    std::fs::copy(path, &dest).map_err(|e| {
        format!(
            "copy certificate {} -> {}: {e}",
            path.display(),
            dest.display()
        )
    })?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_cert(dir: &Path, name: &str, body: &str) -> PathBuf {
        std::fs::create_dir_all(dir).expect("create cert dir");
        let path = dir.join(name);
        std::fs::write(&path, body).expect("write cert");
        path
    }

    fn base_req(tool: &str, theorem: &str) -> ProofRequirement {
        ProofRequirement {
            tool_name: tool.to_string(),
            required_theorem: theorem.to_string(),
            description: "test".to_string(),
            enforced: true,
            expected_statement_hash: None,
            expected_commit_hash: None,
            require_signature: false,
            min_trust_tier: None,
        }
    }

    #[test]
    fn disabled_gate_passes() {
        let gate = ProofGateConfig::default();
        let out = gate.evaluate("nucleusdb_execute_sql");
        assert!(out.passed);
        assert_eq!(out.requirements_checked, 0);
    }

    #[test]
    fn requirement_check_finds_theorem() {
        let dir = std::env::temp_dir().join(format!(
            "proof_gate_{}_{}",
            std::process::id(),
            crate::halo::util::now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        write_cert(
            &dir,
            "ok.lean4export",
            "#THM HeytingLean.NucleusDB.Core.replay_preserves\n#AX propext\n",
        );

        let mut reqs = HashMap::new();
        reqs.insert(
            "nucleusdb_execute_sql".to_string(),
            vec![base_req(
                "nucleusdb_execute_sql",
                "HeytingLean.NucleusDB.Core.replay_preserves",
            )],
        );
        let gate = ProofGateConfig {
            certificate_dir: dir.clone(),
            requirements: reqs,
            enabled: true,
        };
        let out = gate.evaluate("nucleusdb_execute_sql");
        assert!(out.passed);
        assert_eq!(out.requirements_checked, 1);
        assert_eq!(out.requirements_met, 1);
        assert_eq!(out.achieved_trust_tier, Some(TrustTier::Legacy));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn statement_hash_mismatch_fails_requirement() {
        let dir = std::env::temp_dir().join(format!(
            "proof_gate_hash_{}_{}",
            std::process::id(),
            crate::halo::util::now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        write_cert(
            &dir,
            "ok.lean4export",
            "#THM T.Theorem\n#AX propext\n#META theorem_statement_sha256 sha256:abc\n",
        );

        let mut req = base_req("tool_a", "T.Theorem");
        req.expected_statement_hash = Some("sha256:def".to_string());

        let mut reqs = HashMap::new();
        reqs.insert("tool_a".to_string(), vec![req]);
        let gate = ProofGateConfig {
            certificate_dir: dir.clone(),
            requirements: reqs,
            enabled: true,
        };
        let out = gate.evaluate("tool_a");
        assert!(!out.passed);
        assert_eq!(out.requirements_met, 0);
        assert_eq!(
            out.verification_results[0].error.as_deref(),
            Some("theorem statement hash mismatch")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_requirement_fails_when_enforced() {
        let dir = std::env::temp_dir().join(format!(
            "proof_gate_miss_{}_{}",
            std::process::id(),
            crate::halo::util::now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");

        let mut reqs = HashMap::new();
        reqs.insert(
            "tool_a".to_string(),
            vec![base_req("tool_a", "Missing.Theorem")],
        );
        let gate = ProofGateConfig {
            certificate_dir: dir.clone(),
            requirements: reqs,
            enabled: true,
        };
        let out = gate.evaluate("tool_a");
        assert!(!out.passed);
        assert_eq!(out.requirements_met, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn submit_rejects_invalid_signature_metadata() {
        let src = std::env::temp_dir().join(format!(
            "proof_gate_submit_bad_{}_{}.lean4export",
            std::process::id(),
            crate::halo::util::now_unix_secs()
        ));
        std::fs::write(
            &src,
            "#THM T\n#AX propext\n#META signing_key_multibase z6MkhQ...\n#META signature_ed25519 notbase64$$$\n",
        )
        .expect("write cert");
        let err = submit_certificate(&src).expect_err("bad metadata signature should reject");
        assert!(
            err.contains("failed metadata checks") || err.contains("invalid Ed25519 signature")
        );
        let _ = std::fs::remove_file(src);
    }
}
