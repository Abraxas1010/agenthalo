use super::DashboardState;
use crate::container::{current_container_id, ContainerAgentLock};
use crate::discord::recorder::DiscordRecorder;
use crate::discord::status as discord_status;
use crate::encrypted_file::{create_header_if_missing, load_header};
use crate::genesis::{
    harvest_entropy, load_seed_bytes_v2, seed_exists, store_seed_once_v2, GenesisError,
};
use crate::identity::{
    load as load_identity, save as save_identity, DeviceIdentity, NetworkIdentity,
};
use crate::protocol::NucleusDb;
use crate::security::FormalProvenance;
use crate::sql::executor::SqlExecutor;
use crate::verifier::gate::{load_gate_config, ProofGateConfig};
use axum::extract::{Path, Query, State as AxumState};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;

fn discord_recorder(state: &DashboardState) -> DiscordRecorder {
    DiscordRecorder::new(state.discord_db_path.clone())
}

fn provenance_json(module: &'static str, entries: Vec<FormalProvenance>) -> Vec<serde_json::Value> {
    entries
        .into_iter()
        .map(|(name, formal_basis, formal_basis_local)| {
            json!({
                "module": module,
                "theorem": name,
                "formal_basis": formal_basis,
                "formal_basis_local": formal_basis_local,
            })
        })
        .collect()
}

fn collect_all_provenance() -> Vec<serde_json::Value> {
    let mut out = Vec::new();
    out.extend(provenance_json(
        "security",
        crate::security::formal_provenance(),
    ));
    out.extend(provenance_json(
        "transparency.ct6962",
        crate::transparency::ct6962::formal_provenance(),
    ));
    out.extend(provenance_json(
        "vc.ipa",
        crate::vc::ipa::formal_provenance(),
    ));
    out.extend(provenance_json(
        "sheaf.coherence",
        crate::sheaf::coherence::formal_provenance(),
    ));
    out.extend(provenance_json(
        "protocol",
        crate::protocol::formal_provenance(),
    ));
    out
}

fn advisory_gate_config(cfg: &ProofGateConfig) -> ProofGateConfig {
    let mut clone = cfg.clone();
    clone.enabled = true;
    clone
}

fn certificate_count(cfg: &ProofGateConfig) -> usize {
    std::fs::read_dir(&cfg.certificate_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
        .filter(|entry| entry.path().extension().and_then(|s| s.to_str()) == Some("lean4export"))
        .count()
}

pub fn api_router(state: DashboardState) -> Router<DashboardState> {
    Router::new()
        .route("/status", get(api_status))
        .route("/crypto/status", get(api_crypto_status))
        .route("/crypto/create-password", post(api_crypto_create_password))
        .route("/crypto/unlock", post(api_crypto_unlock))
        .route("/crypto/lock", post(api_crypto_lock))
        .route("/genesis/status", get(api_genesis_status))
        .route("/genesis/harvest", post(api_genesis_harvest))
        .route("/genesis/reset", post(api_genesis_reset))
        .route("/identity/status", get(api_identity_status))
        .route("/identity/device", post(api_identity_device_save))
        .route("/identity/network", post(api_identity_network_save))
        .route("/nucleusdb/status", get(api_nucleusdb_status))
        .route("/nucleusdb/history", get(api_nucleusdb_history))
        .route("/nucleusdb/sql", post(api_nucleusdb_sql))
        .route("/container/lock-status", get(api_container_lock_status))
        .route("/container/initialize", post(api_container_initialize))
        .route("/container/deinitialize", post(api_container_deinitialize))
        .route("/containers", get(api_containers))
        .route("/containers/provision", post(api_containers_provision))
        .route("/containers/initialize", post(api_containers_initialize))
        .route(
            "/containers/deinitialize",
            post(api_containers_deinitialize),
        )
        .route(
            "/containers/{session_id}",
            axum::routing::delete(api_containers_destroy),
        )
        .route("/containers/{session_id}/logs", get(api_containers_logs))
        .route("/deploy/catalog", get(api_deploy_catalog))
        .route("/deploy/preflight", post(api_deploy_preflight))
        .route("/deploy/launch", post(api_deploy_launch))
        .route("/mcp/invoke", post(api_mcp_invoke))
        .route("/orchestrator/launch", post(api_orchestrator_launch))
        .route("/orchestrator/task", post(api_orchestrator_task))
        .route("/orchestrator/pipe", post(api_orchestrator_pipe))
        .route("/orchestrator/stop", post(api_orchestrator_stop))
        .route("/orchestrator/agents", get(api_orchestrator_agents))
        .route("/orchestrator/tasks", get(api_orchestrator_tasks))
        .route("/orchestrator/graph", get(api_orchestrator_graph))
        .route("/orchestrator/status", get(api_orchestrator_status))
        .route("/models/status", get(api_models_status))
        .route("/models/search", get(api_models_search))
        .route("/models/pull", post(api_models_pull))
        .route("/models/remove", post(api_models_remove))
        .route("/models/serve", post(api_models_serve))
        .route("/models/stop", post(api_models_stop))
        .route("/formal-proofs", get(api_formal_proofs))
        .route("/discord/status", get(api_discord_status))
        .route("/discord/channels", get(api_discord_channels))
        .route("/discord/search", get(api_discord_search))
        .route("/discord/recent", get(api_discord_recent))
        .route("/discord/verify/{message_id}", get(api_discord_verify))
        .route("/discord/integrity", get(api_discord_integrity))
        .route("/discord/export/{channel_id}", get(api_discord_export))
        .with_state(state)
}

async fn api_status(AxumState(state): AxumState<DashboardState>) -> Json<serde_json::Value> {
    Json(json!({"ok": true, "db_path": state.db_path, "home": crate::config::nucleusdb_dir()}))
}

async fn api_crypto_status(AxumState(state): AxumState<DashboardState>) -> Json<serde_json::Value> {
    let crypto = state.crypto.lock().unwrap_or_else(|e| e.into_inner());
    Json(
        json!({"password_unlocked": crypto.password_unlocked, "header_exists": load_header().ok().flatten().is_some()}),
    )
}

#[derive(Deserialize)]
struct PasswordBody {
    password: String,
    confirm: String,
}
async fn api_crypto_create_password(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<PasswordBody>,
) -> Json<serde_json::Value> {
    match crate::password::validate_password_pair(&body.password, &body.confirm)
        .and_then(|_| create_header_if_missing())
        .and_then(|header| {
            let key = header.kdf.derive_master_key(&body.password)?;
            let mut crypto = state.crypto.lock().unwrap_or_else(|e| e.into_inner());
            crypto.password_unlocked = true;
            crypto.master_key = Some(key);
            Ok(())
        }) {
        Ok(()) => Json(json!({"ok": true})),
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

#[derive(Deserialize)]
struct UnlockBody {
    password: String,
}
async fn api_crypto_unlock(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<UnlockBody>,
) -> Json<serde_json::Value> {
    let result = load_header()
        .and_then(|header: Option<crate::encrypted_file::CryptoHeader>| {
            header.ok_or_else(|| "crypto header missing".to_string())
        })
        .and_then(|header| header.kdf.derive_master_key(&body.password));
    match result {
        Ok(key) => {
            let mut crypto = state.crypto.lock().unwrap_or_else(|e| e.into_inner());
            crypto.password_unlocked = true;
            crypto.master_key = Some(key);
            Json(json!({"ok": true}))
        }
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

async fn api_crypto_lock(AxumState(state): AxumState<DashboardState>) -> Json<serde_json::Value> {
    let mut crypto = state.crypto.lock().unwrap_or_else(|e| e.into_inner());
    crypto.password_unlocked = false;
    crypto.master_key = None;
    Json(json!({"ok": true}))
}

async fn api_genesis_status(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let crypto = state.crypto.lock().unwrap_or_else(|e| e.into_inner());
    let seed_loaded = crypto
        .master_key
        .as_ref()
        .and_then(|key| load_seed_bytes_v2(key).ok().flatten())
        .is_some();
    let did = crypto
        .master_key
        .as_ref()
        .and_then(|key| load_seed_bytes_v2(key).ok().flatten())
        .and_then(|seed| crate::did::did_from_genesis_seed(&seed).ok())
        .map(|d| d.did);
    Json(json!({"seed_exists": seed_exists(), "seed_loaded": seed_loaded, "did": did}))
}

async fn api_genesis_harvest(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let master_key = state
        .crypto
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .master_key;
    let Some(master_key) = master_key else {
        return Json(json!({"ok": false, "error": "unlock crypto first"}));
    };
    match harvest_entropy() {
        Ok(outcome) => {
            match store_seed_once_v2(
                &outcome.combined_entropy,
                &outcome.combined_entropy_sha256,
                &master_key,
            ) {
                Ok(()) => {
                    let did = crate::did::did_from_genesis_seed(&outcome.combined_entropy)
                        .ok()
                        .map(|d| d.did);
                    Json(
                        json!({"ok": true, "did": did, "sources": outcome.sources, "hash": outcome.combined_entropy_sha256}),
                    )
                }
                Err(e) => Json(json!({"ok": false, "error": e})),
            }
        }
        Err(GenesisError {
            error_code,
            message,
            failed_sources,
        }) => Json(
            json!({"ok": false, "error_code": error_code, "error": message, "failed_sources": failed_sources}),
        ),
    }
}

async fn api_genesis_reset() -> Json<serde_json::Value> {
    let path = crate::config::genesis_seed_v2_path();
    let _ = std::fs::remove_file(path);
    Json(json!({"ok": true}))
}

async fn api_identity_status(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let cfg = load_identity();
    let did = state
        .crypto
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .master_key
        .and_then(|key| load_seed_bytes_v2(&key).ok().flatten())
        .and_then(|seed| crate::did::did_from_genesis_seed(&seed).ok())
        .map(|d| d.did);
    Json(json!({"ok": true, "identity": cfg, "did": did}))
}

async fn api_identity_device_save(Json(device): Json<DeviceIdentity>) -> Json<serde_json::Value> {
    let mut cfg = load_identity();
    cfg.device = Some(device);
    Json(json!({"ok": save_identity(&cfg).is_ok()}))
}

async fn api_identity_network_save(
    Json(network): Json<NetworkIdentity>,
) -> Json<serde_json::Value> {
    let mut cfg = load_identity();
    cfg.network = Some(network);
    Json(json!({"ok": save_identity(&cfg).is_ok()}))
}

async fn api_nucleusdb_status(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let db = NucleusDb::load_persistent(&state.db_path, crate::cli::default_witness_cfg());
    match db {
        Ok(mut db) => {
            let mut exec = SqlExecutor::new(&mut db);
            match exec.execute("SHOW STATUS;") {
                crate::sql::executor::SqlResult::Rows { columns, rows } => {
                    Json(json!({"ok": true, "columns": columns, "rows": rows}))
                }
                other => Json(json!({"ok": false, "result": format!("{:?}", other)})),
            }
        }
        Err(e) => Json(json!({"ok": false, "error": format!("{e:?}")})),
    }
}

async fn api_nucleusdb_history(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let db = NucleusDb::load_persistent(&state.db_path, crate::cli::default_witness_cfg());
    match db {
        Ok(mut db) => {
            let mut exec = SqlExecutor::new(&mut db);
            match exec.execute("SHOW HISTORY;") {
                crate::sql::executor::SqlResult::Rows { columns, rows } => {
                    Json(json!({"ok": true, "columns": columns, "rows": rows}))
                }
                other => Json(json!({"ok": false, "result": format!("{:?}", other)})),
            }
        }
        Err(e) => Json(json!({"ok": false, "error": format!("{e:?}")})),
    }
}

#[derive(Deserialize)]
struct SqlBody {
    query: String,
}
async fn api_nucleusdb_sql(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<SqlBody>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let loaded = NucleusDb::load_persistent(&state.db_path, crate::cli::default_witness_cfg());
    match loaded {
        Ok(mut db) => {
            let mut exec = SqlExecutor::new(&mut db);
            let out = exec.execute(&body.query);
            if exec.committed() {
                let wal_path = crate::persistence::default_wal_path(&state.db_path);
                let _ = crate::persistence::persist_snapshot_and_sync_wal(
                    &state.db_path,
                    &wal_path,
                    &db,
                );
            }
            Json(json!({"ok": true, "result": format!("{:?}", out)}))
        }
        Err(e) => Json(json!({"ok": false, "error": format!("{e:?}")})),
    }
}

async fn api_formal_proofs() -> Json<serde_json::Value> {
    let gate = load_gate_config().unwrap_or_default();
    let advisory = advisory_gate_config(&gate);
    let mut tools: Vec<String> = advisory.requirements.keys().cloned().collect();
    tools.sort();
    let tool_status: Vec<_> = tools
        .into_iter()
        .map(|tool| {
            let result = advisory.evaluate(&tool);
            json!({
                "tool": tool,
                "passed": result.passed,
                "requirements_checked": result.requirements_checked,
                "requirements_met": result.requirements_met,
                "trust_tier": result.achieved_trust_tier,
                "details": result.verification_results,
            })
        })
        .collect();
    Json(json!({
        "gate_enabled": gate.enabled,
        "certificate_dir": gate.certificate_dir,
        "certificate_count": certificate_count(&gate),
        "tools": tool_status,
        "provenance": collect_all_provenance(),
    }))
}

async fn api_discord_status() -> Json<serde_json::Value> {
    Json(serde_json::to_value(discord_status::load_status().unwrap_or_default()).unwrap())
}
async fn api_discord_channels() -> Json<serde_json::Value> {
    let status = discord_status::load_status().unwrap_or_default();
    Json(json!({"channels": status.channels}))
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    channel_id: Option<String>,
    limit: Option<usize>,
}
async fn api_discord_search(
    AxumState(state): AxumState<DashboardState>,
    Query(query): Query<SearchQuery>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let recorder = discord_recorder(&state);
    match recorder.search(
        &query.q,
        query.channel_id.as_deref(),
        query.limit.unwrap_or(50),
    ) {
        Ok(rows) => Json(json!({"ok": true, "rows": rows})),
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

async fn api_discord_recent(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let recorder = discord_recorder(&state);
    match recorder.recent(None, 25) {
        Ok(rows) => Json(json!({"ok": true, "rows": rows})),
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

async fn api_discord_verify(
    AxumState(state): AxumState<DashboardState>,
    Path(message_id): Path<String>,
    Query(query): Query<std::collections::BTreeMap<String, String>>,
) -> Json<serde_json::Value> {
    let Some(channel_id) = query.get("channel_id") else {
        return Json(json!({"ok": false, "error": "channel_id required"}));
    };
    let _guard = state.db_lock.lock().await;
    let recorder = discord_recorder(&state);
    match recorder.verify_message(channel_id, &message_id) {
        Ok(Some((verified, value))) => Json(
            json!({"ok": true, "key": format!("msg:{channel_id}:{message_id}"), "verified": verified, "value": value}),
        ),
        Ok(None) => Json(json!({"ok": false, "error": "message not found"})),
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

async fn api_discord_integrity(
    AxumState(state): AxumState<DashboardState>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let recorder = discord_recorder(&state);
    match recorder.integrity_summary() {
        Ok((append_only, seal_count)) => {
            Json(json!({"ok": true, "append_only": append_only, "seal_count": seal_count}))
        }
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

async fn api_discord_export(
    AxumState(state): AxumState<DashboardState>,
    Path(channel_id): Path<String>,
) -> Json<serde_json::Value> {
    let _guard = state.db_lock.lock().await;
    let recorder = discord_recorder(&state);
    match recorder.export_channel(&channel_id) {
        Ok(rows) => Json(json!({"ok": true, "channel_id": channel_id, "records": rows})),
        Err(e) => Json(json!({"ok": false, "error": e})),
    }
}

type ApiResult = Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)>;

fn api_error(
    status: StatusCode,
    error: impl Into<String>,
) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(json!({ "ok": false, "error": error.into() })))
}

fn tool_result<T: serde::Serialize>(value: T) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "result": {
            "structured_content": value
        }
    }))
}

const DASHBOARD_REMOTE_TOOL_TIMEOUT: Duration = Duration::from_secs(60);

fn default_container_launch_request(
    image: Option<String>,
    agent_id: Option<String>,
) -> crate::mcp::tools::ContainerLaunchRequest {
    crate::mcp::tools::ContainerLaunchRequest {
        image: image
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                std::env::var("NUCLEUSDB_CONTAINER_IMAGE")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "nucleusdb:latest".to_string())
            }),
        agent_id: agent_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("container-{}", uuid::Uuid::new_v4().simple())),
        command: vec![
            "nucleusdb-mcp".to_string(),
            "/data/nucleusdb.ndb".to_string(),
            "--transport".to_string(),
            "http".to_string(),
            "--host".to_string(),
            "0.0.0.0".to_string(),
            "--port".to_string(),
            crate::container::mesh::DEFAULT_MCP_PORT.to_string(),
        ],
        runtime_runsc: Some(false),
        host_sock: None,
        env: BTreeMap::new(),
        mesh: Some(crate::mcp::tools::ContainerMeshRequest {
            enabled: Some(true),
            mcp_port: Some(crate::container::mesh::DEFAULT_MCP_PORT),
            registry_volume: std::env::var("NUCLEUSDB_CONTAINER_REGISTRY_VOLUME")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            agent_did: None,
        }),
    }
}

async fn find_container_session(
    state: &DashboardState,
    session_id: &str,
) -> Result<crate::mcp::tools::ContainerSessionView, (StatusCode, Json<serde_json::Value>)> {
    let rmcp::Json(payload) = state
        .mcp_service
        .nucleusdb_container_list()
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    payload
        .sessions
        .into_iter()
        .find(|session| session.session_id == session_id)
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                format!("unknown session `{session_id}`"),
            )
        })
}

async fn call_container_session_tool<T: serde::de::DeserializeOwned>(
    state: &DashboardState,
    session_id: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<T, (StatusCode, Json<serde_json::Value>)> {
    let session = find_container_session(state, session_id).await?;
    let registry = crate::container::PeerRegistry::load(&crate::container::mesh_registry_path())
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let peer = registry.find(&session.agent_id).cloned().ok_or_else(|| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("mesh peer for session `{session_id}` is not registered"),
        )
    })?;
    let auth_token = crate::container::mesh_auth_token();
    let tool = tool_name.to_string();
    tokio::task::spawn_blocking(move || {
        crate::container::call_remote_tool_with_timeout(
            &peer,
            &tool,
            arguments,
            auth_token.as_deref(),
            DASHBOARD_REMOTE_TOOL_TIMEOUT,
        )
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join remote tool: {e}"),
        )
    })?
    .map_err(|e| api_error(StatusCode::BAD_GATEWAY, e))
    .and_then(|value| decode_remote_tool_value(value, tool_name))
}

fn agent_catalog() -> Vec<serde_json::Value> {
    vec![
        json!({"id":"codex","name":"Codex","icon":"⌘","description":"OpenAI Codex CLI in PTY or container mode."}),
        json!({"id":"claude","name":"Claude","icon":"◈","description":"Claude Code CLI with JSON trace capture."}),
        json!({"id":"gemini","name":"Gemini","icon":"✦","description":"Gemini CLI for local operator tasks."}),
        json!({"id":"openclaw","name":"OpenClaw","icon":"⟠","description":"OpenClaw agent runner."}),
        json!({"id":"shell","name":"Shell","icon":"▣","description":"Raw shell execution for deterministic tasks."}),
    ]
}

fn cli_install_hint(agent_id: &str) -> &'static str {
    match agent_id {
        "codex" => "Install the `codex` CLI and ensure it is on PATH.",
        "claude" => "Install the `claude` CLI and ensure it is on PATH.",
        "gemini" => "Install the `gemini` CLI and ensure it is on PATH.",
        "openclaw" => "Install the `openclaw` binary and ensure it is on PATH.",
        _ => "No extra install step required.",
    }
}

fn command_exists(command: &str) -> bool {
    std::process::Command::new("which")
        .arg(command)
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn decode_remote_tool_value<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
    context: &str,
) -> Result<T, (StatusCode, Json<serde_json::Value>)> {
    let is_error = value
        .get("isError")
        .or_else(|| value.get("is_error"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_error {
        return Err(api_error(
            StatusCode::BAD_GATEWAY,
            format!("remote tool returned error for {context}"),
        ));
    }
    let payload = value
        .get("structuredContent")
        .or_else(|| value.get("structured_content"))
        .cloned()
        .unwrap_or(value);
    serde_json::from_value(payload).map_err(|e| {
        api_error(
            StatusCode::BAD_GATEWAY,
            format!("decode remote {context}: {e}"),
        )
    })
}

async fn api_container_lock_status(_: AxumState<DashboardState>) -> ApiResult {
    let container_id = current_container_id();
    let lock = ContainerAgentLock::load_or_create(&container_id)
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({
        "container_id": lock.container_id,
        "state": lock.state_label(),
        "reuse_policy": lock.reuse_policy.as_str(),
        "lock": lock
    })))
}

async fn api_container_initialize(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<crate::mcp::tools::ContainerInitializeRequest>,
) -> ApiResult {
    let rmcp::Json(payload) = state
        .mcp_service
        .nucleusdb_container_initialize(Parameters(body))
        .await
        .map_err(|e| api_error(StatusCode::CONFLICT, e.to_string()))?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_container_deinitialize(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let rmcp::Json(payload) = state
        .mcp_service
        .nucleusdb_container_deinitialize(Parameters(Default::default()))
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_containers(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    let rmcp::Json(payload) = state
        .mcp_service
        .nucleusdb_container_list()
        .await
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_containers_provision(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult {
    let request = default_container_launch_request(
        body.get("image")
            .and_then(|value| value.as_str())
            .map(str::to_string),
        body.get("agent_id")
            .and_then(|value| value.as_str())
            .map(str::to_string),
    );
    let rmcp::Json(payload) = state
        .mcp_service
        .nucleusdb_container_provision(Parameters(request))
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

#[derive(Deserialize)]
struct SessionScopedInitializeRequest {
    session_id: String,
    hookup: crate::orchestrator::ContainerHookupRequest,
    #[serde(default)]
    reuse_policy: Option<crate::container::ReusePolicy>,
}

async fn api_containers_initialize(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<SessionScopedInitializeRequest>,
) -> ApiResult {
    let payload: crate::mcp::tools::ContainerInitializeResponse = call_container_session_tool(
        &state,
        &body.session_id,
        "nucleusdb_container_initialize",
        serde_json::to_value(crate::mcp::tools::ContainerInitializeRequest {
            hookup: body.hookup,
            reuse_policy: body.reuse_policy,
        })
        .unwrap_or_else(|_| json!({})),
    )
    .await?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

#[derive(Deserialize)]
struct SessionScopedDeinitializeRequest {
    session_id: String,
}

async fn api_containers_deinitialize(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<SessionScopedDeinitializeRequest>,
) -> ApiResult {
    let payload: crate::mcp::tools::ContainerDeinitializeResponse = call_container_session_tool(
        &state,
        &body.session_id,
        "nucleusdb_container_deinitialize",
        serde_json::json!({}),
    )
    .await?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_containers_destroy(
    AxumState(_state): AxumState<DashboardState>,
    Path(session_id): Path<String>,
) -> ApiResult {
    crate::container::destroy_container(&session_id)
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(
        json!({ "ok": true, "destroyed": true, "session_id": session_id }),
    ))
}

async fn api_containers_logs(Path(session_id): Path<String>) -> ApiResult {
    let logs = crate::container::container_logs(&session_id, false)
        .map_err(|e| api_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(
        json!({ "ok": true, "session_id": session_id, "logs": logs }),
    ))
}

async fn api_deploy_catalog() -> ApiResult {
    Ok(Json(json!({
        "ok": true,
        "agents": agent_catalog(),
    })))
}

#[derive(Deserialize)]
struct DeployPreflightRequest {
    agent_id: String,
    #[serde(default)]
    admission_mode: Option<String>,
}

async fn api_deploy_preflight(Json(body): Json<DeployPreflightRequest>) -> ApiResult {
    let agent_id = body.agent_id.trim().to_ascii_lowercase();
    crate::orchestrator::agent_pool::normalize_agent_kind(&agent_id)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    let docker_available = tokio::task::spawn_blocking(|| command_exists("docker"))
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("join docker probe: {e}"),
            )
        })?;
    let cli_installed = if agent_id == "shell" {
        true
    } else {
        tokio::task::spawn_blocking({
            let agent_id = agent_id.clone();
            move || command_exists(&agent_id)
        })
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("join cli probe: {e}"),
            )
        })?
    };
    Ok(Json(json!({
        "ok": true,
        "agent_id": agent_id,
        "cli_installed": cli_installed,
        "keys_configured": true,
        "missing_keys": [],
        "docker_available": docker_available,
        "ready": cli_installed,
        "install_hint": (!cli_installed).then_some(cli_install_hint(&body.agent_id)),
        "binary_topology": null,
        "admission": {
            "mode": body.admission_mode.unwrap_or_else(|| "warn".to_string()),
            "allowed": true,
            "forced": false,
            "issues": []
        }
    })))
}

#[derive(Deserialize)]
struct DeployLaunchRequest {
    agent_id: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    container: bool,
    #[serde(default)]
    working_dir: Option<String>,
    #[serde(default)]
    admission_mode: Option<String>,
}

async fn api_deploy_launch(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<DeployLaunchRequest>,
) -> ApiResult {
    let agent_id = crate::orchestrator::agent_pool::normalize_agent_kind(&body.agent_id)
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    let request = crate::orchestrator::LaunchAgentRequest {
        agent: agent_id.clone(),
        agent_name: format!("deploy-{agent_id}"),
        working_dir: body.working_dir,
        env: BTreeMap::new(),
        timeout_secs: 600,
        model: None,
        trace: true,
        capabilities: vec!["memory_read".to_string(), "memory_write".to_string()],
        dispatch_mode: Some(if body.container {
            crate::orchestrator::DispatchMode::Container
        } else {
            crate::orchestrator::DispatchMode::Pty
        }),
        container_hookup: body.container.then(|| {
            crate::orchestrator::ContainerHookupRequest::Cli {
                cli_name: agent_id.clone(),
                model: None,
            }
        }),
    };
    let payload = state
        .orchestrator
        .launch_agent(request)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({
        "ok": true,
        "mode": body.mode,
        "admission": {
            "mode": body.admission_mode.unwrap_or_else(|| "warn".to_string()),
            "allowed": true,
            "forced": false,
            "issues": []
        },
        "agent_id": payload.agent_id,
        "agent": payload,
    })))
}

#[derive(Deserialize)]
struct McpInvokeBody {
    tool: String,
    #[serde(default)]
    params: serde_json::Value,
}

async fn api_mcp_invoke(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<McpInvokeBody>,
) -> ApiResult {
    match body.tool.as_str() {
        "nucleusdb_container_provision" => {
            let req: crate::mcp::tools::ContainerLaunchRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_provision(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_container_initialize" => {
            let req: crate::mcp::tools::ContainerInitializeRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_initialize(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_container_deinitialize" => {
            let req: crate::mcp::tools::ContainerDeinitializeRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_deinitialize(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_container_stop" => {
            let req: crate::mcp::tools::ContainerStopRequest = serde_json::from_value(body.params)
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}")))?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_stop(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_container_logs" => {
            let req: crate::mcp::tools::ContainerLogsRequest = serde_json::from_value(body.params)
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}")))?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_logs(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_provision" => {
            let req: crate::mcp::tools::SubsidiaryProvisionRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_provision(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_initialize" => {
            let req: crate::mcp::tools::SubsidiaryInitializeRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_initialize(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_send_task" => {
            let req: crate::mcp::tools::SubsidiarySendTaskRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_send_task(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_get_result" => {
            let req: crate::mcp::tools::SubsidiaryGetResultRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_get_result(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_deinitialize" => {
            let req: crate::mcp::tools::SubsidiaryDeinitializeRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_deinitialize(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_destroy" => {
            let req: crate::mcp::tools::SubsidiaryDestroyRequest =
                serde_json::from_value(body.params).map_err(|e| {
                    api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}"))
                })?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_destroy(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_subsidiary_list" => {
            let req: crate::mcp::tools::SubsidiaryListRequest = serde_json::from_value(body.params)
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, format!("invalid params: {e}")))?;
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_subsidiary_list(Parameters(req))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_container_list" => {
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_list()
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        "nucleusdb_container_lock_status" => {
            let rmcp::Json(payload) = state
                .mcp_service
                .nucleusdb_container_lock_status(Parameters(Default::default()))
                .await
                .map_err(|e| api_error(StatusCode::BAD_REQUEST, e.to_string()))?;
            Ok(tool_result(payload))
        }
        _ => Err(api_error(
            StatusCode::NOT_FOUND,
            format!("unsupported MCP tool `{}`", body.tool),
        )),
    }
}

async fn api_orchestrator_launch(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<crate::orchestrator::LaunchAgentRequest>,
) -> ApiResult {
    let payload = state
        .orchestrator
        .launch_agent(body)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_orchestrator_task(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<crate::orchestrator::SendTaskRequest>,
) -> ApiResult {
    let payload = state
        .orchestrator
        .send_task(body)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true, "task": payload })))
}

async fn api_orchestrator_pipe(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<crate::orchestrator::PipeRequest>,
) -> ApiResult {
    let payload = state
        .orchestrator
        .pipe(body)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true, "task": payload })))
}

async fn api_orchestrator_stop(
    AxumState(state): AxumState<DashboardState>,
    Json(body): Json<crate::orchestrator::StopRequest>,
) -> ApiResult {
    let payload = state
        .orchestrator
        .stop_agent(body)
        .await
        .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_orchestrator_status(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    Ok(Json(
        serde_json::to_value(state.orchestrator.status().await)
            .unwrap_or_else(|_| json!({"ok": false})),
    ))
}

async fn api_orchestrator_agents(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    Ok(Json(json!({
        "ok": true,
        "agents": state.orchestrator.list_agents().await
    })))
}

async fn api_orchestrator_tasks(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    Ok(Json(json!({
        "ok": true,
        "tasks": state.orchestrator.list_tasks().await
    })))
}

async fn api_orchestrator_graph(AxumState(state): AxumState<DashboardState>) -> ApiResult {
    Ok(Json(json!({
        "ok": true,
        "graph": state.orchestrator.graph_snapshot().await
    })))
}

async fn api_models_status() -> ApiResult {
    let payload = tokio::task::spawn_blocking(crate::halo::local_models::detect_status)
        .await
        .map_err(|e| {
            api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("join models status: {e}"),
            )
        })?;
    Ok(Json(
        serde_json::to_value(payload).unwrap_or_else(|_| json!({"ok": false})),
    ))
}

#[derive(Deserialize)]
struct ModelSearchQuery {
    q: String,
    limit: Option<usize>,
}

async fn api_models_search(Query(query): Query<ModelSearchQuery>) -> ApiResult {
    let q = query.q.clone();
    let limit = query.limit.unwrap_or(8);
    let results =
        tokio::task::spawn_blocking(move || crate::halo::local_models::search_models(&q, limit))
            .await
            .map_err(|e| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("join model search: {e}"),
                )
            })?
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(
        json!({ "ok": true, "count": results.len(), "results": results }),
    ))
}

#[derive(Deserialize)]
struct ModelPullRequest {
    model: String,
    source: Option<String>,
}

async fn api_models_pull(Json(body): Json<ModelPullRequest>) -> ApiResult {
    let model = body.model.clone();
    let source = body.source.clone();
    let (payload, output) = tokio::task::spawn_blocking(move || {
        let mut output = Vec::new();
        let payload =
            crate::halo::local_models::pull_model(&model, source.as_deref(), &mut output)?;
        Ok::<_, String>((payload, output))
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join model pull: {e}"),
        )
    })?
    .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({
        "ok": true,
        "model": payload,
        "output": String::from_utf8_lossy(&output)
    })))
}

#[derive(Deserialize)]
struct ModelRemoveRequest {
    model: String,
    source: Option<String>,
}

async fn api_models_remove(Json(body): Json<ModelRemoveRequest>) -> ApiResult {
    let model = body.model.clone();
    let source = body.source.clone();
    tokio::task::spawn_blocking(move || {
        crate::halo::local_models::remove_model(&model, source.as_deref())
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join model remove: {e}"),
        )
    })?
    .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ModelServeRequest {
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    model: Option<String>,
}

async fn api_models_serve(Json(body): Json<ModelServeRequest>) -> ApiResult {
    let request = crate::halo::local_models::ServeRequest {
        backend: crate::halo::local_models::LocalBackendType::Vllm,
        port: body.port,
        model: body.model,
    };
    let payload =
        tokio::task::spawn_blocking(move || crate::halo::local_models::serve_backend(request))
            .await
            .map_err(|e| {
                api_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("join model serve: {e}"),
                )
            })?
            .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true, "serve": payload })))
}

async fn api_models_stop() -> ApiResult {
    let payload = tokio::task::spawn_blocking(|| {
        crate::halo::local_models::stop_backend(crate::halo::local_models::LocalBackendType::Vllm)
    })
    .await
    .map_err(|e| {
        api_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("join model stop: {e}"),
        )
    })?
    .map_err(|e| api_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ "ok": true, "stop": payload })))
}
