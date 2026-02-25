//! JSON API endpoints for the AgentHALO dashboard.
//!
//! All handlers are thin wrappers around existing library functions.
//! Every endpoint returns JSON and is usable for both the web dashboard
//! and scripting/automation.
//!
//! **Concurrency note:** The H.A.L.O. trace store uses redb, which takes
//! a file-level exclusive lock. All database-touching handlers acquire
//! `state.db_lock` before opening the database to prevent concurrent-open
//! errors when the browser fires parallel requests (e.g. Promise.all).

use super::DashboardState;
use crate::cli::default_witness_cfg;
use crate::halo::addons;
use crate::halo::agentpmt;
use crate::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use crate::halo::auth::{is_authenticated, resolve_api_key};
use crate::halo::config;
use crate::halo::onchain::load_onchain_config_or_default;
use crate::halo::pq::has_wallet;
use crate::halo::trace::{
    cost_buckets, list_sessions, paid_breakdown_by_operation_type, paid_cost_buckets,
    record_paid_operation_for_halo, session_events, session_summary,
};
use crate::halo::trust::query_trust_score;
use crate::halo::viewer::export_session_json;
use crate::halo::wrap;
use crate::halo::x402;
use crate::persistence::{default_wal_path, load_snapshot, persist_snapshot_and_sync_wal};
use crate::protocol::NucleusDb;
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::witness::WitnessSignatureAlgorithm;
use crate::VcBackend;

use axum::extract::{Path, Query, State as AxumState};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::StreamExt;

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn api_router(state: DashboardState) -> Router<DashboardState> {
    Router::new()
        // Status
        .route("/status", get(api_status))
        // Sessions
        .route("/sessions", get(api_sessions))
        .route("/sessions/{id}", get(api_session_detail))
        .route("/sessions/{id}/events", get(api_session_events))
        .route("/sessions/{id}/export", get(api_session_export))
        .route("/sessions/{id}/attest", post(api_session_attest))
        // Costs
        .route("/costs", get(api_costs))
        .route("/costs/daily", get(api_costs_daily))
        .route("/costs/by-agent", get(api_costs_by_agent))
        .route("/costs/by-model", get(api_costs_by_model))
        .route("/costs/paid", get(api_costs_paid))
        // Config
        .route("/config", get(api_config))
        .route("/config/wrap", post(api_config_wrap))
        .route("/config/x402", post(api_config_x402))
        // Trust & Attestations
        .route("/trust/{session_id}", get(api_trust))
        .route("/attestations", get(api_attestations))
        .route("/attestations/verify", post(api_attestation_verify))
        // NucleusDB
        .route("/nucleusdb/status", get(api_nucleusdb_status))
        .route("/nucleusdb/browse", get(api_nucleusdb_browse))
        .route("/nucleusdb/stats", get(api_nucleusdb_stats))
        .route("/nucleusdb/sql", post(api_nucleusdb_sql))
        .route("/nucleusdb/history", get(api_nucleusdb_history))
        .route("/nucleusdb/edit", post(api_nucleusdb_edit))
        .route("/nucleusdb/verify/{key}", get(api_nucleusdb_verify))
        .route(
            "/nucleusdb/key-history/{key}",
            get(api_nucleusdb_key_history),
        )
        .route("/nucleusdb/export", get(api_nucleusdb_export))
        .route(
            "/nucleusdb/vector-search",
            post(api_nucleusdb_vector_search),
        )
        // Capabilities
        .route("/capabilities", get(api_capabilities))
        // x402
        .route("/x402/summary", get(api_x402_summary))
        .route("/x402/balance", get(api_x402_balance))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Query parameter types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct SessionsQuery {
    agent: Option<String>,
    model: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize, Default)]
pub struct CostsQuery {
    monthly: Option<bool>,
}

#[derive(Deserialize)]
pub struct WrapRequest {
    agent: String,
    enable: bool,
}

#[derive(Deserialize)]
pub struct X402ConfigUpdate {
    enabled: Option<bool>,
    network: Option<String>,
    max_auto_approve: Option<u64>,
}

#[derive(Deserialize)]
pub struct SqlRequest {
    query: String,
}

#[derive(Deserialize)]
pub struct VerifyRequest {
    digest: String,
}

#[derive(Deserialize, Default)]
pub struct BrowseQuery {
    page: Option<usize>,
    page_size: Option<usize>,
    prefix: Option<String>,
    sort: Option<String>,
    order: Option<String>,
}

#[derive(Deserialize)]
pub struct EditRequest {
    key: String,
    #[serde(default, rename = "type")]
    value_type: Option<String>,
    /// Value can be a number (u64 legacy), string, JSON object, array, bool, or null.
    value: serde_json::Value,
}

#[derive(Deserialize)]
pub struct VectorSearchRequest {
    query: Vec<f64>,
    #[serde(default = "default_k")]
    k: usize,
    #[serde(default = "default_metric")]
    metric: String,
    /// Optional key prefix filter (e.g., "memory:strategy:" to search only within a namespace).
    #[serde(default)]
    prefix: Option<String>,
}

fn default_k() -> usize {
    10
}

fn default_metric() -> String {
    "cosine".to_string()
}

#[derive(Deserialize, Default)]
pub struct ExportQuery {
    format: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn api_err(code: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (code, Json(json!({"error": msg})))
}

fn internal_err(msg: String) -> (StatusCode, Json<Value>) {
    api_err(StatusCode::INTERNAL_SERVER_ERROR, &msg)
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

async fn api_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let db_path = &state.db_path;
    let creds_path = &state.credentials_path;
    let has_auth = is_authenticated(creds_path) || resolve_api_key(creds_path).is_some();
    let pmt_cfg = agentpmt::load_or_default();
    let x402_cfg = x402::load_x402_config();

    let (session_count, total_cost, total_tokens) = {
        let _guard = state.db_lock.lock().await;
        let sessions = list_sessions(db_path).unwrap_or_default();
        let count = sessions.len();
        let mut cost = 0.0f64;
        let mut tokens = 0u64;
        for s in &sessions {
            if let Ok(Some(summary)) = session_summary(db_path, &s.session_id) {
                cost += summary.estimated_cost_usd;
                tokens += summary.total_input_tokens + summary.total_output_tokens;
            }
        }
        (count, cost, tokens)
    };

    // Agent wrapping status
    let rc = wrap::detect_shell_rc();
    let rc_content = std::fs::read_to_string(&rc).unwrap_or_default();
    let wrap_status = |agent: &str| -> bool {
        let marker = format!("# AGENTHALO_WRAP_{}", agent.to_ascii_uppercase());
        rc_content.contains(&marker)
    };

    Ok(Json(json!({
        "version": "0.3.0",
        "authenticated": has_auth,
        "tool_proxy_enabled": pmt_cfg.enabled,
        "x402_enabled": x402_cfg.enabled,
        "session_count": session_count,
        "total_cost_usd": total_cost,
        "total_tokens": total_tokens,
        "db_path": db_path.to_string_lossy(),
        "wrapping": {
            "claude": wrap_status("claude"),
            "codex": wrap_status("codex"),
            "gemini": wrap_status("gemini"),
        },
        "pq_wallet": has_wallet(),
    })))
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

async fn api_sessions(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<SessionsQuery>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let sessions = list_sessions(&state.db_path).map_err(internal_err)?;

    let mut items: Vec<Value> = Vec::new();
    for s in &sessions {
        if let Some(ref agent_filter) = params.agent {
            if !s
                .agent
                .to_lowercase()
                .contains(&agent_filter.to_lowercase())
            {
                continue;
            }
        }
        if let Some(ref model_filter) = params.model {
            let model = s.model.as_deref().unwrap_or("");
            if !model.to_lowercase().contains(&model_filter.to_lowercase()) {
                continue;
            }
        }

        let summary = session_summary(&state.db_path, &s.session_id)
            .ok()
            .flatten();
        items.push(json!({
            "session": s,
            "summary": summary,
        }));

        if let Some(limit) = params.limit {
            if items.len() >= limit {
                break;
            }
        }
    }

    Ok(Json(json!({"sessions": items, "total": items.len()})))
}

async fn api_session_detail(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let sessions = list_sessions(&state.db_path).map_err(internal_err)?;
    let meta = sessions
        .into_iter()
        .find(|s| s.session_id == id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "session not found"))?;
    let summary = session_summary(&state.db_path, &id).map_err(internal_err)?;
    let events = session_events(&state.db_path, &id).map_err(internal_err)?;

    Ok(Json(json!({
        "session": meta,
        "summary": summary,
        "events": events,
    })))
}

async fn api_session_events(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let events = session_events(&state.db_path, &id).map_err(internal_err)?;
    Ok(Json(json!({"events": events})))
}

async fn api_session_export(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let export = export_session_json(&state.db_path, &id).map_err(internal_err)?;
    Ok(Json(export))
}

async fn api_session_attest(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let resolved = resolve_session_id(&state.db_path, Some(&id)).map_err(internal_err)?;
    let result = attest_session(
        &state.db_path,
        AttestationRequest {
            session_id: resolved.clone(),
            anonymous: false,
        },
    )
    .map_err(internal_err)?;

    let save_path = save_attestation(&resolved, &result).map_err(internal_err)?;
    let _ = record_paid_operation_for_halo(
        "attest",
        0,
        Some(resolved),
        Some(result.attestation_digest.clone()),
        true,
        None,
    );

    Ok(Json(json!({
        "attestation": result,
        "saved_to": save_path.to_string_lossy(),
    })))
}

// ---------------------------------------------------------------------------
// Costs
// ---------------------------------------------------------------------------

async fn api_costs(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<CostsQuery>,
) -> ApiResult {
    let monthly = params.monthly.unwrap_or(false);
    let _guard = state.db_lock.lock().await;
    let rows = cost_buckets(&state.db_path, monthly).map_err(internal_err)?;

    let total_cost: f64 = rows.iter().map(|r| r.cost_usd).sum();
    let total_tokens: u64 = rows.iter().map(|r| r.input_tokens + r.output_tokens).sum();
    let total_sessions: u64 = rows.iter().map(|r| r.sessions).sum();

    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "label": r.label,
                "sessions": r.sessions,
                "input_tokens": r.input_tokens,
                "output_tokens": r.output_tokens,
                "cache_tokens": r.cache_tokens,
                "cost_usd": r.cost_usd,
            })
        })
        .collect();

    Ok(Json(json!({
        "buckets": items,
        "total_sessions": total_sessions,
        "total_tokens": total_tokens,
        "total_cost_usd": total_cost,
        "granularity": if monthly { "monthly" } else { "daily" },
    })))
}

async fn api_costs_daily(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let rows = cost_buckets(&state.db_path, false).map_err(internal_err)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "date": r.label,
                "cost_usd": r.cost_usd,
                "tokens": r.input_tokens + r.output_tokens,
                "sessions": r.sessions,
            })
        })
        .collect();
    Ok(Json(json!({"daily": items})))
}

async fn api_costs_by_agent(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let sessions = list_sessions(&state.db_path).map_err(internal_err)?;
    let mut by_agent: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for s in &sessions {
        if let Ok(Some(summary)) = session_summary(&state.db_path, &s.session_id) {
            *by_agent.entry(s.agent.clone()).or_default() += summary.estimated_cost_usd;
        }
    }
    let items: Vec<Value> = by_agent
        .into_iter()
        .map(|(agent, cost)| json!({"agent": agent, "cost_usd": cost}))
        .collect();
    Ok(Json(json!({"by_agent": items})))
}

async fn api_costs_by_model(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let sessions = list_sessions(&state.db_path).map_err(internal_err)?;
    let mut by_model: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for s in &sessions {
        if let Ok(Some(summary)) = session_summary(&state.db_path, &s.session_id) {
            let model = s.model.clone().unwrap_or_else(|| "unknown".to_string());
            *by_model.entry(model).or_default() += summary.estimated_cost_usd;
        }
    }
    let items: Vec<Value> = by_model
        .into_iter()
        .map(|(model, cost)| json!({"model": model, "cost_usd": cost}))
        .collect();
    Ok(Json(json!({"by_model": items})))
}

async fn api_costs_paid(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let buckets = paid_cost_buckets(&state.db_path, false).unwrap_or_default();
    let by_type = paid_breakdown_by_operation_type(&state.db_path).unwrap_or_default();

    let bucket_items: Vec<Value> = buckets
        .iter()
        .map(|b| {
            json!({
                "label": b.label,
                "operations": b.operations,
                "credits_spent": b.credits_spent,
                "usd_spent": b.usd_spent,
            })
        })
        .collect();

    let type_items: Vec<Value> = by_type
        .into_iter()
        .map(|(op, count, credits, usd)| {
            json!({
                "operation": op,
                "count": count,
                "credits_spent": credits,
                "usd_spent": usd,
            })
        })
        .collect();

    Ok(Json(json!({
        "buckets": bucket_items,
        "by_type": type_items,
    })))
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

async fn api_config(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let creds_path = &state.credentials_path;
    let has_auth = is_authenticated(creds_path) || resolve_api_key(creds_path).is_some();
    let pmt_cfg = agentpmt::load_or_default();
    let addons_cfg = addons::load_or_default();
    let x402_cfg = x402::load_x402_config();
    let onchain_cfg = load_onchain_config_or_default();

    let rc = wrap::detect_shell_rc();
    let rc_content = std::fs::read_to_string(&rc).unwrap_or_default();
    let wrap_status = |agent: &str| -> bool {
        let marker = format!("# AGENTHALO_WRAP_{}", agent.to_ascii_uppercase());
        rc_content.contains(&marker)
    };

    Ok(Json(json!({
        "authentication": {
            "authenticated": has_auth,
        },
        "wrapping": {
            "claude": wrap_status("claude"),
            "codex": wrap_status("codex"),
            "gemini": wrap_status("gemini"),
            "shell_rc": rc.to_string_lossy(),
        },
        "x402": {
            "enabled": x402_cfg.enabled,
            "network": x402_cfg.preferred_network,
            "max_auto_approve": x402_cfg.max_auto_approve,
            "max_auto_approve_usd": x402_cfg.max_auto_approve as f64 / 1_000_000.0,
        },
        "agentpmt": {
            "enabled": pmt_cfg.enabled,
            "budget_tag": pmt_cfg.budget_tag,
        },
        "onchain": {
            "rpc_url": onchain_cfg.rpc_url,
            "chain_id": onchain_cfg.chain_id,
            "chain_name": onchain_cfg.chain_name,
            "contract_address": onchain_cfg.contract_address,
        },
        "addons": {
            "p2pclaw": addons_cfg.p2pclaw_enabled,
            "agentpmt_workflows": addons_cfg.agentpmt_workflows_enabled,
        },
        "pq_wallet": has_wallet(),
        "paths": {
            "home": config::halo_dir().to_string_lossy(),
            "db": state.db_path.to_string_lossy(),
            "credentials": state.credentials_path.to_string_lossy(),
        },
    })))
}

/// Allowed agent names for shell wrapping — prevents shell RC injection.
const ALLOWED_WRAP_AGENTS: &[&str] = &["claude", "codex", "gemini"];

async fn api_config_wrap(
    AxumState(_state): AxumState<DashboardState>,
    Json(req): Json<WrapRequest>,
) -> ApiResult {
    if !ALLOWED_WRAP_AGENTS.contains(&req.agent.as_str()) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "agent must be one of: claude, codex, gemini",
        ));
    }
    let rc = wrap::detect_shell_rc();
    if req.enable {
        wrap::wrap_agent(&req.agent, &rc).map_err(internal_err)?;
    } else {
        wrap::unwrap_agent(&req.agent, &rc).map_err(internal_err)?;
    }
    Ok(Json(json!({
        "ok": true,
        "agent": req.agent,
        "enabled": req.enable,
    })))
}

async fn api_config_x402(
    AxumState(_state): AxumState<DashboardState>,
    Json(req): Json<X402ConfigUpdate>,
) -> ApiResult {
    let mut cfg = x402::load_x402_config();
    if let Some(enabled) = req.enabled {
        cfg.enabled = enabled;
    }
    if let Some(ref network) = req.network {
        if !matches!(network.as_str(), "base" | "base-sepolia") {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "network must be 'base' or 'base-sepolia'",
            ));
        }
        cfg.preferred_network = network.clone();
    }
    if let Some(max) = req.max_auto_approve {
        cfg.max_auto_approve = max;
    }
    x402::save_x402_config(&cfg).map_err(internal_err)?;
    Ok(Json(json!({"ok": true, "config": {
        "enabled": cfg.enabled,
        "network": cfg.preferred_network,
        "max_auto_approve": cfg.max_auto_approve,
    }})))
}

// ---------------------------------------------------------------------------
// Trust & Attestations
// ---------------------------------------------------------------------------

async fn api_trust(
    AxumState(state): AxumState<DashboardState>,
    Path(session_id): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let score = query_trust_score(&state.db_path, Some(&session_id)).map_err(internal_err)?;
    Ok(Json(json!({"trust": score})))
}

async fn api_attestations(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let attest_dir = config::attestations_dir();
    let mut attestations = Vec::new();
    if attest_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&attest_dir) {
            for entry in entries.flatten() {
                if entry
                    .path()
                    .extension()
                    .map(|e| e == "json")
                    .unwrap_or(false)
                {
                    if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
                            attestations.push(val);
                        }
                    }
                }
            }
        }
    }
    Ok(Json(
        json!({"attestations": attestations, "count": attestations.len()}),
    ))
}

async fn api_attestation_verify(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<VerifyRequest>,
) -> ApiResult {
    use crate::halo::attest::AttestationResult;

    // 1. Find the stored attestation by digest
    let attest_dir = config::attestations_dir();
    let mut stored: Option<AttestationResult> = None;
    if attest_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&attest_dir) {
            for entry in entries.flatten() {
                if let Ok(raw) = std::fs::read_to_string(entry.path()) {
                    if let Ok(val) = serde_json::from_str::<AttestationResult>(&raw) {
                        if val.attestation_digest == req.digest {
                            stored = Some(val);
                            break;
                        }
                    }
                }
            }
        }
    }

    let stored = match stored {
        Some(s) => s,
        None => {
            return Ok(Json(json!({
                "digest": req.digest,
                "found": false,
                "verified": false,
                "reason": "no attestation with this digest in local store",
            })));
        }
    };

    // 2. Cryptographic verification: re-attest the session and compare
    let session_id = match &stored.session_id {
        Some(id) => id.clone(),
        None => {
            // Anonymous attestation — verify membership proof if present
            let membership_ok = stored
                .anonymous_membership_proof
                .as_ref()
                .map(|p| crate::halo::attest::verify_anonymous_membership_proof(p).unwrap_or(false))
                .unwrap_or(false);
            return Ok(Json(json!({
                "digest": req.digest,
                "found": true,
                "verified": membership_ok,
                "proof_type": stored.proof_type,
                "event_count": stored.event_count,
                "reason": if membership_ok {
                    "anonymous attestation: membership proof verified"
                } else {
                    "anonymous attestation: cannot re-derive (session id blinded)"
                },
            })));
        }
    };

    // Re-compute attestation from the live session events (needs db lock)
    let _guard = state.db_lock.lock().await;
    let recomputed = attest_session(
        &state.db_path,
        AttestationRequest {
            session_id: session_id.clone(),
            anonymous: false,
        },
    );

    match recomputed {
        Ok(fresh) => {
            let digest_match = fresh.attestation_digest == stored.attestation_digest;
            let root_match = fresh.merkle_root == stored.merkle_root;
            let count_match = fresh.event_count == stored.event_count;
            let verified = digest_match && root_match && count_match;

            Ok(Json(json!({
                "digest": req.digest,
                "found": true,
                "verified": verified,
                "checks": {
                    "digest_match": digest_match,
                    "merkle_root_match": root_match,
                    "event_count_match": count_match,
                },
                "stored_merkle_root": stored.merkle_root,
                "recomputed_merkle_root": fresh.merkle_root,
                "event_count": stored.event_count,
                "proof_type": stored.proof_type,
            })))
        }
        Err(e) => Ok(Json(json!({
            "digest": req.digest,
            "found": true,
            "verified": false,
            "reason": format!("re-attestation failed: {e}"),
            "note": "session events may have been modified or deleted since attestation",
        }))),
    }
}

// ---------------------------------------------------------------------------
// NucleusDB
// ---------------------------------------------------------------------------

async fn api_nucleusdb_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let db_path = &state.db_path;
    let exists = db_path.exists();
    let sessions = if exists {
        let _guard = state.db_lock.lock().await;
        list_sessions(db_path).unwrap_or_default().len()
    } else {
        0
    };
    Ok(Json(json!({
        "db_path": db_path.to_string_lossy(),
        "exists": exists,
        "backend": "binary_merkle",
        "session_count": sessions,
    })))
}

async fn api_nucleusdb_browse(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<BrowseQuery>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let db = load_halo_db(&state.db_path)?;

    let page = params.page.unwrap_or(0);
    let page_size = params.page_size.unwrap_or(50).min(500);
    let sort_field = params.sort.as_deref().unwrap_or("key");
    let sort_order = params.order.as_deref().unwrap_or("asc");

    // Collect all matching key-value pairs with typed information
    let mut items: Vec<(String, Value, usize, String, String)> = Vec::new();
    for (key, idx) in db.keymap.all_keys() {
        if let Some(ref pfx) = params.prefix {
            if !pfx.is_empty() && !key.starts_with(pfx.as_str()) {
                continue;
            }
        }
        let cell = db.state.values.get(idx).copied().unwrap_or(0);
        let tag = db.type_map.get(key);
        let blob = db.blob_store.get(key);
        let typed = crate::typed_value::TypedValue::decode(tag, cell, blob)
            .map_err(|e| internal_err(format!("typed decode failed for key '{key}': {e}")))?;
        let display = typed.display_string();
        let json_val = typed.to_json_value();
        items.push((
            key.to_string(),
            json_val,
            idx,
            tag.as_str().to_string(),
            display,
        ));
    }

    let total = items.len();

    // Sort
    match (sort_field, sort_order) {
        ("key", "desc") => items.sort_by(|a, b| b.0.cmp(&a.0)),
        ("type", "asc") => items.sort_by(|a, b| a.3.cmp(&b.3)),
        ("type", "desc") => items.sort_by(|a, b| b.3.cmp(&a.3)),
        ("index", "asc") => items.sort_by_key(|i| i.2),
        ("index", "desc") => items.sort_by(|a, b| b.2.cmp(&a.2)),
        _ => items.sort_by(|a, b| a.0.cmp(&b.0)), // default: key asc
    }

    // Paginate
    let start = page * page_size;
    let page_items: Vec<Value> = items
        .iter()
        .skip(start)
        .take(page_size)
        .map(|(key, json_val, idx, type_tag, display)| {
            json!({
                "key": key,
                "value": json_val,
                "display": display,
                "index": idx,
                "type": type_tag,
            })
        })
        .collect();

    let total_pages = if total == 0 {
        1
    } else {
        total.div_ceil(page_size)
    };

    Ok(Json(json!({
        "rows": page_items,
        "total": total,
        "page": page,
        "page_size": page_size,
        "total_pages": total_pages,
    })))
}

async fn api_nucleusdb_stats(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let db = load_halo_db(&state.db_path)?;

    let key_count = db.keymap.len();
    let commit_count = db.entries.len();
    let write_mode = format!("{:?}", db.write_mode());

    // Value statistics — only over Integer and Float typed keys (blob-type content
    // hashes are meaningless for min/max/avg/sum).
    let mut min_val: Option<f64> = None;
    let mut max_val: Option<f64> = None;
    let mut sum: f64 = 0.0;
    let mut numeric_count: usize = 0;
    for (key, idx) in db.keymap.all_keys() {
        let tag = db.type_map.get(key);
        let cell = db.state.values.get(idx).copied().unwrap_or(0);
        let numeric = match tag {
            crate::typed_value::TypeTag::Integer => Some(cell as i64 as f64),
            crate::typed_value::TypeTag::Float => Some(f64::from_bits(cell)),
            _ => None,
        };
        if let Some(v) = numeric {
            if v.is_finite() {
                sum += v;
                min_val = Some(min_val.map_or(v, |m: f64| m.min(v)));
                max_val = Some(max_val.map_or(v, |m: f64| m.max(v)));
                numeric_count += 1;
            }
        }
    }
    let avg_val = if numeric_count > 0 {
        sum / numeric_count as f64
    } else {
        0.0
    };

    // Key prefix distribution (top 20 by first segment before '.' or '/')
    let mut prefix_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for (key, _) in db.keymap.all_keys() {
        // Use first two segments for namespace (e.g. "halo:event" from "halo:event:sess-...")
        let pfx = {
            let parts: Vec<&str> = key.splitn(3, ':').collect();
            if parts.len() >= 2 {
                format!("{}:{}", parts[0], parts[1])
            } else {
                key.split_once('.')
                    .or_else(|| key.split_once('/'))
                    .map(|(p, _)| p.to_string())
                    .unwrap_or_else(|| key.to_string())
            }
        };
        *prefix_counts.entry(pfx).or_insert(0) += 1;
    }
    let mut prefix_list: Vec<(String, usize)> = prefix_counts.into_iter().collect();
    prefix_list.sort_by(|a, b| b.1.cmp(&a.1));
    prefix_list.truncate(20);

    // DB file size
    let db_size_bytes = std::fs::metadata(&state.db_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Current STH info
    let sth_info = db.current_sth().map(|sth| {
        json!({
            "tree_size": sth.tree_size,
            "root_hash": crate::transparency::ct6962::hex_encode(&sth.root_hash),
            "timestamp_unix": sth.timestamp_unix_secs,
        })
    });

    // Type distribution
    let mut type_counts: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    for (key, _) in db.keymap.all_keys() {
        let tag = db.type_map.get(key);
        *type_counts.entry(tag.as_str().to_string()).or_default() += 1;
    }
    let blob_count = db.blob_store.len();
    let blob_bytes = db.blob_store.total_bytes();
    let vector_count = db.vector_index.len();
    let vector_dims = db.vector_index.dims();

    Ok(Json(json!({
        "key_count": key_count,
        "commit_count": commit_count,
        "write_mode": write_mode,
        "value_min": min_val,
        "value_max": max_val,
        "value_avg": avg_val,
        "value_sum": sum,
        "db_size_bytes": db_size_bytes,
        "top_prefixes": prefix_list.iter().map(|(p, c)| json!({"prefix": p, "count": c})).collect::<Vec<_>>(),
        "sth": sth_info,
        "type_distribution": type_counts,
        "blob_count": blob_count,
        "blob_total_bytes": blob_bytes,
        "vector_count": vector_count,
        "vector_dims": vector_dims,
    })))
}

async fn api_nucleusdb_edit(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<EditRequest>,
) -> ApiResult {
    let db_path = &state.db_path;
    let _guard = state.db_lock.lock().await;
    let mut db = load_halo_db(db_path)?;

    // Convert JSON value to TypedValue, honoring explicit type when provided.
    let typed = match req.value_type.as_deref() {
        Some(tag) => {
            let tag = crate::typed_value::TypeTag::from_str_tag(tag).ok_or_else(|| {
                api_err(
                    StatusCode::BAD_REQUEST,
                    "invalid type; expected one of: null, integer, float, bool, text, json, bytes, vector",
                )
            })?;
            json_to_typed_value_for_tag(&req.value, tag)
                .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?
        }
        None => json_to_typed_value(&req.value),
    };
    let (idx, cell) = db
        .put_typed(&req.key, typed.clone())
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &format!("typed edit failed: {e}")))?;

    // Build delta and commit
    let delta = crate::state::Delta::new(vec![(idx, cell)]);
    match db.commit(delta, &[]) {
        Ok(entry) => {
            let wal_path = default_wal_path(db_path);
            persist_snapshot_and_sync_wal(db_path, &wal_path, &db)
                .map_err(|e| internal_err(format!("persist after edit: {e:?}")))?;
            Ok(Json(json!({
                "ok": true,
                "key": req.key,
                "value": typed.to_json_value(),
                "type": typed.tag().as_str(),
                "height": entry.height,
            })))
        }
        Err(e) => Ok(Json(json!({
            "error": format!("Commit failed: {e:?}"),
        }))),
    }
}

/// Convert a serde_json::Value to a TypedValue.
fn json_to_typed_value(v: &serde_json::Value) -> crate::typed_value::TypedValue {
    use crate::typed_value::TypedValue;
    match v {
        serde_json::Value::Null => TypedValue::Null,
        serde_json::Value::Bool(b) => TypedValue::Bool(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                TypedValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                TypedValue::Float(f)
            } else {
                TypedValue::Integer(0)
            }
        }
        serde_json::Value::String(s) => {
            // Try to detect JSON inside string
            let trimmed = s.trim();
            if (trimmed.starts_with('{') && trimmed.ends_with('}'))
                || (trimmed.starts_with('[') && trimmed.ends_with(']'))
            {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    return TypedValue::Json(parsed);
                }
            }
            TypedValue::Text(s.clone())
        }
        serde_json::Value::Array(arr) => {
            // Check if all elements are numbers (vector embedding)
            let all_numbers = arr.iter().all(|v| v.is_number());
            if all_numbers && !arr.is_empty() {
                let dims: Vec<f64> = arr.iter().filter_map(|v| v.as_f64()).collect();
                if dims.len() == arr.len() {
                    return TypedValue::Vector(dims);
                }
            }
            TypedValue::Json(serde_json::Value::Array(arr.clone()))
        }
        serde_json::Value::Object(_) => TypedValue::Json(v.clone()),
    }
}

fn json_to_typed_value_for_tag(
    v: &serde_json::Value,
    tag: crate::typed_value::TypeTag,
) -> Result<crate::typed_value::TypedValue, String> {
    use crate::typed_value::{TypeTag, TypedValue};
    match tag {
        TypeTag::Null => {
            if v.is_null() {
                Ok(TypedValue::Null)
            } else {
                Err("type null requires JSON null value".to_string())
            }
        }
        TypeTag::Integer => {
            let i = match v {
                serde_json::Value::Number(n) => n
                    .as_i64()
                    .ok_or_else(|| "type integer requires signed 64-bit integer".to_string())?,
                _ => return Err("type integer requires numeric value".to_string()),
            };
            Ok(TypedValue::Integer(i))
        }
        TypeTag::Float => {
            let f = match v {
                serde_json::Value::Number(n) => n
                    .as_f64()
                    .ok_or_else(|| "type float requires finite numeric value".to_string())?,
                _ => return Err("type float requires numeric value".to_string()),
            };
            Ok(TypedValue::Float(f))
        }
        TypeTag::Bool => match v {
            serde_json::Value::Bool(b) => Ok(TypedValue::Bool(*b)),
            _ => Err("type bool requires boolean value".to_string()),
        },
        TypeTag::Text => match v {
            serde_json::Value::String(s) => Ok(TypedValue::Text(s.clone())),
            _ => Err("type text requires string value".to_string()),
        },
        TypeTag::Json => Ok(TypedValue::Json(v.clone())),
        TypeTag::Bytes => {
            let s = match v {
                serde_json::Value::String(s) => s,
                _ => return Err("type bytes requires hex string value".to_string()),
            };
            let s = s.strip_prefix("0x").unwrap_or(s);
            if s.len() % 2 != 0 {
                return Err("type bytes requires even-length hex string".to_string());
            }
            let mut out = Vec::with_capacity(s.len() / 2);
            let bytes = s.as_bytes();
            let mut i = 0usize;
            while i < bytes.len() {
                let hi = bytes[i] as char;
                let lo = bytes[i + 1] as char;
                let pair = format!("{hi}{lo}");
                let b = u8::from_str_radix(&pair, 16)
                    .map_err(|_| format!("invalid hex byte '{pair}' in bytes value"))?;
                out.push(b);
                i += 2;
            }
            Ok(TypedValue::Bytes(out))
        }
        TypeTag::Vector => {
            let arr = match v {
                serde_json::Value::Array(arr) => arr,
                _ => return Err("type vector requires array of numbers".to_string()),
            };
            if arr.is_empty() {
                return Err("type vector requires at least one dimension".to_string());
            }
            let mut dims = Vec::with_capacity(arr.len());
            for item in arr {
                let f = item
                    .as_f64()
                    .ok_or_else(|| "type vector requires numeric elements".to_string())?;
                dims.push(f);
            }
            Ok(TypedValue::Vector(dims))
        }
    }
}

async fn api_nucleusdb_verify(
    AxumState(state): AxumState<DashboardState>,
    Path(key): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let db = load_halo_db(&state.db_path)?;

    let Some(idx) = db.keymap.get(&key) else {
        return Ok(Json(json!({
            "key": key,
            "found": false,
            "verified": false,
            "error": "Key not found",
        })));
    };

    let Some((value, proof, root)) = db.query(idx) else {
        return Ok(Json(json!({
            "key": key,
            "found": true,
            "verified": false,
            "error": "No value at index",
        })));
    };

    let verified = db.verify_query(idx, value, &proof, root);
    let root_hex = crate::transparency::ct6962::hex_encode(&root);

    // Include typed value info
    let tag = db.type_map.get(&key);
    let blob = db.blob_store.get(&key);
    let typed = crate::typed_value::TypedValue::decode(tag, value, blob)
        .map_err(|e| internal_err(format!("typed decode failed for key '{key}': {e}")))?;

    // For blob types, verify content hash binding
    let blob_verified = if tag.is_blob() {
        if let Some(blob_data) = blob {
            let expected_cell = crate::typed_value::content_hash_u64(&key, blob_data);
            expected_cell == value
        } else {
            false
        }
    } else {
        true // Direct types don't need blob verification
    };

    Ok(Json(json!({
        "key": key,
        "index": idx,
        "value": typed.to_json_value(),
        "display": typed.display_string(),
        "type": tag.as_str(),
        "found": true,
        "verified": verified,
        "blob_verified": blob_verified,
        "root_hash": root_hex,
        "backend": format!("{:?}", db.backend),
    })))
}

async fn api_nucleusdb_key_history(
    AxumState(state): AxumState<DashboardState>,
    Path(key): Path<String>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let db = load_halo_db(&state.db_path)?;

    let Some(idx) = db.keymap.get(&key) else {
        return Ok(Json(json!({
            "key": key,
            "found": false,
            "history": [],
        })));
    };

    // Current value
    let current_value = db.state.values.get(idx).copied().unwrap_or(0);
    let tag = db.type_map.get(&key);
    let blob = db.blob_store.get(&key);
    let typed = crate::typed_value::TypedValue::decode(tag, current_value, blob)
        .map_err(|e| internal_err(format!("typed decode failed for key '{key}': {e}")))?;

    // Commit history for this key (NucleusDB v1 doesn't store per-key deltas,
    // so we show commits + current value — future versions will track per-key changes)
    let commits: Vec<Value> = db
        .entries
        .iter()
        .map(|e| {
            json!({
                "height": e.height,
                "state_root": crate::transparency::ct6962::hex_encode(&e.state_root),
                "timestamp_unix": e.sth.timestamp_unix_secs,
            })
        })
        .collect();

    Ok(Json(json!({
        "key": key,
        "index": idx,
        "found": true,
        "current_value": current_value,
        "current_typed_value": typed.to_json_value(),
        "current_display": typed.display_string(),
        "type": tag.as_str(),
        "commits": commits,
        "note": "Per-key delta history will be available in CommitEntry v2",
    })))
}

async fn api_nucleusdb_export(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<ExportQuery>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let db = load_halo_db(&state.db_path)?;
    let fmt = params.format.as_deref().unwrap_or("json");

    let mut entries: Vec<(String, Value, String)> = Vec::new();
    for (key, idx) in db.keymap.all_keys() {
        let cell = db.state.values.get(idx).copied().unwrap_or(0);
        let tag = db.type_map.get(key);
        let blob = db.blob_store.get(key);
        let typed = crate::typed_value::TypedValue::decode(tag, cell, blob)
            .map_err(|e| internal_err(format!("typed decode failed for key '{key}': {e}")))?;
        entries.push((
            key.to_string(),
            typed.to_json_value(),
            tag.as_str().to_string(),
        ));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    match fmt {
        "csv" => {
            let mut csv = String::from("key,value,type\n");
            for (key, value, type_tag) in &entries {
                let val_str = match value {
                    Value::String(s) => format!("\"{}\"", s.replace('"', "\"\"")),
                    other => other.to_string(),
                };
                csv.push_str(&format!(
                    "{},{},{}\n",
                    key.replace(',', "\\,"),
                    val_str,
                    type_tag
                ));
            }
            Ok(Json(json!({
                "format": "csv",
                "content": csv,
                "count": entries.len(),
            })))
        }
        _ => {
            let map: Vec<Value> = entries
                .iter()
                .map(|(k, v, t)| {
                    json!({
                        "key": k,
                        "value": v,
                        "type": t,
                    })
                })
                .collect();
            Ok(Json(json!({
                "format": "json",
                "content": map,
                "count": entries.len(),
            })))
        }
    }
}

async fn api_nucleusdb_vector_search(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<VectorSearchRequest>,
) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let db = load_halo_db(&state.db_path)?;

    let metric = crate::vector_index::DistanceMetric::from_str_tag(&req.metric)
        .unwrap_or(crate::vector_index::DistanceMetric::Cosine);

    // When a prefix filter is specified, over-fetch so we can return up to k
    // results after filtering.  With small indices this is fine.
    let fetch_k = if req.prefix.is_some() {
        db.vector_index.len()
    } else {
        req.k
    };
    let results = db
        .vector_index
        .search(&req.query, fetch_k, metric)
        .map_err(|e| internal_err(format!("vector search: {e}")))?;

    let items: Vec<Value> = results
        .iter()
        .filter(|r| req.prefix.as_ref().is_none_or(|pfx| r.key.starts_with(pfx)))
        .take(req.k)
        .map(|r| {
            let typed = db.get_typed(&r.key);
            let tag = db.type_map.get(&r.key);
            json!({
                "key": r.key,
                "distance": r.distance,
                "value": typed.as_ref().map(|t| t.to_json_value()),
                "type": tag.as_str(),
            })
        })
        .collect();

    Ok(Json(json!({
        "results": items,
        "query_dims": req.query.len(),
        "k": req.k,
        "metric": req.metric,
        "total_vectors": db.vector_index.len(),
        "vector_count": db.vector_index.len(),
    })))
}

async fn api_nucleusdb_sql(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<SqlRequest>,
) -> ApiResult {
    let db_path = &state.db_path;
    let _guard = state.db_lock.lock().await;
    let mut db = load_halo_db(db_path)?;
    let (result, committed) = {
        let mut executor = SqlExecutor::new(&mut db);
        let out = executor.execute(req.query.trim());
        (out, executor.committed())
    };
    if committed {
        let wal_path = default_wal_path(db_path);
        persist_snapshot_and_sync_wal(db_path, &wal_path, &db)
            .map_err(|e| internal_err(format!("persist after commit: {e:?}")))?;
    }
    match result {
        SqlResult::Rows { columns, rows } => Ok(Json(json!({
            "columns": columns,
            "rows": rows,
        }))),
        SqlResult::Ok { message } => Ok(Json(json!({ "message": message }))),
        SqlResult::Error { message } => Ok(Json(json!({ "error": message }))),
    }
}

/// Load the H.A.L.O. trace store as a NucleusDb instance.
fn load_halo_db(db_path: &std::path::Path) -> Result<NucleusDb, (StatusCode, Json<Value>)> {
    if !db_path.exists() {
        let mut cfg = default_witness_cfg();
        cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
        return Ok(NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            cfg,
        ));
    }
    let mut cfg = default_witness_cfg();
    cfg.signing_algorithm = WitnessSignatureAlgorithm::MlDsa65;
    load_snapshot(db_path, cfg).map_err(|e| internal_err(format!("load NucleusDB: {e:?}")))
}

async fn api_nucleusdb_history(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    // NucleusDB commit history via SHOW HISTORY
    let mut db = load_halo_db(&state.db_path)?;
    let mut executor = SqlExecutor::new(&mut db);
    let result = executor.execute("SHOW HISTORY");
    let commit_history = match result {
        SqlResult::Rows { columns, rows } => json!({ "columns": columns, "rows": rows }),
        _ => json!({ "columns": [], "rows": [] }),
    };

    // Session-level history
    let sessions = list_sessions(&state.db_path).unwrap_or_default();
    let session_items: Vec<Value> = sessions
        .iter()
        .take(50)
        .map(|s| {
            json!({
                "session_id": s.session_id,
                "agent": s.agent,
                "model": s.model,
                "started_at": s.started_at,
                "status": s.status,
            })
        })
        .collect();

    Ok(Json(json!({
        "commits": commit_history,
        "sessions": session_items,
    })))
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

async fn api_capabilities(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let creds_path = &state.credentials_path;
    let has_auth = is_authenticated(creds_path) || resolve_api_key(creds_path).is_some();
    let pmt_cfg = agentpmt::load_or_default();
    let x402_cfg = x402::load_x402_config();
    let addons_cfg = addons::load_or_default();

    Ok(Json(json!({
        "version": "0.3.0",
        "authenticated": has_auth,
        "attestation": true,
        "pq_signing": has_wallet(),
        "contract_audit": true,
        "trust_query": true,
        "x402_payments": x402_cfg.enabled,
        "tool_proxy": pmt_cfg.enabled,
        "addons": {
            "p2pclaw": addons_cfg.p2pclaw_enabled,
            "agentpmt_workflows": addons_cfg.agentpmt_workflows_enabled,
        },
        "mcp_tools": 18,
        "dashboard": true,
    })))
}

// ---------------------------------------------------------------------------
// x402
// ---------------------------------------------------------------------------

async fn api_x402_summary(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let cfg = x402::load_x402_config();
    let paid = {
        let _guard = state.db_lock.lock().await;
        paid_breakdown_by_operation_type(&state.db_path).unwrap_or_default()
    };

    let x402_payments: Vec<&(String, u64, u64, f64)> = paid
        .iter()
        .filter(|(op, _, _, _)| op == "x402_pay")
        .collect();
    let total_x402_count: u64 = x402_payments.iter().map(|(_, c, _, _)| c).sum();
    let total_x402_usd: f64 = x402_payments.iter().map(|(_, _, _, u)| u).sum();

    let balance = if cfg.enabled {
        x402::check_usdc_balance(&cfg).ok()
    } else {
        None
    };

    Ok(Json(json!({
        "enabled": cfg.enabled,
        "network": cfg.preferred_network,
        "max_auto_approve": cfg.max_auto_approve,
        "max_auto_approve_usd": cfg.max_auto_approve as f64 / 1_000_000.0,
        "total_payments": total_x402_count,
        "total_spent_usd": total_x402_usd,
        "wallet": balance.map(|(addr, bal)| json!({
            "address": addr,
            "balance_usdc": bal as f64 / 1_000_000.0,
            "balance_base_units": bal,
        })),
    })))
}

async fn api_x402_balance(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let cfg = x402::load_x402_config();
    if !cfg.enabled {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "x402 payments are disabled",
        ));
    }
    let (address, balance) = x402::check_usdc_balance(&cfg).map_err(internal_err)?;
    Ok(Json(json!({
        "address": address,
        "balance_usdc": balance as f64 / 1_000_000.0,
        "balance_base_units": balance,
        "network": cfg.preferred_network,
    })))
}

// ---------------------------------------------------------------------------
// SSE — Server-Sent Events for live updates
// ---------------------------------------------------------------------------

pub async fn sse_handler(
    AxumState(state): AxumState<DashboardState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let db_lock = state.db_lock.clone();
    let db_path = state.db_path.clone();

    // Get initial count under the lock.
    let initial_count = {
        let _guard = db_lock.lock().await;
        list_sessions(&db_path).map(|s| s.len()).unwrap_or(0)
    };
    let mut last_count = initial_count;

    // Poll at 5s instead of 2s to reduce lock contention with page loads.
    let stream = tokio_stream::wrappers::IntervalStream::new(tokio::time::interval(
        Duration::from_millis(5000),
    ))
    .then(move |_| {
        let db_lock = db_lock.clone();
        let db_path = db_path.clone();
        async move {
            let _guard = db_lock.lock().await;
            list_sessions(&db_path).map(|s| s.len()).unwrap_or(0)
        }
    })
    .map(move |current_count| {
        if current_count != last_count {
            last_count = current_count;
            Ok(Event::default()
                .event("session_update")
                .data(format!("{{\"session_count\":{current_count}}}")))
        } else {
            Ok(Event::default().event("heartbeat").data("{}"))
        }
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}
