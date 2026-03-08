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
use crate::halo::agent_auth;
use crate::halo::agentpmt;
use crate::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use crate::halo::auth::{
    dashboard_auth_required, is_authenticated, is_dashboard_authenticated, load_credentials,
    save_credentials,
};
use crate::halo::config;
use crate::halo::crypto_scope::CryptoScope;
use crate::halo::encrypted_file;
use crate::halo::http_client;
use crate::halo::metrics::diversity::{build_snapshot, extract_tool_counts};
use crate::halo::onchain::load_onchain_config_or_default;
use crate::halo::p2pclaw;
use crate::halo::pq::has_wallet;
use crate::halo::schema::{
    EventType, SessionMetadata, SessionStatus as HaloSessionStatus, TraceEvent,
};
use crate::halo::trace::{
    cost_buckets, list_sessions, now_unix_secs, paid_breakdown_by_operation_type,
    paid_cost_buckets, record_paid_operation_for_halo, session_events, session_summary,
    TraceWriter,
};
use crate::halo::trace_topology::{map_halo_trace_events, trace_persistence, ToolMetric};
use crate::halo::trust::{query_trust_score, EpistemicTrust};
use crate::halo::viewer::export_session_json;
use crate::halo::wrap;
use crate::halo::x402;
use crate::halo::{proxy, vault};
use crate::immutable::WriteMode;
use crate::persistence::{default_wal_path, load_snapshot, persist_snapshot_and_sync_wal};
use crate::pod::acl::{AccessGrant, GrantPermissions, GrantRequest};
use crate::protocol::NucleusDb;
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::verifier::gate as proof_gate;
use crate::witness::WitnessSignatureAlgorithm;
use crate::VcBackend;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State as AxumState};
use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use bip39::{Language, Mnemonic};
use futures_util::SinkExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use zeroize::{Zeroize, Zeroizing};

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn api_router(state: DashboardState) -> Router<DashboardState> {
    Router::new()
        // Status
        .route("/status", get(api_status))
        // Crypto lock/session
        .route("/crypto/status", get(api_crypto_status))
        .route("/crypto/create-password", post(api_crypto_create_password))
        .route("/crypto/unlock", post(api_crypto_unlock))
        .route("/crypto/lock", post(api_crypto_lock))
        .route("/crypto/change-password", post(api_crypto_change_password))
        // Agent credentials
        .route("/agents/list", get(api_agents_list))
        .route("/agents/authorize", post(api_agents_authorize))
        .route("/agents/revoke", post(api_agents_revoke))
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
        .route("/networking/available", get(api_networking_available))
        .route("/metrics/diversity", get(api_metrics_diversity))
        .route("/metrics/trace-topology", get(api_metrics_trace_topology))
        .route("/orchestrator/agents", get(api_orch_agents))
        .route("/orchestrator/tasks", get(api_orch_tasks))
        .route("/orchestrator/graph", get(api_orch_graph))
        .route("/orchestrator/mesh", get(api_orch_mesh))
        .route("/orchestrator/launch", post(api_orch_launch))
        .route("/orchestrator/task", post(api_orch_task))
        .route("/orchestrator/pipe", post(api_orch_pipe))
        .route("/orchestrator/stop", post(api_orch_stop))
        .route("/orchestrator/agents/{id}/ws", get(ws_orch_agent_output))
        .route("/addons", get(api_addons_get).post(api_addons_post))
        .route("/p2pclaw/configure", post(api_p2pclaw_configure))
        .route("/p2pclaw/status", get(api_p2pclaw_status))
        .route("/p2pclaw/briefing", get(api_p2pclaw_briefing))
        // Config
        .route("/config", get(api_config))
        .route("/config/wrap", post(api_config_wrap))
        .route("/config/x402", post(api_config_x402))
        .route("/auth/oauth/start/{provider}", get(api_auth_oauth_start))
        .route("/auth/oauth/callback", get(api_auth_oauth_callback))
        .route(
            "/openrouter/oauth/callback",
            get(api_openrouter_oauth_callback),
        )
        .route(
            "/openrouter/oauth/exchange",
            post(api_openrouter_oauth_exchange),
        )
        .route("/profile", get(api_get_profile).post(api_save_profile))
        .route(
            "/identity/device",
            get(api_identity_device_scan).post(api_identity_device_save),
        )
        .route(
            "/identity/network",
            get(api_identity_network).post(api_identity_network_save),
        )
        .route("/identity/anonymous", post(api_identity_anonymous))
        .route(
            "/identity/tier",
            get(api_identity_tier_status).post(api_identity_tier_update),
        )
        .route("/genesis/status", get(api_genesis_status))
        .route("/genesis/harvest", post(api_genesis_harvest))
        .route("/genesis/reset", post(api_genesis_reset))
        .route("/identity/status", get(api_identity_status))
        .route("/identity/social", get(api_identity_social_status))
        .route(
            "/identity/ledger/migrate-legacy-signatures",
            post(api_identity_ledger_migrate_legacy_signatures),
        )
        .route(
            "/identity/social/connect",
            post(api_identity_social_connect),
        )
        .route("/identity/social/revoke", post(api_identity_social_revoke))
        .route("/identity/pod-share", get(api_identity_pod_share_schema))
        .route("/identity/pod-share", post(api_identity_pod_share))
        .route(
            "/identity/social/oauth/start/{provider}",
            get(api_identity_social_oauth_start),
        )
        .route(
            "/identity/social/oauth/callback",
            get(api_identity_social_oauth_callback),
        )
        .route(
            "/identity/super-secure",
            get(api_identity_super_secure_status),
        )
        .route(
            "/identity/super-secure",
            post(api_identity_super_secure_update),
        )
        .route("/agentpmt/tools", get(api_agentpmt_tools))
        .route("/agentpmt/refresh", post(api_agentpmt_refresh))
        .route("/agentpmt/enable", post(api_agentpmt_enable))
        .route("/agentpmt/disconnect", post(api_agentpmt_disconnect))
        .route(
            "/agentpmt/anonymous-wallet",
            post(api_agentpmt_anonymous_wallet),
        )
        .route("/agentaddress/status", get(api_agentaddress_status))
        .route("/agentaddress/chains", get(api_agentaddress_chains))
        .route("/agentaddress/generate", post(api_agentaddress_generate))
        .route(
            "/agentaddress/credentials",
            post(api_agentaddress_credentials),
        )
        .route(
            "/agentaddress/disconnect",
            post(api_agentaddress_disconnect),
        )
        // WDK wallet
        .route("/wdk/available", get(api_wdk_available))
        .route("/wdk/status", get(api_wdk_status))
        .route("/wdk/create", post(api_wdk_create))
        .route("/wdk/import", post(api_wdk_import))
        .route("/wdk/unlock", post(api_wdk_unlock))
        .route("/wdk/accounts", get(api_wdk_accounts))
        .route("/wdk/balances", get(api_wdk_balances))
        .route("/wdk/send", post(api_wdk_send))
        .route("/wdk/quote", post(api_wdk_quote))
        .route("/wdk/fees", get(api_wdk_fees))
        .route("/wdk/lock", post(api_wdk_lock))
        .route("/wdk/delete", post(api_wdk_delete))
        .route("/vault/keys", get(api_vault_keys))
        .route(
            "/vault/keys/{provider}",
            post(api_vault_set_key).delete(api_vault_delete_key),
        )
        .route("/vault/test/{provider}", post(api_vault_test_key))
        // Trust & Attestations
        .route("/trust", get(api_attestations))
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
        // CLI agent install & auth
        .route("/cli/detect/{agent}", get(api_cli_detect))
        .route("/cli/install/{agent}", post(api_cli_install))
        .route("/cli/auth/{agent}", post(api_cli_auth))
        // OpenClaw harness
        .route("/openclaw/gateway-status", get(api_openclaw_gateway_status))
        .route("/openclaw/wire-mcp", post(api_openclaw_wire_mcp))
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
        .route("/nucleusdb/commits", get(api_nucleusdb_history))
        .route("/nucleusdb/edit", post(api_nucleusdb_edit))
        .route("/nucleusdb/verify/{key}", get(api_nucleusdb_verify))
        .route(
            "/nucleusdb/key-history/{key}",
            get(api_nucleusdb_key_history),
        )
        .route("/nucleusdb/export", get(api_nucleusdb_export))
        .route("/nucleusdb/vectors", get(api_nucleusdb_vectors))
        .route("/nucleusdb/proofs", get(api_nucleusdb_proofs))
        .route("/nucleusdb/sharing", get(api_nucleusdb_grants))
        .route(
            "/nucleusdb/vector-search",
            post(api_nucleusdb_vector_search),
        )
        .route("/nucleusdb/memory/store", post(api_nucleusdb_memory_store))
        .route(
            "/nucleusdb/memory/recall",
            post(api_nucleusdb_memory_recall),
        )
        .route(
            "/nucleusdb/memory/ingest",
            post(api_nucleusdb_memory_ingest),
        )
        .route("/nucleusdb/memory/stats", get(api_nucleusdb_memory_stats))
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
        // JSON 404 fallback for unmatched /api/* routes.
        .fallback(api_fallback_not_found)
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

#[derive(Deserialize, Default)]
pub struct DiversityQuery {
    window_seconds: Option<u64>,
}

#[derive(Deserialize, Default)]
pub struct TraceTopologyQuery {
    window_seconds: Option<u64>,
    max_chain_degree: Option<usize>,
    max_entries: Option<usize>,
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
struct AddonsUpdateRequest {
    name: String,
    enabled: bool,
}

#[derive(Deserialize, Default)]
struct P2PClawConfigureRequest {
    endpoint_url: Option<String>,
    agent_id: Option<String>,
    agent_name: Option<String>,
    auth_secret: Option<String>,
    tier: Option<String>,
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

#[derive(Deserialize)]
pub struct MemoryStoreRequest {
    text: String,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize)]
pub struct MemoryRecallRequest {
    query: String,
    #[serde(default = "default_memory_k")]
    k: usize,
}

#[derive(Deserialize)]
pub struct MemoryIngestRequest {
    document: String,
    #[serde(default)]
    source: Option<String>,
}

fn default_k() -> usize {
    10
}

fn default_memory_k() -> usize {
    5
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

#[derive(Deserialize)]
pub struct SocialConnectRequest {
    provider: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    selected: Option<bool>,
    #[serde(default)]
    expires_in_days: Option<u64>,
}

#[derive(Deserialize)]
pub struct SocialRevokeRequest {
    provider: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct SocialOauthStartQuery {
    #[serde(default)]
    expires_in_days: Option<u64>,
}

#[derive(Deserialize)]
pub struct SocialOauthCallbackQuery {
    provider: String,
    state: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    expires_in_days: Option<u64>,
}

#[derive(Deserialize)]
pub struct SuperSecureUpdateRequest {
    option: String,
    enabled: bool,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Deserialize, Default)]
pub struct IdentityPodShareRequest {
    #[serde(default)]
    key_patterns: Vec<String>,
    #[serde(default)]
    include_ledger: bool,
    #[serde(default)]
    grantee_puf_hex: Option<String>,
    #[serde(default)]
    require_grants: Option<bool>,
}

#[derive(Deserialize)]
pub struct IdentityTierUpdateRequest {
    tier: String,
    #[serde(default)]
    applied_by: Option<String>,
    #[serde(default)]
    step_failures: Option<usize>,
}

#[derive(Deserialize, Default)]
pub struct GenesisResetRequest {
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
pub struct WdkPassphraseRequest {
    passphrase: String,
}

#[derive(Deserialize)]
pub struct WdkImportRequest {
    seed: String,
    passphrase: String,
}

#[derive(Deserialize)]
pub struct WdkDeleteRequest {
    confirm: String,
}

#[derive(Deserialize)]
pub struct WdkTransferRequest {
    chain: String,
    to: String,
    amount: String,
}

#[derive(Deserialize, Default)]
pub struct AgentAddressGenerateRequest {
    #[serde(default)]
    persist_public_address: Option<bool>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct AuthOauthStartQuery {
    #[serde(default)]
    expires_in_minutes: Option<u64>,
}

#[derive(Deserialize)]
pub struct AuthOauthCallbackQuery {
    provider: String,
    state: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    sub: Option<String>,
}

#[derive(Deserialize)]
pub struct OpenRouterOauthCallbackQuery {
    #[serde(default)]
    code: Option<String>,
}

#[derive(Deserialize)]
pub struct OpenRouterExchangeRequest {
    code: String,
    code_verifier: String,
}

#[derive(Deserialize)]
pub struct CryptoCreatePasswordRequest {
    password: String,
    confirm: String,
}

#[derive(Deserialize, Default)]
pub struct CryptoUnlockRequest {
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    agent_sk: Option<String>,
    #[serde(default)]
    scopes: Option<Vec<String>>,
}

#[derive(Deserialize)]
pub struct CryptoChangePasswordRequest {
    current_password: String,
    new_password: String,
    confirm: String,
}

#[derive(Deserialize)]
pub struct AgentAuthorizeRequest {
    label: String,
    scopes: Vec<String>,
    #[serde(default)]
    expires_days: Option<u64>,
}

#[derive(Deserialize)]
pub struct AgentRevokeRequest {
    agent_id: String,
}

#[derive(Deserialize)]
struct OrchestratorLaunchApiRequest {
    agent: String,
    agent_name: Option<String>,
    working_dir: Option<String>,
    #[serde(default)]
    env: std::collections::BTreeMap<String, String>,
    timeout_secs: Option<u64>,
    model: Option<String>,
    trace: Option<bool>,
    #[serde(default)]
    capabilities: Vec<String>,
}

#[derive(Deserialize)]
struct OrchestratorTaskApiRequest {
    agent_id: String,
    task: String,
    timeout_secs: Option<u64>,
    wait: Option<bool>,
}

#[derive(Deserialize)]
struct OrchestratorPipeApiRequest {
    source_task_id: String,
    target_agent_id: String,
    transform: Option<String>,
    task_prefix: Option<String>,
}

#[derive(Deserialize)]
struct OrchestratorStopApiRequest {
    agent_id: String,
    force: Option<bool>,
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

fn orchestrator_mcp_proxy_enabled() -> bool {
    crate::halo::orchestrator_proxy::orchestrator_proxy_enabled()
}

async fn call_orchestrator_tool_via_mcp(
    tool_name: &str,
    arguments: Value,
) -> Result<Value, (StatusCode, Json<Value>)> {
    crate::halo::orchestrator_proxy::call_orchestrator_tool(tool_name, arguments)
        .await
        .map_err(|e| api_err(StatusCode::BAD_GATEWAY, &e))
}

fn genesis_api_err(
    status: StatusCode,
    error_code: &str,
    message: impl Into<String>,
    technical_detail: Option<String>,
) -> (StatusCode, Json<Value>) {
    let mut body = json!({
        "success": false,
        "error_code": error_code,
        "message": message.into(),
        "failed_sources": [],
    });
    if let Some(detail) = technical_detail.filter(|s| !s.trim().is_empty()) {
        body["technical_detail"] = json!(detail);
    }
    (status, Json(body))
}

fn parse_identity_tier(raw: &str) -> Option<crate::halo::identity::IdentitySecurityTier> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "max-safe" | "max_safe" | "maxsafe" => {
            Some(crate::halo::identity::IdentitySecurityTier::MaxSafe)
        }
        "less-safe" | "less_safe" | "lesssafe" | "balanced" | "a_little_rebellious" => {
            Some(crate::halo::identity::IdentitySecurityTier::LessSafe)
        }
        "low-security" | "low_security" | "low" | "why-bother" => {
            Some(crate::halo::identity::IdentitySecurityTier::LowSecurity)
        }
        _ => None,
    }
}

fn identity_tier_as_str(tier: &crate::halo::identity::IdentitySecurityTier) -> &'static str {
    match tier {
        crate::halo::identity::IdentitySecurityTier::MaxSafe => "max-safe",
        crate::halo::identity::IdentitySecurityTier::LessSafe => "less-safe",
        crate::halo::identity::IdentitySecurityTier::LowSecurity => "low-security",
    }
}

fn default_identity_tier_label() -> &'static str {
    crate::halo::identity::default_security_tier_str()
}

fn genesis_is_completed_status(status: &str) -> bool {
    status.trim().eq_ignore_ascii_case("completed")
}

fn normalize_genesis_reset_reason(reason: Option<&str>) -> String {
    reason
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(160).collect::<String>())
        .unwrap_or_else(|| "operator_reset".to_string())
}

fn genesis_reset_enabled() -> bool {
    matches!(
        std::env::var("AGENTHALO_ENABLE_GENESIS_RESET")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes")
    )
}

fn ensure_genesis_signing_wallet() -> Result<(), String> {
    if crate::halo::pq::has_wallet() {
        return Ok(());
    }
    crate::halo::pq::keygen_pq(false).map(|_| ())
}

async fn record_genesis_trace_event(
    state: &DashboardState,
    outcome: &str,
    harvest: Option<&crate::halo::genesis_entropy::HarvestOutcome>,
    err: Option<&crate::halo::genesis_entropy::GenesisError>,
    reset_reason: Option<&str>,
) -> Result<(), String> {
    let now = now_unix_secs();
    let session_id = format!(
        "genesis-{}-{}",
        now,
        &uuid::Uuid::new_v4().as_simple().to_string()[..6]
    );
    let mut content = json!({
        "outcome": outcome,
        "sources_attempted": 4u64,
        "sources_succeeded": harvest.map(|h| h.sources_count).unwrap_or(0),
        "policy_pass": harvest.map(|h| h.sources_count >= 2).unwrap_or(false),
        "error_code": err.map(|e| e.error_code.clone()),
        "message": err.map(|e| e.message.clone()),
        "duration_ms": harvest.map(|h| h.duration_ms).unwrap_or(0),
        "failed_sources": err.map(|e| e.failed_sources.clone()).unwrap_or_default(),
        "reset_reason": reset_reason,
    });
    if let Some(h) = harvest {
        content["combined_entropy_sha256"] = json!(h.combined_entropy_sha256);
        content["sources"] = json!(h.sources);
        content["curby_pulse_id"] = json!(h.curby_pulse_id);
    }

    let _guard = state.db_lock.lock().await;
    let mut writer = TraceWriter::new(&state.db_path)?;
    writer.start_session(SessionMetadata {
        session_id,
        agent: "genesis".to_string(),
        model: None,
        started_at: now,
        ended_at: None,
        prompt: Some("genesis entropy harvest".to_string()),
        status: HaloSessionStatus::Running,
        user_id: None,
        machine_id: None,
        puf_digest: None,
    })?;
    writer.write_event(TraceEvent {
        seq: 0,
        timestamp: now,
        event_type: EventType::GenesisHarvest,
        content,
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        tool_name: None,
        tool_input: None,
        tool_output: None,
        file_path: None,
        content_hash: String::new(),
    })?;
    let end_status = if err.is_some() {
        HaloSessionStatus::Failed
    } else {
        HaloSessionStatus::Completed
    };
    writer.end_session(end_status)?;
    Ok(())
}

fn rollback_profile(previous: &crate::halo::profile::UserProfile) -> Result<(), String> {
    crate::halo::profile::save(previous).map_err(|e| format!("profile rollback failed: {e}"))
}

fn rollback_identity(previous: &crate::halo::identity::IdentityConfig) -> Result<(), String> {
    crate::halo::identity::save(previous).map_err(|e| format!("identity rollback failed: {e}"))
}

fn agentaddress_api_base() -> String {
    std::env::var("AGENTHALO_AGENTADDRESS_API_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://www.agentpmt.com".to_string())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AgentAddressSource {
    External,
    Genesis,
}

fn parse_agentaddress_source(raw: Option<&str>) -> Result<AgentAddressSource, String> {
    let Some(source) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(AgentAddressSource::External);
    };
    match source.to_ascii_lowercase().as_str() {
        "external" | "agentpmt_external_noauth" | "agentpmt" => Ok(AgentAddressSource::External),
        "genesis" | "genesis_derived" => Ok(AgentAddressSource::Genesis),
        other => Err(format!(
            "unsupported source '{other}' (expected 'external' or 'genesis')"
        )),
    }
}

fn agentaddress_supported_chains() -> Vec<&'static str> {
    vec![
        "Ethereum",
        "Base",
        "Arbitrum",
        "Optimism",
        "Polygon",
        "BNB Chain",
        "Avalanche",
        "zkSync",
        "Linea",
        "Scroll",
        "Blast",
        "Mantle",
        "Fantom",
        "Gnosis",
        "Cronos",
        "Celo",
        "Moonbeam",
        "Harmony",
        "Zora",
        "Metis",
        "Aurora",
        "Taiko",
        "Sei",
        "Sepolia",
        "Holesky",
        "Base Sepolia",
        "Arbitrum Sepolia",
        "OP Sepolia",
    ]
}

fn is_evm_address(raw: &str) -> bool {
    let trimmed = raw.trim();
    let Some(hex) = trimmed.strip_prefix("0x") else {
        return false;
    };
    hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn lock_wdk_manager<'a>(
    state: &'a DashboardState,
) -> Result<std::sync::MutexGuard<'a, crate::halo::wdk_proxy::WdkManager>, (StatusCode, Json<Value>)>
{
    state
        .wdk_manager
        .lock()
        .map_err(|e| internal_err(format!("WDK manager lock poisoned: {e}")))
}

fn lock_crypto_state<'a>(
    state: &'a DashboardState,
) -> Result<std::sync::MutexGuard<'a, super::CryptoState>, (StatusCode, Json<Value>)> {
    state
        .crypto_state
        .lock()
        .map_err(|e| internal_err(format!("crypto state lock poisoned: {e}")))
}

fn wdk_seed_word_count(seed: &str) -> usize {
    seed.split_whitespace().count()
}

fn wdk_is_valid_seed_phrase(seed: &str) -> bool {
    let normalized = seed.trim();
    if !matches!(wdk_seed_word_count(normalized), 12 | 24) {
        return false;
    }
    Mnemonic::parse_in_normalized(Language::English, normalized).is_ok()
}

fn wdk_is_supported_chain(chain: &str) -> bool {
    matches!(
        chain.trim().to_ascii_lowercase().as_str(),
        "bitcoin" | "ethereum" | "polygon" | "arbitrum"
    )
}

fn wdk_is_hex_40(input: &str) -> bool {
    if input.len() != 40 {
        return false;
    }
    input.chars().all(|c| c.is_ascii_hexdigit())
}

fn wdk_is_valid_address(chain: &str, address: &str) -> bool {
    let chain = chain.trim().to_ascii_lowercase();
    let address = address.trim();
    if address.is_empty() {
        return false;
    }
    if chain == "bitcoin" {
        let len = address.len();
        let bech32 = address.starts_with("bc1") || address.starts_with("tb1");
        let legacy = (address.starts_with('1')
            || address.starts_with('3')
            || address.starts_with('m')
            || address.starts_with('n')
            || address.starts_with('2'))
            && address.chars().all(|c| c.is_ascii_alphanumeric());
        return (bech32 || legacy) && (26..=90).contains(&len);
    }
    if matches!(chain.as_str(), "ethereum" | "polygon" | "arbitrum") {
        let Some(rest) = address.strip_prefix("0x") else {
            return false;
        };
        return wdk_is_hex_40(rest);
    }
    false
}

fn wdk_is_valid_amount(chain: &str, amount: &str) -> bool {
    let amount = amount.trim();
    if amount.is_empty() || amount.len() > 80 || !amount.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let parsed = amount.parse::<u128>().ok().unwrap_or(0);
    if parsed == 0 {
        return false;
    }
    if chain.eq_ignore_ascii_case("bitcoin") {
        parsed <= 21_000_000_u128.saturating_mul(100_000_000)
    } else {
        true
    }
}

fn validate_wdk_transfer(req: &WdkTransferRequest) -> Result<(String, String, String), String> {
    let chain = req.chain.trim().to_ascii_lowercase();
    let to = req.to.trim().to_string();
    let amount = req.amount.trim().to_string();
    if !wdk_is_supported_chain(&chain) {
        return Err("unsupported chain; expected bitcoin|ethereum|polygon|arbitrum".to_string());
    }
    if !wdk_is_valid_address(&chain, &to) {
        return Err(format!("invalid recipient address for chain {chain}"));
    }
    if !wdk_is_valid_amount(&chain, &amount) {
        return Err("amount must be a positive integer string within allowed range".to_string());
    }
    Ok((chain, to, amount))
}

fn wdk_unlock_delay_secs(failed_attempts: u32) -> u64 {
    let exponent = failed_attempts.min(8);
    let delay = 1u64.checked_shl(exponent).unwrap_or(u64::MAX);
    delay.clamp(2, 300)
}

fn migration_status_name(status: &crate::halo::migration::MigrationStatus) -> &'static str {
    match status {
        crate::halo::migration::MigrationStatus::Fresh => "fresh",
        crate::halo::migration::MigrationStatus::NeedsPasswordCreation => "needs_password_creation",
        crate::halo::migration::MigrationStatus::V2Locked => "v2_locked",
        crate::halo::migration::MigrationStatus::V2Unlocked => "v2_unlocked",
    }
}

fn require_scope(
    state: &DashboardState,
    scope: CryptoScope,
) -> Result<(), (StatusCode, Json<Value>)> {
    // Backward-compatible mode: if password vault has not been initialized yet,
    // preserve legacy behavior and allow access.
    if !encrypted_file::header_exists() {
        return Ok(());
    }
    let mut crypto = lock_crypto_state(state)?;
    crypto.session.get_scope_key(scope).map_err(|_| {
        (
            StatusCode::LOCKED,
            Json(json!({
                "error": format!("unlock required (scope: {})", scope.as_str()),
                "required_scope": scope.as_str(),
                "code": "crypto_locked",
            })),
        )
    })?;
    Ok(())
}

/// Returns the scope key bytes if v2 crypto is active, or `None` for pre-migration installs.
/// Validates unlock status as a side-effect (returns LOCKED error if session not unlocked).
fn get_scope_key_bytes(
    state: &DashboardState,
    scope: CryptoScope,
) -> Result<Option<[u8; 32]>, (StatusCode, Json<Value>)> {
    if !encrypted_file::header_exists() {
        return Ok(None);
    }
    let mut crypto = lock_crypto_state(state)?;
    let sk = crypto.session.get_scope_key(scope).map_err(|_| {
        (
            StatusCode::LOCKED,
            Json(json!({
                "error": format!("unlock required (scope: {})", scope.as_str()),
                "required_scope": scope.as_str(),
                "code": "crypto_locked",
            })),
        )
    })?;
    Ok(Some(*sk.key_bytes()))
}

fn header_salt_bytes(header: &encrypted_file::CryptoHeader) -> Result<[u8; 32], String> {
    let raw = hex::decode(&header.kdf.salt_hex).map_err(|e| format!("kdf salt decode: {e}"))?;
    if raw.len() != 32 {
        return Err(format!(
            "crypto header salt must be 32 bytes, got {}",
            raw.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn crypto_scope_targets() -> Vec<(std::path::PathBuf, CryptoScope)> {
    vec![
        (crate::halo::config::pq_wallet_v2_path(), CryptoScope::Sign),
        (crate::halo::config::vault_v2_path(), CryptoScope::Vault),
        (crate::halo::config::wdk_seed_v2_path(), CryptoScope::Wallet),
        (
            crate::halo::config::identity_v2_path(),
            CryptoScope::Identity,
        ),
        (
            crate::halo::config::profile_v2_path(),
            CryptoScope::Identity,
        ),
        (
            crate::halo::config::genesis_seed_v2_path(),
            CryptoScope::Genesis,
        ),
    ]
}

fn derive_scope_key_from_master(master: &[u8; 32], scope: CryptoScope) -> Result<[u8; 32], String> {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"agenthalo-scope-v2"), master);
    let mut out = [0u8; 32];
    hk.expand(scope.hkdf_info(), &mut out)
        .map_err(|_| "hkdf expand failed".to_string())?;
    Ok(out)
}

fn verify_master_key_against_crypto_state(
    header: &encrypted_file::CryptoHeader,
    master: &[u8; 32],
) -> Result<bool, String> {
    if encrypted_file::verify_password_with_header(header, master) {
        return Ok(true);
    }
    let targets = crypto_scope_targets();
    if !targets.iter().any(|(path, _)| path.exists()) {
        return Ok(false);
    }
    for (path, scope) in targets {
        if !path.exists() {
            continue;
        }
        let mut scope_key = derive_scope_key_from_master(master, scope)?;
        let file = encrypted_file::EncryptedFileV2::load(&path)?;
        let ok = file.decrypt(&scope_key).is_ok();
        scope_key.zeroize();
        if ok {
            return Ok(true);
        }
    }
    Ok(false)
}

fn social_provider_specs() -> Vec<(&'static str, bool, &'static str)> {
    vec![
        ("google", true, "https://accounts.google.com/"),
        ("github", true, "https://github.com/login"),
        ("microsoft", false, "https://login.live.com/"),
        ("discord", false, "https://discord.com/login"),
        ("apple", false, "https://appleid.apple.com/sign-in"),
        ("facebook", false, "https://www.facebook.com/login/"),
    ]
}

fn auth_oauth_provider_supported(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "github" | "google"
    )
}

fn clamp_auth_oauth_expiry_minutes(minutes: Option<u64>) -> u64 {
    minutes.unwrap_or(10).clamp(1, 30)
}

fn social_provider_supported(provider: &str) -> bool {
    let needle = crate::halo::identity_ledger::normalize_social_provider(provider);
    social_provider_specs()
        .iter()
        .any(|(name, _, _)| *name == needle)
}

fn social_provider_oauth_bridge(provider: &str) -> bool {
    let needle = crate::halo::identity_ledger::normalize_social_provider(provider);
    social_provider_specs()
        .iter()
        .find(|(name, _, _)| *name == needle)
        .map(|(_, bridge, _)| *bridge)
        .unwrap_or(false)
}

fn social_provider_login_url(provider: &str) -> String {
    let needle = crate::halo::identity_ledger::normalize_social_provider(provider);
    social_provider_specs()
        .iter()
        .find(|(name, _, _)| *name == needle)
        .map(|(_, _, url)| (*url).to_string())
        .unwrap_or_else(|| "https://agenthalo.dev".to_string())
}

fn oauth_state_secret(state: &DashboardState) -> String {
    state.oauth_state_secret.as_ref().clone()
}

fn register_oauth_state(
    state: &DashboardState,
    state_token: &str,
    expires_at: u64,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut guard = state
        .oauth_issued_states
        .lock()
        .map_err(|e| internal_err(format!("oauth state lock poisoned: {e}")))?;
    let now = now_unix_secs();
    guard.retain(|_, expiry| *expiry >= now);
    guard.insert(state_token.to_string(), expires_at);
    Ok(())
}

fn consume_oauth_state(
    state: &DashboardState,
    state_token: &str,
    now: u64,
) -> Result<(), (StatusCode, Json<Value>)> {
    let mut guard = state
        .oauth_issued_states
        .lock()
        .map_err(|e| internal_err(format!("oauth state lock poisoned: {e}")))?;
    guard.retain(|_, expiry| *expiry >= now);
    match guard.remove(state_token) {
        Some(expiry) if expiry >= now => Ok(()),
        _ => Err(api_err(
            StatusCode::UNAUTHORIZED,
            "OAuth state was not issued by this dashboard or was already used",
        )),
    }
}

fn clamp_social_expiry_days(days: Option<u64>) -> u64 {
    let raw = days.unwrap_or(30);
    raw.clamp(1, 365)
}

fn html_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn path_display_hint(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| path.display().to_string())
}

fn url_encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{:02X}", b));
        }
    }
    out
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
    // v1 path: static vault initialized at startup from pq_wallet.json
    if let Some(v) = state.vault.as_ref() {
        return Ok(v.clone());
    }
    // v2 path: derive vault from scope key if crypto is unlocked
    if encrypted_file::header_exists() {
        let mut crypto = lock_crypto_state(state)?;
        if let Ok(sk) = crypto.session.get_scope_key(CryptoScope::Vault) {
            let v =
                vault::Vault::from_scope_key(sk.key_bytes(), &crate::halo::config::vault_path());
            return Ok(std::sync::Arc::new(v));
        }
    }
    Err(api_err(
        StatusCode::BAD_REQUEST,
        "vault unavailable — unlock crypto first",
    ))
}

fn require_sensitive_access(state: &DashboardState) -> Result<(), (StatusCode, Json<Value>)> {
    let authenticated = is_dashboard_authenticated(&state.credentials_path);
    if authenticated {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "authentication required: open Setup and sign in with GitHub or Google, then retry",
                "code": "auth_required",
                "setup_route": "#/setup",
                "next_steps": [
                    "Open Setup",
                    "Select Continue with GitHub or Continue with Google"
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
            let resp = http_client::get("https://api.anthropic.com/v1/models")?
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
            let resp = http_client::get("https://api.openai.com/v1/models")?
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
            let resp = http_client::get(&url)?
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
            let resp = http_client::get("https://openrouter.ai/api/v1/models")?
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
            let resp = http_client::get("https://api.pinata.cloud/data/testAuthentication")?
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
            "billing": response.get("x_agenthalo").cloned().unwrap_or(Value::Null),
            "stream": response.get("x_agenthalo_stream").cloned().unwrap_or(Value::Null),
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

fn stream_trace_response(
    request: &proxy::ChatCompletionRequest,
    telemetry: &proxy::StreamTelemetry,
    billing: Option<Value>,
) -> Value {
    let prompt_tokens = telemetry
        .prompt_tokens
        .unwrap_or_else(|| estimate_message_tokens(&request.messages));
    let completion_tokens = telemetry
        .completion_tokens
        .unwrap_or_else(|| estimate_text_tokens(&telemetry.completion_preview));
    let mut response = json!({
        "id": telemetry
            .generation_id
            .clone()
            .unwrap_or_else(|| format!("chatcmpl-stream-{}", now_unix_secs())),
        "object": "chat.completion",
        "created": now_unix_secs(),
        "model": request.model,
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens
        },
        "choices": [{
            "message": {
                "content": telemetry.completion_preview.clone()
            }
        }],
        "x_agenthalo_stream": {
            "completed": telemetry.completed
        }
    });
    if let Some(billing) = billing {
        response["x_agenthalo"] = billing;
    }
    response
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
    let has_auth = is_dashboard_authenticated(creds_path);
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
        "onchain_simulation_mode": crate::halo::onchain::onchain_simulation_enabled(),
        "session_count": session_count,
        "total_cost_usd": total_cost,
        "total_tokens": total_tokens,
        "db_path": path_display_hint(db_path),
        "wrapping": {
            "claude": wrap_status("claude"),
            "codex": wrap_status("codex"),
            "gemini": wrap_status("gemini"),
        },
        "pq_wallet": has_wallet(),
    })))
}

async fn api_fallback_not_found(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "endpoint not found",
            "path": uri.path(),
        })),
    )
}

async fn api_crypto_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let password_protected = encrypted_file::header_exists();
    let mut crypto = lock_crypto_state(&state)?;
    crypto.session.reap_expired();
    let locked = !crypto.session.is_unlocked();
    let mut active_scopes = crypto
        .session
        .active_scopes()
        .into_iter()
        .map(|s| s.as_str().to_string())
        .collect::<Vec<_>>();
    active_scopes.sort();
    let status = if password_protected {
        if locked {
            crate::halo::migration::MigrationStatus::V2Locked
        } else {
            crate::halo::migration::MigrationStatus::V2Unlocked
        }
    } else {
        crate::halo::migration::detect_migration_status()
    };
    crypto.migration_status = status.clone();
    let retry_after_secs = crypto
        .session
        .locked_until_unix()
        .saturating_sub(now_unix_secs());
    Ok(Json(json!({
        "locked": locked,
        "migration_status": migration_status_name(&status),
        "active_scopes": active_scopes,
        "password_protected": password_protected,
        "failed_attempts": crypto.session.failed_attempts(),
        "retry_after_secs": retry_after_secs,
    })))
}

async fn api_crypto_create_password(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<CryptoCreatePasswordRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    crate::halo::password::validate_password_pair(&req.password, &req.confirm)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    crate::halo::config::ensure_halo_dir().map_err(internal_err)?;

    if !crate::halo::pq::has_wallet() {
        crate::halo::pq::keygen_pq(false).map_err(internal_err)?;
    }

    let status = crate::halo::migration::detect_migration_status();
    let mut migrated_files = Vec::new();
    if matches!(
        status,
        crate::halo::migration::MigrationStatus::NeedsPasswordCreation
    ) {
        let report =
            crate::halo::migration::migrate_v1_to_v2(&req.password).map_err(internal_err)?;
        migrated_files = report.files_migrated;
    } else {
        let _ = encrypted_file::create_header_if_missing().map_err(internal_err)?;
    }

    let header = encrypted_file::load_header()
        .map_err(internal_err)?
        .ok_or_else(|| internal_err("crypto header missing after password creation".to_string()))?;
    let mut verify_master = header
        .kdf
        .derive_master_key(&req.password)
        .map_err(internal_err)?;
    let mut updated_header = header.clone();
    updated_header.password_verifier_hex =
        Some(encrypted_file::password_verifier_hex(&verify_master));
    encrypted_file::save_header(&updated_header).map_err(internal_err)?;
    verify_master.zeroize();
    let salt = header_salt_bytes(&header).map_err(internal_err)?;

    let mut crypto = lock_crypto_state(&state)?;
    crypto
        .session
        .unlock_with_password(&req.password, &salt)
        .map_err(|e| api_err(StatusCode::UNAUTHORIZED, &e))?;
    crypto.migration_status = crate::halo::migration::MigrationStatus::V2Unlocked;
    let mut scopes = crypto
        .session
        .active_scopes()
        .into_iter()
        .map(|s| s.as_str().to_string())
        .collect::<Vec<_>>();
    scopes.sort();

    Ok(Json(json!({
        "ok": true,
        "migrated_files": migrated_files,
        "active_scopes": scopes,
        "migration_status": "v2_unlocked",
    })))
}

async fn api_crypto_unlock(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<CryptoUnlockRequest>,
) -> ApiResult {
    if let Some(password) = req.password.as_deref() {
        let header = encrypted_file::load_header()
            .map_err(internal_err)?
            .ok_or_else(|| api_err(StatusCode::PRECONDITION_REQUIRED, "password not configured"))?;

        {
            let crypto = lock_crypto_state(&state)?;
            let now = now_unix_secs();
            if let Err((until, _)) = crypto.session.check_throttle(now) {
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({
                        "error": format!("too many failed unlock attempts; retry in {}s", until.saturating_sub(now)),
                        "retry_after_secs": until.saturating_sub(now),
                    })),
                ));
            }
        }

        let mut candidate_master = header
            .kdf
            .derive_master_key(password)
            .map_err(|_| api_err(StatusCode::UNAUTHORIZED, "invalid password"))?;
        let verified = verify_master_key_against_crypto_state(&header, &candidate_master)
            .map_err(internal_err)?;
        if !verified {
            let mut crypto = lock_crypto_state(&state)?;
            let now = now_unix_secs();
            if let Err((until, _)) = crypto.session.check_throttle(now) {
                candidate_master.zeroize();
                return Err((
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({
                        "error": format!("too many failed unlock attempts; retry in {}s", until.saturating_sub(now)),
                        "retry_after_secs": until.saturating_sub(now),
                    })),
                ));
            }
            candidate_master.zeroize();
            crypto.session.record_failed_attempt(now);
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": "invalid password",
                    "retry_after_secs": crypto.session.locked_until_unix().saturating_sub(now),
                })),
            ));
        }
        let verifier_upgrade = if header.password_verifier_hex.is_none() {
            Some(encrypted_file::password_verifier_hex(&candidate_master))
        } else {
            None
        };
        let mut crypto = lock_crypto_state(&state)?;
        let now = now_unix_secs();
        if let Err((until, _)) = crypto.session.check_throttle(now) {
            candidate_master.zeroize();
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": format!("too many failed unlock attempts; retry in {}s", until.saturating_sub(now)),
                    "retry_after_secs": until.saturating_sub(now),
                })),
            ));
        }
        crypto
            .session
            .unlock_with_master_key(candidate_master)
            .map_err(internal_err)?;
        if let Some(verifier) = verifier_upgrade {
            let mut upgraded = header.clone();
            upgraded.password_verifier_hex = Some(verifier);
            if let Err(err) = encrypted_file::save_header(&upgraded) {
                eprintln!(
                    "warning: failed to persist password verifier upgrade after unlock: {}",
                    err
                );
            }
        }
        crypto.migration_status = crate::halo::migration::MigrationStatus::V2Unlocked;
        let mut scopes = crypto
            .session
            .active_scopes()
            .into_iter()
            .map(|s| s.as_str().to_string())
            .collect::<Vec<_>>();
        scopes.sort();
        return Ok(Json(json!({
            "ok": true,
            "mode": "password",
            "unlocked_scopes": scopes,
        })));
    }

    let agent_id = req.agent_id.as_deref().ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            "agent_id is required for agent unlock",
        )
    })?;
    let agent_sk = req.agent_sk.as_deref().ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            "agent_sk is required for agent unlock",
        )
    })?;
    let scope_names = req
        .scopes
        .as_ref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            api_err(
                StatusCode::BAD_REQUEST,
                "scopes are required for agent unlock",
            )
        })?;
    let mut unlocked_scopes = Vec::new();
    let mut crypto = lock_crypto_state(&state)?;
    for name in scope_names {
        let scope = CryptoScope::parse(name)
            .ok_or_else(|| api_err(StatusCode::BAD_REQUEST, &format!("unknown scope {}", name)))?;
        let scope_key =
            agent_auth::agent_unlock_scope(agent_id, agent_sk, scope).map_err(internal_err)?;
        crypto.session.insert_scope_key(scope_key);
        unlocked_scopes.push(scope.as_str().to_string());
    }
    unlocked_scopes.sort();
    crypto.migration_status = crate::halo::migration::MigrationStatus::V2Unlocked;
    Ok(Json(json!({
        "ok": true,
        "mode": "agent",
        "unlocked_scopes": unlocked_scopes,
    })))
}

async fn api_crypto_lock(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let mut crypto = lock_crypto_state(&state)?;
    crypto.session.lock();
    crypto.migration_status = if encrypted_file::header_exists() {
        crate::halo::migration::MigrationStatus::V2Locked
    } else {
        crate::halo::migration::detect_migration_status()
    };
    Ok(Json(json!({"ok": true, "locked": true})))
}

async fn api_crypto_change_password(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<CryptoChangePasswordRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Identity)?;
    crate::halo::password::validate_password_pair(&req.new_password, &req.confirm)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    let old_header = encrypted_file::load_header()
        .map_err(internal_err)?
        .ok_or_else(|| api_err(StatusCode::PRECONDITION_REQUIRED, "password not configured"))?;
    let mut old_master = old_header
        .kdf
        .derive_master_key(&req.current_password)
        .map_err(|_| api_err(StatusCode::UNAUTHORIZED, "current password is incorrect"))?;
    let old_password_verified =
        verify_master_key_against_crypto_state(&old_header, &old_master).map_err(internal_err)?;
    if !old_password_verified {
        old_master.zeroize();
        return Err(api_err(
            StatusCode::UNAUTHORIZED,
            "current password is incorrect",
        ));
    }
    let new_kdf = encrypted_file::KdfParams::random_v2();
    let mut new_master = new_kdf
        .derive_master_key(&req.new_password)
        .map_err(internal_err)?;

    let targets = crypto_scope_targets();
    for (path, scope) in targets {
        if !path.exists() {
            continue;
        }
        let mut old_scope_key =
            derive_scope_key_from_master(&old_master, scope).map_err(internal_err)?;
        let file = encrypted_file::EncryptedFileV2::load(&path).map_err(internal_err)?;
        let plain = Zeroizing::new(file.decrypt(&old_scope_key).map_err(internal_err)?);
        old_scope_key.zeroize();
        let mut new_scope_key =
            derive_scope_key_from_master(&new_master, scope).map_err(internal_err)?;
        let rebuilt = encrypted_file::EncryptedFileV2::encrypt(
            plain.as_slice(),
            &new_scope_key,
            scope,
            &new_kdf,
        )
        .map_err(internal_err)?;
        new_scope_key.zeroize();
        rebuilt.save(&path).map_err(internal_err)?;
    }

    let new_header = encrypted_file::CryptoHeader {
        schema: encrypted_file::CRYPTO_HEADER_SCHEMA.to_string(),
        kdf: new_kdf.clone(),
        created_at: now_unix_secs(),
        password_protected: true,
        password_verifier_hex: Some(encrypted_file::password_verifier_hex(&new_master)),
    };
    encrypted_file::save_header(&new_header).map_err(internal_err)?;
    old_master.zeroize();
    new_master.zeroize();

    let mut crypto = lock_crypto_state(&state)?;
    let salt = header_salt_bytes(&new_header).map_err(internal_err)?;
    crypto
        .session
        .unlock_with_password(&req.new_password, &salt)
        .map_err(|e| api_err(StatusCode::UNAUTHORIZED, &e))?;
    let reencapsulated = agent_auth::reencapsulate_all_agents(&mut crypto.session).unwrap_or(0);
    crypto.migration_status = crate::halo::migration::MigrationStatus::V2Unlocked;

    Ok(Json(json!({
        "ok": true,
        "agents_reencapsulated": reencapsulated,
    })))
}

async fn api_agents_list(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Identity)?;
    let agents = agent_auth::list_agents().map_err(internal_err)?;
    Ok(Json(json!({ "agents": agents })))
}

async fn api_agents_authorize(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AgentAuthorizeRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Sign)?;
    let scopes: Vec<CryptoScope> = req
        .scopes
        .iter()
        .filter_map(|s| CryptoScope::parse(s))
        .filter(|s| *s != CryptoScope::Admin)
        .collect();
    if scopes.is_empty() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "at least one valid scope is required",
        ));
    }
    let mut crypto = lock_crypto_state(&state)?;
    let (cred, secret) =
        agent_auth::authorize_agent(&mut crypto.session, &req.label, &scopes, req.expires_days)
            .map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "agent_id": cred.agent_id,
        "label": cred.label,
        "scopes": cred.scopes.keys().cloned().collect::<Vec<_>>(),
        "expires_at": cred.expires_at,
        "agent_sk": secret.secret_key_hex,
        "algorithm": secret.algorithm,
        "warning": "This secret key is shown once. Copy and store it securely.",
    })))
}

async fn api_agents_revoke(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AgentRevokeRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Identity)?;
    agent_auth::revoke_agent(&req.agent_id).map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "agent_id": req.agent_id,
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
    require_scope(&state, CryptoScope::Sign)?;
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

fn collect_recent_trace_events(
    db_path: &std::path::Path,
    window_seconds: u64,
) -> Result<Vec<TraceEvent>, String> {
    let cutoff = now_unix_secs().saturating_sub(window_seconds);
    let sessions = list_sessions(db_path)?;
    let mut out = Vec::new();
    for session in sessions {
        let events = session_events(db_path, &session.session_id)?;
        out.extend(events.into_iter().filter(|ev| ev.timestamp >= cutoff));
    }
    Ok(out)
}

async fn api_metrics_diversity(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<DiversityQuery>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let window_seconds = params.window_seconds.unwrap_or(300).clamp(10, 86_400);
    let _guard = state.db_lock.lock().await;
    let events =
        collect_recent_trace_events(&state.db_path, window_seconds).map_err(internal_err)?;
    let counts = extract_tool_counts(&events);
    let snapshot = build_snapshot(&counts);

    Ok(Json(json!({
        "score": snapshot.score,
        "raw_tsallis": snapshot.raw_tsallis,
        "max_tsallis": snapshot.max_tsallis,
        "total_calls": snapshot.total_calls,
        "tool_distribution": snapshot.distribution,
        "tool_counts": snapshot.tools,
        "window_seconds": window_seconds,
    })))
}

async fn api_metrics_trace_topology(
    AxumState(state): AxumState<DashboardState>,
    Query(params): Query<TraceTopologyQuery>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let window_seconds = params.window_seconds.unwrap_or(300).clamp(10, 86_400);
    let max_chain_degree = params.max_chain_degree.unwrap_or(2).clamp(1, 4);
    let max_entries = params.max_entries.unwrap_or(24).clamp(1, 64);

    let _guard = state.db_lock.lock().await;
    let raw_events =
        collect_recent_trace_events(&state.db_path, window_seconds).map_err(internal_err)?;
    let topology_events = map_halo_trace_events(&raw_events);
    let mut tool_names = topology_events
        .iter()
        .map(|event| event.tool_name.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    tool_names.sort();

    if tool_names.is_empty() {
        return Ok(Json(json!({
            "window_seconds": window_seconds,
            "max_chain_degree": max_chain_degree,
            "entry_count": 0,
            "tools": [],
            "entries": [],
        })));
    }

    // First-pass H0 topology with uniform dissimilarity weights.
    // max_chain_degree controls how many k-step chain endpoint edges contribute.
    let metric = ToolMetric::from_uniform(tool_names.clone(), 1);
    let mut entries = trace_persistence(&topology_events, &metric, max_chain_degree);
    entries.truncate(max_entries);

    let payload = entries
        .into_iter()
        .map(|entry| {
            let persistence = if entry.death == u32::MAX {
                serde_json::Value::Null
            } else {
                json!(entry.death.saturating_sub(entry.birth))
            };
            json!({
                "birth": entry.birth,
                "death": if entry.death == u32::MAX { serde_json::Value::Null } else { json!(entry.death) },
                "persistence": persistence,
                "representative": entry.representative,
            })
        })
        .collect::<Vec<_>>();

    Ok(Json(json!({
        "window_seconds": window_seconds,
        "max_chain_degree": max_chain_degree,
        "entry_count": payload.len(),
        "tools": tool_names,
        "entries": payload,
    })))
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

async fn api_get_profile(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let profile = crate::halo::profile::load();
    Ok(Json(serde_json::to_value(&profile).unwrap_or_default()))
}

async fn api_save_profile(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let mut profile = crate::halo::profile::load();
    let previous_profile = profile.clone();
    let rename = body
        .get("rename")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let old_name = profile.display_name.clone();
    if let Some(name_raw) = body.get("display_name").and_then(|v| v.as_str()) {
        let name = name_raw.trim();
        if name.is_empty() {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "display_name must not be empty",
            ));
        }
        let changed = old_name
            .as_ref()
            .map(|prev| prev.trim() != name)
            .unwrap_or(true);
        if changed && profile.name_locked && profile.has_name() && !rename {
            return Err(api_err(
                StatusCode::CONFLICT,
                "profile name is locked; send rename=true to rotate it",
            ));
        }
        if changed && profile.has_name() {
            profile.name_revision = profile.name_revision.saturating_add(1);
        }
        profile.display_name = Some(name.to_string());
        profile.name_locked = true;
    }
    if let Some(avatar_type) = body.get("avatar_type").and_then(|v| v.as_str()) {
        profile.avatar_type = Some(avatar_type.to_string());
    }
    if let Some(avatar_data) = body.get("avatar_data").and_then(|v| v.as_str()) {
        if avatar_data.len() > 512 * 1024 {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "Avatar data exceeds 512KB limit",
            ));
        }
        profile.avatar_data = Some(avatar_data.to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    if profile.created_at.is_none() {
        profile.created_at = Some(now.clone());
    }
    profile.updated_at = Some(now);

    crate::halo::profile::save(&profile).map_err(internal_err)?;
    if let Err(e) = crate::halo::identity_ledger::append_profile_update(
        profile.display_name.as_deref(),
        profile.avatar_type.as_deref(),
        profile.name_locked,
        profile.name_revision,
    ) {
        let rollback_err = rollback_profile(&previous_profile).err();
        let msg = if let Some(rb) = rollback_err {
            format!("identity ledger append failed: {e}; {rb}")
        } else {
            format!("identity ledger append failed; profile rolled back: {e}")
        };
        return Err(internal_err(msg));
    }
    let mut out = serde_json::to_value(&profile).unwrap_or_default();
    if let Some(obj) = out.as_object_mut() {
        obj.insert("ledger_logged".to_string(), json!(true));
        obj.insert("ledger_error".to_string(), Value::Null);
    }
    Ok(Json(out))
}

async fn api_identity_device_scan(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let tier = crate::puf::core::PufTier::detect();
    let components: Vec<Value> = crate::puf::core::collect_auto()
        .map(|result| {
            result
                .components
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "entropy_bits": c.entropy_bits,
                        "stable": c.stable,
                        "value_preview": String::from_utf8_lossy(&c.value[..c.value.len().min(32)]).to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Json(json!({
        "tier": format!("{tier:?}"),
        "components": components,
    })))
}

async fn api_identity_device_save(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let browser_fp = body
        .get("browser_fingerprint")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let selected: Vec<String> = body
        .get("selected_components")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let puf_result = crate::puf::core::collect_auto();
    let puf_fingerprint_hex = puf_result.as_ref().map(|r| {
        format!(
            "sha256:{}",
            crate::transparency::ct6962::hex_encode(&r.fingerprint)
        )
    });
    let puf_tier = puf_result
        .as_ref()
        .map(|r| format!("{:?}", r.tier).to_ascii_lowercase());
    let mut components = puf_result
        .as_ref()
        .map(|result| result.components.clone())
        .unwrap_or_default();
    if !selected.is_empty() {
        components.retain(|c| selected.contains(&c.name));
    }

    let mut entropy_bits = components.iter().map(|c| c.entropy_bits).sum::<u32>();
    if let Some(fp) = browser_fp.clone() {
        components.push(crate::puf::core::PufComponent {
            name: "browser_fingerprint".to_string(),
            value: fp.into_bytes(),
            entropy_bits: 32,
            stable: true,
        });
        entropy_bits = entropy_bits.saturating_add(32);
    }

    if components.is_empty() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "no identity components selected",
        ));
    }

    let digest = crate::puf::core::canonical_fingerprint(&components);
    let hex = format!(
        "sha256:{}",
        crate::transparency::ct6962::hex_encode(&digest)
    );

    let mut cfg = crate::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.device = Some(crate::halo::identity::DeviceIdentity {
        enabled: true,
        browser_fingerprint: browser_fp,
        selected_components: selected,
        composite_fingerprint_hex: Some(hex.clone()),
        puf_fingerprint_hex: puf_fingerprint_hex.clone(),
        puf_tier: puf_tier.clone(),
        entropy_bits,
        last_collected: Some(chrono::Utc::now().to_rfc3339()),
    });
    crate::halo::identity::save(&cfg).map_err(internal_err)?;
    if let Err(e) = crate::halo::identity_ledger::append_device_update(
        true,
        entropy_bits,
        components.len(),
        cfg.device
            .as_ref()
            .and_then(|d| d.browser_fingerprint.as_deref())
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
        puf_fingerprint_hex.as_deref(),
        puf_tier.as_deref(),
    ) {
        let rollback_err = rollback_identity(&previous_cfg).err();
        let msg = if let Some(rb) = rollback_err {
            format!("identity ledger append failed: {e}; {rb}")
        } else {
            format!("identity ledger append failed; identity rolled back: {e}")
        };
        return Err(internal_err(msg));
    }

    Ok(Json(json!({
        "fingerprint_hex": hex,
        "entropy_bits": entropy_bits,
        "puf_fingerprint_hex": puf_fingerprint_hex,
        "puf_tier": puf_tier,
        "ledger_logged": true,
        "ledger_error": Value::Null,
    })))
}

async fn api_identity_network(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let local_ip = (|| -> Option<String> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        socket.local_addr().ok().map(|addr| addr.ip().to_string())
    })();
    let mac_address = mac_address::get_mac_address()
        .ok()
        .flatten()
        .map(|mac| mac.to_string());

    Ok(Json(json!({
        "local_ip": local_ip,
        "mac_address": mac_address,
    })))
}

async fn api_identity_network_save(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let share_local_ip = body
        .get("share_local_ip")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let share_public_ip = body
        .get("share_public_ip")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let share_mac = body
        .get("share_mac")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let local_ip_hash = if share_local_ip {
        body.get("local_ip")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|ip| {
                let mut h = sha2::Sha256::new();
                use sha2::Digest;
                h.update(ip.as_bytes());
                format!(
                    "sha256:{}",
                    crate::transparency::ct6962::hex_encode(&h.finalize())
                )
            })
    } else {
        None
    };

    let public_ip_hash = if share_public_ip {
        body.get("public_ip")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|ip| {
                let mut h = sha2::Sha256::new();
                use sha2::Digest;
                h.update(ip.as_bytes());
                format!(
                    "sha256:{}",
                    crate::transparency::ct6962::hex_encode(&h.finalize())
                )
            })
    } else {
        None
    };

    let mac_addresses: Vec<String> = if share_mac {
        body.get("mac_addresses")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut cfg = crate::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.network = Some(crate::halo::identity::NetworkIdentity {
        share_local_ip,
        share_public_ip,
        share_mac,
        local_ip_hash,
        public_ip_hash,
        mac_addresses,
    });
    crate::halo::identity::save(&cfg).map_err(internal_err)?;
    let network = cfg.network.as_ref();
    if let Err(e) = crate::halo::identity_ledger::append_network_update(
        network.map(|n| n.share_local_ip).unwrap_or(false),
        network.map(|n| n.share_public_ip).unwrap_or(false),
        network.map(|n| n.share_mac).unwrap_or(false),
        network
            .and_then(|n| n.local_ip_hash.as_deref())
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        network
            .and_then(|n| n.public_ip_hash.as_deref())
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        network.map(|n| n.mac_addresses.len()).unwrap_or(0),
    ) {
        let rollback_err = rollback_identity(&previous_cfg).err();
        let msg = if let Some(rb) = rollback_err {
            format!("identity ledger append failed: {e}; {rb}")
        } else {
            format!("identity ledger append failed; identity rolled back: {e}")
        };
        return Err(internal_err(msg));
    }

    Ok(Json(json!({
        "ok": true,
        "ledger_logged": true,
        "ledger_error": Value::Null,
    })))
}

async fn api_identity_anonymous(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<Value>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut cfg = crate::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.anonymous_mode = enabled;
    let mut cleared_device = false;
    let mut cleared_network = false;
    if enabled {
        cleared_device = cfg.device.is_some();
        cleared_network = cfg.network.is_some();
        cfg.device = None;
        cfg.network = None;
    }
    crate::halo::identity::save(&cfg).map_err(internal_err)?;
    if let Err(e) = crate::halo::identity_ledger::append_anonymous_mode_update(
        enabled,
        cleared_device,
        cleared_network,
    ) {
        let rollback_err = rollback_identity(&previous_cfg).err();
        let msg = if let Some(rb) = rollback_err {
            format!("identity ledger append failed: {e}; {rb}")
        } else {
            format!("identity ledger append failed; identity rolled back: {e}")
        };
        return Err(internal_err(msg));
    }
    Ok(Json(json!({
        "anonymous_mode": enabled,
        "ledger_logged": true,
        "ledger_error": Value::Null,
    })))
}

async fn api_identity_tier_status(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let cfg = crate::halo::identity::load();
    let tier = cfg
        .security_tier
        .as_ref()
        .map(identity_tier_as_str)
        .unwrap_or(default_identity_tier_label());
    Ok(Json(json!({
        "tier": tier,
        "configured": cfg.security_tier.is_some(),
        "default_tier": default_identity_tier_label(),
    })))
}

async fn api_identity_tier_update(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<IdentityTierUpdateRequest>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let tier = parse_identity_tier(&req.tier).ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            "tier must be one of: max-safe, less-safe, low-security",
        )
    })?;
    let mut cfg = crate::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.security_tier = Some(tier.clone());
    crate::halo::identity::save(&cfg).map_err(internal_err)?;
    let tier_wire = identity_tier_as_str(&tier);
    if let Err(e) = crate::halo::identity_ledger::append_safety_tier_applied(
        tier_wire,
        req.applied_by
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("dashboard_setup"),
        req.step_failures.unwrap_or(0),
    ) {
        let rollback_err = rollback_identity(&previous_cfg).err();
        let msg = if let Some(rb) = rollback_err {
            format!("identity ledger append failed: {e}; {rb}")
        } else {
            format!("identity ledger append failed; identity rolled back: {e}")
        };
        return Err(internal_err(msg));
    }
    Ok(Json(json!({
        "ok": true,
        "tier": tier_wire,
        "ledger_logged": true,
        "ledger_error": Value::Null,
    })))
}

async fn api_genesis_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let seed_stored = crate::halo::genesis_seed::seed_exists();
    let genesis_key = get_scope_key_bytes(&state, CryptoScope::Genesis)
        .ok()
        .flatten();
    let seed_hash = crate::halo::genesis_seed::load_seed_sha256_prefer_v2(genesis_key.as_ref())
        .ok()
        .flatten();

    // Derive DID URI from genesis seed if available (lightweight: HKDF + Ed25519 only).
    let did_uri: Option<String> = genesis_key
        .as_ref()
        .and_then(|key| {
            crate::halo::genesis_seed::load_seed_bytes_v2(key)
                .ok()
                .flatten()
        })
        .or_else(|| crate::halo::genesis_seed::load_seed_bytes().ok().flatten())
        .map(|seed| crate::halo::did::did_uri_from_genesis_seed(&seed));

    let latest = crate::halo::identity_ledger::latest_genesis_event().map_err(internal_err)?;
    if let Some(entry) = latest {
        let completed = genesis_is_completed_status(&entry.status);
        let curby_pulse_id = entry.payload.get("curby_pulse_id").and_then(|v| v.as_u64());
        let sources_count = entry
            .payload
            .get("policy")
            .and_then(|p| p.get("actual_sources"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let combined_entropy_sha256 = entry
            .payload
            .get("combined_entropy_sha256")
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(Json(json!({
            "completed": completed,
            "status": entry.status,
            "summary": entry.payload,
            "genesis_entropy_sha256": entry.genesis_entropy_sha256,
            "curby_pulse_id": curby_pulse_id,
            "sources_count": sources_count,
            "combined_entropy_sha256": combined_entropy_sha256,
            "seed_stored": seed_stored,
            "seed_hash_sha256": seed_hash,
            "did_uri": did_uri,
            "seq": entry.seq,
            "timestamp": entry.timestamp,
            "entry_hash": entry.entry_hash,
            "signed": entry.signature.is_some(),
            "signature_required_for_genesis": true,
        })));
    }
    Ok(Json(json!({
        "completed": false,
        "status": "missing",
        "seed_stored": seed_stored,
        "seed_hash_sha256": seed_hash,
        "did_uri": did_uri,
        "signature_required_for_genesis": true,
    })))
}

async fn api_genesis_harvest(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_scope(&state, CryptoScope::Genesis)?;
    if let Err(e) = ensure_genesis_signing_wallet() {
        return Err(genesis_api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "WALLET_BOOTSTRAP_FAILED",
            "could not initialize PQ wallet for genesis signing",
            Some(e),
        ));
    }

    let latest_genesis = match crate::halo::identity_ledger::latest_genesis_event() {
        Ok(v) => v,
        Err(e) => {
            return Err(genesis_api_err(
                StatusCode::SERVICE_UNAVAILABLE,
                "LEDGER_READ_FAILURE",
                "could not read genesis ledger state",
                Some(e),
            ));
        }
    };

    if let Some(latest) = latest_genesis {
        if genesis_is_completed_status(&latest.status) {
            let sovereign = match crate::halo::genesis_seed::load_seed_bytes() {
                Ok(Some(seed)) => {
                    match crate::halo::twine_anchor::perform_sovereign_binding_ceremony(
                        &seed,
                        latest
                            .payload
                            .get("combined_entropy_sha256")
                            .and_then(|v| v.as_str())
                            .unwrap_or(""),
                        latest
                            .payload
                            .get("curby_pulse_id")
                            .and_then(|v| v.as_u64()),
                        latest.timestamp,
                    ) {
                        Ok(r) => json!({
                            "attestation_sha256": r.attestation_sha256,
                            "binding_sha256": r.binding_sha256,
                            "did_subject": r.did_subject,
                            "evm_address": r.evm_address,
                        }),
                        Err(e) => {
                            eprintln!(
                                "warning: sovereign binding recovery failed (non-fatal): {e}"
                            );
                            Value::Null
                        }
                    }
                }
                _ => match crate::halo::twine_anchor::recover_sovereign_binding_from_ledger() {
                    Ok(Some(r)) => json!({
                        "attestation_sha256": r.attestation_sha256,
                        "binding_sha256": r.binding_sha256,
                        "did_subject": r.did_subject,
                        "evm_address": r.evm_address,
                    }),
                    _ => Value::Null,
                },
            };
            return Ok(Json(json!({
                "success": true,
                "already_completed": true,
                "completed": true,
                "sources_count": latest
                    .payload
                    .get("policy")
                    .and_then(|p| p.get("actual_sources"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                "curby_pulse_id": latest.payload.get("curby_pulse_id").and_then(|v| v.as_u64()),
                "combined_entropy_sha256": latest.payload.get("combined_entropy_sha256").cloned().unwrap_or(Value::Null),
                "genesis_entropy_sha256": latest.genesis_entropy_sha256,
                "sovereign_binding": sovereign,
            })));
        }
    }

    let harvest = tokio::task::spawn_blocking(crate::halo::genesis_entropy::harvest_entropy)
        .await
        .map_err(|e| {
            genesis_api_err(
                StatusCode::SERVICE_UNAVAILABLE,
                "HARVEST_RUNTIME_FAILURE",
                "genesis harvest runtime worker failed",
                Some(e.to_string()),
            )
        })?;

    match harvest {
        Ok(result) => {
            if let Err(e) =
                record_genesis_trace_event(&state, "success", Some(&result), None, None).await
            {
                eprintln!("warning: failed to write genesis success trace event: {e}");
            }

            // Check for existing seed via both v2 and legacy paths.
            let genesis_key = get_scope_key_bytes(&state, CryptoScope::Genesis)
                .ok()
                .flatten();

            // Try v2 first, then legacy. Propagate read errors for corrupted seeds.
            let existing_sha_result = if let Some(ref gk) = genesis_key {
                crate::halo::genesis_seed::load_seed_sha256_v2(gk).and_then(|v| match v {
                    Some(s) => Ok(Some(s)),
                    None => crate::halo::genesis_seed::load_seed_sha256(),
                })
            } else {
                crate::halo::genesis_seed::load_seed_sha256()
            };

            match existing_sha_result {
                Ok(Some(existing)) if existing == result.combined_entropy_sha256 => {}
                Ok(Some(existing)) => {
                    return Err(genesis_api_err(
                        StatusCode::CONFLICT,
                        "GENESIS_SEED_MISMATCH",
                        "existing sealed genesis seed hash does not match harvested value",
                        Some(format!(
                            "existing={}, new={}",
                            existing, result.combined_entropy_sha256
                        )),
                    ));
                }
                Err(e) if crate::halo::genesis_seed::seed_exists() => {
                    // Seed file exists but cannot be read/decrypted — corruption.
                    return Err(genesis_api_err(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "SEED_READ_FAILURE",
                        "could not read existing sealed genesis seed",
                        Some(e),
                    ));
                }
                Ok(None) | Err(_) => {
                    // No seed exists yet — store it.
                    let store_result = if let Some(ref gk) = genesis_key {
                        crate::halo::genesis_seed::store_seed_once_v2(
                            &result.combined_entropy,
                            &result.combined_entropy_sha256,
                            gk,
                        )
                    } else {
                        crate::halo::genesis_seed::store_seed_once(
                            &result.combined_entropy,
                            &result.combined_entropy_sha256,
                        )
                    };
                    if let Err(e) = store_result {
                        return Err(genesis_api_err(
                            StatusCode::SERVICE_UNAVAILABLE,
                            "SEED_STORAGE_FAILURE",
                            "could not seal genesis seed",
                            Some(e),
                        ));
                    }
                }
            }

            let payload = json!({
                "combined_entropy_sha256": result.combined_entropy_sha256,
                "sources": result.sources,
                "failed_sources": result.failed_sources,
                "policy": {
                    "min_sources": 2,
                    "actual_sources": result.sources_count,
                },
                "curby_pulse_id": result.curby_pulse_id,
                "drand_normalization": "sha512",
                "duration_ms": result.duration_ms,
            });
            let sign_key = get_scope_key_bytes(&state, CryptoScope::Sign)
                .ok()
                .flatten();
            let entry = crate::halo::identity_ledger::append_genesis_event_with_sign_key(
                "completed",
                payload,
                sign_key.as_ref(),
            )
            .map_err(|e| {
                genesis_api_err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "LEDGER_APPEND_FAILURE",
                    "could not append genesis completion entry to identity ledger",
                    Some(e),
                )
            })?;

            // Perform sovereign binding ceremony: attestation + DID↔EVM binding
            let binding_result = crate::halo::twine_anchor::perform_sovereign_binding_ceremony(
                &result.combined_entropy,
                &result.combined_entropy_sha256,
                result.curby_pulse_id,
                entry.timestamp,
            );
            let sovereign = match &binding_result {
                Ok(r) => json!({
                    "attestation_sha256": r.attestation_sha256,
                    "binding_sha256": r.binding_sha256,
                    "did_subject": r.did_subject,
                    "evm_address": r.evm_address,
                }),
                Err(e) => {
                    eprintln!("warning: sovereign binding ceremony failed (non-fatal): {e}");
                    Value::Null
                }
            };

            Ok(Json(json!({
                "success": true,
                "completed": true,
                "sources_count": result.sources_count,
                "curby_pulse_id": result.curby_pulse_id,
                "combined_entropy_sha256": result.combined_entropy_sha256,
                "sources": result.sources,
                "failed_sources": result.failed_sources,
                "duration_ms": result.duration_ms,
                "ledger_seq": entry.seq,
                "ledger_entry_hash": entry.entry_hash,
                "ledger_signed": entry.signature.is_some(),
                "genesis_entropy_sha256": entry.genesis_entropy_sha256,
                "sovereign_binding": sovereign,
            })))
        }
        Err(err) => {
            if let Err(e) =
                record_genesis_trace_event(&state, "failure", None, Some(&err), None).await
            {
                eprintln!("warning: failed to write genesis failure trace event: {e}");
            }
            let status = match err.error_code.as_str() {
                crate::halo::genesis_entropy::ERR_CURBY_UNREACHABLE
                | crate::halo::genesis_entropy::ERR_NIST_UNREACHABLE
                | crate::halo::genesis_entropy::ERR_DRAND_UNREACHABLE
                | crate::halo::genesis_entropy::ERR_ALL_REMOTE_FAILED => StatusCode::BAD_GATEWAY,
                crate::halo::genesis_entropy::ERR_INSUFFICIENT_ENTROPY
                | crate::halo::genesis_entropy::ERR_ENTROPY_QUALITY_FAILURE => {
                    StatusCode::SERVICE_UNAVAILABLE
                }
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            Err((
                status,
                Json(json!({
                    "success": false,
                    "error_code": err.error_code,
                    "message": err.message,
                    "failed_sources": err.failed_sources,
                })),
            ))
        }
    }
}

async fn api_genesis_reset(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<GenesisResetRequest>,
) -> ApiResult {
    if !genesis_reset_enabled() {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            "genesis reset is disabled by policy (set AGENTHALO_ENABLE_GENESIS_RESET=1 to enable)",
        ));
    }
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Genesis)?;
    if matches!(
        crate::halo::identity_ledger::latest_completed_genesis_hash(),
        Ok(Some(_))
    ) {
        return Err(api_err(
            StatusCode::CONFLICT,
            "genesis reset is blocked after a completed genesis commit",
        ));
    }
    let reason = normalize_genesis_reset_reason(req.reason.as_deref());
    let payload = json!({
        "reason": reason,
        "reset_at": now_unix_secs(),
    });
    let entry = crate::halo::identity_ledger::append_genesis_event("reset", payload)
        .map_err(internal_err)?;
    record_genesis_trace_event(&state, "reset", None, None, Some(&reason))
        .await
        .map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "completed": false,
        "status": "reset",
        "ledger_seq": entry.seq,
        "ledger_entry_hash": entry.entry_hash,
    })))
}

fn persist_social_token_secret(
    state: &DashboardState,
    provider: &str,
    token: &str,
) -> Result<String, (StatusCode, Json<Value>)> {
    let provider = crate::halo::identity_ledger::normalize_social_provider(provider);
    let vault_provider = format!("social_{provider}");
    let env_var = vault::provider_default_env_var(&vault_provider);
    if let Some(vault) = state.vault.as_ref() {
        vault
            .set_key(&vault_provider, &env_var, token)
            .map_err(internal_err)?;
        return Ok("vault".to_string());
    }

    // Fallback for hosts without a PQ wallet/vault: credentials.json (0600).
    let mut creds = load_credentials(&state.credentials_path).unwrap_or_default();
    creds.oauth_token = Some(token.to_string());
    creds.oauth_provider = Some(provider.clone());
    creds.created_at = now_unix_secs();
    save_credentials(&state.credentials_path, &creds).map_err(internal_err)?;
    Ok("credentials".to_string())
}

fn persist_social_connection(
    state: &DashboardState,
    provider: &str,
    token: &str,
    source: &str,
    expires_in_days: Option<u64>,
    selected: bool,
) -> Result<Value, (StatusCode, Json<Value>)> {
    let provider = crate::halo::identity_ledger::normalize_social_provider(provider);
    if !social_provider_supported(&provider) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "unsupported social provider",
        ));
    }
    if token.trim().is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "token must not be empty"));
    }

    let storage = persist_social_token_secret(state, &provider, token)?;
    let now = now_unix_secs();
    let expires_days = clamp_social_expiry_days(expires_in_days);
    let expires_at = Some(now.saturating_add(expires_days.saturating_mul(86_400)));

    crate::halo::identity_ledger::append_social_connect(
        crate::halo::identity_ledger::SocialConnectInput {
            provider: &provider,
            token,
            expires_at,
            source,
        },
    )
    .map_err(internal_err)?;

    let mut cfg = crate::halo::identity::load();
    cfg.version = Some(1);
    let p = cfg.social.providers.entry(provider.clone()).or_default();
    p.selected = selected;
    p.expires_at = expires_at;
    p.source = Some(source.to_string());
    p.last_connected_at = Some(chrono::Utc::now().to_rfc3339());
    cfg.social.last_updated = Some(chrono::Utc::now().to_rfc3339());
    crate::halo::identity::save(&cfg).map_err(internal_err)?;

    let projection =
        crate::halo::identity_ledger::project_ledger_status(now).map_err(internal_err)?;
    Ok(json!({
        "ok": true,
        "provider": provider,
        "expires_at": expires_at,
        "storage": storage,
        "projection": projection,
    }))
}

async fn api_identity_status(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let profile = crate::halo::profile::load();
    let identity = crate::halo::identity::load();
    let profile_set = profile.has_name();
    let anonymous_mode = identity.anonymous_mode;
    let device_configured = identity.device.as_ref().map(|d| d.enabled).unwrap_or(false);
    let network_configured = identity
        .network
        .as_ref()
        .map(crate::halo::identity::network_is_configured)
        .unwrap_or(false);
    let social_projection = crate::halo::identity_ledger::project_ledger_status(now_unix_secs())
        .unwrap_or(crate::halo::identity_ledger::LedgerProjection {
            providers: Vec::new(),
            total_entries: 0,
            head_hash: None,
            chain_valid: false,
            signed_entries: 0,
            unsigned_entries: 0,
            fully_signed: false,
        });
    let social_active = social_projection
        .providers
        .iter()
        .filter(|p| p.active)
        .count();
    let identity_done = profile_set || anonymous_mode;
    let tier = identity
        .security_tier
        .as_ref()
        .map(identity_tier_as_str)
        .unwrap_or(default_identity_tier_label());

    Ok(Json(json!({
        "profile_set": profile_set,
        "anonymous_mode": anonymous_mode,
        "security_tier": tier,
        "default_security_tier": default_identity_tier_label(),
        "name_locked": profile.name_locked,
        "name_revision": profile.name_revision,
        "device_configured": device_configured,
        "network_configured": network_configured,
        "social_active_count": social_active,
        "social_chain_valid": social_projection.chain_valid,
        "identity_ledger_total_entries": social_projection.total_entries,
        "identity_ledger_signed_entries": social_projection.signed_entries,
        "identity_ledger_unsigned_entries": social_projection.unsigned_entries,
        "identity_ledger_fully_signed": social_projection.fully_signed,
        "identity_done": identity_done,
        "display_name": profile.display_name,
        "avatar_type": profile.avatar_type,
    })))
}

async fn api_identity_ledger_migrate_legacy_signatures(
    AxumState(state): AxumState<DashboardState>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Sign)?;
    let updated =
        crate::halo::identity_ledger::migrate_legacy_signatures().map_err(internal_err)?;
    let ledger = crate::halo::identity_ledger::project_ledger_status(now_unix_secs())
        .map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "updated_entries": updated,
        "ledger": ledger,
    })))
}

async fn api_identity_social_status(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let now = now_unix_secs();
    let projection =
        crate::halo::identity_ledger::project_ledger_status(now).map_err(internal_err)?;
    let cfg = crate::halo::identity::load();
    let providers: Vec<Value> = social_provider_specs()
        .into_iter()
        .map(|(name, bridge, login_url)| {
            let state = cfg.social.providers.get(name).cloned().unwrap_or_default();
            let projected = projection
                .providers
                .iter()
                .find(|p| p.provider == name)
                .cloned();
            json!({
                "provider": name,
                "oauth_bridge_supported": bridge,
                "login_url": login_url,
                "selected": state.selected,
                "configured_expires_at": state.expires_at,
                "active": projected.as_ref().map(|p| p.active).unwrap_or(false),
                "expired": projected.as_ref().map(|p| p.expired).unwrap_or(false),
                "most_recent_seq": projected.as_ref().and_then(|p| p.most_recent_seq),
                "most_recent_at": projected.as_ref().and_then(|p| p.most_recent_at),
                "expires_at": projected.as_ref().and_then(|p| p.expires_at),
                "active_token_ref_sha256": projected.as_ref().and_then(|p| p.active_token_ref_sha256.clone()),
                "last_status": projected.as_ref().and_then(|p| p.last_status.clone()),
            })
        })
        .collect();

    Ok(Json(json!({
        "providers": providers,
        "ledger": {
            "total_entries": projection.total_entries,
            "head_hash": projection.head_hash,
            "chain_valid": projection.chain_valid,
            "signed_entries": projection.signed_entries,
            "unsigned_entries": projection.unsigned_entries,
            "fully_signed": projection.fully_signed,
        }
    })))
}

async fn api_identity_social_connect(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<SocialConnectRequest>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let provider = crate::halo::identity_ledger::normalize_social_provider(&req.provider);
    let token = req.token.as_deref().ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            "token is required for social connect",
        )
    })?;
    let source = req.source.as_deref().unwrap_or("manual");
    let selected = req.selected.unwrap_or(true);
    let out = persist_social_connection(
        &state,
        &provider,
        token,
        source,
        req.expires_in_days,
        selected,
    )?;
    Ok(Json(out))
}

async fn api_identity_social_revoke(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<SocialRevokeRequest>,
) -> ApiResult {
    require_scope(&state, CryptoScope::Identity)?;
    let provider = crate::halo::identity_ledger::normalize_social_provider(&req.provider);
    if !social_provider_supported(&provider) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "unsupported social provider",
        ));
    }

    let vault_provider = format!("social_{provider}");
    if let Some(vault) = state.vault.as_ref() {
        let _ = vault.delete_key(&vault_provider);
    } else if let Ok(mut creds) = load_credentials(&state.credentials_path) {
        if creds.oauth_provider.as_deref() == Some(provider.as_str()) {
            creds.oauth_token = None;
            creds.oauth_provider = None;
            let _ = save_credentials(&state.credentials_path, &creds);
        }
    }

    crate::halo::identity_ledger::append_social_revoke(&provider, req.reason.as_deref())
        .map_err(internal_err)?;
    let mut cfg = crate::halo::identity::load();
    if let Some(p) = cfg.social.providers.get_mut(&provider) {
        p.selected = false;
        p.expires_at = None;
        p.source = Some("revoked".to_string());
    }
    cfg.social.last_updated = Some(chrono::Utc::now().to_rfc3339());
    crate::halo::identity::save(&cfg).map_err(internal_err)?;
    let projection = crate::halo::identity_ledger::project_ledger_status(now_unix_secs())
        .map_err(internal_err)?;
    Ok(Json(json!({
        "ok": true,
        "provider": provider,
        "projection": projection,
    })))
}

async fn api_identity_social_oauth_start(
    AxumState(state): AxumState<DashboardState>,
    Path(provider): Path<String>,
    Query(query): Query<SocialOauthStartQuery>,
) -> ApiResult {
    let provider = crate::halo::identity_ledger::normalize_social_provider(&provider);
    if !social_provider_supported(&provider) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "unsupported social provider",
        ));
    }
    let expires_days = clamp_social_expiry_days(query.expires_in_days);
    let login_url = social_provider_login_url(&provider);
    if !social_provider_oauth_bridge(&provider) {
        return Ok(Json(json!({
            "provider": provider,
            "oauth_bridge_supported": false,
            "manual_login_url": login_url,
            "message": "This provider requires external OAuth setup. Complete auth externally, then connect with token."
        })));
    }

    let now = now_unix_secs();
    let secret = oauth_state_secret(&state);
    let state_token = crate::halo::identity_ledger::encode_oauth_state(
        &provider,
        now.saturating_add(600),
        &secret,
    );
    register_oauth_state(&state, &state_token, now.saturating_add(600))?;
    let callback = format!(
        "http://127.0.0.1:3100/api/identity/social/oauth/callback?provider={provider}&expires_in_days={expires_days}"
    );
    let oauth_url = format!(
        "https://agenthalo.dev/auth/{provider}?redirect={}&state={}",
        url_encode(&callback),
        url_encode(&state_token)
    );
    Ok(Json(json!({
        "provider": provider,
        "oauth_bridge_supported": true,
        "oauth_url": oauth_url,
        "expires_in_days": expires_days,
    })))
}

async fn api_identity_social_oauth_callback(
    AxumState(state): AxumState<DashboardState>,
    Query(query): Query<SocialOauthCallbackQuery>,
) -> Html<String> {
    let provider = crate::halo::identity_ledger::normalize_social_provider(&query.provider);
    let token = query
        .token
        .clone()
        .or(query.access_token.clone())
        .unwrap_or_default();
    let now = now_unix_secs();
    let secret = oauth_state_secret(&state);
    let validate =
        crate::halo::identity_ledger::decode_oauth_state(&query.state, &provider, now, &secret);
    let (ok, message) = if let Err(e) = validate {
        (false, format!("OAuth callback rejected: {e}"))
    } else if let Err((_, body)) = consume_oauth_state(&state, &query.state, now) {
        let err = body
            .0
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("OAuth state rejected");
        (false, err.to_string())
    } else if token.trim().is_empty() {
        (false, "OAuth callback missing token".to_string())
    } else {
        match persist_social_connection(
            &state,
            &provider,
            token.trim(),
            "oauth_callback",
            query.expires_in_days,
            true,
        ) {
            Ok(_) => (true, format!("Connected {provider} successfully.")),
            Err((_, body)) => {
                let err = body
                    .0
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                (false, format!("OAuth callback failed: {err}"))
            }
        }
    };

    let status = if ok { "ok" } else { "error" };
    let escaped = html_escape(&message);
    Html(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentHALO OAuth</title></head><body style=\"font-family:ui-monospace,monospace;background:#070a05;color:#7cff85;padding:20px\"><h2>AgentHALO Social Login</h2><p>{escaped}</p><script>(function(){{try{{if(window.opener)window.opener.postMessage({{type:'agenthalo-social-oauth',status:'{status}',provider:'{provider}',message:{msg}}},'*');}}catch(_e){{}}setTimeout(function(){{window.close();}},1200);}})();</script></body></html>",
        msg = serde_json::to_string(&message).unwrap_or_else(|_| "\"\"".to_string()),
    ))
}

async fn api_identity_super_secure_status(
    AxumState(_state): AxumState<DashboardState>,
) -> ApiResult {
    let cfg = crate::halo::identity::load();
    Ok(Json(json!({
        "passkey_enabled": cfg.super_secure.passkey_enabled,
        "security_key_enabled": cfg.super_secure.security_key_enabled,
        "totp_enabled": cfg.super_secure.totp_enabled,
        "totp_label": cfg.super_secure.totp_label,
        "last_updated": cfg.super_secure.last_updated,
    })))
}

async fn api_identity_super_secure_update(
    AxumState(_state): AxumState<DashboardState>,
    Json(req): Json<SuperSecureUpdateRequest>,
) -> ApiResult {
    let option = req.option.trim().to_ascii_lowercase();
    if option != "passkey" && option != "security_key" && option != "totp" {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "option must be one of: passkey, security_key, totp",
        ));
    }
    let mut cfg = crate::halo::identity::load();
    let previous_cfg = cfg.clone();
    match option.as_str() {
        "passkey" => cfg.super_secure.passkey_enabled = req.enabled,
        "security_key" => cfg.super_secure.security_key_enabled = req.enabled,
        "totp" => {
            cfg.super_secure.totp_enabled = req.enabled;
            if let Some(label) = req
                .metadata
                .as_ref()
                .and_then(|m| m.get("label"))
                .and_then(|v| v.as_str())
            {
                cfg.super_secure.totp_label = Some(label.to_string());
            }
        }
        _ => {}
    }
    cfg.super_secure.last_updated = Some(chrono::Utc::now().to_rfc3339());
    crate::halo::identity::save(&cfg).map_err(internal_err)?;
    if let Err(e) = crate::halo::identity_ledger::append_super_secure_update(
        &option,
        req.enabled,
        req.metadata.unwrap_or(Value::Null),
    ) {
        let rollback_err = rollback_identity(&previous_cfg).err();
        let msg = if let Some(rb) = rollback_err {
            format!("identity ledger append failed: {e}; {rb}")
        } else {
            format!("identity ledger append failed; identity rolled back: {e}")
        };
        return Err(internal_err(msg));
    }

    Ok(Json(json!({
        "ok": true,
        "option": option,
        "enabled": req.enabled,
        "state": {
            "passkey_enabled": cfg.super_secure.passkey_enabled,
            "security_key_enabled": cfg.super_secure.security_key_enabled,
            "totp_enabled": cfg.super_secure.totp_enabled,
            "totp_label": cfg.super_secure.totp_label,
            "last_updated": cfg.super_secure.last_updated,
        }
    })))
}

async fn api_identity_pod_share_schema(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    Ok(Json(json!({
        "namespace": "identity/*",
        "default_patterns": crate::pod::identity_share::default_identity_patterns(),
        "notes": [
            "Raw OAuth/social tokens are never shared.",
            "Use key_patterns to constrain fields (e.g., identity/profile/*).",
            "Set require_grants=true with grantee_puf_hex to enforce POD grants.",
            "Responses include a proof_envelope (payload hash, Merkle root, ledger head binding, optional ML-DSA signature).",
        ],
        "examples": {
            "profile_only": ["identity/profile/*"],
            "social_only": ["identity/social/*"],
            "high_assurance_only": ["identity/super_secure/*"],
        }
    })))
}

async fn api_identity_pod_share(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<IdentityPodShareRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let now = now_unix_secs();
    let profile = crate::halo::profile::load();
    let identity = crate::halo::identity::load();
    let ledger = crate::halo::identity_ledger::project_ledger_status(now).map_err(internal_err)?;

    let mut records =
        crate::pod::identity_share::materialize_identity_records(&profile, &identity, &ledger);
    if !req.include_ledger {
        records.retain(|r| !r.key.starts_with("identity/ledger/"));
    }

    let patterns: Vec<String> = if req.key_patterns.is_empty() {
        crate::pod::identity_share::default_identity_patterns()
    } else {
        req.key_patterns
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    };
    let selected = crate::pod::identity_share::select_records_by_patterns(&records, &patterns);

    let mut denied_keys: Vec<String> = Vec::new();
    let require_grants = req.require_grants.unwrap_or(false);
    let grantee_filter = req
        .grantee_puf_hex
        .as_ref()
        .map(|hex| decode_hex_32(hex, "grantee_puf_hex"))
        .transpose()?;

    let shared = if require_grants || grantee_filter.is_some() {
        let grantee = grantee_filter.ok_or_else(|| {
            api_err(
                StatusCode::BAD_REQUEST,
                "grantee_puf_hex is required when require_grants is true",
            )
        })?;
        let guard = state
            .grant_store
            .read()
            .map_err(|e| internal_err(format!("grant store read lock poisoned: {e}")))?;
        let (allowed, denied) =
            crate::pod::identity_share::filter_records_by_grants(&selected, &guard, &grantee);
        denied_keys = denied;
        allowed
    } else {
        selected
    };
    let proof_envelope = crate::pod::identity_share::build_share_envelope(
        &shared,
        &ledger,
        &patterns,
        require_grants,
        req.grantee_puf_hex.as_deref(),
    )
    .map_err(internal_err)?;
    let proof_verification = crate::pod::identity_share::verify_share_envelope(
        &proof_envelope,
        &shared,
        &ledger,
        &patterns,
        require_grants,
        req.grantee_puf_hex.as_deref(),
    );

    Ok(Json(json!({
        "ok": true,
        "patterns": patterns,
        "require_grants": require_grants,
        "grantee_puf_hex": req.grantee_puf_hex,
        "include_ledger": req.include_ledger,
        "record_count": shared.len(),
        "records": shared,
        "share_map": crate::pod::identity_share::records_to_json_map(&shared),
        "proof_envelope": proof_envelope,
        "proof_verification": proof_verification,
        "denied_keys": denied_keys,
        "ledger": {
            "total_entries": ledger.total_entries,
            "head_hash": ledger.head_hash,
            "chain_valid": ledger.chain_valid,
            "signed_entries": ledger.signed_entries,
            "unsigned_entries": ledger.unsigned_entries,
            "fully_signed": ledger.fully_signed,
        },
    })))
}

fn p2pclaw_load_config_for_api() -> Result<p2pclaw::P2PClawConfig, (StatusCode, Json<Value>)> {
    p2pclaw::load_config().map_err(|e| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("P2PCLAW not configured: {e}"),
        )
    })
}

async fn api_networking_available(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let addons_cfg = addons::load_or_default();
    Ok(Json(json!({
        "networks": [
            {
                "id": "p2pclaw",
                "name": "P2PCLAW Research Hive",
                "description": "Decentralized research collaboration with peer validation",
                "enabled": addons_cfg.p2pclaw_enabled,
                "configurable": true,
                "coming_soon": false
            },
            {
                "id": "nym-mesh",
                "name": "Nym Mixnet Mesh",
                "description": "Privacy-preserving agent communication",
                "enabled": false,
                "configurable": false,
                "coming_soon": true
            },
            {
                "id": "didcomm-federation",
                "name": "DIDComm Federation",
                "description": "Cross-organization agent identity mesh",
                "enabled": false,
                "configurable": false,
                "coming_soon": true
            }
        ]
    })))
}

fn epistemic_trust_floor() -> f64 {
    std::env::var("AGENTHALO_EPISTEMIC_TRUST_FLOOR")
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .unwrap_or(0.1)
        .clamp(0.0, 1.0)
}

fn base_agent_trust(agent_type: &str) -> f64 {
    match agent_type.trim().to_ascii_lowercase().as_str() {
        "claude" | "codex" | "gemini" => 0.9,
        "openclaw" => 0.8,
        "shell" => 0.6,
        _ => 0.5,
    }
}

fn enrich_orchestrator_agents_payload(mut payload: Value) -> Value {
    let floor = epistemic_trust_floor();
    let model = EpistemicTrust::new(floor);
    if let Some(agents) = payload
        .get_mut("agents")
        .and_then(|value| value.as_array_mut())
    {
        for agent in agents {
            let agent_type = agent
                .get("agent_type")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let base = base_agent_trust(agent_type);
            let trust = model.nucleus(base);
            if let Some(obj) = agent.as_object_mut() {
                obj.insert("epistemic_trust".to_string(), json!(trust));
                obj.insert(
                    "trust_fixed_point".to_string(),
                    json!(model.is_fixed_point(base)),
                );
                obj.insert("trust_floor".to_string(), json!(model.floor()));
            }
        }
    }
    payload
}

async fn api_orch_agents(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp("orchestrator_list", json!({})).await?;
        return Ok(Json(enrich_orchestrator_agents_payload(payload)));
    }

    let orchestrator = local_orchestrator(&state)?;
    let trust_model = EpistemicTrust::new(epistemic_trust_floor());
    let agents = orchestrator.list_agents().await;
    Ok(Json(json!({
        "agents": agents.into_iter().map(|a| {
            let base_trust = base_agent_trust(&a.agent_type);
            let trust = trust_model.nucleus(base_trust);
            json!({
                "agent_id": a.agent_id,
                "agent_name": a.agent_name,
                "agent_type": a.agent_type,
                "status": match a.status {
                    crate::orchestrator::agent_pool::AgentStatus::Idle => "idle",
                    crate::orchestrator::agent_pool::AgentStatus::Busy { .. } => "busy",
                    crate::orchestrator::agent_pool::AgentStatus::Stopped { .. } => "stopped",
                },
                "tasks_completed": a.tasks_completed,
                "total_cost_usd": a.total_cost_usd,
                "capabilities": a.capabilities,
                "launched_at": a.launched_at,
                "epistemic_trust": trust,
                "trust_fixed_point": trust_model.is_fixed_point(base_trust),
                "trust_floor": trust_model.floor(),
            })
        }).collect::<Vec<_>>()
    })))
}

async fn api_orch_tasks(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp("orchestrator_tasks", json!({})).await?;
        return Ok(Json(payload));
    }
    let orchestrator = local_orchestrator(&state)?;
    let tasks = orchestrator.list_tasks().await;
    Ok(Json(json!({ "tasks": tasks })))
}

async fn api_orch_graph(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp("orchestrator_graph", json!({})).await?;
        return Ok(Json(enrich_orchestrator_graph_payload(payload)));
    }
    let orchestrator = local_orchestrator(&state)?;
    let graph = orchestrator.graph_snapshot().await;
    Ok(Json(enrich_orchestrator_graph_payload(
        json!({ "graph": graph }),
    )))
}

async fn api_orch_mesh(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp("orchestrator_mesh_status", json!({}))
            .await
            .unwrap_or_else(|_| {
                json!({
                    "enabled": false,
                    "self_agent_id": Value::Null,
                    "peers": [],
                    "network_name": Value::Null
                })
            });
        return Ok(Json(payload));
    }
    let orchestrator = local_orchestrator(&state)?;
    Ok(Json(
        serde_json::to_value(orchestrator.mesh_status_async().await)
            .map_err(|e| internal_err(format!("serialize mesh status: {e}")))?,
    ))
}

async fn api_orch_launch(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<OrchestratorLaunchApiRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp(
            "orchestrator_launch",
            json!({
                "agent": req.agent,
                "agent_name": req.agent_name.unwrap_or_else(|| "agent".to_string()),
                "working_dir": req.working_dir,
                "env": req.env,
                "timeout_secs": req.timeout_secs.unwrap_or(600),
                "model": req.model,
                "trace": req.trace.unwrap_or(true),
                "capabilities": req.capabilities,
            }),
        )
        .await?;
        return Ok(Json(payload));
    }
    let orchestrator = local_orchestrator(&state)?;
    let launched = orchestrator
        .launch_agent(crate::orchestrator::LaunchAgentRequest {
            agent: req.agent,
            agent_name: req.agent_name.unwrap_or_else(|| "agent".to_string()),
            working_dir: req.working_dir,
            env: req.env,
            timeout_secs: req.timeout_secs.unwrap_or(600),
            model: req.model,
            trace: req.trace.unwrap_or(true),
            capabilities: req.capabilities,
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({
        "agent_id": launched.agent_id,
        "status": "idle",
        "agent_name": launched.agent_name,
        "agent_type": launched.agent_type,
        "capabilities": launched.capabilities,
        "model": launched.model,
    })))
}

async fn api_orch_task(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<OrchestratorTaskApiRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp(
            "orchestrator_send_task",
            json!({
                "agent_id": req.agent_id,
                "task": req.task,
                "timeout_secs": req.timeout_secs,
                "wait": req.wait.unwrap_or(true),
            }),
        )
        .await?;
        return Ok(Json(payload));
    }
    let orchestrator = local_orchestrator(&state)?;
    let task = orchestrator
        .send_task(crate::orchestrator::SendTaskRequest {
            agent_id: req.agent_id,
            task: req.task,
            timeout_secs: req.timeout_secs,
            wait: req.wait.unwrap_or(true),
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({ "task": task })))
}

async fn api_orch_pipe(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<OrchestratorPipeApiRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp(
            "orchestrator_pipe",
            json!({
                "source_task_id": req.source_task_id,
                "target_agent_id": req.target_agent_id,
                "transform": req.transform,
                "task_prefix": req.task_prefix,
            }),
        )
        .await?;
        return Ok(Json(payload));
    }
    let orchestrator = local_orchestrator(&state)?;
    let submitted = orchestrator
        .pipe(crate::orchestrator::PipeRequest {
            source_task_id: req.source_task_id.clone(),
            target_agent_id: req.target_agent_id.clone(),
            transform: req.transform,
            task_prefix: req.task_prefix,
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({
        "source_task_id": req.source_task_id,
        "target_agent_id": req.target_agent_id,
        "status": submitted
            .as_ref()
            .map(|task| match task.status {
                crate::orchestrator::task::TaskStatus::Complete => "complete",
                crate::orchestrator::task::TaskStatus::Failed => "failed",
                crate::orchestrator::task::TaskStatus::Timeout => "timeout",
                _ => "running",
            })
            .unwrap_or("linked"),
        "task_id": submitted.map(|t| t.task_id),
    })))
}

async fn api_orch_stop(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<OrchestratorStopApiRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    if orchestrator_mcp_proxy_enabled() {
        let payload = call_orchestrator_tool_via_mcp(
            "orchestrator_stop",
            json!({
                "agent_id": req.agent_id,
                "force": req.force.unwrap_or(false),
            }),
        )
        .await?;
        return Ok(Json(payload));
    }
    let orchestrator = local_orchestrator(&state)?;
    let stopped = orchestrator
        .stop_agent(crate::orchestrator::StopRequest {
            agent_id: req.agent_id,
            force: req.force.unwrap_or(false),
        })
        .await
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({
        "agent_id": stopped.agent_id,
        "status": stopped.status,
        "trace_session_id": stopped.trace_session_id,
        "attestation_ready": stopped.attestation_ready,
    })))
}

async fn ws_orch_agent_output(
    ws: WebSocketUpgrade,
    Path(agent_id): Path<String>,
    AxumState(state): AxumState<DashboardState>,
) -> Response {
    if let Err((status, body)) = require_sensitive_access(&state) {
        return (status, body).into_response();
    }
    if orchestrator_mcp_proxy_enabled() {
        return ws
            .on_upgrade(move |socket| async move {
                handle_orch_proxy_ws(socket, agent_id).await;
            })
            .into_response();
    }
    let Ok(orchestrator) = local_orchestrator(&state) else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"local orchestrator unavailable"})),
        )
            .into_response();
    };
    let Some(session) = orchestrator.current_agent_session(&agent_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"orchestrator agent session not found"})),
        )
            .into_response();
    };
    ws.on_upgrade(move |socket| async move {
        handle_orch_ws(socket, session).await;
    })
    .into_response()
}

async fn handle_orch_ws(socket: WebSocket, session: Arc<crate::cockpit::pty_manager::PtySession>) {
    let (mut tx, mut rx) = futures_util::StreamExt::split(socket);
    let mut out_rx = session.subscribe_output();
    loop {
        tokio::select! {
            outbound = out_rx.recv() => {
                match outbound {
                    Ok(crate::cockpit::pty_manager::SessionEvent::Output(bytes)) => {
                        if tx.send(Message::Binary(bytes.into())).await.is_err() {
                            break;
                        }
                    }
                    Ok(crate::cockpit::pty_manager::SessionEvent::Status(status)) => {
                        let msg = json!({"type":"status","status": format!("{status:?}")}).to_string();
                        if tx.send(Message::Text(msg.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            inbound = futures_util::StreamExt::next(&mut rx) => {
                match inbound {
                    Some(Ok(Message::Text(text))) => {
                        if session.write_input(text.as_bytes()).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if session.write_input(&bytes).is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = tx.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Pong(_))) => {}
                }
            }
        }
    }
    let _ = tx.close().await;
}

async fn handle_orch_proxy_ws(socket: WebSocket, agent_id: String) {
    let (mut tx, mut rx) = futures_util::StreamExt::split(socket);
    let mut poll = tokio::time::interval(Duration::from_millis(750));
    let mut last_payload = String::new();

    loop {
        tokio::select! {
            _ = poll.tick() => {
                let payload = match call_orchestrator_tool_via_mcp("orchestrator_tasks", json!({})).await {
                    Ok(p) => p,
                    Err((status, body)) => {
                        let msg = json!({
                            "type": "error",
                            "status": status.as_u16(),
                            "error": body.0.get("error").and_then(|v| v.as_str()).unwrap_or("proxy task poll failed"),
                        });
                        let _ = tx.send(Message::Text(msg.to_string().into())).await;
                        break;
                    }
                };
                let status_msg = proxy_ws_status_payload(&payload, &agent_id);
                let serialized = status_msg.to_string();
                if serialized != last_payload {
                    if tx.send(Message::Text(serialized.clone().into())).await.is_err() {
                        break;
                    }
                    last_payload = serialized;
                }
                if matches!(
                    status_msg.get("status").and_then(|v| v.as_str()),
                    Some("complete" | "failed" | "timeout")
                ) {
                    break;
                }
            }
            inbound = futures_util::StreamExt::next(&mut rx) => {
                match inbound {
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = tx.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
    let _ = tx.close().await;
}

fn proxy_ws_status_payload(tasks_payload: &Value, agent_id: &str) -> Value {
    let maybe_latest = tasks_payload
        .get("tasks")
        .and_then(|v| v.as_array())
        .and_then(|tasks| {
            tasks
                .iter()
                .rev()
                .find(|task| task.get("agent_id").and_then(|v| v.as_str()) == Some(agent_id))
        });
    match maybe_latest {
        Some(task) => json!({
            "type": "task_status",
            "agent_id": agent_id,
            "task_id": task.get("task_id"),
            "status": task.get("status").cloned().unwrap_or(json!("running")),
            "answer": task.get("answer").cloned().unwrap_or(Value::Null),
            "output": task.get("output")
                .cloned()
                .or_else(|| task.get("result").cloned())
                .unwrap_or(Value::Null),
            "result": task.get("result")
                .cloned()
                .or_else(|| task.get("output").cloned())
                .unwrap_or(Value::Null),
            "error": task.get("error").cloned().unwrap_or(Value::Null),
            "exit_code": task.get("exit_code").cloned().unwrap_or(Value::Null),
            "trace_session_id": task.get("trace_session_id").cloned().unwrap_or(Value::Null),
        }),
        None => json!({
            "type": "task_status",
            "agent_id": agent_id,
            "status": "idle",
        }),
    }
}

fn enrich_orchestrator_graph_payload(payload: Value) -> Value {
    let graph = payload.get("graph").cloned().unwrap_or(Value::Null);
    let derived_node_count = graph
        .get("nodes")
        .and_then(Value::as_object)
        .map(|nodes| nodes.len())
        .unwrap_or(0);
    let derived_edge_count = graph
        .get("edges")
        .and_then(Value::as_array)
        .map(|edges| edges.len())
        .unwrap_or(0);
    let node_count = payload
        .get("node_count")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(derived_node_count);
    let edge_count = payload
        .get("edge_count")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(derived_edge_count);
    let nodes_shape = payload
        .get("nodes_shape")
        .and_then(Value::as_str)
        .unwrap_or("object_map");
    json!({
        "graph": graph,
        "node_count": node_count,
        "edge_count": edge_count,
        "nodes_shape": nodes_shape,
    })
}

fn local_orchestrator(
    state: &DashboardState,
) -> Result<Arc<crate::orchestrator::Orchestrator>, (StatusCode, Json<Value>)> {
    state.orchestrator.clone().ok_or_else(|| {
        api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "local orchestrator unavailable",
        )
    })
}

async fn api_addons_get(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let addons_cfg = addons::load_or_default();
    Ok(Json(json!({
        "ok": true,
        "addons": addons_cfg
    })))
}

async fn api_addons_post(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AddonsUpdateRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let updated = addons::set_enabled(req.name.trim(), req.enabled)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({
        "ok": true,
        "addons": updated
    })))
}

async fn api_p2pclaw_configure(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<P2PClawConfigureRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let mut cfg = p2pclaw::load_or_default();
    if let Some(endpoint_url) = req.endpoint_url {
        cfg.endpoint_url = endpoint_url.trim().to_string();
    }
    if let Some(agent_id) = req.agent_id {
        let trimmed = agent_id.trim();
        if trimmed.is_empty() {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "agent_id must not be empty",
            ));
        }
        cfg.agent_id = trimmed.to_string();
    }
    if let Some(agent_name) = req.agent_name {
        let trimmed = agent_name.trim();
        if trimmed.is_empty() {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "agent_name must not be empty",
            ));
        }
        cfg.agent_name = trimmed.to_string();
    }
    if let Some(tier) = req.tier {
        let tier = tier.trim().to_ascii_lowercase();
        if !matches!(tier.as_str(), "tier1" | "tier2") {
            return Err(api_err(
                StatusCode::BAD_REQUEST,
                "tier must be one of: tier1, tier2",
            ));
        }
        cfg.tier = tier;
    }
    let configured = p2pclaw::configure(&mut cfg, req.auth_secret)
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    Ok(Json(json!({
        "ok": true,
        "auth_in_vault": configured.auth_in_vault,
        "auth_configured": configured.auth_configured,
        "config": cfg
    })))
}

async fn api_p2pclaw_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let mut cfg = p2pclaw_load_config_for_api()?;
    let swarm = p2pclaw::ping(&cfg).map_err(|e| api_err(StatusCode::BAD_GATEWAY, &e))?;
    cfg.last_connected_at = now_unix_secs();
    let _ = p2pclaw::save_config(&cfg);
    Ok(Json(json!({
        "ok": true,
        "config": cfg,
        "swarm": swarm
    })))
}

async fn api_p2pclaw_briefing(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let cfg = p2pclaw_load_config_for_api()?;
    let briefing = p2pclaw::get_briefing(&cfg).map_err(|e| api_err(StatusCode::BAD_GATEWAY, &e))?;
    Ok(Json(json!({
        "ok": true,
        "briefing_markdown": briefing
    })))
}

async fn api_config(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let creds_path = &state.credentials_path;
    let has_auth = is_dashboard_authenticated(creds_path);
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

    let profile = crate::halo::profile::load();
    let identity_cfg = crate::halo::identity::load();
    let pmt_auth = agentpmt::has_bearer_token();
    let (agentpmt_ok, pmt_tool_count) = resolve_agentpmt_setup_status(&pmt_cfg, pmt_auth);
    let anonymous_wallet_connected = identity_cfg.anonymous_mode;
    let agentaddress_connected = identity_cfg.agent_address.is_some();
    let agentaddress_address = identity_cfg
        .agent_address
        .as_ref()
        .map(|a| a.evm_address.clone());
    let wallet_complete = agentpmt_ok || anonymous_wallet_connected || agentaddress_connected;
    let has_pq = has_wallet();
    let legacy_identity_ok = has_auth || has_pq;
    let identity_ok = profile.has_name() || identity_cfg.anonymous_mode || legacy_identity_ok;
    let llm_ok = state
        .vault
        .as_ref()
        .and_then(|v| v.get_key("openrouter").ok())
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false)
        // v2 fallback: try scope-key-derived vault when crypto is unlocked
        || configured_vault(&state)
            .ok()
            .and_then(|v| v.get_key("openrouter").ok())
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false)
        || std::env::var("OPENROUTER_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_some();
    let setup_complete = identity_ok && wallet_complete && llm_ok;

    Ok(Json(json!({
        "authentication": {
            "authenticated": has_auth,
            "required": dashboard_auth_required(),
        },
        "wrapping": {
            "claude": wrap_status("claude"),
            "codex": wrap_status("codex"),
            "gemini": wrap_status("gemini"),
            "shell_rc": path_display_hint(&rc),
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
        "wallet_status": {
            "agentpmt_connected": agentpmt_ok,
            "agentpmt_auth_configured": pmt_auth,
            "anonymous_wallet_connected": anonymous_wallet_connected,
            "agentaddress_connected": agentaddress_connected,
            "agentaddress_address": agentaddress_address,
            "wallet_complete": wallet_complete,
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
            // Legacy compatibility field; mapped to wallet completion.
            "agentpmt": wallet_complete,
            "wallet": wallet_complete,
            "llm": llm_ok,
            "complete": setup_complete,
        },
        "pq_wallet": has_pq,
        "vault": {
            "available": state.vault.is_some(),
            "path": path_display_hint(&config::vault_path()),
        },
        "paths": {
            "home": path_display_hint(&config::halo_dir()),
            "db": path_display_hint(&state.db_path),
            "credentials": path_display_hint(&state.credentials_path),
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

async fn api_agentpmt_disconnect(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    // Remove vault key
    if let Some(ref vault) = state.vault {
        let _ = vault.delete_key("agentpmt");
    }
    // Disable config and clear catalog
    agentpmt::disconnect().map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "disconnected": true })))
}

#[derive(Deserialize)]
struct AnonymousWalletRequest {
    #[serde(default)]
    label: Option<String>,
}

async fn api_agentpmt_anonymous_wallet(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AnonymousWalletRequest>,
) -> ApiResult {
    let result = agentpmt::create_anonymous_wallet(req.label.as_deref()).map_err(|e| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("anonymous wallet creation failed: {e}"),
        )
    })?;

    // Try to extract and persist the bearer token from the response
    let mut token_saved = false;
    if let Some(parsed) = agentpmt::extract_mcp_text(&result) {
        let token = parsed
            .get("bearer_token")
            .or_else(|| parsed.get("token"))
            .or_else(|| parsed.get("api_key"))
            .and_then(|v| v.as_str());
        if let Some(tok) = token {
            if let Some(ref vault) = state.vault {
                let _ = vault.set_key("agentpmt", "AGENTPMT_API_KEY", tok);
                // Enable proxy
                let mut cfg = agentpmt::load_or_default();
                cfg.enabled = true;
                cfg.updated_at = chrono::Utc::now().timestamp() as u64;
                let path = agentpmt::agentpmt_config_path();
                let _ = agentpmt::save_config(&path, &cfg);
                token_saved = true;
            }
        }
    }

    Ok(Json(json!({
        "ok": true,
        "token_saved": token_saved,
        "result": result,
    })))
}

async fn api_agentaddress_status(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let identity = crate::halo::identity::load();
    let entry = identity.agent_address.clone();
    Ok(Json(json!({
        "ok": true,
        "connected": entry.is_some(),
        "agent_address": entry.as_ref().map(|a| a.evm_address.clone()),
        "generated_at": entry.as_ref().map(|a| a.generated_at.clone()),
        "source": entry.as_ref().and_then(|a| a.source.clone()),
    })))
}

async fn api_agentaddress_chains(AxumState(_state): AxumState<DashboardState>) -> ApiResult {
    let chains = agentaddress_supported_chains();
    Ok(Json(json!({
        "ok": true,
        "count": chains.len(),
        "chains": chains,
        "note": "+30 more EVM-compatible networks are supported by AgentAddress.",
    })))
}

async fn api_agentaddress_generate(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<AgentAddressGenerateRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let source = parse_agentaddress_source(req.source.as_deref())
        .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    let (provider, endpoint, source_tag, data, genesis_seed_sha256) = match source {
        AgentAddressSource::External => {
            let base = agentaddress_api_base();
            let endpoint = format!("{}/api/external/agentaddress", base.trim_end_matches('/'));
            let resp = http_client::post(&endpoint)
                .map_err(|e| api_err(StatusCode::BAD_GATEWAY, &e))?
                .header("Content-Type", "application/json")
                .send_json(json!({}))
                .map_err(|e| {
                    api_err(
                        StatusCode::BAD_GATEWAY,
                        &format!("AgentAddress request failed: {e}"),
                    )
                })?;
            let payload: Value = resp.into_body().read_json().map_err(|e| {
                api_err(
                    StatusCode::BAD_GATEWAY,
                    &format!("parse AgentAddress response: {e}"),
                )
            })?;
            let data = payload
                .get("data")
                .cloned()
                .unwrap_or_else(|| payload.clone());
            (
                "AgentAddress",
                endpoint,
                "agentpmt_external_noauth".to_string(),
                data,
                None,
            )
        }
        AgentAddressSource::Genesis => {
            let genesis_key = get_scope_key_bytes(&state, CryptoScope::Genesis)?;
            let mnemonic =
                crate::halo::genesis_seed::derive_wallet_mnemonic_prefer_v2(genesis_key.as_ref())
                    .map_err(internal_err)?
                    .ok_or_else(|| {
                        api_err(
                            StatusCode::PRECONDITION_FAILED,
                            "genesis seed not available; complete Genesis ceremony first",
                        )
                    })?;
            let derived = crate::halo::evm_wallet::derive_from_mnemonic(&mnemonic, None)
                .map_err(internal_err)?;
            let seed_hash =
                crate::halo::genesis_seed::load_seed_sha256_prefer_v2(genesis_key.as_ref())
                    .map_err(internal_err)?
                    .unwrap_or_default();
            let data = json!({
                "evmAddress": derived.evm_address,
                "evmPrivateKey": derived.private_key_hex,
                "mnemonic": mnemonic,
                "derivationPath": derived.derivation_path,
            });
            (
                "LocalGenesis",
                "local://genesis-seed-derivation".to_string(),
                "genesis_derived".to_string(),
                data,
                Some(seed_hash),
            )
        }
    };
    let address = data
        .get("evmAddress")
        .and_then(serde_json::Value::as_str)
        .or_else(|| data.get("evm_address").and_then(serde_json::Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if !is_evm_address(&address) {
        return Err(api_err(
            StatusCode::BAD_GATEWAY,
            "AgentAddress response missing a valid evmAddress",
        ));
    }

    let persist_public = req.persist_public_address.unwrap_or(true);
    let mut ledger_logged = false;
    let mut ledger_error = None::<String>;
    let mut vault_stored = false;
    let mut vault_error = None::<String>;

    // Store private key and mnemonic in the encrypted vault (AES-256-GCM at rest).
    let private_key = data
        .get("evmPrivateKey")
        .or_else(|| data.get("evm_private_key"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mnemonic = data
        .get("mnemonic")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    if let Some(ref vault) = state.vault {
        let mut all_ok = true;
        if !private_key.is_empty() {
            if let Err(e) = vault.set_key(
                "agent_wallet_private_key",
                "AGENT_WALLET_PRIVATE_KEY",
                &private_key,
            ) {
                vault_error = Some(format!("store private key: {e}"));
                all_ok = false;
            }
        }
        if !mnemonic.is_empty() {
            if let Err(e) =
                vault.set_key("agent_wallet_mnemonic", "AGENT_WALLET_MNEMONIC", &mnemonic)
            {
                let msg = format!("store mnemonic: {e}");
                vault_error = Some(match vault_error {
                    Some(prev) => format!("{prev}; {msg}"),
                    None => msg,
                });
                all_ok = false;
            }
        }
        vault_stored = all_ok && (!private_key.is_empty() || !mnemonic.is_empty());
    } else {
        vault_error = Some("vault not available — credentials shown but not stored".to_string());
    }

    if persist_public {
        let mut identity = crate::halo::identity::load();
        identity.agent_address = Some(crate::halo::identity::AgentAddressIdentity {
            evm_address: address.clone(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            source: Some(source_tag.clone()),
        });
        crate::halo::identity::save(&identity).map_err(internal_err)?;
        let ledger_kind = match source {
            AgentAddressSource::Genesis => {
                crate::halo::identity_ledger::IdentityLedgerKind::WalletCreated
            }
            AgentAddressSource::External => {
                crate::halo::identity_ledger::IdentityLedgerKind::WalletImported
            }
        };
        match crate::halo::identity_ledger::append_wallet_event(
            ledger_kind,
            "agent_address_generated",
            json!({
                "provider": "agentaddress",
                "source": source_tag,
                "evm_address": address,
                "vault_stored": vault_stored,
                "genesis_seed_sha256": genesis_seed_sha256,
            }),
        ) {
            Ok(_) => {
                ledger_logged = true;
            }
            Err(e) => {
                ledger_error = Some(e);
            }
        }
    }

    // Strip sensitive fields from the response if vault storage succeeded —
    // the frontend can still display the address but doesn't need the raw secrets.
    let safe_data = if vault_stored {
        let mut d = data.clone();
        if let Some(obj) = d.as_object_mut() {
            obj.remove("evmPrivateKey");
            obj.remove("evm_private_key");
            obj.remove("mnemonic");
        }
        d
    } else {
        // Vault unavailable — let the frontend show them so the user can back up manually.
        data.clone()
    };

    Ok(Json(json!({
        "ok": true,
        "provider": provider,
        "source": match source {
            AgentAddressSource::External => "external",
            AgentAddressSource::Genesis => "genesis",
        },
        "endpoint": endpoint,
        "persist_public_address": persist_public,
        "vault_stored": vault_stored,
        "vault_error": vault_error,
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
        "genesis_seed_sha256": genesis_seed_sha256,
        "data": safe_data,
    })))
}

async fn api_agentaddress_credentials(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let identity = crate::halo::identity::load();
    let address = identity
        .agent_address
        .as_ref()
        .map(|a| a.evm_address.clone())
        .unwrap_or_default();
    let vault = state
        .vault
        .as_ref()
        .ok_or_else(|| api_err(StatusCode::SERVICE_UNAVAILABLE, "vault not available"))?;
    let private_key = vault.get_key("agent_wallet_private_key").ok();
    let mnemonic = vault.get_key("agent_wallet_mnemonic").ok();
    Ok(Json(json!({
        "ok": true,
        "address": address,
        "has_private_key": private_key.is_some(),
        "private_key": private_key,
        "has_mnemonic": mnemonic.is_some(),
        "mnemonic": mnemonic,
    })))
}

async fn api_agentaddress_disconnect(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let mut identity = crate::halo::identity::load();
    let prev = identity.agent_address.take();
    crate::halo::identity::save(&identity).map_err(internal_err)?;
    let address = prev
        .as_ref()
        .map(|x| x.evm_address.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = crate::halo::identity_ledger::append_wallet_event(
        crate::halo::identity_ledger::IdentityLedgerKind::WalletDeleted,
        "agent_address_disconnected",
        json!({
            "provider": "agentaddress",
            "evm_address": address,
        }),
    );
    Ok(Json(json!({
        "ok": true,
        "disconnected": prev.is_some(),
    })))
}

async fn api_wdk_available(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    Ok(Json(json!({
        "available": crate::halo::wdk_proxy::WdkManager::is_available(),
        "wallet_exists": crate::halo::wdk_proxy::wallet_exists(),
    })))
}

async fn api_wdk_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    let wallet_exists = crate::halo::wdk_proxy::wallet_exists();
    let available = crate::halo::wdk_proxy::WdkManager::is_available();
    let mgr = lock_wdk_manager(&state)?;
    let sidecar_running = mgr.is_running();
    let sidecar = if sidecar_running {
        mgr.get("/status")
            .unwrap_or_else(|e| json!({ "initialized": false, "error": e }))
    } else {
        json!({ "initialized": false })
    };
    Ok(Json(json!({
        "available": available,
        "wallet_exists": wallet_exists,
        "sidecar_running": sidecar_running,
        "sidecar": sidecar,
    })))
}

async fn api_wdk_create(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<WdkPassphraseRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    if req.passphrase.trim().len() < 8 {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "passphrase must be at least 8 characters",
        ));
    }
    if crate::halo::wdk_proxy::wallet_exists() {
        return Err(api_err(
            StatusCode::CONFLICT,
            "wallet already exists; unlock or delete it first",
        ));
    }
    let genesis_key = get_scope_key_bytes(&state, CryptoScope::Genesis)?;
    let seed = crate::halo::genesis_seed::derive_wallet_mnemonic_prefer_v2(genesis_key.as_ref())
        .map_err(internal_err)?
        .ok_or_else(|| {
            api_err(
                StatusCode::PRECONDITION_FAILED,
                "genesis seed not available; complete Genesis ceremony before wallet creation",
            )
        })?;
    let genesis_seed_sha256 =
        crate::halo::genesis_seed::load_seed_sha256_prefer_v2(genesis_key.as_ref())
            .map_err(internal_err)?
            .unwrap_or_default();
    if !crate::halo::wdk_proxy::WdkManager::is_available() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "WDK sidecar unavailable; install with `cd wdk-sidecar && npm install`",
        ));
    }

    let mut mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        mgr.start().map_err(internal_err)?;
    }
    if let Err(e) = mgr.post("/init", &json!({"seed": seed})) {
        let _ = mgr.post("/destroy", &json!({}));
        mgr.stop();
        return Err(internal_err(e));
    }
    let encrypted =
        crate::halo::wdk_proxy::encrypt_seed(&seed, &req.passphrase).map_err(internal_err)?;
    if let Err(e) = crate::halo::wdk_proxy::save_encrypted_seed(&encrypted) {
        let _ = mgr.post("/destroy", &json!({}));
        mgr.stop();
        return Err(internal_err(e));
    }
    let (ledger_logged, ledger_error) = match crate::halo::identity_ledger::append_wallet_event(
        crate::halo::identity_ledger::IdentityLedgerKind::WalletCreated,
        "created",
        json!({
            "chains": encrypted.chains,
            "kdf": encrypted.kdf,
            "genesis_bound": true,
            "genesis_seed_sha256": genesis_seed_sha256,
        }),
    ) {
        Ok(_) => (true, None::<String>),
        Err(e) => (false, Some(e)),
    };
    let accounts = mgr.get("/accounts").unwrap_or_else(|_| json!({}));
    Ok(Json(json!({
        "ok": true,
        "message": "wallet created from genesis-bound entropy and encrypted",
        "genesis_bound": true,
        "genesis_seed_sha256": genesis_seed_sha256,
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
        "accounts": accounts.get("accounts").cloned().unwrap_or(Value::Array(Vec::new())),
    })))
}

async fn api_wdk_import(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<WdkImportRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    let seed = req.seed.trim();
    if !wdk_is_valid_seed_phrase(seed) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "seed phrase must be a valid 12 or 24-word BIP-39 mnemonic",
        ));
    }
    if req.passphrase.trim().len() < 8 {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "passphrase must be at least 8 characters",
        ));
    }
    if crate::halo::wdk_proxy::wallet_exists() {
        return Err(api_err(
            StatusCode::CONFLICT,
            "wallet already exists; delete it first to import a new seed",
        ));
    }
    if !crate::halo::wdk_proxy::WdkManager::is_available() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "WDK sidecar unavailable; install with `cd wdk-sidecar && npm install`",
        ));
    }

    let mut mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        mgr.start().map_err(internal_err)?;
    }
    if let Err(e) = mgr.post("/init", &json!({"seed": seed})) {
        let _ = mgr.post("/destroy", &json!({}));
        mgr.stop();
        return Err(internal_err(e));
    }
    let encrypted =
        crate::halo::wdk_proxy::encrypt_seed(seed, &req.passphrase).map_err(internal_err)?;
    if let Err(e) = crate::halo::wdk_proxy::save_encrypted_seed(&encrypted) {
        let _ = mgr.post("/destroy", &json!({}));
        mgr.stop();
        return Err(internal_err(e));
    }
    let (ledger_logged, ledger_error) = match crate::halo::identity_ledger::append_wallet_event(
        crate::halo::identity_ledger::IdentityLedgerKind::WalletImported,
        "imported",
        json!({
            "chains": encrypted.chains,
            "kdf": encrypted.kdf,
        }),
    ) {
        Ok(_) => (true, None::<String>),
        Err(e) => (false, Some(e)),
    };
    let accounts = mgr.get("/accounts").unwrap_or_else(|_| json!({}));
    Ok(Json(json!({
        "ok": true,
        "message": "wallet imported and encrypted",
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
        "accounts": accounts.get("accounts").cloned().unwrap_or(Value::Array(Vec::new())),
    })))
}

async fn api_wdk_unlock(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<WdkPassphraseRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    let now = now_unix_secs();
    {
        let throttle = state
            .wdk_unlock_state
            .lock()
            .map_err(|e| internal_err(format!("WDK unlock throttle lock poisoned: {e}")))?;
        if throttle.locked_until_unix > now {
            let retry_after = throttle.locked_until_unix.saturating_sub(now);
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "error": format!("too many failed unlock attempts; retry in {}s", retry_after),
                    "retry_after_secs": retry_after,
                })),
            ));
        }
    }
    let encrypted = crate::halo::wdk_proxy::load_encrypted_seed()
        .ok_or_else(|| api_err(StatusCode::NOT_FOUND, "no WDK wallet found"))?;
    let seed = match crate::halo::wdk_proxy::decrypt_seed(&encrypted, &req.passphrase) {
        Ok(seed) => {
            if let Ok(mut throttle) = state.wdk_unlock_state.lock() {
                throttle.failed_attempts = 0;
                throttle.locked_until_unix = 0;
            }
            seed
        }
        Err(e) => {
            let mut retry_after = 0;
            if let Ok(mut throttle) = state.wdk_unlock_state.lock() {
                throttle.failed_attempts = throttle.failed_attempts.saturating_add(1);
                retry_after = wdk_unlock_delay_secs(throttle.failed_attempts);
                throttle.locked_until_unix = now.saturating_add(retry_after);
            }
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({
                    "error": e,
                    "retry_after_secs": retry_after,
                })),
            ));
        }
    };
    let mut mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        mgr.start().map_err(internal_err)?;
    }
    let sidecar = mgr
        .post("/init", &json!({"seed": seed}))
        .map_err(internal_err)?;
    let (ledger_logged, ledger_error) = match crate::halo::identity_ledger::append_wallet_event(
        crate::halo::identity_ledger::IdentityLedgerKind::WalletUnlocked,
        "unlocked",
        json!({
            "sidecar_initialized": sidecar.get("initialized").and_then(|v| v.as_bool()).unwrap_or(false),
        }),
    ) {
        Ok(_) => (true, None::<String>),
        Err(e) => (false, Some(e)),
    };
    Ok(Json(json!({
        "ok": true,
        "sidecar": sidecar,
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
    })))
}

async fn api_wdk_accounts(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    let mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "wallet is locked; unlock it first",
        ));
    }
    mgr.get("/accounts").map(Json).map_err(internal_err)
}

async fn api_wdk_balances(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    let mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "wallet is locked; unlock it first",
        ));
    }
    mgr.get("/balances").map(Json).map_err(internal_err)
}

async fn api_wdk_send(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<WdkTransferRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let (chain, to, amount) =
        validate_wdk_transfer(&req).map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    require_scope(&state, CryptoScope::Wallet)?;
    let mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "wallet is locked; unlock it first",
        ));
    }
    mgr.post(
        "/send",
        &json!({
            "chain": chain,
            "to": to,
            "amount": amount,
        }),
    )
    .map(Json)
    .map_err(internal_err)
}

async fn api_wdk_quote(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<WdkTransferRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let (chain, to, amount) =
        validate_wdk_transfer(&req).map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
    require_scope(&state, CryptoScope::Wallet)?;
    let mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "wallet is locked; unlock it first",
        ));
    }
    mgr.post(
        "/quote",
        &json!({
            "chain": chain,
            "to": to,
            "amount": amount,
        }),
    )
    .map(Json)
    .map_err(internal_err)
}

async fn api_wdk_fees(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    let mgr = lock_wdk_manager(&state)?;
    if !mgr.is_running() {
        return Err(api_err(
            StatusCode::SERVICE_UNAVAILABLE,
            "wallet is locked; unlock it first",
        ));
    }
    mgr.get("/fees").map(Json).map_err(internal_err)
}

async fn api_wdk_lock(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    let mut mgr = lock_wdk_manager(&state)?;
    if mgr.is_running() {
        let _ = mgr.post("/destroy", &json!({}));
    }
    mgr.stop();
    let (ledger_logged, ledger_error) = match crate::halo::identity_ledger::append_wallet_event(
        crate::halo::identity_ledger::IdentityLedgerKind::WalletLocked,
        "locked",
        json!({}),
    ) {
        Ok(_) => (true, None::<String>),
        Err(e) => (false, Some(e)),
    };
    if let Ok(mut throttle) = state.wdk_unlock_state.lock() {
        throttle.failed_attempts = 0;
        throttle.locked_until_unix = 0;
    }
    Ok(Json(json!({
        "ok": true,
        "message": "wallet locked",
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
    })))
}

async fn api_wdk_delete(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<WdkDeleteRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Wallet)?;
    if req.confirm.trim() != "DELETE" {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "send {\"confirm\":\"DELETE\"} to confirm permanent wallet deletion",
        ));
    }
    let mut mgr = lock_wdk_manager(&state)?;
    if mgr.is_running() {
        let _ = mgr.post("/destroy", &json!({}));
    }
    mgr.stop();

    let seed_path = crate::halo::wdk_proxy::encrypted_seed_path();
    if seed_path.exists() {
        std::fs::remove_file(&seed_path).map_err(|e| {
            internal_err(format!(
                "failed to delete encrypted seed at {}: {e}",
                seed_path.display()
            ))
        })?;
    }
    let (ledger_logged, ledger_error) = match crate::halo::identity_ledger::append_wallet_event(
        crate::halo::identity_ledger::IdentityLedgerKind::WalletDeleted,
        "deleted",
        json!({}),
    ) {
        Ok(_) => (true, None::<String>),
        Err(e) => (false, Some(e)),
    };
    if let Ok(mut throttle) = state.wdk_unlock_state.lock() {
        throttle.failed_attempts = 0;
        throttle.locked_until_unix = 0;
    }

    Ok(Json(json!({
        "ok": true,
        "message": "wallet permanently deleted",
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
    })))
}

async fn api_auth_oauth_start(
    AxumState(state): AxumState<DashboardState>,
    Path(provider): Path<String>,
    Query(query): Query<AuthOauthStartQuery>,
) -> ApiResult {
    let provider = provider.trim().to_ascii_lowercase();
    if !auth_oauth_provider_supported(&provider) {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "unsupported provider; expected github|google",
        ));
    }
    let expires_minutes = clamp_auth_oauth_expiry_minutes(query.expires_in_minutes);
    let expires_at = now_unix_secs().saturating_add(expires_minutes.saturating_mul(60));
    let secret = oauth_state_secret(&state);
    let state_token =
        crate::halo::identity_ledger::encode_oauth_state(&provider, expires_at, &secret);
    register_oauth_state(&state, &state_token, expires_at)?;
    let redirect = format!("http://127.0.0.1:3100/api/auth/oauth/callback?provider={provider}");
    let oauth_url = format!(
        "https://agenthalo.dev/auth/{provider}?redirect={}&state={}",
        url_encode(&redirect),
        url_encode(&state_token)
    );

    Ok(Json(json!({
        "ok": true,
        "provider": provider,
        "oauth_url": oauth_url,
        "expires_in_minutes": expires_minutes,
    })))
}

async fn api_auth_oauth_callback(
    AxumState(state): AxumState<DashboardState>,
    Query(query): Query<AuthOauthCallbackQuery>,
) -> Result<Html<String>, (StatusCode, Json<Value>)> {
    let provider = query.provider.trim().to_ascii_lowercase();
    if !auth_oauth_provider_supported(&provider) {
        return Ok(Html(
            "<!doctype html><html><body>Unsupported OAuth provider.</body></html>".to_string(),
        ));
    }

    let now = now_unix_secs();
    let secret = oauth_state_secret(&state);
    let state_ok =
        crate::halo::identity_ledger::decode_oauth_state(&query.state, &provider, now, &secret);
    if let Err(e) = state_ok {
        let message = html_escape(&format!("OAuth state rejected: {e}"));
        return Ok(Html(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentHALO OAuth</title></head><body style=\"font-family:ui-monospace,monospace;background:#070a05;color:#ff6b6b;padding:20px\"><h2>AgentHALO Auth</h2><p>{message}</p><script>(function(){{try{{if(window.opener)window.opener.postMessage({{type:'agenthalo-auth-oauth',status:'error',provider:'{provider}',message:{msg}}},'*');}}catch(_e){{}}setTimeout(function(){{window.close();}},1800);}})();</script></body></html>",
            msg = serde_json::to_string(&format!("OAuth state rejected: {e}")).unwrap_or_else(|_| "\"OAuth state rejected\"".to_string())
        )));
    }

    if let Err((_, body)) = consume_oauth_state(&state, &query.state, now) {
        let err = body
            .0
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("OAuth state rejected");
        let message = html_escape(err);
        return Ok(Html(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentHALO OAuth</title></head><body style=\"font-family:ui-monospace,monospace;background:#070a05;color:#ff6b6b;padding:20px\"><h2>AgentHALO Auth</h2><p>{message}</p><script>(function(){{try{{if(window.opener)window.opener.postMessage({{type:'agenthalo-auth-oauth',status:'error',provider:'{provider}',message:{msg}}},'*');}}catch(_e){{}}setTimeout(function(){{window.close();}},1800);}})();</script></body></html>",
            msg = serde_json::to_string(err).unwrap_or_else(|_| "\"OAuth state rejected\"".to_string())
        )));
    }

    let token = query
        .token
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| {
            query
                .access_token
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().to_string())
        });
    let Some(token) = token else {
        let message = "OAuth callback missing token/access_token";
        let escaped = html_escape(message);
        return Ok(Html(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentHALO OAuth</title></head><body style=\"font-family:ui-monospace,monospace;background:#070a05;color:#ff6b6b;padding:20px\"><h2>AgentHALO Auth</h2><p>{escaped}</p><script>(function(){{try{{if(window.opener)window.opener.postMessage({{type:'agenthalo-auth-oauth',status:'error',provider:'{provider}',message:{msg}}},'*');}}catch(_e){{}}setTimeout(function(){{window.close();}},1800);}})();</script></body></html>",
            msg = serde_json::to_string(message).unwrap_or_else(|_| "\"OAuth callback missing token\"".to_string())
        )));
    };

    let mut creds = load_credentials(&state.credentials_path).unwrap_or_default();
    creds.oauth_provider = Some(provider.clone());
    creds.oauth_token = Some(token);
    let user_id = query
        .user_id
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .map(|v| v.trim().to_string())
        .or_else(|| {
            query
                .sub
                .as_deref()
                .filter(|v| !v.trim().is_empty())
                .map(|v| v.trim().to_string())
        });
    if let Some(uid) = user_id {
        creds.user_id = Some(uid);
    }
    if creds.created_at == 0 {
        creds.created_at = now_unix_secs();
    }

    if let Err(e) = save_credentials(&state.credentials_path, &creds) {
        let message = html_escape(&format!("Failed to persist credentials: {e}"));
        return Ok(Html(format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentHALO OAuth</title></head><body style=\"font-family:ui-monospace,monospace;background:#070a05;color:#ff6b6b;padding:20px\"><h2>AgentHALO Auth</h2><p>{message}</p><script>(function(){{try{{if(window.opener)window.opener.postMessage({{type:'agenthalo-auth-oauth',status:'error',provider:'{provider}',message:{msg}}},'*');}}catch(_e){{}}setTimeout(function(){{window.close();}},1800);}})();</script></body></html>",
            msg = serde_json::to_string(&format!("Failed to persist credentials: {e}")).unwrap_or_else(|_| "\"Failed to persist credentials\"".to_string())
        )));
    }

    Ok(Html(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>AgentHALO OAuth</title></head><body style=\"font-family:ui-monospace,monospace;background:#070a05;color:#7cff85;padding:20px\"><h2>AgentHALO Auth</h2><p>OAuth login successful for {provider}. You can close this window.</p><script>(function(){{try{{if(window.opener)window.opener.postMessage({{type:'agenthalo-auth-oauth',status:'ok',provider:'{provider}',message:{msg}}},'*');}}catch(_e){{}}setTimeout(function(){{window.close();}},1200);}})();</script></body></html>",
        msg = serde_json::to_string("OAuth login successful").unwrap_or_else(|_| "\"OAuth login successful\"".to_string())
    )))
}

async fn api_openrouter_oauth_callback(
    Query(query): Query<OpenRouterOauthCallbackQuery>,
) -> Html<String> {
    let Some(code) = query.code.as_deref().filter(|c| !c.trim().is_empty()) else {
        return Html(
            "<!doctype html><html><head><meta charset=\"utf-8\">\
            <title>AgentHALO</title></head>\
            <body style=\"font-family:ui-monospace,monospace;background:#070a05;\
            color:#ff6b6b;padding:20px\">\
            <h2>AgentHALO</h2><p>OpenRouter authorization failed — no code received.</p>\
            <script>(function(){try{if(window.opener)window.opener.postMessage(\
            {type:'agenthalo-openrouter-oauth',status:'error',\
            message:'No authorization code received'},window.location.origin);}catch(_e){}\
            setTimeout(function(){window.close();},1800);})();</script>\
            </body></html>"
                .to_string(),
        );
    };

    let safe_code = serde_json::to_string(code.trim())
        .unwrap_or_else(|_| "\"\"".to_string())
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");

    Html(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
        <title>AgentHALO</title></head>\
        <body style=\"font-family:ui-monospace,monospace;background:#070a05;\
        color:#7cff85;padding:20px\">\
        <h2>AgentHALO</h2><p>Authorization received. Exchanging token...</p>\
        <script>(function(){{try{{if(window.opener)window.opener.postMessage(\
        {{type:'agenthalo-openrouter-oauth',status:'ok',code:{safe_code}}},window.location.origin);}}catch(_e){{}}\
        setTimeout(function(){{window.close();}},1500);}})();</script>\
        </body></html>"
    ))
}

fn normalize_openrouter_exchange_inputs(
    req: &OpenRouterExchangeRequest,
) -> Result<(String, String), (StatusCode, Json<Value>)> {
    let code = req.code.trim();
    let code_verifier = req.code_verifier.trim();
    if code.is_empty() || code_verifier.is_empty() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "missing code or code_verifier",
        ));
    }
    Ok((code.to_string(), code_verifier.to_string()))
}

async fn api_openrouter_oauth_exchange(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<OpenRouterExchangeRequest>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Vault)?;

    let (code, code_verifier) = normalize_openrouter_exchange_inputs(&req)?;

    let exchange_body = json!({
        "code": code,
        "code_verifier": code_verifier,
        "code_challenge_method": "S256",
    });

    let resp = http_client::post("https://openrouter.ai/api/v1/auth/keys")
        .map_err(internal_err)?
        .send_json(exchange_body)
        .map_err(|e| {
            api_err(
                StatusCode::BAD_GATEWAY,
                &format!("OpenRouter key exchange failed: {e}"),
            )
        })?;

    if !resp.status().is_success() {
        return Err(api_err(
            StatusCode::BAD_GATEWAY,
            &format!("OpenRouter returned HTTP {}", resp.status().as_u16()),
        ));
    }

    let body: Value = resp.into_body().read_json().map_err(|e| {
        api_err(
            StatusCode::BAD_GATEWAY,
            &format!("Failed to parse OpenRouter response: {e}"),
        )
    })?;

    let key = body
        .get("key")
        .and_then(Value::as_str)
        .filter(|k| !k.trim().is_empty())
        .ok_or_else(|| {
            api_err(
                StatusCode::BAD_GATEWAY,
                "OpenRouter response missing 'key' field",
            )
        })?
        .trim()
        .to_string();

    let vault = configured_vault(&state)?;
    vault
        .set_key("openrouter", "OPENROUTER_API_KEY", &key)
        .map_err(internal_err)?;

    let verified = provider_test_request("openrouter", &key).is_ok();
    let _ = vault.set_test_result("openrouter", verified);

    Ok(Json(json!({
        "ok": true,
        "provider": "openrouter",
        "verified": verified,
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

async fn api_vault_keys(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Vault)?;
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
    require_scope(&state, CryptoScope::Vault)?;
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
    require_scope(&state, CryptoScope::Vault)?;
    let vault = configured_vault(&state)?;
    vault.delete_key(&provider).map_err(internal_err)?;
    Ok(Json(json!({ "ok": true, "provider": provider })))
}

async fn api_vault_test_key(
    AxumState(state): AxumState<DashboardState>,
    Path(provider): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    require_scope(&state, CryptoScope::Vault)?;
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
) -> Result<Response, (StatusCode, Json<Value>)> {
    require_sensitive_access(&state)?;
    let Some(vault) = state.vault.clone() else {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "vault unavailable: configure PQ wallet and API keys first",
        ));
    };

    if req.stream.unwrap_or(false) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
        let (trace_tx, trace_rx) = tokio::sync::oneshot::channel::<Value>();
        let mut req_for_proxy = req.clone();
        req_for_proxy.stream = Some(true);
        let req_for_trace = req.clone();
        let state_for_trace = state.clone();
        tokio::task::spawn_blocking(move || {
            let stream_result =
                proxy::proxy_chat_stream_sync(&vault, &req_for_proxy, |data_payload| {
                    tx.send(Ok(Event::default().data(data_payload)))
                        .map_err(|_| "stream receiver dropped".to_string())
                });
            match stream_result {
                Ok(telemetry) => {
                    let _ = trace_tx.send(stream_trace_response(&req_for_proxy, &telemetry, None));
                }
                Err(err) => {
                    let _ =
                        trace_tx.send(stream_trace_response(&req_for_proxy, &err.telemetry, None));
                    let _ = tx.send(Ok(Event::default().event("error").data(
                        json!({"error": format!("upstream stream failed: {}", err.message)})
                            .to_string(),
                    )));
                    let _ = tx.send(Ok(Event::default().data("[DONE]")));
                }
            }
        });
        tokio::spawn(async move {
            if let Ok(summary) = trace_rx.await {
                if let Err(e) = record_proxy_trace(&state_for_trace, &req_for_trace, &summary).await
                {
                    eprintln!("warning: stream proxy trace write failed: {e}");
                }
            }
        });
        let stream = UnboundedReceiverStream::new(rx);
        return Ok(Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response());
    }

    let req_for_proxy = req.clone();
    let response =
        tokio::task::spawn_blocking(move || proxy::proxy_chat_sync(&vault, &req_for_proxy))
            .await
            .map_err(|e| internal_err(format!("proxy task join: {e}")))?
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;

    if let Err(e) = record_proxy_trace(&state, &req, &response).await {
        eprintln!("warning: proxy trace write failed: {e}");
    }

    Ok(Json(response).into_response())
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
) -> Result<Response, (StatusCode, Json<Value>)> {
    let cfg = crate::halo::pricing::load_proxy_config();
    if !cfg.enabled {
        return Err(api_err(
            StatusCode::FORBIDDEN,
            "metered proxy is disabled by operator",
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
    if req.stream.unwrap_or(false) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Event, Infallible>>();
        let (trace_tx, trace_rx) = tokio::sync::oneshot::channel::<Value>();
        let mut req_for_proxy = req.clone();
        req_for_proxy.stream = Some(true);
        let req_for_trace = req.clone();
        let state_for_trace = state.clone();
        tokio::task::spawn_blocking(move || {
            let stream_result = proxy::metered_proxy_stream_sync(
                &vault,
                &key_store,
                &customer_key,
                &req_for_proxy,
                &pricing_table,
                markup_pct,
                |data_payload| {
                    tx.send(Ok(Event::default().data(data_payload)))
                        .map_err(|_| "stream receiver dropped".to_string())
                },
            );
            match stream_result {
                Ok(settled) => {
                    let billing = json!({
                        "model": settled.or_model,
                        "input_tokens": settled.input_tokens,
                        "output_tokens": settled.output_tokens,
                        "upstream_cost_usd": settled.upstream_cost_usd,
                        "cost_usd": settled.customer_cost_usd,
                        "remaining_balance_usd": settled.remaining_balance_usd,
                        "generation_id": settled.generation_id,
                    });
                    let _ = trace_tx.send(stream_trace_response(
                        &req_for_proxy,
                        &settled.telemetry,
                        Some(billing),
                    ));
                }
                Err(err) => {
                    let settled = err.settled;
                    let billing = json!({
                        "model": settled.or_model,
                        "input_tokens": settled.input_tokens,
                        "output_tokens": settled.output_tokens,
                        "upstream_cost_usd": settled.upstream_cost_usd,
                        "cost_usd": settled.customer_cost_usd,
                        "remaining_balance_usd": settled.remaining_balance_usd,
                        "generation_id": settled.generation_id,
                        "stream_error": err.message,
                    });
                    let _ = trace_tx.send(stream_trace_response(
                        &req_for_proxy,
                        &settled.telemetry,
                        Some(billing),
                    ));
                    let _ = tx.send(Ok(Event::default().event("error").data(
                        json!({"error": format!("metered upstream stream failed: {}", err.message)}).to_string(),
                    )));
                    let _ = tx.send(Ok(Event::default().data("[DONE]")));
                }
            }
        });
        tokio::spawn(async move {
            if let Ok(summary) = trace_rx.await {
                if let Err(e) = record_proxy_trace(&state_for_trace, &req_for_trace, &summary).await
                {
                    eprintln!("warning: metered stream proxy trace write failed: {e}");
                }
            }
        });
        let stream = UnboundedReceiverStream::new(rx);
        return Ok(Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
            .into_response());
    }

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

    Ok(Json(result.body).into_response())
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
    headers: HeaderMap,
    Json(req): Json<FundBalanceRequest>,
) -> ApiResult {
    use crate::halo::funding;

    // Operator credits require admin auth.
    if matches!(req.source, funding::FundingSource::OperatorCredit { .. }) {
        require_sensitive_access(&state)?;
    } else {
        // Non-operator funding requests are webhook-driven and must include
        // a verified HMAC signature header.
        let sig = headers
            .get("x-agentpmt-signature")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .unwrap_or("");
        if sig.is_empty() {
            return Err(api_err(
                StatusCode::UNAUTHORIZED,
                "missing X-AgentPMT-Signature header",
            ));
        }
        funding::verify_webhook_signature(&key_id, &req.source, req.amount_usd, sig)
            .map_err(|e| api_err(StatusCode::UNAUTHORIZED, &e))?;
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
    let count = attestations.len();
    Ok(Json(json!({
        "status": "ok",
        "attestations": attestations,
        "count": count,
        // Backward-compat key — /api/trust consumers used this name.
        "attestation_count": count,
    })))
}

async fn api_attestation_verify(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<VerifyRequest>,
) -> ApiResult {
    if !is_authenticated(&state.credentials_path) {
        return Err(api_err(
            StatusCode::UNAUTHORIZED,
            "authentication required to verify attestations",
        ));
    }
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
        "db_path": path_display_hint(db_path),
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

async fn api_nucleusdb_memory_store(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<MemoryStoreRequest>,
) -> ApiResult {
    let text = req.text.trim();
    if text.is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "text must be non-empty"));
    }
    let text = text.to_string();
    let source = req.source.clone();
    let db_path = state.db_path.clone();
    let db_lock = state.db_lock.clone();
    let memory_store = state.memory_store.clone();
    let memory_db_cache = state.memory_db_cache.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _guard = db_lock.blocking_lock();
        let mut cache = memory_db_cache
            .lock()
            .map_err(|e| internal_err(format!("memory DB cache lock poisoned: {e}")))?;
        refresh_memory_db_cache_locked(&mut cache, &db_path)?;
        let mut db = cache
            .db
            .as_ref()
            .ok_or_else(|| internal_err("memory DB cache missing after refresh".to_string()))?
            .clone();
        let stored = memory_store
            .store_memory(&mut db, &text, source.as_deref())
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
        let wal_path = default_wal_path(&db_path);
        persist_snapshot_and_sync_wal(&db_path, &wal_path, &db)
            .map_err(|e| internal_err(format!("persist memory store: {e:?}")))?;
        let sealed = matches!(db.write_mode(), WriteMode::AppendOnly);
        cache.db = Some(db);
        cache.file_fingerprint = db_file_fingerprint(&db_path);
        Ok(json!({
            "ok": true,
            "key": stored.key,
            "source": stored.source,
            "created": stored.created,
            "dims": memory_store.embedding_model().dims(),
            "sealed": sealed,
        }))
    })
    .await
    .map_err(|e| internal_err(format!("memory store task join failed: {e}")))??;
    Ok(Json(result))
}

async fn api_nucleusdb_memory_recall(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<MemoryRecallRequest>,
) -> ApiResult {
    let query = req.query.trim();
    if query.is_empty() {
        return Err(api_err(StatusCode::BAD_REQUEST, "query must be non-empty"));
    }
    let k = req.k.clamp(1, 20);
    let query = query.to_string();
    let db_path = state.db_path.clone();
    let db_lock = state.db_lock.clone();
    let memory_store = state.memory_store.clone();
    let memory_db_cache = state.memory_db_cache.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _guard = db_lock.blocking_lock();
        let mut cache = memory_db_cache
            .lock()
            .map_err(|e| internal_err(format!("memory DB cache lock poisoned: {e}")))?;
        refresh_memory_db_cache_locked(&mut cache, &db_path)?;
        let db = cache
            .db
            .as_ref()
            .ok_or_else(|| internal_err("memory DB cache missing after refresh".to_string()))?;
        let results = memory_store
            .recall(db, &query, k)
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
        Ok(json!({
            "ok": true,
            "query": query,
            "k": k,
            "count": results.len(),
            "results": results,
        }))
    })
    .await
    .map_err(|e| internal_err(format!("memory recall task join failed: {e}")))??;
    Ok(Json(result))
}

async fn api_nucleusdb_memory_ingest(
    AxumState(state): AxumState<DashboardState>,
    Json(req): Json<MemoryIngestRequest>,
) -> ApiResult {
    let document = req.document.trim();
    if document.is_empty() {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "document must be non-empty",
        ));
    }
    let document = document.to_string();
    let source = req.source.clone();
    let db_path = state.db_path.clone();
    let db_lock = state.db_lock.clone();
    let memory_store = state.memory_store.clone();
    let memory_db_cache = state.memory_db_cache.clone();
    let result = tokio::task::spawn_blocking(move || {
        let _guard = db_lock.blocking_lock();
        let mut cache = memory_db_cache
            .lock()
            .map_err(|e| internal_err(format!("memory DB cache lock poisoned: {e}")))?;
        refresh_memory_db_cache_locked(&mut cache, &db_path)?;
        let mut db = cache
            .db
            .as_ref()
            .ok_or_else(|| internal_err("memory DB cache missing after refresh".to_string()))?
            .clone();
        let stored = memory_store
            .ingest_document(&mut db, &document, source.as_deref())
            .map_err(|e| api_err(StatusCode::BAD_REQUEST, &e))?;
        let wal_path = default_wal_path(&db_path);
        persist_snapshot_and_sync_wal(&db_path, &wal_path, &db)
            .map_err(|e| internal_err(format!("persist memory ingest: {e:?}")))?;
        let sealed = matches!(db.write_mode(), WriteMode::AppendOnly);
        cache.db = Some(db);
        cache.file_fingerprint = db_file_fingerprint(&db_path);
        Ok(json!({
            "ok": true,
            "chunks": stored.len(),
            "keys": stored.iter().map(|r| r.key.clone()).collect::<Vec<_>>(),
            "source": source,
            "sealed": sealed,
        }))
    })
    .await
    .map_err(|e| internal_err(format!("memory ingest task join failed: {e}")))??;
    Ok(Json(result))
}

async fn api_nucleusdb_memory_stats(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let db_path = state.db_path.clone();
    let db_lock = state.db_lock.clone();
    let memory_store = state.memory_store.clone();
    let memory_db_cache = state.memory_db_cache.clone();
    let stats = tokio::task::spawn_blocking(move || {
        let _guard = db_lock.blocking_lock();
        let mut cache = memory_db_cache
            .lock()
            .map_err(|e| internal_err(format!("memory DB cache lock poisoned: {e}")))?;
        refresh_memory_db_cache_locked(&mut cache, &db_path)?;
        let db = cache
            .db
            .as_ref()
            .ok_or_else(|| internal_err("memory DB cache missing after refresh".to_string()))?;
        Ok::<crate::memory::MemoryStats, (StatusCode, Json<Value>)>(memory_store.stats(db))
    })
    .await
    .map_err(|e| internal_err(format!("memory stats task join failed: {e}")))??;
    Ok(Json(json!({
        "ok": true,
        "total_memories": stats.total_memories,
        "total_dims": stats.total_dims,
        "model": stats.model,
        "index_size": stats.index_size,
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
    if !req.permissions.read
        && !req.permissions.write
        && !req.permissions.append
        && !req.permissions.control
    {
        return Err(api_err(
            StatusCode::BAD_REQUEST,
            "permissions must enable at least one of read/write/append/control",
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

fn db_file_fingerprint(path: &std::path::Path) -> Option<super::DbFileFingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    Some(super::DbFileFingerprint {
        modified,
        len: meta.len(),
    })
}

fn refresh_memory_db_cache_locked(
    cache: &mut super::MemoryDbCache,
    db_path: &std::path::Path,
) -> Result<(), (StatusCode, Json<Value>)> {
    let disk_fingerprint = db_file_fingerprint(db_path);
    let should_reload = cache.db.is_none() || cache.file_fingerprint != disk_fingerprint;
    if should_reload {
        cache.db = Some(load_halo_db(db_path)?);
        cache.file_fingerprint = disk_fingerprint;
    }
    Ok(())
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

async fn api_nucleusdb_vectors(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let mut db = load_halo_db(&state.db_path)?;
    let mut executor = SqlExecutor::new(&mut db);
    let type_summary = match executor.execute("SHOW TYPES") {
        SqlResult::Rows { columns, rows } => json!({ "columns": columns, "rows": rows }),
        SqlResult::Error { message } => {
            return Err(internal_err(format!("SHOW TYPES failed: {message}")));
        }
        _ => json!({ "columns": [], "rows": [] }),
    };
    let vector_rows = type_summary
        .get("rows")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_array().cloned())
                .filter(|cells| !cells.is_empty())
                .filter(|cells| {
                    cells
                        .first()
                        .and_then(|v| v.as_str())
                        .map(|t| t.eq_ignore_ascii_case("vector"))
                        .unwrap_or(false)
                })
                .map(Value::Array)
                .collect::<Vec<Value>>()
        })
        .unwrap_or_default();
    Ok(Json(json!({
        "status": "ok",
        "endpoint": "/api/nucleusdb/vectors",
        "typed_value_summary": type_summary,
        "vector_type_rows": vector_rows,
        "note": "Use /api/nucleusdb/vector-search for nearest-neighbor queries.",
    })))
}

async fn api_nucleusdb_proofs(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let _guard = state.db_lock.lock().await;
    let mut db = load_halo_db(&state.db_path)?;
    let mut executor = SqlExecutor::new(&mut db);
    let status_summary = match executor.execute("SHOW STATUS") {
        SqlResult::Rows { columns, rows } => json!({ "columns": columns, "rows": rows }),
        SqlResult::Error { message } => {
            return Err(internal_err(format!("SHOW STATUS failed: {message}")));
        }
        _ => json!({ "columns": [], "rows": [] }),
    };
    let history_summary = match executor.execute("SHOW HISTORY") {
        SqlResult::Rows { columns, rows } => json!({ "columns": columns, "rows": rows }),
        SqlResult::Error { message } => {
            return Err(internal_err(format!("SHOW HISTORY failed: {message}")));
        }
        _ => json!({ "columns": [], "rows": [] }),
    };
    Ok(Json(json!({
        "status": "ok",
        "endpoint": "/api/nucleusdb/proofs",
        "status_summary": status_summary,
        "history_summary": history_summary,
        "proof_gate": proof_gate::load_gate_config().ok().map(|cfg| {
            json!({
                "enabled": cfg.enabled,
                "tool_count": cfg.requirements.len(),
                "requirements_total": cfg.requirements.values().map(|v| v.len()).sum::<usize>(),
                "certificate_dir": cfg.certificate_dir
            })
        }),
    })))
}

// ---------------------------------------------------------------------------
// Capabilities
// ---------------------------------------------------------------------------

async fn api_capabilities(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    const MCP_NATIVE_TOOL_COUNT: usize = 22;
    let creds_path = &state.credentials_path;
    let has_auth = is_dashboard_authenticated(creds_path);
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
        "mcp_tools": MCP_NATIVE_TOOL_COUNT + proxied_mcp_tools,
        "mcp_native_tools": MCP_NATIVE_TOOL_COUNT,
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

// ---------------------------------------------------------------------------
// CLI agent install, detect & auth
// ---------------------------------------------------------------------------

/// Per-CLI metadata used by the detect/install/auth endpoints.
struct CliAgentMeta {
    npm_package: &'static str,
    auth_cmd: &'static str,
    auth_args: &'static [&'static str],
    detect_cmd: &'static str,
}

fn cli_agent_meta(agent: &str) -> Option<CliAgentMeta> {
    match agent {
        "claude" => Some(CliAgentMeta {
            npm_package: "@anthropic-ai/claude-code",
            auth_cmd: "claude",
            auth_args: &["auth", "login"],
            detect_cmd: "claude",
        }),
        "codex" => Some(CliAgentMeta {
            npm_package: "@openai/codex",
            auth_cmd: "codex",
            auth_args: &["login"],
            detect_cmd: "codex",
        }),
        "gemini" => Some(CliAgentMeta {
            npm_package: "@google/gemini-cli",
            // Gemini uses interactive first-run auth; launching it opens the
            // auth prompt in the PTY where the user can select "Login with Google".
            auth_cmd: "gemini",
            auth_args: &[],
            detect_cmd: "gemini",
        }),
        "openclaw" => Some(CliAgentMeta {
            npm_package: "openclaw@latest",
            // Onboard wizard handles auth, gateway setup, and daemon install.
            auth_cmd: "openclaw",
            auth_args: &["onboard", "--install-daemon"],
            detect_cmd: "openclaw",
        }),
        _ => None,
    }
}

/// GET /api/cli/detect/{agent} — check if a CLI is installed on the host.
async fn api_cli_detect(Path(agent): Path<String>) -> ApiResult {
    let meta = cli_agent_meta(&agent).ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("unknown CLI agent `{agent}`"),
        )
    })?;
    let installed = tokio::task::spawn_blocking({
        let cmd = meta.detect_cmd.to_string();
        move || {
            std::process::Command::new("which")
                .arg(&cmd)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    })
    .await
    .unwrap_or(false);
    let path = if installed {
        tokio::task::spawn_blocking({
            let cmd = meta.detect_cmd.to_string();
            move || {
                std::process::Command::new("which")
                    .arg(&cmd)
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
            }
        })
        .await
        .unwrap_or(None)
    } else {
        None
    };
    let authenticated = deploy::cli_authenticated(&agent);
    Ok(Json(json!({
        "agent": agent,
        "installed": installed,
        "path": path,
        "authenticated": authenticated,
    })))
}

/// POST /api/cli/install/{agent} — install a CLI agent via npm to the writable
/// /data/npm-global prefix. CLIs are also background-installed at boot; this
/// endpoint lets the frontend trigger or retry on demand.
async fn api_cli_install(
    AxumState(state): AxumState<DashboardState>,
    Path(agent): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let meta = cli_agent_meta(&agent).ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("unknown CLI agent `{agent}`"),
        )
    })?;
    // Check if already installed
    let detect = meta.detect_cmd.to_string();
    let already = tokio::task::spawn_blocking({
        let d = detect.clone();
        move || {
            std::process::Command::new("which")
                .arg(&d)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    })
    .await
    .unwrap_or(false);
    if already {
        return Ok(Json(json!({
            "agent": agent,
            "success": true,
            "installed": true,
            "stdout": format!("{agent} already installed"),
            "stderr": "",
        })));
    }
    // Install to writable npm prefix
    let pkg = meta.npm_package.to_string();
    let _ = std::fs::create_dir_all("/data/npm-global");
    let result = tokio::task::spawn_blocking(move || {
        std::process::Command::new("npm")
            .args(["install", "-g", &pkg])
            .env("NPM_CONFIG_PREFIX", "/data/npm-global")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .await
    .map_err(|e| {
        api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("join error: {e}"),
        )
    })?
    .map_err(|e| {
        api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("install failed: {e}"),
        )
    })?;

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let success = result.status.success();
    Ok(Json(json!({
        "agent": agent,
        "success": success,
        "installed": success,
        "stdout": stdout,
        "stderr": stderr,
    })))
}

/// POST /api/cli/auth/{agent} — launch the CLI auth/login flow via a cockpit
/// PTY session. The CLI opens a browser for OAuth; the session ID is returned
/// so the frontend can attach a terminal if desired.
async fn api_cli_auth(
    AxumState(state): AxumState<DashboardState>,
    Path(agent): Path<String>,
) -> ApiResult {
    require_sensitive_access(&state)?;
    let meta = cli_agent_meta(&agent).ok_or_else(|| {
        api_err(
            StatusCode::BAD_REQUEST,
            &format!("unknown CLI agent `{agent}`"),
        )
    })?;
    let cmd = meta.auth_cmd.to_string();
    let args: Vec<String> = meta.auth_args.iter().map(|s| s.to_string()).collect();
    let id = state
        .pty_manager
        .create_session(
            &cmd,
            &args,
            vec![],
            None,
            120,
            24,
            Some(format!("{agent}-auth")),
        )
        .map_err(|e| {
            api_err(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("failed to start auth: {e}"),
            )
        })?;
    Ok(Json(json!({
        "agent": agent,
        "session_id": id,
        "ws_url": format!("/api/cockpit/sessions/{}/ws", id),
    })))
}

// ---------------------------------------------------------------------------
// OpenClaw harness — gateway status & MCP wiring
// ---------------------------------------------------------------------------

/// GET /api/openclaw/gateway-status — check if the OpenClaw gateway daemon is running.
async fn api_openclaw_gateway_status() -> ApiResult {
    let installed = tokio::task::spawn_blocking(|| {
        std::process::Command::new("which")
            .arg("openclaw")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false);

    if !installed {
        return Ok(Json(json!({
            "installed": false,
            "gateway_running": false,
            "detail": "openclaw CLI not found on PATH",
        })));
    }

    let status_output = tokio::task::spawn_blocking(|| {
        std::process::Command::new("openclaw")
            .args(["gateway", "status"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
    })
    .await
    .map_err(|e| {
        api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("join error: {e}"),
        )
    })?
    .map_err(|e| {
        api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("status check failed: {e}"),
        )
    })?;

    let stdout = String::from_utf8_lossy(&status_output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&status_output.stderr).to_string();
    let running = status_output.status.success();

    Ok(Json(json!({
        "installed": true,
        "gateway_running": running,
        "stdout": stdout.trim(),
        "stderr": stderr.trim(),
    })))
}

/// POST /api/openclaw/wire-mcp — inject NucleusDB + HALO MCP server configs
/// into the user's `~/.openclaw/openclaw.json` so OpenClaw agents can call
/// all NucleusDB tools via stdio transport.
async fn api_openclaw_wire_mcp(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    require_sensitive_access(&state)?;

    let result = tokio::task::spawn_blocking(|| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let config_path = std::path::Path::new(&home)
            .join(".openclaw")
            .join("openclaw.json");

        // Read existing config or start fresh
        let mut config: serde_json::Value = if config_path.exists() {
            let contents = std::fs::read_to_string(&config_path)
                .map_err(|e| format!("failed to read {}: {e}", config_path.display()))?;
            serde_json::from_str(&contents)
                .map_err(|e| format!("failed to parse {}: {e}", config_path.display()))?
        } else {
            // Create parent dir if needed
            if let Some(parent) = config_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
            }
            serde_json::json!({})
        };

        // Resolve paths to our MCP server binaries
        let nucleusdb_mcp_path = resolve_binary("nucleusdb-mcp");
        let agenthalo_mcp_path = resolve_binary("agenthalo-mcp-server");

        // Build MCP server entries
        let nucleusdb_entry = serde_json::json!({
            "command": nucleusdb_mcp_path.as_deref().unwrap_or("nucleusdb-mcp"),
            "args": []
        });
        let agenthalo_entry = serde_json::json!({
            "command": agenthalo_mcp_path.as_deref().unwrap_or("agenthalo-mcp-server"),
            "args": []
        });

        // Navigate to agents.defaults.mcpServers (create path if missing)
        let agents = config
            .as_object_mut()
            .ok_or_else(|| "config root is not an object".to_string())?
            .entry("agents")
            .or_insert_with(|| serde_json::json!({}));
        let defaults = agents
            .as_object_mut()
            .ok_or_else(|| "agents is not an object".to_string())?
            .entry("defaults")
            .or_insert_with(|| serde_json::json!({}));
        let mcp_servers = defaults
            .as_object_mut()
            .ok_or_else(|| "defaults is not an object".to_string())?
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}));
        let servers_map = mcp_servers
            .as_object_mut()
            .ok_or_else(|| "mcpServers is not an object".to_string())?;

        servers_map.insert("nucleusdb".to_string(), nucleusdb_entry);
        servers_map.insert("agenthalo".to_string(), agenthalo_entry);

        // Write back with pretty formatting
        let serialized = serde_json::to_string_pretty(&config)
            .map_err(|e| format!("serialization failed: {e}"))?;

        // Atomic write: temp file + rename
        let tmp_path = config_path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &serialized)
            .map_err(|e| format!("failed to write {}: {e}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &config_path)
            .map_err(|e| format!("failed to rename to {}: {e}", config_path.display()))?;

        Ok::<_, String>(serde_json::json!({
            "config_path": config_path.display().to_string(),
            "nucleusdb_mcp": nucleusdb_mcp_path,
            "agenthalo_mcp": agenthalo_mcp_path,
            "tools_wired": ["nucleusdb", "agenthalo"],
        }))
    })
    .await
    .map_err(|e| {
        api_err(
            StatusCode::INTERNAL_SERVER_ERROR,
            &format!("join error: {e}"),
        )
    })?
    .map_err(|e| api_err(StatusCode::INTERNAL_SERVER_ERROR, &e))?;

    Ok(Json(json!({
        "success": true,
        "detail": result,
    })))
}

/// Try to find a binary on PATH, returning the absolute path if found.
fn resolve_binary(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Query;

    #[test]
    fn stream_trace_response_includes_billing_and_stream_metadata() {
        let request = proxy::ChatCompletionRequest {
            model: "openai/gpt-4o-mini".to_string(),
            messages: vec![proxy::Message {
                role: "user".to_string(),
                content: Value::String("hello".to_string()),
            }],
            temperature: None,
            max_tokens: Some(32),
            stream: Some(true),
            top_p: None,
        };
        let telemetry = proxy::StreamTelemetry {
            prompt_tokens: Some(12),
            completion_tokens: Some(7),
            generation_id: Some("chatcmpl-test".to_string()),
            completion_preview: "hi there".to_string(),
            completed: true,
        };
        let body = stream_trace_response(
            &request,
            &telemetry,
            Some(json!({
                "model": "openai/gpt-4o-mini",
                "cost_usd": 0.001
            })),
        );
        assert_eq!(body["id"], "chatcmpl-test");
        assert_eq!(body["usage"]["prompt_tokens"], 12);
        assert_eq!(body["usage"]["completion_tokens"], 7);
        assert_eq!(body["choices"][0]["message"]["content"], "hi there");
        assert_eq!(body["x_agenthalo"]["cost_usd"], 0.001);
        assert_eq!(body["x_agenthalo_stream"]["completed"], true);
    }

    #[test]
    fn stream_trace_response_falls_back_when_usage_missing() {
        let request = proxy::ChatCompletionRequest {
            model: "openai/gpt-4o-mini".to_string(),
            messages: vec![proxy::Message {
                role: "user".to_string(),
                content: Value::String("12345678".to_string()),
            }],
            temperature: None,
            max_tokens: Some(32),
            stream: Some(true),
            top_p: None,
        };
        let telemetry = proxy::StreamTelemetry {
            prompt_tokens: None,
            completion_tokens: None,
            generation_id: None,
            completion_preview: "abcd".to_string(),
            completed: false,
        };
        let body = stream_trace_response(&request, &telemetry, None);
        assert_eq!(body["usage"]["prompt_tokens"], 2);
        assert_eq!(body["usage"]["completion_tokens"], 1);
        assert_eq!(body["x_agenthalo_stream"]["completed"], false);
    }

    #[tokio::test]
    async fn openrouter_oauth_callback_reports_missing_code() {
        let html =
            api_openrouter_oauth_callback(Query(OpenRouterOauthCallbackQuery { code: None }))
                .await
                .0;
        assert!(html.contains("no code received"));
        assert!(html.contains("agenthalo-openrouter-oauth"));
        assert!(html.contains("status:'error'"));
        assert!(html.contains("window.location.origin"));
    }

    #[tokio::test]
    async fn openrouter_oauth_callback_rejects_empty_string_code() {
        let html = api_openrouter_oauth_callback(Query(OpenRouterOauthCallbackQuery {
            code: Some("   ".to_string()),
        }))
        .await
        .0;
        assert!(html.contains("status:'error'"));
        assert!(html.contains("no code received"));
    }

    #[tokio::test]
    async fn openrouter_oauth_callback_escapes_script_breakout_payload() {
        let payload = "</script><img src=x onerror=alert(1)>\n";
        let html = api_openrouter_oauth_callback(Query(OpenRouterOauthCallbackQuery {
            code: Some(payload.to_string()),
        }))
        .await
        .0;

        assert!(html.contains("status:'ok'"));
        assert!(html.contains("window.location.origin"));
        assert!(!html.contains(payload));
        assert!(!html.contains("</script><img"));
        assert!(html.contains("\\u003c"));
        assert!(html.contains("\\u003e"));
    }

    #[test]
    fn normalize_openrouter_exchange_inputs_rejects_empty_verifier() {
        let req = OpenRouterExchangeRequest {
            code: "abc".to_string(),
            code_verifier: " ".to_string(),
        };
        let err = normalize_openrouter_exchange_inputs(&req).err();
        assert!(err.is_some());
    }

    #[test]
    fn normalize_openrouter_exchange_inputs_trims_values() {
        let req = OpenRouterExchangeRequest {
            code: "  code-123  ".to_string(),
            code_verifier: "\tverifier-xyz\n".to_string(),
        };
        let (code, verifier) =
            normalize_openrouter_exchange_inputs(&req).expect("expected trimmed values");
        assert_eq!(code, "code-123");
        assert_eq!(verifier, "verifier-xyz");
    }
}
