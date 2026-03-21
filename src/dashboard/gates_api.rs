//! Unified Gates aggregation API.
//!
//! Collects status from all gate subsystems into a single response.
//! Read-only aggregation — gate mutations go through their existing APIs (H3).
//! Worktree creation is the only write operation (via container::worktree).

use super::gate_check;
use super::editor_api::resolve_workspace_root;
use super::DashboardState;
use axum::extract::State as AxumState;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(code: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (code, Json(json!({"error": msg})))
}

fn internal_err(msg: String) -> (StatusCode, Json<Value>) {
    api_err(StatusCode::INTERNAL_SERVER_ERROR, &msg)
}

/// Build the `/api/gates` sub-router. Receives full DashboardState.
pub fn router() -> Router<DashboardState> {
    Router::new()
        .route("/status", get(api_gates_status))
        .route("/worktree/create", post(api_worktree_create))
        .route("/worktree/list", get(api_worktree_list))
}

// ---------------------------------------------------------------------------
// Worktree enforcement helper (H2)
// ---------------------------------------------------------------------------

/// Check whether the given working directory is a git worktree.
/// Returns Ok(()) if enforcement is disabled or the directory is a worktree.
/// Returns Err(message) if enforcement is enabled and the directory is NOT a worktree.
pub fn enforce_worktree_gate(working_dir: &std::path::Path) -> Result<(), String> {
    if std::env::var("AGENTHALO_WORKTREE_GATE")
        .unwrap_or_else(|_| "1".into())
        == "0"
    {
        return Ok(());
    }
    let config = gate_check::load_config(working_dir);
    // Force worktree_required = true for the gate check regardless of config
    let enforced_config = gate_check::CodeGuardConfig {
        gates: gate_check::GateConfig {
            worktree_required: true,
            ..config.gates
        },
        ..config
    };
    gate_check::check_worktree_gate(working_dir, &enforced_config)
        .map_err(|e| e.message)
}

// ---------------------------------------------------------------------------
// GET /api/gates/status — Aggregated status of ALL gates
// ---------------------------------------------------------------------------

async fn api_gates_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    // Category 1: Git Gates
    let git_gates = tokio::task::spawn_blocking(move || collect_git_gates())
        .await
        .map_err(|e| internal_err(format!("git gates join: {e}")))?;

    // Category 2: Communication Gates
    let comm_gates = collect_communication_gates(&state).await;

    // Category 3: Internal Gates
    let internal_gates = collect_internal_gates(&state);

    Ok(Json(json!({
        "ok": true,
        "git_gates": git_gates,
        "communication_gates": comm_gates,
        "internal_gates": internal_gates,
    })))
}

/// Collect git gate data (worktree + codeguard) — runs in spawn_blocking (H4).
fn collect_git_gates() -> Value {
    let profile = crate::halo::workspace_profile::load_active_profile().unwrap_or_default();
    let worktree_enforcement_enabled =
        std::env::var("AGENTHALO_WORKTREE_GATE").unwrap_or_else(|_| "1".into()) == "1";

    // List active worktrees — richer info per worktree
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let worktrees_raw = crate::container::worktree::list_managed_worktrees(
        &cwd,
        &profile.worktree_prefix,
    )
    .unwrap_or_default();
    let worktrees: Vec<Value> = worktrees_raw
        .iter()
        .map(|wt| {
            let short_branch = wt.branch.strip_prefix("refs/heads/").unwrap_or(&wt.branch);
            json!({
                "path": wt.path.display().to_string(),
                "repo_path": wt.repo_path.display().to_string(),
                "session_id": wt.session_id,
                "branch": short_branch,
                "created_at": wt.created_at,
                "injections_count": wt.injections.len(),
            })
        })
        .collect();

    // Also list ALL git worktrees (not just HALO-managed) for full visibility
    let all_worktrees = list_all_git_worktrees(&cwd);

    // CodeGuard summary from active workspace
    let codeguard = match resolve_workspace_root(None) {
        Ok(root) => {
            let config = gate_check::load_config(&root);
            let manifest_path = gate_check::find_manifest(&root);
            let manifest_json: Option<serde_json::Value> = manifest_path.as_ref().and_then(|p| {
                std::fs::read_to_string(p)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            });
            let bindings_count = manifest_json
                .as_ref()
                .and_then(|m| m.get("nodes"))
                .and_then(|n| n.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            let gate1 = gate_check::check_worktree_gate(&root, &config);
            let gate2 = gate_check::check_schema_gate(&root, &config);
            let gate3 = match &manifest_json {
                Some(m) => gate_check::check_pre_push_gate(&root, &[], m, &config).is_ok(),
                None => true, // no manifest = no pre-push gate to fail
            };

            // Detect if we're in a worktree or main checkout
            let is_worktree = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["rev-parse", "--git-dir"])
                .output()
                .ok()
                .map(|o| {
                    let dir = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    // In a worktree, git-dir is typically .git/worktrees/<name>
                    dir.contains("/worktrees/")
                })
                .unwrap_or(false);

            // Get current branch and HEAD
            let current_branch = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["symbolic-ref", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
            let head_short = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

            // Dirty file count
            let dirty_count = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["status", "--porcelain"])
                .output()
                .ok()
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter(|l| !l.is_empty())
                        .count()
                })
                .unwrap_or(0);

            json!({
                "workspace_root": root.display().to_string(),
                "is_worktree": is_worktree,
                "current_branch": current_branch,
                "head": head_short,
                "dirty_files": dirty_count,
                "manifest_exists": manifest_path.is_some(),
                "manifest_path": manifest_path.map(|p| p.display().to_string()),
                "bindings_count": bindings_count,
                "gates": {
                    "worktree_required": config.gates.worktree_required,
                    "schema_required": config.gates.schema_required,
                    "pre_push_hostile_audit": config.gates.pre_push_hostile_audit,
                    "pre_push_human_approval": config.gates.pre_push_human_approval,
                },
                "gate1_pass": gate1.is_ok(),
                "gate1_enabled": config.gates.worktree_required,
                "gate2_pass": gate2.is_ok(),
                "gate2_enabled": config.gates.schema_required,
                "gate3_pass": gate3,
                "gate3_enabled": config.gates.pre_push_hostile_audit || config.gates.pre_push_human_approval,
            })
        }
        Err(_) => json!({
            "workspace_root": null,
            "is_worktree": false,
            "manifest_exists": false,
            "bindings_count": 0,
            "gates": {},
            "gate1_pass": false,
            "gate1_enabled": false,
            "gate2_pass": false,
            "gate2_enabled": false,
            "gate3_pass": false,
            "gate3_enabled": false,
        }),
    };

    json!({
        "worktree_enforcement": {
            "enabled": worktree_enforcement_enabled,
            "managed_worktrees": worktrees,
            "all_worktrees": all_worktrees,
        },
        "codeguard": codeguard,
        "workspace_profile": {
            "name": profile.profile_name,
            "lean_project_path": profile.lean_project_path,
            "worktree_isolation": profile.worktree_isolation,
            "worktree_base": profile.worktree_base,
            "worktree_prefix": profile.worktree_prefix,
            "worktree_branch": profile.worktree_branch,
            "max_worktrees": profile.max_worktrees,
            "external_write_policy": format!("{:?}", profile.external_write_policy),
            "hidden_nav_items": profile.hidden_nav_items,
        },
    })
}

/// List ALL git worktrees (including non-HALO-managed) for full visibility.
fn list_all_git_worktrees(repo_path: &std::path::Path) -> Vec<Value> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "list", "--porcelain"])
        .output();
    let output = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        _ => return vec![],
    };

    let mut worktrees = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    let mut current_head: Option<String> = None;
    let mut is_bare = false;

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            current_path = Some(rest.to_string());
            current_branch = None;
            current_head = None;
            is_bare = false;
        } else if let Some(rest) = line.strip_prefix("HEAD ") {
            current_head = Some(rest[..8.min(rest.len())].to_string());
        } else if let Some(rest) = line.strip_prefix("branch ") {
            current_branch = Some(
                rest.strip_prefix("refs/heads/")
                    .unwrap_or(rest)
                    .to_string(),
            );
        } else if line == "bare" {
            is_bare = true;
        } else if line.is_empty() {
            if let Some(path) = current_path.take() {
                if !is_bare {
                    worktrees.push(json!({
                        "path": path,
                        "branch": current_branch.take(),
                        "head": current_head.take(),
                    }));
                }
            }
        }
    }
    // Handle last entry if file doesn't end with blank line
    if let Some(path) = current_path {
        if !is_bare {
            worktrees.push(json!({
                "path": path,
                "branch": current_branch,
                "head": current_head,
            }));
        }
    }
    worktrees
}

/// Collect communication gate data.
async fn collect_communication_gates(state: &DashboardState) -> Value {
    // Proxy Governor — from live DashboardState (H6)
    let proxy_governor = match state.proxy_governor.status() {
        Ok(status) => serde_json::to_value(&status).unwrap_or(json!({"error": "serialize"})),
        Err(e) => json!({"error": e}),
    };

    // Privacy Controller — default classification behavior
    let privacy = json!({
        "default_level": "Maximum",
        "classifier": "url-based (peer→P2P, local→None, public-infra→None, else→Maximum)",
    });

    // Mesh — check if mesh networking is enabled
    let mesh_enabled = crate::container::mesh_enabled();
    let mesh = json!({
        "enabled": mesh_enabled,
    });

    // OpenClaw — check if CLI is installed
    let openclaw = tokio::task::spawn_blocking(|| {
        let installed = std::process::Command::new("which")
            .arg("openclaw")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        json!({
            "installed": installed,
        })
    })
    .await
    .unwrap_or(json!({"installed": false}));

    // P2PCLAW — check config existence
    let p2pclaw = tokio::task::spawn_blocking(|| {
        let configured = crate::halo::p2pclaw::load_config().is_ok();
        json!({
            "configured": configured,
        })
    })
    .await
    .unwrap_or(json!({"configured": false}));

    // DIDComm — check identity existence
    let didcomm = tokio::task::spawn_blocking(|| {
        let identity_exists = crate::halo::config::identity_config_path().exists();
        json!({
            "identity_present": identity_exists,
        })
    })
    .await
    .unwrap_or(json!({"identity_present": false}));

    // Nym — check availability
    let nym_available = crate::halo::config::nym_state_path().exists();

    json!({
        "proxy_governor": proxy_governor,
        "privacy_controller": privacy,
        "mesh": mesh,
        "openclaw": openclaw,
        "p2pclaw": p2pclaw,
        "didcomm": didcomm,
        "nym": { "available": nym_available },
    })
}

/// Collect internal gate data.
fn collect_internal_gates(state: &DashboardState) -> Value {
    // Proof Gate (H6: use existing load_gate_config)
    let proof_gate = match crate::verifier::gate::load_gate_config() {
        Ok(cfg) => {
            let requirements_count: usize = cfg.requirements.values().map(|v| v.len()).sum();
            let cert_dir = &cfg.certificate_dir;
            let certs_count = std::fs::read_dir(cert_dir)
                .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| {
                    e.path().extension().map(|x| x == "json").unwrap_or(false)
                }).count())
                .unwrap_or(0);
            json!({
                "enabled": cfg.enabled,
                "requirements_count": requirements_count,
                "certificates_count": certs_count,
            })
        }
        Err(_) => json!({
            "enabled": false,
            "requirements_count": 0,
            "certificates_count": 0,
        }),
    };

    // Admission mode
    let admission_mode = crate::halo::admission::AdmissionMode::parse(
        std::env::var("AGENTHALO_ADMISSION_MODE").ok().as_deref(),
    )
    .unwrap_or(crate::halo::admission::AdmissionMode::Warn);

    // EVM Gate
    let (formal_basis, formal_basis_local) = crate::halo::evm_gate::evm_gate_formal_provenance();

    // Crypto state (H6: from shared DashboardState)
    let crypto = {
        let guard = state.crypto_state.lock();
        match guard {
            Ok(mut crypto) => {
                crypto.session.reap_expired();
                let locked = crate::halo::encrypted_file::header_exists()
                    && !crypto.session.is_unlocked();
                let scoped_keys = crypto.session.active_scopes().len();
                json!({
                    "locked": locked,
                    "scoped_keys": scoped_keys,
                    "migration_status": format!("{:?}", crypto.migration_status),
                })
            }
            Err(_) => json!({
                "locked": true,
                "scoped_keys": 0,
                "migration_status": "unknown",
            }),
        }
    };

    // Governors — from shared registry (H6)
    let governors = state.governor_registry.snapshot_all();

    // Policy Registry (H6)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let policy = crate::halo::policy_registry::collect_snapshot(
        Some(&*state.governor_registry),
        now,
    );
    let invariant_violations = crate::halo::policy_registry::validate_invariants(&policy);

    json!({
        "proof_gate": proof_gate,
        "admission": {
            "mode": admission_mode.as_str(),
        },
        "evm_gate": {
            "formal_basis": formal_basis,
            "formal_basis_local": formal_basis_local,
        },
        "crypto": crypto,
        "governors": governors,
        "policy_registry": {
            "schema_version": policy.schema_version,
            "digest": policy.digest,
            "entries_count": policy.entries.len(),
            "invariant_violations": invariant_violations.len(),
            "violation_details": invariant_violations,
        },
    })
}

// ---------------------------------------------------------------------------
// POST /api/gates/worktree/create
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct WorktreeCreateRequest {
    purpose: String,
    #[serde(default)]
    base_ref: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
}

async fn api_worktree_create(
    AxumState(_state): AxumState<DashboardState>,
    Json(body): Json<WorktreeCreateRequest>,
) -> ApiResult {
    let purpose = body.purpose.clone();
    let agent_id = body.agent_id.clone().unwrap_or_else(|| "dashboard".to_string());

    let result = tokio::task::spawn_blocking(move || {
        let profile = crate::halo::workspace_profile::load_active_profile()
            .map_err(|e| format!("load profile: {e}"))?;
        let repo_path = match &profile.lean_project_path {
            Some(p) if !p.is_empty() => {
                std::path::PathBuf::from(crate::halo::workspace_profile::expand_tilde_pub(p))
            }
            _ => std::env::current_dir().map_err(|e| format!("cwd: {e}"))?,
        };
        let session_id = format!("{}_{}", purpose, chrono_compact_now());
        // H5: use existing create_worktree from container::worktree
        crate::container::worktree::create_worktree(
            &repo_path,
            &profile,
            &agent_id,
            &session_id,
        )
    })
    .await
    .map_err(|e| internal_err(format!("worktree create join: {e}")))?
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, &e))?;

    Ok(Json(json!({
        "ok": true,
        "worktree_path": result.path.display().to_string(),
        "session_id": result.session_id,
        "branch": result.branch,
        "created_at": result.created_at,
    })))
}

fn chrono_compact_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

// ---------------------------------------------------------------------------
// GET /api/gates/worktree/list
// ---------------------------------------------------------------------------

async fn api_worktree_list(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let worktrees = tokio::task::spawn_blocking(|| {
        let profile = crate::halo::workspace_profile::load_active_profile().unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        crate::container::worktree::list_managed_worktrees(&cwd, &profile.worktree_prefix)
            .unwrap_or_default()
    })
    .await
    .map_err(|e| internal_err(format!("worktree list join: {e}")))?;

    Ok(Json(json!({
        "ok": true,
        "worktrees": worktrees,
    })))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforce_worktree_gate_disabled_by_env() {
        // With AGENTHALO_WORKTREE_GATE=0, enforcement is bypassed
        std::env::set_var("AGENTHALO_WORKTREE_GATE", "0");
        let result = enforce_worktree_gate(std::path::Path::new("/nonexistent"));
        std::env::remove_var("AGENTHALO_WORKTREE_GATE");
        assert!(result.is_ok());
    }

    #[test]
    fn chrono_compact_produces_digits() {
        let ts = chrono_compact_now();
        assert!(ts.chars().all(|c| c.is_ascii_digit()));
        assert!(!ts.is_empty());
    }
}
