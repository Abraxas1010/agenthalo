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
use crate::cockpit::deploy::{self, LaunchRequest};
use crate::halo::addons;
use crate::halo::agentpmt;
use crate::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use crate::halo::auth::{is_authenticated, resolve_api_key};
use crate::halo::config;
use crate::halo::onchain::load_onchain_config_or_default;
use crate::halo::pq::has_wallet;
use crate::halo::schema::{
    EventType, SessionMetadata, SessionStatus as HaloSessionStatus, TraceEvent,
};
use crate::halo::trace::{
    cost_buckets, list_sessions, now_unix_secs, paid_breakdown_by_operation_type,
    paid_cost_buckets, record_paid_operation_for_halo, session_events, session_summary,
    TraceWriter,
};
use crate::halo::trust::query_trust_score;
use crate::halo::viewer::export_session_json;
use crate::halo::wrap;
use crate::halo::x402;
use crate::halo::{proxy, vault};
use crate::persistence::{default_wal_path, load_snapshot, persist_snapshot_and_sync_wal};
use crate::pod::acl::{AccessGrant, GrantPermissions, GrantRequest};
use crate::protocol::NucleusDb;
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::witness::WitnessSignatureAlgorithm;
use crate::VcBackend;

use axum::extract::{Path, Query, State as AxumState};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
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
        .route("/agentpmt/tools", get(api_agentpmt_tools))
        .route("/agentpmt/refresh", post(api_agentpmt_refresh))
        .route("/agentpmt/enable", post(api_agentpmt_enable))
        .route("/vault/keys", get(api_vault_keys))
        .route(
            "/vault/keys/{provider}",
            post(api_vault_set_key).delete(api_vault_delete_key),
        )
        .route("/vault/test/{provider}", post(api_vault_test_key))
        // Trust & Attestations
        .route("/trust/{session_id}", get(api_trust))
        .route("/attestations", get(api_attestations))
        .route("/attestations/verify", post(api_attestation_verify))
        // Cockpit
        .route(
            "/cockpit/sessions",
            get(api_cockpit_sessions).post(api_cockpit_create_session),
        )
        .route(
            "/cockpit/sessions/{id}",
            axum::routing::delete(api_cockpit_destroy_session),
        )
        .route(
            "/cockpit/sessions/{id}/resize",
            post(api_cockpit_resize_session),
        )
        .route(
            "/cockpit/sessions/{id}/ws",
            get(crate::cockpit::ws_bridge::ws_handler),
        )
        // Deploy
        .route("/deploy/catalog", get(api_deploy_catalog))
        .route("/deploy/preflight", post(api_deploy_preflight))
        .route("/deploy/launch", post(api_deploy_launch))
        .route("/deploy/status/{id}", get(api_deploy_status))
        // OpenAI-compatible proxy
        .route("/proxy/v1/chat/completions", post(api_proxy_chat))
        .route("/proxy/v1/models", get(api_proxy_models))
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
        .route(
            "/nucleusdb/grants",
            get(api_nucleusdb_grants).post(api_nucleusdb_grants_create),
        )
        .route(
            "/nucleusdb/grants/{grant_id_hex}/revoke",
            post(api_nucleusdb_grants_revoke),
        )
        // Capabilities
        .route("/capabilities", get(api_capabilities))
        // x402
        .route("/x402/summary", get(api_x402_summary))
        .route("/x402/balance", get(api_x402_balance))
        // External metered proxy (customer-facing)
        .route("/v1/chat/completions", post(api_metered_proxy_chat))
        .route("/v1/models", get(api_metered_proxy_models))
        // Customer API key management (admin)
        .route(
            "/admin/keys",
            get(api_admin_list_keys).post(api_admin_create_key),
        )
        .route(
            "/admin/keys/{key_id}",
            get(api_admin_get_key).delete(api_admin_revoke_key),
        )
        .route("/admin/keys/{key_id}/balance", post(api_admin_add_balance))
        .route("/admin/keys/{key_id}/suspend", post(api_admin_suspend_key))
        .route(
            "/admin/keys/{key_id}/activate",
            post(api_admin_activate_key),
        )
        .route(
            "/admin/proxy-config",
            get(api_admin_get_proxy_config).post(api_admin_set_proxy_config),
        )
        // Funding (AgentPMT tokens + x402direct only)
        .route("/admin/keys/{key_id}/fund", post(api_admin_fund_balance))
        // Metered IPFS storage (customer-facing, same auth as /v1/chat/completions)
        .route("/v1/storage/pin", post(api_metered_pin_json))
        .route("/v1/storage/pins", get(api_metered_list_pins))
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

#[derive(Deserialize, Default)]
pub struct GrantListQuery {
    /// Return only active grants.
    active: Option<bool>,
    /// Include revoked grants in response (ignored when active=true).
    include_revoked: Option<bool>,
    /// Optional grantee PUF filter (hex, 32 bytes).
    grantee_puf_hex: Option<String>,
    /// Optional key filter — only grants whose pattern matches this key.
    key: Option<String>,
}

#[derive(Deserialize)]
pub struct GrantCreateRequest {
    grantor_puf_hex: String,
    grantee_puf_hex: String,
    key_pattern: String,
    permissions: GrantPermissions,
    expires_at: Option<u64>,
}

#[derive(Deserialize)]
pub struct VaultSetKeyRequest {
    key: String,
    #[serde(default)]
    env_var: Option<String>,
}

#[derive(Deserialize)]
pub struct CockpitCreateSessionRequest {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    cols: Option<u16>,
    #[serde(default)]
    rows: Option<u16>,
    #[serde(default)]
    agent_type: Option<String>,
}

#[derive(Deserialize)]
pub struct CockpitResizeRequest {
    cols: u16,
    rows: u16,
}

#[derive(Deserialize)]
pub struct DeployPreflightRequest {
    agent_id: String,
}

#[derive(Deserialize)]
pub struct AdminCreateKeyRequest {
    label: String,
    #[serde(default)]
    initial_balance_usd: Option<f64>,
}

#[derive(Deserialize)]
pub struct AdminAddBalanceRequest {
    amount_usd: f64,
}

#[derive(Deserialize)]
pub struct AdminProxyConfigRequest {
    enabled: Option<bool>,
    markup_pct: Option<f64>,
    rate_limit_rpm: Option<u32>,
    daily_token_limit: Option<u64>,
}

/// Fund a customer's balance — only AgentPMT tokens, x402direct, or operator credit.
#[derive(Deserialize)]
pub struct FundBalanceRequest {
    /// Funding source (tagged union).
    source: crate::halo::funding::FundingSource,
    /// Amount in USD (used for operator credits; for AgentPMT/x402, amount
    /// comes from the validated source).
    #[serde(default)]
    amount_usd: Option<f64>,
}

/// Pin JSON to IPFS via the metered Pinata proxy.
#[derive(Deserialize)]
pub struct PinJsonApiRequest {
    content: serde_json::Value,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
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

fn decode_hex_32(input: &str, field_name: &str) -> Result<[u8; 32], (StatusCode, Json<Value>)> {
    let raw = input.trim().strip_prefix("0x").unwrap_or(input.trim());
    if raw.len() != 64 {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            &format!("{field_name} must be exactly 32 bytes (64 hex chars)"),
        ));
    }
    let mut out = [0u8; 32];
    for (idx, chunk) in raw.as_bytes().chunks_exact(2).enumerate() {
        let pair = std::str::from_utf8(chunk).map_err(|_| {
            api_err(
                StatusCode::BAD_REQUEST,
                &format!("{field_name} must be valid hex"),
            )
        })?;
        out[idx] = u8::from_str_radix(pair, 16).map_err(|_| {
            api_err(
                StatusCode::BAD_REQUEST,
                &format!("{field_name} must be valid hex"),
            )
        })?;
    }
    Ok(out)
}

fn encode_hex_prefixed(bytes: &[u8; 32]) -> String {
    format!("0x{}", crate::transparency::ct6962::hex_encode(bytes))
}

fn grant_to_json(g: &AccessGrant) -> Value {
    json!({
        "grant_id_hex": encode_hex_prefixed(&g.grant_id),
        "grantor_puf_hex": encode_hex_prefixed(&g.grantor_puf),
        "grantee_puf_hex": encode_hex_prefixed(&g.grantee_puf),
        "key_pattern": g.key_pattern,
        "permissions": g.permissions,
        "expires_at": g.expires_at,
        "created_at": g.created_at,
        "revoked": g.revoked,
        "active": g.is_active(),
        "nonce": g.nonce,
    })
}

fn persist_grants_to_disk(
    store: &crate::pod::acl::GrantStore,
    path: &std::path::Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create grants dir {}: {e}", parent.display()))?;
    }
    let payload = serde_json::to_vec_pretty(store.list_all())
        .map_err(|e| format!("serialize grants for {}: {e}", path.display()))?;
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, payload)
        .map_err(|e| format!("write temp grants {}: {e}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| {
        format!(
            "rename grants {} -> {}: {e}",
            tmp_path.display(),
            path.display()
        )
    })
}

fn configured_vault(
    state: &DashboardState,
) -> Result<std::sync::Arc<vault::Vault>, (StatusCode, Json<Value>)> {
    state.vault.clone().ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: PQ wallet not initialized",
        )
    })
}

fn require_sensitive_access(state: &DashboardState) -> Result<(), (StatusCode, Json<Value>)> {
    let authenticated = is_authenticated(&state.credentials_path)
        || resolve_api_key(&state.credentials_path).is_some();
    if authenticated {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "authentication required: run `agenthalo login` or configure AGENTHALO_API_KEY, then retry",
                "code": "auth_required",
                "setup_route": "#/setup",
                "next_steps": [
                    "agenthalo login",
                    "export AGENTHALO_API_KEY=your-agenthalo-key"
                ]
            })),
        ))
    }
}

fn agentpmt_refresh_interval_secs(env_var: &str, default_secs: i64) -> i64 {
    std::env::var(env_var)
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default_secs)
}

fn agentpmt_catalog_is_stale(refreshed_at: Option<&str>, max_age_secs: i64) -> bool {
    let now = chrono::Utc::now();
    refreshed_at
        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
        .map(|ts| {
            now.signed_duration_since(ts.with_timezone(&chrono::Utc))
                .num_seconds()
                >= max_age_secs
        })
        .unwrap_or(true)
}

fn resolve_agentpmt_setup_status(
    pmt_cfg: &agentpmt::AgentPmtConfig,
    pmt_auth: bool,
) -> (bool, usize) {
    if !pmt_cfg.enabled || !pmt_auth {
        return (false, 0);
    }

    let max_age_secs = agentpmt_refresh_interval_secs("AGENTHALO_AGENTPMT_SETUP_REFRESH_SECS", 300);
    let mut catalog = agentpmt::load_tool_catalog();
    let stale = agentpmt_catalog_is_stale(catalog.refreshed_at.as_deref(), max_age_secs);
    if catalog.tools.is_empty() || stale {
        match agentpmt::refresh_tool_catalog() {
            Ok(fresh) => catalog = fresh,
            Err(_) => return (false, 0),
        }
    }

    // Prefer marketplace_tool_count (actual vendor products) over meta-tool count
    let count = if catalog.marketplace_tool_count > 0 {
        catalog.marketplace_tool_count
    } else {
        catalog.tools.len()
    };
    (count > 0, count)
}

fn sanitize_proxy_error(provider: &str, err: &ureq::Error) -> String {
    let msg = err.to_string();
    if msg.contains("key=") {
        format!("{} upstream error (credentials redacted)", provider)
    } else {
        format!("{} upstream error: {}", provider, msg)
    }
}

fn validate_cockpit_command(
    command: &str,
    args: &[String],
    agent_type: Option<&str>,
) -> Result<(), String> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("command must not be empty".to_string());
    }
    if args.len() > 32 {
        return Err("too many args (max 32)".to_string());
    }
    if args
        .iter()
        .any(|a| a.len() > 256 || a.contains('\n') || a.contains('\r'))
    {
        return Err("invalid arg (contains newline or exceeds 256 chars)".to_string());
    }

    let mut allowed: std::collections::BTreeSet<String> = deploy::agent_catalog()
        .into_iter()
        .map(|a| a.cli_command.to_string())
        .collect();
    for shell_cmd in ["bash", "/bin/bash", "sh", "/bin/sh"] {
        allowed.insert(shell_cmd.to_string());
    }

    let custom_allowed = std::env::var("AGENTHALO_ALLOW_CUSTOM_COCKPIT")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    if !allowed.contains(cmd) {
        if custom_allowed && agent_type == Some("custom") {
            // Opt-in escape hatch for trusted local experimentation.
            return Ok(());
        }
        return Err(format!(
            "command `{cmd}` is not allowed; use deploy catalog commands"
        ));
    }

    // Shell PTY sessions are interactive; disallow one-shot command execution flags.
    if matches!(cmd, "bash" | "/bin/bash" | "sh" | "/bin/sh")
        && args.iter().any(|a| a == "-c" || a == "--command")
    {
        return Err("shell command execution flags (-c/--command) are not allowed".to_string());
    }

    Ok(())
}

fn extract_bearer_token(headers: &HeaderMap) -> Result<String, (StatusCode, Json<Value>)> {
    let raw = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| api_err(StatusCode::UNAUTHORIZED, "missing Authorization header"))?;
    let token = raw
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            api_err(
                StatusCode::UNAUTHORIZED,
                "expected Authorization: Bearer <api_key>",
            )
        })?;
    Ok(token.to_string())
}

fn provider_test_request(provider: &str, api_key: &str) -> Result<(), String> {
    match provider {
        "anthropic" => {
            let resp = ureq::get("https://api.anthropic.com/v1/models")
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .call()
                .map_err(|e| sanitize_proxy_error("anthropic", &e))?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!(
                    "Anthropic API returned HTTP {}",
                    resp.status().as_u16()
                ))
            }
        }
        "openai" | "openclaw" => {
            let resp = ureq::get("https://api.openai.com/v1/models")
                .header("Authorization", &format!("Bearer {api_key}"))
                .call()
                .map_err(|e| sanitize_proxy_error("openai", &e))?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!(
                    "OpenAI API returned HTTP {}",
                    resp.status().as_u16()
                ))
            }
        }
        "google" => {
            let url =
                format!("https://generativelanguage.googleapis.com/v1beta/models?key={api_key}");
            let resp = ureq::get(&url)
                .call()
                .map_err(|e| sanitize_proxy_error("google", &e))?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!(
                    "Google API returned HTTP {}",
                    resp.status().as_u16()
                ))
            }
        }
        "openrouter" => {
            let resp = ureq::get("https://openrouter.ai/api/v1/models")
                .header("Authorization", &format!("Bearer {api_key}"))
                .header("X-Title", "AgentHALO")
                .call()
                .map_err(|e| sanitize_proxy_error("openrouter", &e))?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!(
                    "OpenRouter API returned HTTP {}",
                    resp.status().as_u16()
                ))
            }
        }
        "pinata" => {
            let resp = ureq::get("https://api.pinata.cloud/data/testAuthentication")
                .header("Authorization", &format!("Bearer {api_key}"))
                .call()
                .map_err(|e| sanitize_proxy_error("pinata", &e))?;
            if resp.status().is_success() {
                Ok(())
            } else {
                Err(format!(
                    "Pinata API returned HTTP {}",
                    resp.status().as_u16()
                ))
            }
        }
        other => Err(format!("provider `{other}` does not support API test")),
    }
}

fn estimate_text_tokens(text: &str) -> u64 {
    (text.len() as u64).div_ceil(4)
}

fn estimate_message_tokens(messages: &[proxy::Message]) -> u64 {
    messages
        .iter()
        .map(|m| match &m.content {
            Value::String(s) => estimate_text_tokens(s),
            Value::Array(items) => items
                .iter()
                .filter_map(|it| it.get("text").and_then(|v| v.as_str()))
                .map(estimate_text_tokens)
                .sum(),
            other => estimate_text_tokens(&other.to_string()),
        })
        .sum()
}

fn extract_completion_text(response: &Value) -> String {
    response
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

async fn record_proxy_trace(
    state: &DashboardState,
    request: &proxy::ChatCompletionRequest,
    response: &Value,
) -> Result<(), String> {
    let started = now_unix_secs();
    let session_id = format!(
        "proxy-{}-{}",
        started,
        &uuid::Uuid::new_v4().as_simple().to_string()[..6]
    );

    let prompt_tokens = response
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| estimate_message_tokens(&request.messages));

    let completion_text = extract_completion_text(response);
    let completion_tokens = response
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| estimate_text_tokens(&completion_text));

    let _guard = state.db_lock.lock().await;
    let mut writer = TraceWriter::new(&state.db_path)?;
    writer.start_session(SessionMetadata {
        session_id: session_id.clone(),
        agent: "proxy".to_string(),
        model: Some(request.model.clone()),
        started_at: started,
        ended_at: None,
        prompt: Some("proxy chat completion".to_string()),
        status: HaloSessionStatus::Running,
        user_id: None,
        machine_id: None,
        puf_digest: None,
    })?;

    writer.write_event(TraceEvent {
        seq: 0,
        timestamp: started,
        event_type: EventType::Assistant,
        content: json!({
            "route": "/api/proxy/v1/chat/completions",
            "model": request.model,
            "message_count": request.messages.len(),
            "preview": completion_text.chars().take(400).collect::<String>(),
        }),
        input_tokens: Some(prompt_tokens),
        output_tokens: Some(completion_tokens),
        cache_read_tokens: None,
        tool_name: None,
        tool_input: None,
        tool_output: None,
        file_path: None,
        content_hash: String::new(),
    })?;

    writer.end_session(HaloSessionStatus::Completed)?;
    Ok(())
}

async fn flush_cockpit_trace_if_done(
    state: &DashboardState,
    session: std::sync::Arc<crate::cockpit::pty_manager::PtySession>,
) {
    if session.is_trace_flushed() {
        return;
    }

    let status = session.status();
    let (final_status, failed_reason): (Option<HaloSessionStatus>, Option<String>) = match status {
        crate::cockpit::session::SessionStatus::Done { exit_code } => {
            if exit_code == 0 {
                (Some(HaloSessionStatus::Completed), None)
            } else {
                (
                    Some(HaloSessionStatus::Failed),
                    Some(format!("exit_code={exit_code}")),
                )
            }
        }
        crate::cockpit::session::SessionStatus::Error { message } => {
            (Some(HaloSessionStatus::Failed), Some(message))
        }
        _ => (None, None),
    };
    let Some(final_status) = final_status else {
        return;
    };

    let info = session.info();
    let telemetry = session.telemetry_snapshot();
    let completion_preview = String::new();
    let started = info.created_at;

    let write_result = async {
        let _guard = state.db_lock.lock().await;
        let mut writer = TraceWriter::new(&state.db_path)?;
        writer.start_session(SessionMetadata {
            session_id: info.id.clone(),
            agent: info
                .agent_type
                .clone()
                .unwrap_or_else(|| "cockpit".to_string()),
            model: None,
            started_at: started,
            ended_at: None,
            prompt: Some("cockpit PTY session".to_string()),
            status: HaloSessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })?;

        writer.write_event(TraceEvent {
            seq: 0,
            timestamp: started,
            event_type: EventType::BashCommand,
            content: json!({
                "command": info.command,
                "args": info.args,
            }),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })?;

        writer.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::SystemMessage,
            content: json!({
                "runtime_secs": telemetry.runtime_secs,
                "input_bytes": telemetry.input_bytes,
                "output_bytes": telemetry.output_bytes,
                "estimated_input_tokens": telemetry.estimated_input_tokens,
                "estimated_output_tokens": telemetry.estimated_output_tokens,
                "completion_preview": completion_preview,
            }),
            input_tokens: Some(telemetry.estimated_input_tokens),
            output_tokens: Some(telemetry.estimated_output_tokens),
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })?;

        if let Some(reason) = failed_reason {
            writer.write_event(TraceEvent {
                seq: 0,
                timestamp: now_unix_secs(),
                event_type: EventType::Error,
                content: json!({ "reason": reason }),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })?;
        }

        writer.end_session(final_status)?;
        Ok::<(), String>(())
    }
    .await;

    if let Err(err) = write_result {
        eprintln!(
            "warning: failed to flush cockpit trace for {}: {}",
            info.id, err
        );
        return;
    }

    let _ = session.mark_trace_flushed();
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

    let pmt_auth = agentpmt::has_bearer_token();
    let (agentpmt_ok, pmt_tool_count) = resolve_agentpmt_setup_status(&pmt_cfg, pmt_auth);
    let has_pq = has_wallet();
    let identity_ok = has_auth || has_pq;
    let llm_ok = state
        .vault
        .as_ref()
        .and_then(|v| v.get_key("openrouter").ok())
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
        || std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_some();

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
            "endpoint": agentpmt::resolved_mcp_endpoint(&pmt_cfg),
            "auth_configured": pmt_auth,
            "tool_count": pmt_tool_count,
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
        "setup_complete": {
            "identity": identity_ok,
            "agentpmt": agentpmt_ok,
            "llm": llm_ok,
            "complete": identity_ok && agentpmt_ok,
        },
        "pq_wallet": has_pq,
        "vault": {
            "available": state.vault.is_some(),
            "path": config::vault_path().to_string_lossy(),
        },
        "paths": {
            "home": config::halo_dir().to_string_lossy(),
            "db": state.db_path.to_string_lossy(),
            "credentials": state.credentials_path.to_string_lossy(),
        },
    })))
}

async fn api_agentpmt_tools(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let pmt_cfg = agentpmt::load_or_default();
    let auth_configured = agentpmt::has_bearer_token();
    let mut source = "cache".to_string();
    let mut refresh_error: Option<String> = None;
    let mut catalog = agentpmt::load_tool_catalog();
    let refresh_interval_secs =
        agentpmt_refresh_interval_secs("AGENTHALO_AGENTPMT_CATALOG_REFRESH_SECS", 900);
    let mut stale =
        agentpmt_catalog_is_stale(catalog.refreshed_at.as_deref(), refresh_interval_secs);
    let should_refresh_live =
        pmt_cfg.enabled && auth_configured && (catalog.tools.is_empty() || stale);

    if should_refresh_live {
        match agentpmt::refresh_tool_catalog() {
            Ok(fresh) => {
                source = "live".to_string();
                catalog = fresh;
                stale = false;
            }
            Err(e) => {
                source = "cache".to_string();
                refresh_error = Some(e);
            }
        }
    }

    let tools: Vec<Value> = catalog
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": format!("agentpmt/{}", t.name),
                "description": t.description,
                "category": t.category,
            })
        })
        .collect();

    Ok(Json(json!({
        "enabled": pmt_cfg.enabled,
        "endpoint": agentpmt::resolved_mcp_endpoint(&pmt_cfg),
        "auth_configured": auth_configured,
        "count": tools.len(),
        "refreshed_at": catalog.refreshed_at,
        "stale": stale,
        "refresh_interval_secs": refresh_interval_secs,
        "refresh_attempted": should_refresh_live,
        "source": source,
        "refresh_error": refresh_error,
        "tools": tools,
    })))
}

async fn api_agentpmt_refresh(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let catalog = agentpmt::refresh_tool_catalog().map_err(|e| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("agentpmt refresh failed: {e}"),
        )
    })?;
    let tools: Vec<Value> = catalog
        .tools
        .iter()
        .map(|t| {
            json!({
                "name": format!("agentpmt/{}", t.name),
                "description": t.description,
                "category": t.category,
            })
        })
        .collect();
    let marketplace_count = catalog.marketplace_tool_count;
    Ok(Json(json!({
        "ok": true,
        "count": if marketplace_count > 0 { marketplace_count } else { tools.len() },
        "mcp_tool_count": tools.len(),
        "marketplace_tool_count": marketplace_count,
        "refreshed_at": catalog.refreshed_at,
        "tools": tools,
    })))
}

async fn api_agentpmt_enable(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let mut cfg = agentpmt::load_or_default();
    cfg.enabled = true;
    cfg.updated_at = chrono::Utc::now().timestamp() as u64;
    let path = agentpmt::agentpmt_config_path();
    agentpmt::save_config(&path, &cfg).map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "enabled": true })))
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

async fn api_vault_keys(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let vault = configured_vault(&state)?;
    let keys = vault.list_keys().map_err(internal_err)?;
    Ok(Json(json!({ "keys": keys })))
}

async fn api_vault_set_key(
    AxumState(state): AxumState<DashboardState>,
    Path(provider): Path<String>,
    Json(req): Json<VaultSetKeyRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let vault = configured_vault(&state)?;
    let env_var = req
        .env_var
        .unwrap_or_else(|| vault::provider_default_env_var(&provider));
    vault
        .set_key(&provider, &env_var, req.key.trim())
        .map_err(internal_err)?;
    Ok(Json(
        json!({ "ok": true, "provider": provider, "env_var": env_var }),
    ))
}

async fn api_vault_delete_key(
    AxumState(state): AxumState<DashboardState>,
    Path(provider): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let vault = configured_vault(&state)?;
    vault.delete_key(&provider).map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "provider": provider })))
}

async fn api_vault_test_key(
    AxumState(state): AxumState<DashboardState>,
    Path(provider): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let vault = configured_vault(&state)?;
    let key = vault.get_key(&provider).map_err(|_| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("no API key configured for {}", provider),
        )
    })?;
    let test_result = provider_test_request(&provider.to_ascii_lowercase(), &key);
    match test_result {
        Ok(()) => {
            let _ = vault.set_test_result(&provider, true);
            Ok(Json(
                json!({ "ok": true, "provider": provider, "tested": true }),
            ))
        }
        Err(e) => {
            let _ = vault.set_test_result(&provider, false);
            Ok(Json(
                json!({ "ok": false, "provider": provider, "tested": false, "error": e }),
            ))
        }
    }
}

async fn api_cockpit_sessions(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    for handle in state.pty_manager.list_session_handles() {
        flush_cockpit_trace_if_done(&state, handle).await;
    }
    let sessions = state.pty_manager.list_sessions();
    Ok(Json(
        json!({ "sessions": sessions, "count": sessions.len() }),
    ))
}

async fn api_cockpit_create_session(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<CockpitCreateSessionRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    validate_cockpit_command(req.command.trim(), &req.args, req.agent_type.as_deref())
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    let cols = req.cols.unwrap_or(120).max(20);
    let rows = req.rows.unwrap_or(36).max(10);
    let id = state
        .pty_manager
        .create_session(
            req.command.trim(),
            &req.args,
            vec![],
            req.working_dir.as_deref(),
            cols,
            rows,
            req.agent_type.clone(),
        )
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({
        "id": id,
        "status": "active",
        "ws_url": format!("/api/cockpit/sessions/{}/ws", id),
    })))
}

async fn api_cockpit_destroy_session(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let session = state.pty_manager.get_session(&id);
    state
        .pty_manager
        .destroy_session(&id)
        .map_err(|e| api_err(StatusCode::NOT_FOUND, &e))?;
    if let Some(session) = session {
        flush_cockpit_trace_if_done(&state, session).await;
    }
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn api_cockpit_resize_session(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
    Json(req): Json<CockpitResizeRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    state
        .pty_manager
        .resize_session(&id, req.cols.max(20), req.rows.max(10))
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(
        json!({ "ok": true, "id": id, "cols": req.cols, "rows": req.rows }),
    ))
}

async fn api_deploy_catalog(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    Ok(Json(json!({ "agents": deploy::agent_catalog() })))
}

async fn api_deploy_preflight(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<DeployPreflightRequest>,
) -> ApiResult {
    let result = deploy::preflight(&req.agent_id, state.vault.as_deref())
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!(result)))
}

async fn api_deploy_launch(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<LaunchRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let result = deploy::launch(&req, &state.pty_manager, state.vault.as_deref())
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!(result)))
}

async fn api_deploy_status(
    AxumState(state): AxumState<DashboardState>,
    Path(id): Path<String>,
) -> ApiResult {
    let session = state
        .pty_manager
        .get_session(&id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "session not found"))?;
    Ok(Json(json!({ "id": id, "status": session.status() })))
}

async fn api_proxy_models(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let models = match state.vault.as_ref() {
        Some(vault) => proxy::list_available_models(vault),
        None => Vec::new(),
    };
    Ok(Json(json!({ "object": "list", "data": models })))
}

async fn api_proxy_chat(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<proxy::ChatCompletionRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if req.stream.unwrap_or(false) {
        return Err(api_err(
            StatusCode::NOT_IMPLEMENTED,
            "streaming not yet supported",
        ));
    }
    let Some(vault) = state.vault.clone() else {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: configure PQ wallet and API keys first",
        ));
    };

    let req_for_proxy = req.clone();
    let response =
        tokio::task::spawn_blocking(move || proxy::proxy_chat_sync(&vault, &req_for_proxy))
            .await
            .map_err(|e| internal_err(format!("proxy task join: {e}")))?
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    if let Err(e) = record_proxy_trace(&state, &req, &response).await {
        eprintln!("warning: proxy trace write failed: {e}");
    }

    Ok(Json(response))
}

async fn api_metered_proxy_models(
    AxumState(state): AxumState<DashboardState>,
    headers: HeaderMap,
) -> ApiResult {
    let cfg = crate::halo::pricing::load_proxy_config();
    if !cfg.enabled {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            "metered proxy is disabled by operator",
        ));
    }

    let customer_key = extract_bearer_token(&headers)?;
    if state.key_store.validate_key(&customer_key).is_none() {
        return Err(api_err(StatusCode::UNAUTHORIZED, "invalid API key"));
    }

    let Some(vault) = state.vault.clone() else {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: configure OpenRouter key first",
        ));
    };

    let models = proxy::list_available_models(&vault);
    Ok(Json(json!({"object":"list","data":models})))
}

async fn api_metered_proxy_chat(
    AxumState(state): AxumState<DashboardState>,
    headers: HeaderMap,
    Json(req): Json<proxy::ChatCompletionRequest>,
) -> ApiResult {
    let cfg = crate::halo::pricing::load_proxy_config();
    if !cfg.enabled {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            "metered proxy is disabled by operator",
        ));
    }
    if req.stream.unwrap_or(false) {
        return Err(api_err(
            StatusCode::NOT_IMPLEMENTED,
            "streaming not yet supported",
        ));
    }

    let customer_key = extract_bearer_token(&headers)?;
    let customer = state
        .key_store
        .validate_key(&customer_key)
        .ok_or_else(|| api_err(StatusCode::UNAUTHORIZED, "invalid API key"))?;
    if !customer.active {
        return Err(api_err(StatusCode::FORBIDDEN, "API key is suspended"));
    }
    if cfg.daily_token_limit > 0
        && state.key_store.today_tokens(&customer.key_id) > cfg.daily_token_limit
    {
        return Err(api_err(
            StatusCode::TOO_MANY_REQUESTS,
            "daily token limit reached for API key",
        ));
    }

    let Some(vault) = state.vault.clone() else {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: configure OpenRouter key first",
        ));
    };
    let key_store = state.key_store.clone();
    let pricing_table = state.pricing_table.clone();
    let markup_pct = cfg.markup_pct;
    let req_for_proxy = req.clone();

    let result = tokio::task::spawn_blocking(move || {
        proxy::metered_proxy_sync(
            &vault,
            &key_store,
            &customer_key,
            &req_for_proxy,
            &pricing_table,
            markup_pct,
        )
    })
    .await
    .map_err(|e| internal_err(format!("metered proxy task join: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    if let Err(e) = record_proxy_trace(&state, &req, &result.body).await {
        eprintln!("warning: metered proxy trace write failed: {e}");
    }

    Ok(Json(result.body))
}

async fn api_admin_list_keys(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let mut keys = state.key_store.list_keys();
    keys.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(Json(json!({"keys": keys, "count": keys.len()})))
}

async fn api_admin_create_key(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AdminCreateKeyRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let label = req.label.trim();
    if label.is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "label must not be empty"));
    }
    let key = state
        .key_store
        .create_key(label, req.initial_balance_usd.unwrap_or(0.0).max(0.0));
    Ok(Json(json!({"ok": true, "key": key})))
}

async fn api_admin_get_key(
    AxumState(state): AxumState<DashboardState>,
    Path(key_id): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let key = state
        .key_store
        .get_key(&key_id)
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "key not found"))?;
    Ok(Json(json!({"key": key})))
}

async fn api_admin_revoke_key(
    AxumState(state): AxumState<DashboardState>,
    Path(key_id): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if !state.key_store.revoke_key(&key_id) {
        return Err(api_err(StatusCode::NOT_FOUND, "key not found"));
    }
    Ok(Json(json!({"ok": true, "key_id": key_id})))
}

async fn api_admin_add_balance(
    AxumState(state): AxumState<DashboardState>,
    Path(key_id): Path<String>,
    Json(req): Json<AdminAddBalanceRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if req.amount_usd <= 0.0 {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "amount_usd must be positive",
        ));
    }
    if state.key_store.get_key(&key_id).is_none() {
        return Err(api_err(StatusCode::NOT_FOUND, "key not found"));
    }
    let balance = state.key_store.add_balance(&key_id, req.amount_usd);
    Ok(Json(
        json!({"ok": true, "key_id": key_id, "balance_usd": balance }),
    ))
}

async fn api_admin_suspend_key(
    AxumState(state): AxumState<DashboardState>,
    Path(key_id): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if !state.key_store.suspend_key(&key_id) {
        return Err(api_err(StatusCode::NOT_FOUND, "key not found"));
    }
    Ok(Json(json!({"ok": true, "key_id": key_id, "active": false})))
}

async fn api_admin_activate_key(
    AxumState(state): AxumState<DashboardState>,
    Path(key_id): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if !state.key_store.activate_key(&key_id) {
        return Err(api_err(StatusCode::NOT_FOUND, "key not found"));
    }
    Ok(Json(json!({"ok": true, "key_id": key_id, "active": true})))
}

async fn api_admin_get_proxy_config(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let cfg = crate::halo::pricing::load_proxy_config();
    Ok(Json(json!({"proxy_config": cfg})))
}

async fn api_admin_set_proxy_config(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AdminProxyConfigRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let mut cfg = crate::halo::pricing::load_proxy_config();
    if let Some(enabled) = req.enabled {
        cfg.enabled = enabled;
    }
    if let Some(markup_pct) = req.markup_pct {
        if markup_pct < 0.0 {
            return Err(api_err(StatusCode::BAD_REQUEST, "markup_pct must be >= 0"));
        }
        cfg.markup_pct = markup_pct;
    }
    if let Some(rpm) = req.rate_limit_rpm {
        cfg.rate_limit_rpm = rpm;
    }
    if let Some(limit) = req.daily_token_limit {
        cfg.daily_token_limit = limit;
    }
    crate::halo::pricing::save_proxy_config(&cfg).map_err(internal_err)?;
    Ok(Json(json!({"ok": true, "proxy_config": cfg})))
}

// ---------------------------------------------------------------------------
// Funding — all balance additions go through AgentPMT or x402direct
// ---------------------------------------------------------------------------

/// Fund a customer's balance via validated funding source.
///
/// This replaces the raw `add_balance` endpoint for customer-facing use.
/// Only AgentPMT tokens, x402direct, and operator credits are accepted.
/// Every funding event is recorded in the append-only funding ledger.
async fn api_admin_fund_balance(
    AxumState(state): AxumState<DashboardState>,
    Path(key_id): Path<String>,
    Json(req): Json<FundBalanceRequest>,
) -> ApiResult {
    use crate::halo::funding;

    // Operator credits require admin auth.  AgentPMT/x402 funding can
    // come via webhook (TODO: add webhook signature verification).
    if matches!(req.source, funding::FundingSource::OperatorCredit { .. }) {
        require_sensitive_access(&state)?;
    }

    // Validate the funding source.
    let validation = funding::validate_funding_source(&req.source, &key_id);
    if !validation.valid {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            &validation
                .error
                .unwrap_or_else(|| "invalid funding source".to_string()),
        ));
    }

    // Determine amount.
    let amount = match &req.source {
        funding::FundingSource::OperatorCredit { .. } => req.amount_usd.unwrap_or(0.0).max(0.0),
        _ => validation.amount_usd,
    };
    if amount <= 0.0 {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "funding amount must be positive",
        ));
    }

    // Check customer exists.
    if state.key_store.get_key(&key_id).is_none() {
        return Err(api_err(StatusCode::NOT_FOUND, "key not found"));
    }

    // Credit the balance.
    let new_balance = state.key_store.add_balance(&key_id, amount);

    // Record in the funding ledger.
    let entry = funding::create_ledger_entry(&key_id, req.source, amount, new_balance);
    if let Err(e) = funding::record_funding(&entry) {
        eprintln!("warning: failed to record funding ledger entry: {e}");
    }

    Ok(Json(json!({
        "ok": true,
        "key_id": key_id,
        "funded_usd": amount,
        "balance_usd": new_balance,
        "source_type": validation.source_type,
        "receipt_id": validation.receipt_id,
    })))
}

// ---------------------------------------------------------------------------
// Metered IPFS Storage (Pinata proxy)
// ---------------------------------------------------------------------------

async fn api_metered_pin_json(
    AxumState(state): AxumState<DashboardState>,
    headers: HeaderMap,
    Json(req): Json<PinJsonApiRequest>,
) -> ApiResult {
    let cfg = crate::halo::pricing::load_proxy_config();
    if !cfg.enabled {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            "metered proxy is disabled by operator",
        ));
    }

    let customer_key = extract_bearer_token(&headers)?;

    let Some(vault) = state.vault.clone() else {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: configure Pinata JWT first",
        ));
    };

    let key_store = state.key_store.clone();
    let markup_pct = cfg.markup_pct;

    let pin_req = crate::halo::pinata::PinJsonRequest {
        content: req.content,
        name: req.name,
        metadata: req.metadata,
    };

    let result = tokio::task::spawn_blocking(move || {
        crate::halo::pinata::metered_pin_json(
            &vault,
            &key_store,
            &customer_key,
            &pin_req,
            markup_pct,
        )
    })
    .await
    .map_err(|e| internal_err(format!("pin task join: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    Ok(Json(json!({
        "ok": true,
        "ipfs_hash": result.ipfs_hash,
        "pin_size": result.pin_size,
        "cost_usd": result.cost_usd,
        "remaining_balance_usd": result.remaining_balance_usd,
        "timestamp": result.timestamp,
    })))
}

async fn api_metered_list_pins(
    AxumState(state): AxumState<DashboardState>,
    headers: HeaderMap,
) -> ApiResult {
    let cfg = crate::halo::pricing::load_proxy_config();
    if !cfg.enabled {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            "metered proxy is disabled by operator",
        ));
    }

    let customer_key = extract_bearer_token(&headers)?;

    let Some(vault) = state.vault.clone() else {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: configure Pinata JWT first",
        ));
    };

    let key_store = state.key_store.clone();

    let result = tokio::task::spawn_blocking(move || {
        crate::halo::pinata::metered_list_pins(&vault, &key_store, &customer_key)
    })
    .await
    .map_err(|e| internal_err(format!("list pins task join: {e}")))?
    .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    Ok(Json(result))
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
    let (grant_count, grant_active_count) = {
        let guard = state
            .grant_store
            .read()
            .map_err(|e| internal_err(format!("grant store read lock poisoned: {e}")))?;
        let all = guard.list_all();
        (all.len(), all.iter().filter(|g| g.is_active()).count())
    };

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
        "grant_count": grant_count,
        "grant_active_count": grant_active_count,
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

async fn api_nucleusdb_grants(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<GrantListQuery>,
) -> ApiResult {
    let only_active = params.active.unwrap_or(false);
    let include_revoked = params.include_revoked.unwrap_or(false) && !only_active;
    let grantee_filter = match params.grantee_puf_hex {
        Some(hex) if !hex.trim().is_empty() => Some(decode_hex_32(&hex, "grantee_puf_hex")?),
        _ => None,
    };
    let key_filter = params.key.filter(|k| !k.trim().is_empty());

    let guard = state
        .grant_store
        .read()
        .map_err(|e| internal_err(format!("grant store read lock poisoned: {e}")))?;
    let mut grants: Vec<&AccessGrant> = guard.list_all().iter().collect();
    if !include_revoked {
        grants.retain(|g| !g.revoked);
    }
    if only_active {
        grants.retain(|g| g.is_active());
    }
    if let Some(grantee) = grantee_filter.as_ref() {
        grants.retain(|g| &g.grantee_puf == grantee);
    }
    if let Some(key) = key_filter.as_ref() {
        grants.retain(|g| g.matches_key(key));
    }
    grants.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    let items: Vec<Value> = grants.into_iter().map(grant_to_json).collect();
    let active_total = guard.list_all().iter().filter(|g| g.is_active()).count();
    let total = items.len();

    Ok(Json(json!({
        "grants": items,
        "total": total,
        "active_total": active_total,
    })))
}

async fn api_nucleusdb_grants_create(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<GrantCreateRequest>,
) -> ApiResult {
    let key_pattern = req.key_pattern.trim();
    if key_pattern.is_empty() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "key_pattern must not be empty",
        ));
    }
    if key_pattern != "*" && key_pattern.contains('*') && !key_pattern.ends_with('*') {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "key_pattern '*' is only supported as trailing glob suffix",
        ));
    }
    if !req.permissions.read && !req.permissions.write && !req.permissions.append {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "permissions must enable at least one of read/write/append",
        ));
    }
    if let Some(expires_at) = req.expires_at {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if expires_at <= now {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "expires_at must be a future Unix timestamp",
            ));
        }
    }

    let request = GrantRequest {
        grantor_puf: decode_hex_32(&req.grantor_puf_hex, "grantor_puf_hex")?,
        grantee_puf: decode_hex_32(&req.grantee_puf_hex, "grantee_puf_hex")?,
        key_pattern: key_pattern.to_string(),
        permissions: req.permissions,
        expires_at: req.expires_at,
    };

    let mut guard = state
        .grant_store
        .write()
        .map_err(|e| internal_err(format!("grant store write lock poisoned: {e}")))?;
    let grant = guard.create(request);
    persist_grants_to_disk(&guard, &state.grant_store_path).map_err(internal_err)?;

    Ok(Json(json!({
        "ok": true,
        "grant": grant_to_json(&grant),
    })))
}

async fn api_nucleusdb_grants_revoke(
    AxumState(state): AxumState<DashboardState>,
    Path(grant_id_hex): Path<String>,
) -> ApiResult {
    let grant_id = decode_hex_32(&grant_id_hex, "grant_id_hex")?;
    let mut guard = state
        .grant_store
        .write()
        .map_err(|e| internal_err(format!("grant store write lock poisoned: {e}")))?;
    let found = guard.revoke(&grant_id);
    if !found {
        return Err(api_err(StatusCode::NOT_FOUND, "grant not found"));
    }
    persist_grants_to_disk(&guard, &state.grant_store_path).map_err(internal_err)?;
    let grant = guard
        .get(&grant_id)
        .ok_or_else(|| internal_err("grant revoked but could not be loaded".to_string()))?;

    Ok(Json(json!({
        "ok": true,
        "grant": grant_to_json(grant),
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
    let proxied_mcp_tools = if pmt_cfg.enabled {
        agentpmt::proxied_tools_for_listing().len()
    } else {
        0
    };
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
        "tool_proxy_endpoint": agentpmt::resolved_mcp_endpoint(&pmt_cfg),
        "tool_proxy_auth_configured": agentpmt::has_bearer_token(),
        "addons": {
            "p2pclaw": addons_cfg.p2pclaw_enabled,
            "agentpmt_workflows": addons_cfg.agentpmt_workflows_enabled,
        },
        "mcp_tools": 18 + proxied_mcp_tools,
        "mcp_native_tools": 18,
        "mcp_proxied_tools": proxied_mcp_tools,
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
