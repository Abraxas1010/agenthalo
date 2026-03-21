//! CodeGuard page API endpoints.
//!
//! All endpoints live under `/api/codeguard/`. They follow the same patterns
//! as `/api/files/*`: `spawn_blocking` for file I/O, `guard_traversal` for paths.
//!
//! Authentication:
//! - GET endpoints: readable by agents and humans alike.
//! - PUT/POST endpoints that mutate state: human dashboard auth required (H3).

use super::editor_api::{err, resolve_workspace_root};
use super::gate_check::{
    build_graph, check_pre_push_gate, check_schema_gate, check_worktree_gate, find_manifest,
    load_config, save_config, CodeGuardConfig,
};
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// In-memory store for active audit sessions.
static AUDIT_SESSIONS: std::sync::LazyLock<Arc<Mutex<HashMap<String, AuditSession>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

#[derive(Debug, Clone, serde::Serialize)]
struct AuditSession {
    session_id: String,
    status: AuditStatus,
    findings: Vec<AuditFinding>,
    decision: Option<AuditDecision>,
    created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AuditStatus {
    InProgress,
    Complete,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, serde::Serialize)]
struct AuditFinding {
    severity: String,
    description: String,
    affected_files: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AuditDecision {
    action: String,
    justification: Option<String>,
    decided_at: u64,
}

/// Build the `/api/codeguard` sub-router.
pub fn router() -> Router {
    Router::new()
        .route("/manifest", get(api_manifest).put(api_manifest_write))
        .route("/graph", get(api_graph))
        .route("/config", get(api_config).put(api_config_write))
        .route("/scan", post(api_scan))
        .route("/verify", post(api_verify))
        .route("/gate-check", post(api_gate_check))
        .route("/audit/spawn", post(api_audit_spawn))
        .route("/audit/{session_id}/status", get(api_audit_status))
        .route("/audit/{session_id}/decision", post(api_audit_decision))
}

// -- Query params -----------------------------------------------------------

#[derive(Deserialize)]
struct RootQuery {
    #[serde(default)]
    root: Option<String>,
}

#[derive(Deserialize)]
struct GateCheckBody {
    agent_working_dir: String,
    modified_files: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct AuditSpawnBody {
    #[serde(default)]
    worktree_path: Option<String>,
    #[serde(default)]
    implementing_agent_id: Option<String>,
}

#[derive(Deserialize)]
struct AuditDecisionBody {
    action: String,
    justification: Option<String>,
}

// -- GET /api/codeguard/manifest --------------------------------------------

async fn api_manifest(Query(q): Query<RootQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        match find_manifest(&root) {
            Some(path) => {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read manifest: {e}"))?;
                let parsed: Value =
                    serde_json::from_str(&content).map_err(|e| format!("parse manifest: {e}"))?;
                Ok(json!({ "ok": true, "manifest": parsed, "path": path.display().to_string() }))
            }
            None => Err("no codeguard.lock.json found".to_string()),
        }
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::NOT_FOUND, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- PUT /api/codeguard/manifest (Human only — H3) --------------------------

async fn api_manifest_write(Json(body): Json<Value>) -> impl IntoResponse {
    let root_override = body
        .get("root")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let manifest_data = match body.get("manifest") {
        Some(m) => m.clone(),
        None => return err(StatusCode::BAD_REQUEST, "missing 'manifest' field").into_response(),
    };

    let root = match resolve_workspace_root(root_override.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let manifest_path =
            find_manifest(&root).unwrap_or_else(|| root.join("codeguard.lock.json"));
        let json =
            serde_json::to_string_pretty(&manifest_data).map_err(|e| format!("serialize: {e}"))?;
        if let Some(parent) = manifest_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create dir: {e}"))?;
        }
        std::fs::write(&manifest_path, json).map_err(|e| format!("write manifest: {e}"))?;
        Ok::<_, String>(json!({
            "ok": true,
            "path": manifest_path.display().to_string(),
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- GET /api/codeguard/graph -----------------------------------------------

async fn api_graph(Query(q): Query<RootQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        match find_manifest(&root) {
            Some(path) => {
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("read manifest: {e}"))?;
                let manifest: Value =
                    serde_json::from_str(&content).map_err(|e| format!("parse: {e}"))?;
                let graph = build_graph(&manifest, &root);
                Ok(json!({ "ok": true, "graph": graph }))
            }
            None => Err("no codeguard.lock.json found".to_string()),
        }
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::NOT_FOUND, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- GET /api/codeguard/config ----------------------------------------------

async fn api_config(Query(q): Query<RootQuery>) -> impl IntoResponse {
    let root = match resolve_workspace_root(q.root.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let config = load_config(&root);
        Ok::<_, String>(json!({ "ok": true, "config": config }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- PUT /api/codeguard/config (Human only) ---------------------------------

async fn api_config_write(Json(body): Json<Value>) -> impl IntoResponse {
    let root_override = body
        .get("root")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let config_data = match body.get("config") {
        Some(c) => c.clone(),
        None => return err(StatusCode::BAD_REQUEST, "missing 'config' field").into_response(),
    };

    let root = match resolve_workspace_root(root_override.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let config: CodeGuardConfig =
            serde_json::from_value(config_data).map_err(|e| format!("invalid config: {e}"))?;
        save_config(&root, &config)?;
        Ok::<_, String>(json!({ "ok": true }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- POST /api/codeguard/scan -----------------------------------------------

async fn api_scan(Json(body): Json<Value>) -> impl IntoResponse {
    let root_override = body
        .get("root")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let root = match resolve_workspace_root(root_override.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let mut proposed_bindings = Vec::new();

        let scan_dirs = vec![
            root.clone(),
            root.join("src"),
            root.join("lean"),
            root.join("projects"),
        ];

        for dir in scan_dirs {
            if !dir.is_dir() {
                continue;
            }
            scan_directory(&dir, &root, &mut proposed_bindings, 0)?;
        }

        let existing = find_manifest(&root)
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .and_then(|c| serde_json::from_str::<Value>(&c).ok());

        let binding_count = proposed_bindings.len();
        let proposed = json!({
            "version": existing.as_ref()
                .and_then(|e| e.get("version"))
                .cloned()
                .unwrap_or(json!("1.0")),
            "bindings": proposed_bindings,
            "scan_summary": {
                "files_scanned": binding_count,
                "existing_bindings": existing.as_ref()
                    .and_then(|e| e.get("bindings"))
                    .and_then(|b| b.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0),
            }
        });

        Ok::<_, String>(json!({ "ok": true, "proposed": proposed }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

fn scan_directory(
    dir: &std::path::Path,
    root: &std::path::Path,
    bindings: &mut Vec<Value>,
    depth: usize,
) -> Result<(), String> {
    if depth > 5 {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == ".lake"
        {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            scan_directory(&path, root, bindings, depth + 1)?;
        } else if path.is_file() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if matches!(
                ext,
                "lean" | "rs" | "py" | "js" | "ts" | "sol" | "v" | "thy"
            ) {
                if let Ok(rel) = path.strip_prefix(root) {
                    let rel_str = rel.to_string_lossy().to_string();
                    let hash = sha256_of_file(&path);
                    bindings.push(json!({
                        "codePath": rel_str,
                        "codeHash": hash,
                        "locked": false,
                        "reviewRequired": false,
                    }));
                }
            }
        }
    }
    Ok(())
}

/// Compute SHA-256 of a file by shelling out to `sha256sum`.
fn sha256_of_file(path: &std::path::Path) -> String {
    let output = std::process::Command::new("sha256sum").arg(path).output();
    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            s.split_whitespace().next().unwrap_or("").to_string()
        }
        _ => String::new(),
    }
}

// -- POST /api/codeguard/verify ---------------------------------------------

async fn api_verify(Json(body): Json<Value>) -> impl IntoResponse {
    let root_override = body
        .get("root")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let root = match resolve_workspace_root(root_override.as_deref()) {
        Ok(r) => r,
        Err(e) => return e.into_response(),
    };

    let result = tokio::task::spawn_blocking(move || {
        let manifest_path = find_manifest(&root)
            .ok_or_else(|| "no codeguard.lock.json found".to_string())?;

        // Look for codeguard_verify.sh in the repo
        let verify_script = root.join("scripts").join("codeguard_verify.sh");
        if !verify_script.is_file() {
            return Ok::<_, String>(json!({
                "ok": true,
                "verified": false,
                "error": "codeguard_verify.sh not found in repo",
            }));
        }

        let output = std::process::Command::new("bash")
            .arg(&verify_script)
            .arg(&manifest_path)
            .current_dir(&root)
            .output()
            .map_err(|e| format!("exec verify: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok::<_, String>(json!({
                "ok": true,
                "verified": true,
                "output": stdout.trim(),
            }))
        } else {
            Ok::<_, String>(json!({
                "ok": true,
                "verified": false,
                "output": stdout.trim(),
                "error": stderr.trim(),
            }))
        }
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- POST /api/codeguard/gate-check -----------------------------------------

async fn api_gate_check(Json(body): Json<GateCheckBody>) -> impl IntoResponse {
    let working_dir = body.agent_working_dir.clone();
    let modified_files = body.modified_files.clone().unwrap_or_default();

    let result = tokio::task::spawn_blocking(move || {
        let working_path = std::path::PathBuf::from(&working_dir);
        if !working_path.is_dir() {
            return Err("agent_working_dir does not exist".to_string());
        }

        let repo_root = if let Ok(repo) = git2::Repository::discover(&working_path) {
            repo.workdir()
                .map(|p| p.to_path_buf())
                .unwrap_or(working_path.clone())
        } else {
            working_path.clone()
        };

        let config = load_config(&repo_root);
        let mut gate_results = Vec::new();

        // Gate 1: Worktree
        match check_worktree_gate(&working_path, &config) {
            Ok(()) => gate_results.push(json!({
                "gate": "worktree",
                "status": "pass",
                "enabled": config.gates.worktree_required,
            })),
            Err(e) => gate_results.push(json!({
                "gate": "worktree",
                "status": "fail",
                "enabled": config.gates.worktree_required,
                "error": e.message,
            })),
        }

        // Gate 2: Schema
        match check_schema_gate(&repo_root, &config) {
            Ok(()) => gate_results.push(json!({
                "gate": "schema",
                "status": "pass",
                "enabled": config.gates.schema_required,
            })),
            Err(e) => gate_results.push(json!({
                "gate": "schema",
                "status": "fail",
                "enabled": config.gates.schema_required,
                "error": e.message,
            })),
        }

        // Gate 3: Pre-push
        if !modified_files.is_empty() {
            let manifest = find_manifest(&repo_root)
                .and_then(|p| std::fs::read_to_string(&p).ok())
                .and_then(|c| serde_json::from_str::<Value>(&c).ok())
                .unwrap_or(json!({"version": "1.0", "bindings": []}));

            match check_pre_push_gate(&repo_root, &modified_files, &manifest, &config) {
                Ok(warnings) => gate_results.push(json!({
                    "gate": "pre_push",
                    "status": if warnings.is_empty() { "pass" } else { "warn" },
                    "enabled": config.gates.pre_push_human_approval || config.gates.pre_push_hostile_audit,
                    "warnings": warnings,
                })),
                Err(e) => gate_results.push(json!({
                    "gate": "pre_push",
                    "status": "fail",
                    "enabled": config.gates.pre_push_human_approval || config.gates.pre_push_hostile_audit,
                    "error": e.message,
                })),
            }
        } else {
            gate_results.push(json!({
                "gate": "pre_push",
                "status": "skip",
                "enabled": config.gates.pre_push_human_approval || config.gates.pre_push_hostile_audit,
                "reason": "no modified files provided",
            }));
        }

        let all_pass = gate_results.iter().all(|g| {
            let status = g.get("status").and_then(|s| s.as_str()).unwrap_or("");
            status == "pass" || status == "skip"
        });

        Ok::<_, String>(json!({
            "ok": true,
            "gates": gate_results,
            "all_pass": all_pass,
        }))
    })
    .await;

    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e)) => err(StatusCode::BAD_REQUEST, &e).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("task: {e}")).into_response(),
    }
}

// -- POST /api/codeguard/audit/spawn (Human only) ---------------------------

async fn api_audit_spawn(Json(_body): Json<AuditSpawnBody>) -> impl IntoResponse {
    let session_id = format!("audit-{}", now_unix_secs());
    let session = AuditSession {
        session_id: session_id.clone(),
        status: AuditStatus::InProgress,
        findings: vec![],
        decision: None,
        created_at: now_unix_secs(),
    };

    if let Ok(mut sessions) = AUDIT_SESSIONS.lock() {
        sessions.insert(session_id.clone(), session);
    }

    // v1: placeholder — audit agent spawn will be integrated with PtyManager
    // or Orchestrator in a future iteration. The session is tracked and
    // the decision flow works end-to-end.
    Json(json!({
        "ok": true,
        "audit_session_id": session_id,
        "status": "in_progress",
        "message": "Audit agent session created. Configure an LLM backend in .codeguard/config.json to enable hostile audit.",
    }))
    .into_response()
}

// -- GET /api/codeguard/audit/{session_id}/status ---------------------------

async fn api_audit_status(
    axum::extract::Path(session_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let sessions = AUDIT_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    match sessions.get(&session_id) {
        Some(session) => Json(json!({
            "ok": true,
            "session_id": session.session_id,
            "status": session.status,
            "findings": session.findings,
            "decision": session.decision,
            "created_at": session.created_at,
        }))
        .into_response(),
        None => err(StatusCode::NOT_FOUND, "audit session not found").into_response(),
    }
}

// -- POST /api/codeguard/audit/{session_id}/decision (Human only — H11) -----

async fn api_audit_decision(
    axum::extract::Path(session_id): axum::extract::Path<String>,
    Json(body): Json<AuditDecisionBody>,
) -> impl IntoResponse {
    let mut sessions = AUDIT_SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    match sessions.get_mut(&session_id) {
        Some(session) => {
            // H11: CRIT/HIGH override requires justification
            let has_critical = session
                .findings
                .iter()
                .any(|f| matches!(f.severity.as_str(), "CRITICAL" | "HIGH"));

            if has_critical
                && body.action == "approve"
                && body
                    .justification
                    .as_ref()
                    .map(|j| j.trim().is_empty())
                    .unwrap_or(true)
            {
                return err(
                    StatusCode::BAD_REQUEST,
                    "H11: approving with CRITICAL/HIGH findings requires a justification",
                )
                .into_response();
            }

            session.decision = Some(AuditDecision {
                action: body.action.clone(),
                justification: body.justification.clone(),
                decided_at: now_unix_secs(),
            });

            Json(json!({
                "ok": true,
                "session_id": session_id,
                "decision": session.decision,
            }))
            .into_response()
        }
        None => err(StatusCode::NOT_FOUND, "audit session not found").into_response(),
    }
}

// -- Helpers ----------------------------------------------------------------

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
