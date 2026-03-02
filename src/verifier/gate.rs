//! Proof gate for tool-level theorem requirements.

use super::checker::{has_theorem, verify_export, VerificationResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofRequirement {
    pub tool_name: String,
    pub required_theorem: String,
    pub description: String,
    pub enforced: bool,
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
    pub certificate_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GateResult {
    pub tool_name: String,
    pub passed: bool,
    pub requirements_checked: usize,
    pub requirements_met: usize,
    pub verification_results: Vec<RequirementCheck>,
    pub elapsed_ms: u64,
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
                    verification_results: vec![],
                    elapsed_ms: 0,
                };
            }
        };

        let mut checks = Vec::with_capacity(reqs.len());
        let mut met = 0usize;
        let mut enforced_total = 0usize;
        let mut enforced_met = 0usize;
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
            }
            checks.push(check);
        }

        GateResult {
            tool_name: tool_name.to_string(),
            passed: enforced_met == enforced_total,
            requirements_checked: reqs.len(),
            requirements_met: met,
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
            return RequirementCheck {
                theorem_name: req.required_theorem.clone(),
                found: true,
                verified: result.all_checked && result.axioms_trusted,
                certificate_path: Some(path.display().to_string()),
                error: if result.axioms_trusted {
                    None
                } else {
                    Some("untrusted axioms used".to_string())
                },
            };
        }

        RequirementCheck {
            theorem_name: req.required_theorem.clone(),
            found: false,
            verified: false,
            certificate_path: None,
            error: Some("no certificate found containing this theorem".to_string()),
        }
    }
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
            vec![ProofRequirement {
                tool_name: "nucleusdb_execute_sql".to_string(),
                required_theorem: "HeytingLean.NucleusDB.Core.replay_preserves".to_string(),
                description: "test".to_string(),
                enforced: true,
            }],
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
            vec![ProofRequirement {
                tool_name: "tool_a".to_string(),
                required_theorem: "Missing.Theorem".to_string(),
                description: "test".to_string(),
                enforced: true,
            }],
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
}
