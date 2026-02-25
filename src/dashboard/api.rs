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
        .route("/nucleusdb/key-history/{key}", get(api_nucleusdb_key_history))
        .route("/nucleusdb/export", get(api_nucleusdb_export))
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
    value: u64,
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
                .map(|p| {
                    crate::halo::attest::verify_anonymous_membership_proof(p).unwrap_or(false)
                })
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

    // Collect all matching key-value pairs
    let mut items: Vec<(String, u64, usize)> = Vec::new();
    for (key, idx) in db.keymap.all_keys() {
        if let Some(ref pfx) = params.prefix {
            if !pfx.is_empty() && !key.starts_with(pfx.as_str()) {
                continue;
            }
        }
        let value = db.state.values.get(idx).copied().unwrap_or(0);
        items.push((key.to_string(), value, idx));
    }

    let total = items.len();

    // Sort
    match (sort_field, sort_order) {
        ("key", "desc") => items.sort_by(|a, b| b.0.cmp(&a.0)),
        ("value", "asc") => items.sort_by_key(|i| i.1),
        ("value", "desc") => items.sort_by(|a, b| b.1.cmp(&a.1)),
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
        .map(|(key, value, idx)| {
            json!({
                "key": key,
                "value": value,
                "index": idx,
            })
        })
        .collect();

    let total_pages = if total == 0 { 1 } else { (total + page_size - 1) / page_size };

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

    // Value statistics
    let mut min_val: Option<u64> = None;
    let mut max_val: Option<u64> = None;
    let mut sum: u64 = 0;
    for (_, idx) in db.keymap.all_keys() {
        let value = db.state.values.get(idx).copied().unwrap_or(0);
        sum = sum.saturating_add(value);
        min_val = Some(min_val.map_or(value, |m: u64| m.min(value)));
        max_val = Some(max_val.map_or(value, |m: u64| m.max(value)));
    }
    let avg_val = if key_count > 0 {
        sum as f64 / key_count as f64
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
    })))
}

async fn api_nucleusdb_edit(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<EditRequest>,
) -> ApiResult {
    let db_path = &state.db_path;
    let _guard = state.db_lock.lock().await;
    let mut db = load_halo_db(db_path)?;
    {
        // Check key existence before creating executor (borrow checker)
        let key_exists = db.keymap.get(&req.key).is_some();
        let sql = if key_exists {
            format!(
                "UPDATE data SET value = {} WHERE key = '{}'; COMMIT",
                req.value,
                req.key.replace('\'', "''")
            )
        } else {
            format!(
                "INSERT INTO data (key, value) VALUES ('{}', {}); COMMIT",
                req.key.replace('\'', "''"),
                req.value
            )
        };
        let mut executor = SqlExecutor::new(&mut db);
        let result = executor.execute(&sql);
        if let SqlResult::Error { message } = result {
            return Ok(Json(json!({ "error": message })));
        }
        if executor.committed() {
            let wal_path = default_wal_path(db_path);
            persist_snapshot_and_sync_wal(db_path, &wal_path, &db)
                .map_err(|e| internal_err(format!("persist after edit: {e:?}")))?;
        }
    }
    Ok(Json(json!({
        "ok": true,
        "key": req.key,
        "value": req.value,
    })))
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

    Ok(Json(json!({
        "key": key,
        "index": idx,
        "value": value,
        "found": true,
        "verified": verified,
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

    let mut entries: Vec<(String, u64)> = Vec::new();
    for (key, idx) in db.keymap.all_keys() {
        let value = db.state.values.get(idx).copied().unwrap_or(0);
        entries.push((key.to_string(), value));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    match fmt {
        "csv" => {
            let mut csv = String::from("key,value\n");
            for (key, value) in &entries {
                csv.push_str(&format!("{},{}\n", key.replace(',', "\\,"), value));
            }
            Ok(Json(json!({
                "format": "csv",
                "content": csv,
                "count": entries.len(),
            })))
        }
        _ => {
            let map: serde_json::Map<String, Value> = entries
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            Ok(Json(json!({
                "format": "json",
                "content": map,
                "count": entries.len(),
            })))
        }
    }
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
