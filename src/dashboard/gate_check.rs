//! CodeGuard gate enforcement logic.
//!
//! Called from API endpoints to enforce worktree isolation,
//! schema validation, and pre-push verification.
//! All gate logic lives here — never in JavaScript (constraint H2).

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Gate configuration loaded from `.codeguard/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeGuardConfig {
    pub gates: GateConfig,
    #[serde(default)]
    pub audit: AuditConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    /// Gate 1: agent must work in a git worktree, not the main checkout.
    #[serde(default)]
    pub worktree_required: bool,
    /// Gate 2: a valid `codeguard.lock.json` manifest must exist.
    #[serde(default)]
    pub schema_required: bool,
    /// Gate 3: pre-push hostile audit by a separate agent.
    #[serde(default)]
    pub pre_push_hostile_audit: bool,
    /// Gate 3: pre-push human approval.
    #[serde(default)]
    pub pre_push_human_approval: bool,
    /// Hold timeout for pre-push gate (seconds). Default 3600 (1 hour).
    #[serde(default = "default_push_hold_timeout")]
    pub push_hold_timeout_secs: u64,
}

fn default_push_hold_timeout() -> u64 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    /// Model for the audit agent. `None` = system default.
    #[serde(default)]
    pub model: Option<String>,
    /// If true, prefer a different model than the implementing agent.
    #[serde(default = "default_prefer_different")]
    pub prefer_different_model: bool,
}

fn default_prefer_different() -> bool {
    true
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            model: None,
            prefer_different_model: true,
        }
    }
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            worktree_required: false,
            schema_required: false,
            pre_push_hostile_audit: false,
            pre_push_human_approval: false,
            push_hold_timeout_secs: default_push_hold_timeout(),
        }
    }
}

impl Default for CodeGuardConfig {
    fn default() -> Self {
        Self {
            gates: GateConfig::default(),
            audit: AuditConfig::default(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct GateError {
    pub gate: &'static str,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct GateWarning {
    pub node_id: String,
    pub file: String,
    pub kind: GateWarningKind,
}

#[derive(Debug, Serialize)]
pub enum GateWarningKind {
    LockedNodeModified,
    ReviewRequired,
    NewNodeNotInManifest,
    OrphanNode,
}

/// Gate 1: Check that the agent is working in a git worktree, not the main checkout.
///
/// **SCOPE LIMITATION**: This check only applies to API-level file operations.
/// An agent with shell access can bypass this by writing files directly.
/// Container-level isolation (see `container/worktree.rs`) provides the
/// filesystem-level enforcement when enabled.
pub fn check_worktree_gate(
    agent_working_dir: &Path,
    config: &CodeGuardConfig,
) -> Result<(), GateError> {
    if !config.gates.worktree_required {
        return Ok(());
    }

    // A git worktree has --git-dir != --git-common-dir.
    // In the main checkout they are the same.
    let git_dir = run_git(agent_working_dir, &["rev-parse", "--git-dir"]);
    let common_dir = run_git(agent_working_dir, &["rev-parse", "--git-common-dir"]);

    match (git_dir, common_dir) {
        (Ok(gd), Ok(cd)) => {
            // Normalize to canonical paths for comparison
            let gd_canon = Path::new(&gd)
                .canonicalize()
                .unwrap_or_else(|_| gd.clone().into());
            let cd_canon = Path::new(&cd)
                .canonicalize()
                .unwrap_or_else(|_| cd.clone().into());
            if gd_canon == cd_canon {
                Err(GateError {
                    gate: "worktree",
                    code: "worktree_required",
                    message: "Gate 1: Agent must work in a git worktree, not the main checkout. \
                              Note: this gate enforces API-level isolation only."
                        .into(),
                })
            } else {
                Ok(())
            }
        }
        _ => Err(GateError {
            gate: "worktree",
            code: "not_a_git_repo",
            message: "Gate 1: working directory is not a git repository.".into(),
        }),
    }
}

/// Gate 2: Check that a valid `codeguard.lock.json` exists for the repo.
pub fn check_schema_gate(
    repo_root: &Path,
    config: &CodeGuardConfig,
) -> Result<(), GateError> {
    if !config.gates.schema_required {
        return Ok(());
    }

    let manifest_path = find_manifest(repo_root);
    match manifest_path {
        Some(p) => {
            // Validate it's parseable JSON with the expected structure
            let content = std::fs::read_to_string(&p).map_err(|e| GateError {
                gate: "schema",
                code: "manifest_unreadable",
                message: format!("Gate 2: cannot read manifest at {}: {e}", p.display()),
            })?;
            let parsed: serde_json::Value =
                serde_json::from_str(&content).map_err(|e| GateError {
                    gate: "schema",
                    code: "manifest_invalid_json",
                    message: format!("Gate 2: manifest is not valid JSON: {e}"),
                })?;
            if !parsed.get("version").is_some() || !parsed.get("bindings").is_some() {
                return Err(GateError {
                    gate: "schema",
                    code: "manifest_missing_fields",
                    message: "Gate 2: manifest missing required fields (version, bindings)."
                        .into(),
                });
            }
            Ok(())
        }
        None => Err(GateError {
            gate: "schema",
            code: "manifest_not_found",
            message: "Gate 2: no codeguard.lock.json found in the repository.".into(),
        }),
    }
}

/// Gate 3: Pre-push verification — check modified files against manifest bindings.
/// Returns warnings for locked/review-required nodes.
pub fn check_pre_push_gate(
    repo_root: &Path,
    modified_files: &[String],
    manifest: &serde_json::Value,
    config: &CodeGuardConfig,
) -> Result<Vec<GateWarning>, GateError> {
    if !config.gates.pre_push_human_approval && !config.gates.pre_push_hostile_audit {
        return Ok(vec![]);
    }

    let bindings = manifest
        .get("bindings")
        .and_then(|b| b.as_array())
        .unwrap_or(&Vec::new())
        .clone();

    let mut warnings = Vec::new();
    for file in modified_files {
        for binding in &bindings {
            let code_path = binding
                .get("codePath")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let locked = binding
                .get("locked")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let review_required = binding
                .get("reviewRequired")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let node_id = binding
                .get("bindingId")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            if file == code_path || file.starts_with(code_path) {
                if locked {
                    warnings.push(GateWarning {
                        node_id: node_id.clone(),
                        file: file.clone(),
                        kind: GateWarningKind::LockedNodeModified,
                    });
                }
                if review_required {
                    warnings.push(GateWarning {
                        node_id,
                        file: file.clone(),
                        kind: GateWarningKind::ReviewRequired,
                    });
                }
            }
        }
    }

    // Check for files not in any binding
    let all_code_paths: Vec<&str> = bindings
        .iter()
        .filter_map(|b| b.get("codePath").and_then(|v| v.as_str()))
        .collect();
    for file in modified_files {
        let covered = all_code_paths
            .iter()
            .any(|cp| file == *cp || file.starts_with(cp));
        if !covered {
            warnings.push(GateWarning {
                node_id: String::new(),
                file: file.clone(),
                kind: GateWarningKind::NewNodeNotInManifest,
            });
        }
    }

    // If any locked node is modified, that's a hard block
    let has_locked_violation = warnings
        .iter()
        .any(|w| matches!(w.kind, GateWarningKind::LockedNodeModified));
    if has_locked_violation {
        return Err(GateError {
            gate: "pre_push",
            code: "locked_node_modified",
            message: "Gate 3: one or more locked nodes were modified. Push blocked.".into(),
        });
    }

    Ok(warnings)
}

/// Load config from `.codeguard/config.json`, or return defaults (all gates disabled).
pub fn load_config(repo_root: &Path) -> CodeGuardConfig {
    let config_path = repo_root.join(".codeguard").join("config.json");
    if let Ok(content) = std::fs::read_to_string(&config_path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        CodeGuardConfig::default()
    }
}

/// Save config to `.codeguard/config.json`.
pub fn save_config(repo_root: &Path, config: &CodeGuardConfig) -> Result<(), String> {
    let dir = repo_root.join(".codeguard");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("create .codeguard dir: {e}"))?;
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("serialize config: {e}"))?;
    std::fs::write(dir.join("config.json"), json)
        .map_err(|e| format!("write config: {e}"))?;
    Ok(())
}

/// Search for `codeguard.lock.json` in common locations.
pub fn find_manifest(repo_root: &Path) -> Option<std::path::PathBuf> {
    // Check common locations
    let candidates = [
        repo_root.join("codeguard.lock.json"),
        repo_root.join(".codeguard").join("codeguard.lock.json"),
        repo_root.join("projects").join("heyting-docs").join("codeguard.lock.json"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Build a viewer-optimized graph from the manifest.
pub fn build_graph(manifest: &serde_json::Value, repo_root: &Path) -> serde_json::Value {
    let bindings = manifest
        .get("bindings")
        .and_then(|b| b.as_array())
        .cloned()
        .unwrap_or_default();

    let mut nodes = Vec::new();
    let mut links = Vec::new();

    for binding in &bindings {
        let id = binding
            .get("bindingId")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let code_path = binding
            .get("codePath")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let locked = binding
            .get("locked")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let review_required = binding
            .get("reviewRequired")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let binding_hash = binding
            .get("bindingHash")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Check if the code file exists
        let file_exists = repo_root.join(code_path).is_file();
        let status = if locked {
            "locked"
        } else if review_required {
            "reviewRequired"
        } else if !file_exists {
            "ghost"
        } else {
            "unlocked"
        };

        nodes.push(serde_json::json!({
            "id": id,
            "codePath": code_path,
            "status": status,
            "locked": locked,
            "reviewRequired": review_required,
            "hashVerified": !binding_hash.is_empty(),
            "fileExists": file_exists,
        }));

        // Create edges between bindings that share paths
        // (e.g., witness or artifact overlap)
        let witness_path = binding
            .get("witnessPath")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let artifact_path = binding
            .get("artifactPath")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        for other in &bindings {
            let other_id = other
                .get("bindingId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if other_id == id {
                continue;
            }
            let other_code = other
                .get("codePath")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            // Link if this binding's witness/artifact is another binding's code
            if (!witness_path.is_empty() && witness_path == other_code)
                || (!artifact_path.is_empty() && artifact_path == other_code)
            {
                links.push(serde_json::json!({
                    "source": id,
                    "target": other_id,
                    "type": if !binding_hash.is_empty() { "strong" } else { "weak" },
                }));
            }
        }
    }

    serde_json::json!({
        "nodes": nodes,
        "links": links,
    })
}

/// Run a git command and return its stdout trimmed.
fn run_git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|e| format!("git exec: {e}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_all_gates_disabled() {
        let cfg = CodeGuardConfig::default();
        assert!(!cfg.gates.worktree_required);
        assert!(!cfg.gates.schema_required);
        assert!(!cfg.gates.pre_push_hostile_audit);
        assert!(!cfg.gates.pre_push_human_approval);
        assert_eq!(cfg.gates.push_hold_timeout_secs, 3600);
    }

    #[test]
    fn config_roundtrip() {
        let cfg = CodeGuardConfig {
            gates: GateConfig {
                worktree_required: true,
                schema_required: true,
                pre_push_hostile_audit: false,
                pre_push_human_approval: true,
                push_hold_timeout_secs: 7200,
            },
            audit: AuditConfig {
                model: Some("claude-opus-4-6".into()),
                prefer_different_model: false,
            },
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let parsed: CodeGuardConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.gates.worktree_required);
        assert_eq!(parsed.audit.model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn gate1_disabled_passes() {
        let cfg = CodeGuardConfig::default();
        assert!(check_worktree_gate(Path::new("/tmp"), &cfg).is_ok());
    }

    #[test]
    fn gate2_disabled_passes() {
        let cfg = CodeGuardConfig::default();
        assert!(check_schema_gate(Path::new("/tmp"), &cfg).is_ok());
    }

    #[test]
    fn gate3_disabled_passes() {
        let cfg = CodeGuardConfig::default();
        let manifest = serde_json::json!({"version": "1.0", "bindings": []});
        let result = check_pre_push_gate(Path::new("/tmp"), &["foo.rs".into()], &manifest, &cfg);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn gate3_locked_node_blocks() {
        let cfg = CodeGuardConfig {
            gates: GateConfig {
                pre_push_human_approval: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let manifest = serde_json::json!({
            "version": "1.0",
            "bindings": [{
                "bindingId": "test-node",
                "codePath": "src/main.rs",
                "locked": true,
                "bindingHash": "abc123"
            }]
        });
        let result =
            check_pre_push_gate(Path::new("/tmp"), &["src/main.rs".into()], &manifest, &cfg);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, "locked_node_modified");
    }

    #[test]
    fn gate3_unlocked_node_warns_new() {
        let cfg = CodeGuardConfig {
            gates: GateConfig {
                pre_push_human_approval: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let manifest = serde_json::json!({
            "version": "1.0",
            "bindings": [{
                "bindingId": "test-node",
                "codePath": "src/main.rs",
                "locked": false,
                "bindingHash": "abc123"
            }]
        });
        let result =
            check_pre_push_gate(Path::new("/tmp"), &["src/new_file.rs".into()], &manifest, &cfg);
        assert!(result.is_ok());
        let warnings = result.unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(matches!(
            warnings[0].kind,
            GateWarningKind::NewNodeNotInManifest
        ));
    }

    #[test]
    fn find_manifest_returns_none_for_missing() {
        assert!(find_manifest(Path::new("/tmp/nonexistent_codeguard_test")).is_none());
    }

    #[test]
    fn build_graph_empty_manifest() {
        let manifest = serde_json::json!({"version": "1.0", "bindings": []});
        let graph = build_graph(&manifest, Path::new("/tmp"));
        assert_eq!(graph["nodes"].as_array().unwrap().len(), 0);
        assert_eq!(graph["links"].as_array().unwrap().len(), 0);
    }
}
