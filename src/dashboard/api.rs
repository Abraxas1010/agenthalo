use super::DashboardState;
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
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

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
