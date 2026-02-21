use crate::multitenant::{MultiTenantError, MultiTenantNucleusDb, MultiTenantPolicy, TenantRole};
use crate::protocol::{NucleusDb, VcBackend};
use crate::security_utils::{ct_eq_32, domain_hash_32};
use crate::sheaf::coherence::LocalSection;
use crate::state::{Delta, State};
use crate::transparency::ct6962::hex_encode;
use crate::witness::{WitnessConfig, WitnessSignatureAlgorithm};
use axum::extract::{DefaultBodyLimit, Path, State as AxumState};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const CONTROL_TOKEN_HEADER: &str = "x-nucleusdb-control-token";
const API_DEFAULT_MAX_BODY_BYTES: usize = 1_048_576;
const API_DEFAULT_MAX_INITIAL_VALUES: usize = 100_000;
const API_DEFAULT_MAX_WRITES: usize = 50_000;
const CHECKPOINT_ROOT_ENV: &str = "NUCLEUSDB_CHECKPOINT_ROOT";

#[derive(Clone)]
struct ApiState {
    manager: Arc<MultiTenantNucleusDb>,
    control_token_hash: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Serialize)]
struct ApiError {
    ok: bool,
    error: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RegisterTenantRequest {
    tenant_id: String,
    auth_token: String,
    initial_values: Vec<u64>,
    backend: Option<String>,
    threshold: Option<usize>,
    witnesses: Option<Vec<String>>,
    witness_seed: Option<String>,
    witness_signing_algorithm: Option<String>,
    wal_path: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct RegisterTenantResponse {
    ok: bool,
    tenant_id: String,
}

#[derive(Clone, Debug, Deserialize)]
struct RegisterFromWalRequest {
    tenant_id: String,
    auth_token: String,
    wal_path: String,
    threshold: Option<usize>,
    witnesses: Option<Vec<String>>,
    witness_seed: Option<String>,
    witness_signing_algorithm: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct RegisterPrincipalRequest {
    actor_principal_id: Option<String>,
    actor_token: Option<String>,
    actor_auth_token: Option<String>,
    principal_id: String,
    principal_token: String,
    role: String,
}

#[derive(Clone, Debug, Serialize)]
struct OkResponse {
    ok: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct LocalSectionInput {
    lens_id: String,
    kv: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct CommitRequest {
    principal_id: Option<String>,
    token: Option<String>,
    auth_token: Option<String>,
    writes: Vec<(usize, u64)>,
    local_views: Option<Vec<LocalSectionInput>>,
}

#[derive(Clone, Debug, Serialize)]
struct CommitResponse {
    ok: bool,
    height: u64,
    state_root: String,
    sth_tree_size: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct QueryRequest {
    principal_id: Option<String>,
    token: Option<String>,
    auth_token: Option<String>,
    index: usize,
}

#[derive(Clone, Debug, Serialize)]
struct QueryResponse {
    ok: bool,
    index: usize,
    value: u64,
    state_root: String,
    verified: bool,
}

#[derive(Clone, Debug, Deserialize)]
struct SnapshotRequest {
    principal_id: Option<String>,
    token: Option<String>,
    auth_token: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct CheckpointRequest {
    principal_id: Option<String>,
    token: Option<String>,
    auth_token: Option<String>,
    checkpoint_label: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct SnapshotResponse {
    ok: bool,
    tenant_id: String,
    backend: String,
    entries: usize,
    state_values: Vec<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct HealthResponse {
    ok: bool,
    service: String,
}

fn token_hash(token: &str) -> [u8; 32] {
    domain_hash_32(b"nucleusdb.api.control-token.v1", token.as_bytes())
}

fn env_usize(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(default)
}

fn max_initial_values() -> usize {
    env_usize(
        "NUCLEUSDB_API_MAX_INITIAL_VALUES",
        API_DEFAULT_MAX_INITIAL_VALUES,
    )
}

fn max_writes() -> usize {
    env_usize("NUCLEUSDB_API_MAX_WRITES", API_DEFAULT_MAX_WRITES)
}

fn max_body_bytes() -> usize {
    env_usize("NUCLEUSDB_API_MAX_BODY_BYTES", API_DEFAULT_MAX_BODY_BYTES)
}

fn checkpoint_root() -> PathBuf {
    std::env::var(CHECKPOINT_ROOT_ENV)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("artifacts/nucleusdb/checkpoints"))
}

fn sanitize_label(label: Option<&str>) -> String {
    let src = label.unwrap_or("manual");
    let filtered: String = src
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(32)
        .collect();
    if filtered.is_empty() {
        "manual".to_string()
    } else {
        filtered
    }
}

fn checkpoint_path_for(tenant_id: &str, label: Option<&str>) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let tid: String = tenant_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let safe_tenant = if tid.is_empty() {
        "tenant".to_string()
    } else {
        tid
    };
    let safe_label = sanitize_label(label);
    checkpoint_root().join(format!("{}_{}_{}.redb", safe_tenant, safe_label, now))
}

fn require_control_token(
    headers: &HeaderMap,
    state: &ApiState,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let Some(expected_hash) = state.control_token_hash.as_ref() else {
        return Ok(());
    };
    let got = headers
        .get(CONTROL_TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(token_hash)
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ApiError {
                    ok: false,
                    error: format!("missing required header {CONTROL_TOKEN_HEADER}"),
                }),
            )
        })?;
    if !ct_eq_32(expected_hash, &got) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                ok: false,
                error: "invalid control token".to_string(),
            }),
        ));
    }
    Ok(())
}

fn resolve_principal_auth(
    principal_id: Option<String>,
    token: Option<String>,
    auth_token: Option<String>,
) -> Result<(String, String), (StatusCode, Json<ApiError>)> {
    if let Some(tok) = token {
        let principal = principal_id.unwrap_or_else(|| "admin".to_string());
        if tok.is_empty() {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiError {
                    ok: false,
                    error: "token must not be empty".to_string(),
                }),
            ));
        }
        return Ok((principal, tok));
    }
    if let Some(tok) = auth_token {
        if tok.is_empty() {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ApiError {
                    ok: false,
                    error: "auth_token must not be empty".to_string(),
                }),
            ));
        }
        return Ok(("admin".to_string(), tok));
    }
    Err((
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ApiError {
            ok: false,
            error: "missing auth: provide token or auth_token".to_string(),
        }),
    ))
}

fn parse_backend(backend: Option<&str>) -> Option<VcBackend> {
    match backend
        .unwrap_or("binary_merkle")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "ipa" => Some(VcBackend::Ipa),
        "kzg" => Some(VcBackend::Kzg),
        "binary_merkle" | "merkle" => Some(VcBackend::BinaryMerkle),
        _ => None,
    }
}

fn parse_sig_alg(tag: Option<&str>) -> Option<WitnessSignatureAlgorithm> {
    tag.map(str::trim)
        .and_then(WitnessSignatureAlgorithm::from_tag)
}

fn build_witness_cfg(
    threshold: Option<usize>,
    witnesses: Option<Vec<String>>,
    seed: Option<&str>,
    sig_alg: Option<&str>,
) -> Result<WitnessConfig, (StatusCode, Json<ApiError>)> {
    let threshold = threshold.unwrap_or(2);
    let witnesses = witnesses.unwrap_or_else(|| vec!["w1".into(), "w2".into(), "w3".into()]);
    let mut cfg = match seed {
        Some(s) => WitnessConfig::with_seed(threshold, witnesses, s),
        None => WitnessConfig::with_generated_keys(threshold, witnesses),
    };
    if let Some(alg) = parse_sig_alg(sig_alg) {
        cfg.signing_algorithm = alg;
        cfg.allowed_algorithms.insert(alg);
    } else if sig_alg.is_some() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                ok: false,
                error: "invalid witness_signing_algorithm".to_string(),
            }),
        ));
    }
    Ok(cfg)
}

fn to_local_sections(inputs: Option<Vec<LocalSectionInput>>) -> Vec<LocalSection> {
    inputs
        .unwrap_or_default()
        .into_iter()
        .map(|s| LocalSection {
            lens_id: s.lens_id,
            kv: s.kv,
        })
        .collect()
}

fn map_mt_error(err: MultiTenantError) -> (StatusCode, Json<ApiError>) {
    match err {
        MultiTenantError::TenantAuthFailed { tenant_id } => (
            StatusCode::UNAUTHORIZED,
            Json(ApiError {
                ok: false,
                error: format!("tenant auth failed: {tenant_id}"),
            }),
        ),
        MultiTenantError::TenantPrincipalNotFound {
            tenant_id,
            principal_id,
        } => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                ok: false,
                error: format!("principal not found: {tenant_id}/{principal_id}"),
            }),
        ),
        MultiTenantError::TenantPermissionDenied {
            tenant_id,
            principal_id,
            required,
            got,
        } => (
            StatusCode::FORBIDDEN,
            Json(ApiError {
                ok: false,
                error: format!(
                    "permission denied for {tenant_id}/{principal_id}: need {required:?}, got {got:?}"
                ),
            }),
        ),
        MultiTenantError::TenantNotFound { tenant_id } => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                ok: false,
                error: format!("tenant not found: {tenant_id}"),
            }),
        ),
        MultiTenantError::TenantAlreadyExists { tenant_id } => (
            StatusCode::CONFLICT,
            Json(ApiError {
                ok: false,
                error: format!("tenant already exists: {tenant_id}"),
            }),
        ),
        MultiTenantError::TenantPrincipalAlreadyExists {
            tenant_id,
            principal_id,
        } => (
            StatusCode::CONFLICT,
            Json(ApiError {
                ok: false,
                error: format!("principal already exists: {tenant_id}/{principal_id}"),
            }),
        ),
        MultiTenantError::TenantPolicyViolation { tenant_id, reason } => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                ok: false,
                error: format!("tenant policy violation ({tenant_id}): {reason}"),
            }),
        ),
        MultiTenantError::QueryIndexMissing { tenant_id, idx } => (
            StatusCode::NOT_FOUND,
            Json(ApiError {
                ok: false,
                error: format!("query index missing for tenant {tenant_id}: {idx}"),
            }),
        ),
        other => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiError {
                ok: false,
                error: format!("internal error: {other:?}"),
            }),
        ),
    }
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        ok: true,
        service: "nucleusdb-multitenant-api".to_string(),
    })
}

async fn list_tenants(
    AxumState(state): AxumState<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Vec<String>>, (StatusCode, Json<ApiError>)> {
    require_control_token(&headers, &state)?;
    state.manager.tenant_ids().map(Json).map_err(map_mt_error)
}

async fn register_tenant(
    AxumState(state): AxumState<ApiState>,
    headers: HeaderMap,
    Json(req): Json<RegisterTenantRequest>,
) -> Result<Json<RegisterTenantResponse>, (StatusCode, Json<ApiError>)> {
    require_control_token(&headers, &state)?;
    if req.initial_values.len() > max_initial_values() {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ApiError {
                ok: false,
                error: format!(
                    "initial_values too large: {} > {}",
                    req.initial_values.len(),
                    max_initial_values()
                ),
            }),
        ));
    }
    let cfg = build_witness_cfg(
        req.threshold,
        req.witnesses,
        req.witness_seed.as_deref(),
        req.witness_signing_algorithm.as_deref(),
    )?;
    let backend = parse_backend(req.backend.as_deref()).ok_or_else(|| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                ok: false,
                error: "invalid backend".to_string(),
            }),
        )
    })?;
    let db = NucleusDb::new(State::new(req.initial_values), backend, cfg);
    state
        .manager
        .register_tenant_with_wal_path(
            req.tenant_id.clone(),
            &req.auth_token,
            db,
            req.wal_path.map(PathBuf::from),
        )
        .map_err(map_mt_error)?;
    Ok(Json(RegisterTenantResponse {
        ok: true,
        tenant_id: req.tenant_id,
    }))
}

async fn register_from_wal(
    AxumState(state): AxumState<ApiState>,
    headers: HeaderMap,
    Json(req): Json<RegisterFromWalRequest>,
) -> Result<Json<RegisterTenantResponse>, (StatusCode, Json<ApiError>)> {
    require_control_token(&headers, &state)?;
    let cfg = build_witness_cfg(
        req.threshold,
        req.witnesses,
        req.witness_seed.as_deref(),
        req.witness_signing_algorithm.as_deref(),
    )?;
    let wal_path = PathBuf::from(req.wal_path);
    state
        .manager
        .register_tenant_from_wal(req.tenant_id.clone(), &req.auth_token, cfg, wal_path)
        .map_err(map_mt_error)?;
    Ok(Json(RegisterTenantResponse {
        ok: true,
        tenant_id: req.tenant_id,
    }))
}

async fn register_principal(
    AxumState(state): AxumState<ApiState>,
    Path(tenant_id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<RegisterPrincipalRequest>,
) -> Result<Json<OkResponse>, (StatusCode, Json<ApiError>)> {
    require_control_token(&headers, &state)?;
    let role = TenantRole::from_tag(&req.role).ok_or_else(|| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(ApiError {
                ok: false,
                error: "invalid role, expected reader|writer|admin".to_string(),
            }),
        )
    })?;
    let (actor_principal, actor_token) = resolve_principal_auth(
        req.actor_principal_id,
        req.actor_token,
        req.actor_auth_token,
    )?;
    state
        .manager
        .register_principal(
            &tenant_id,
            &actor_principal,
            &actor_token,
            &req.principal_id,
            &req.principal_token,
            role,
        )
        .map_err(map_mt_error)?;
    Ok(Json(OkResponse { ok: true }))
}

async fn commit(
    AxumState(state): AxumState<ApiState>,
    Path(tenant_id): Path<String>,
    Json(req): Json<CommitRequest>,
) -> Result<Json<CommitResponse>, (StatusCode, Json<ApiError>)> {
    if req.writes.len() > max_writes() {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ApiError {
                ok: false,
                error: format!("writes too large: {} > {}", req.writes.len(), max_writes()),
            }),
        ));
    }
    let (principal_id, token) =
        resolve_principal_auth(req.principal_id, req.token, req.auth_token)?;
    let local_views = to_local_sections(req.local_views);
    let entry = state
        .manager
        .commit_as(
            &tenant_id,
            &principal_id,
            &token,
            Delta::new(req.writes),
            &local_views,
        )
        .map_err(map_mt_error)?;
    Ok(Json(CommitResponse {
        ok: true,
        height: entry.height,
        state_root: hex_encode(&entry.state_root),
        sth_tree_size: entry.sth.tree_size,
    }))
}

async fn query(
    AxumState(state): AxumState<ApiState>,
    Path(tenant_id): Path<String>,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, (StatusCode, Json<ApiError>)> {
    let (principal_id, token) =
        resolve_principal_auth(req.principal_id, req.token, req.auth_token)?;
    let (value, proof, state_root) = state
        .manager
        .query_as(&tenant_id, &principal_id, &token, req.index)
        .map_err(map_mt_error)?;
    let verified = state
        .manager
        .verify_query_as(
            &tenant_id,
            &principal_id,
            &token,
            req.index,
            value,
            &proof,
            state_root,
        )
        .map_err(map_mt_error)?;
    Ok(Json(QueryResponse {
        ok: true,
        index: req.index,
        value,
        state_root: hex_encode(&state_root),
        verified,
    }))
}

async fn snapshot(
    AxumState(state): AxumState<ApiState>,
    Path(tenant_id): Path<String>,
    Json(req): Json<SnapshotRequest>,
) -> Result<Json<SnapshotResponse>, (StatusCode, Json<ApiError>)> {
    let (principal_id, token) =
        resolve_principal_auth(req.principal_id, req.token, req.auth_token)?;
    let snap = state
        .manager
        .snapshot_tenant_as(&tenant_id, &principal_id, &token)
        .map_err(map_mt_error)?;
    let backend = match snap.backend {
        VcBackend::Ipa => "ipa",
        VcBackend::Kzg => "kzg",
        VcBackend::BinaryMerkle => "binary_merkle",
    }
    .to_string();
    Ok(Json(SnapshotResponse {
        ok: true,
        tenant_id: snap.tenant_id,
        backend,
        entries: snap.entries,
        state_values: snap.state_values,
    }))
}

async fn checkpoint(
    AxumState(state): AxumState<ApiState>,
    Path(tenant_id): Path<String>,
    Json(req): Json<CheckpointRequest>,
) -> Result<Json<OkResponse>, (StatusCode, Json<ApiError>)> {
    let (principal_id, token) =
        resolve_principal_auth(req.principal_id, req.token, req.auth_token)?;
    state
        .manager
        .checkpoint_tenant(
            &tenant_id,
            &principal_id,
            &token,
            checkpoint_path_for(&tenant_id, req.checkpoint_label.as_deref()),
        )
        .map_err(map_mt_error)?;
    Ok(Json(OkResponse { ok: true }))
}

pub fn app_with_manager(manager: Arc<MultiTenantNucleusDb>) -> Router {
    let control_token_hash = std::env::var("NUCLEUSDB_API_CONTROL_TOKEN")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| token_hash(v.trim()));
    let state = ApiState {
        manager,
        control_token_hash,
    };
    Router::new()
        .layer(DefaultBodyLimit::max(max_body_bytes()))
        .route("/v1/health", get(health))
        .route("/v1/tenants", get(list_tenants))
        .route("/v1/tenants/register", post(register_tenant))
        .route("/v1/tenants/register_from_wal", post(register_from_wal))
        .route(
            "/v1/tenants/{tenant_id}/principals/register",
            post(register_principal),
        )
        .route("/v1/tenants/{tenant_id}/commit", post(commit))
        .route("/v1/tenants/{tenant_id}/query", post(query))
        .route("/v1/tenants/{tenant_id}/snapshot", post(snapshot))
        .route("/v1/tenants/{tenant_id}/checkpoint", post(checkpoint))
        .with_state(state)
}

pub async fn serve_multitenant(addr: SocketAddr, policy: MultiTenantPolicy) -> std::io::Result<()> {
    let manager = Arc::new(MultiTenantNucleusDb::new(policy));
    let app = app_with_manager(manager);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await
}
