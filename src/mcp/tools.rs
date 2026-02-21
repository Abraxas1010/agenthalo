use crate::cli::{default_witness_cfg, parse_backend};
use crate::persistence::{init_wal, load_wal, truncate_wal};
use crate::protocol::{NucleusDb, QueryProof, VcBackend};
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::transparency::ct6962::hex_encode;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
struct ServiceState {
    db: NucleusDb,
    db_path: PathBuf,
    wal_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct NucleusDbMcpService {
    state: Arc<Mutex<ServiceState>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    pub db_path: Option<String>,
    pub backend: Option<String>,
    pub wal_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenDatabaseRequest {
    pub db_path: Option<String>,
    pub wal_path: Option<String>,
    pub prefer_wal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteSqlRequest {
    pub sql: String,
    pub persist: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryRequest {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryRangeRequest {
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyRequest {
    pub key: String,
    pub expected_value: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryRequest {
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointRequest {
    pub db_path: Option<String>,
    pub wal_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OperationStatus {
    pub ok: bool,
    pub message: String,
    pub db_path: String,
    pub wal_path: String,
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SqlExecutionResponse {
    pub status: String,
    pub message: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryResultRow {
    pub key: String,
    pub index: usize,
    pub value: u64,
    pub verified: bool,
    pub proof_kind: String,
    pub state_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryRangeResponse {
    pub pattern: String,
    pub rows: Vec<QueryResultRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyResponse {
    pub key: String,
    pub verified: bool,
    pub reason: String,
    pub value: Option<u64>,
    pub expected_value: Option<u64>,
    pub state_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatusResponse {
    pub db_path: String,
    pub wal_path: String,
    pub backend: String,
    pub state_len: usize,
    pub entries: usize,
    pub key_count: usize,
    pub sth_tree_size: u64,
    pub sth_root: String,
    pub sth_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryEntryResponse {
    pub height: u64,
    pub state_root: String,
    pub tree_size: u64,
    pub timestamp: u64,
    pub backend: String,
    pub witness_algorithm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryResponse {
    pub entries: Vec<HistoryEntryResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExportResponse {
    pub key_count: usize,
    pub json: String,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for NucleusDbMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "NucleusDB MCP server for verifiable SQL execution, query proofs, and release-safe checkpointing."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[tool_router(router = tool_router)]
impl NucleusDbMcpService {
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self, String> {
        let db_path = db_path.as_ref().to_path_buf();
        let wal_path = Self::default_wal_path(&db_path);
        let state = if db_path.exists() {
            Self::load_state(db_path, wal_path, false)?
        } else {
            Self::create_state(db_path, wal_path, VcBackend::BinaryMerkle)?
        };
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            tool_router: Self::tool_router(),
        })
    }

    fn default_wal_path(db_path: &Path) -> PathBuf {
        let mut wal = db_path.to_path_buf();
        wal.set_extension("wal");
        wal
    }

    fn backend_label(backend: &VcBackend) -> &'static str {
        match backend {
            VcBackend::Ipa => "ipa",
            VcBackend::Kzg => "kzg",
            VcBackend::BinaryMerkle => "binary_merkle",
        }
    }

    fn proof_kind_name(proof: &QueryProof) -> &'static str {
        match proof {
            QueryProof::Ipa(_) => "ipa",
            QueryProof::Kzg(_) => "kzg",
            QueryProof::BinaryMerkle(_) => "binary_merkle",
        }
    }

    fn create_state(
        db_path: PathBuf,
        wal_path: PathBuf,
        backend: VcBackend,
    ) -> Result<ServiceState, String> {
        let cfg = default_witness_cfg();
        let db = NucleusDb::new(State::new(vec![]), backend, cfg);
        db.save_persistent(&db_path)
            .map_err(|e| format!("failed to save snapshot {}: {e:?}", db_path.display()))?;
        init_wal(&wal_path, &db)
            .map_err(|e| format!("failed to initialize WAL {}: {e:?}", wal_path.display()))?;
        Ok(ServiceState {
            db,
            db_path,
            wal_path,
        })
    }

    fn load_state(
        db_path: PathBuf,
        wal_path: PathBuf,
        prefer_wal: bool,
    ) -> Result<ServiceState, String> {
        let cfg = default_witness_cfg();
        let db = if prefer_wal && wal_path.exists() {
            load_wal(&wal_path, cfg)
                .map_err(|e| format!("failed to load WAL {}: {e:?}", wal_path.display()))?
        } else if db_path.exists() {
            NucleusDb::load_persistent(&db_path, cfg)
                .map_err(|e| format!("failed to load snapshot {}: {e:?}", db_path.display()))?
        } else {
            return Err(format!(
                "database file does not exist: {}",
                db_path.display()
            ));
        };
        init_wal(&wal_path, &db)
            .map_err(|e| format!("failed to initialize WAL {}: {e:?}", wal_path.display()))?;
        Ok(ServiceState {
            db,
            db_path,
            wal_path,
        })
    }

    fn query_row(db: &NucleusDb, key: &str, idx: usize) -> Result<QueryResultRow, String> {
        let Some((value, proof, root)) = db.query(idx) else {
            return Err(format!("no value for key '{key}'"));
        };
        let verified = db.verify_query(idx, value, &proof, root);
        Ok(QueryResultRow {
            key: key.to_string(),
            index: idx,
            value,
            verified,
            proof_kind: Self::proof_kind_name(&proof).to_string(),
            state_root: hex_encode(&root),
        })
    }

    #[tool(
        name = "nucleusdb_create_database",
        description = "Create a new NucleusDB snapshot and WAL with selected backend."
    )]
    pub async fn create_database(
        &self,
        Parameters(req): Parameters<CreateDatabaseRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        let current_db_path = { self.state.lock().await.db_path.clone() };
        let db_path = req.db_path.map(PathBuf::from).unwrap_or(current_db_path);
        let wal_path = req
            .wal_path
            .map(PathBuf::from)
            .unwrap_or_else(|| Self::default_wal_path(&db_path));
        let backend = parse_backend(req.backend.as_deref().unwrap_or("merkle"))
            .map_err(|e| McpError::invalid_params(e, None))?;
        let state = Self::create_state(db_path.clone(), wal_path.clone(), backend.clone())
            .map_err(|e| McpError::internal_error(e, None))?;
        let mut guard = self.state.lock().await;
        *guard = state;
        Ok(Json(OperationStatus {
            ok: true,
            message: "database created".to_string(),
            db_path: db_path.display().to_string(),
            wal_path: wal_path.display().to_string(),
            backend: Self::backend_label(&backend).to_string(),
        }))
    }

    #[tool(
        name = "nucleusdb_open_database",
        description = "Open an existing NucleusDB snapshot (or WAL) and switch active server state."
    )]
    pub async fn open_database(
        &self,
        Parameters(req): Parameters<OpenDatabaseRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        let current_db_path = { self.state.lock().await.db_path.clone() };
        let db_path = req.db_path.map(PathBuf::from).unwrap_or(current_db_path);
        let wal_path = req
            .wal_path
            .map(PathBuf::from)
            .unwrap_or_else(|| Self::default_wal_path(&db_path));
        let prefer_wal = req.prefer_wal.unwrap_or(false);
        let state = Self::load_state(db_path.clone(), wal_path.clone(), prefer_wal)
            .map_err(|e| McpError::invalid_params(e, None))?;
        let backend = state.db.backend.clone();
        let mut guard = self.state.lock().await;
        *guard = state;
        Ok(Json(OperationStatus {
            ok: true,
            message: "database opened".to_string(),
            db_path: db_path.display().to_string(),
            wal_path: wal_path.display().to_string(),
            backend: Self::backend_label(&backend).to_string(),
        }))
    }

    #[tool(
        name = "nucleusdb_execute_sql",
        description = "Execute SQL statements against the active database."
    )]
    pub async fn execute_sql(
        &self,
        Parameters(req): Parameters<ExecuteSqlRequest>,
    ) -> Result<Json<SqlExecutionResponse>, McpError> {
        let mut guard = self.state.lock().await;
        let mut exec = SqlExecutor::new(&mut guard.db);
        let (response, success) = match exec.execute(&req.sql) {
            SqlResult::Rows { columns, rows } => (
                SqlExecutionResponse {
                    status: "rows".to_string(),
                    message: format!("returned {} row(s)", rows.len()),
                    columns,
                    rows,
                },
                true,
            ),
            SqlResult::Ok { message } => (
                SqlExecutionResponse {
                    status: "ok".to_string(),
                    message,
                    columns: Vec::new(),
                    rows: Vec::new(),
                },
                true,
            ),
            SqlResult::Error { message } => (
                SqlExecutionResponse {
                    status: "error".to_string(),
                    message,
                    columns: Vec::new(),
                    rows: Vec::new(),
                },
                false,
            ),
        };
        if success && req.persist.unwrap_or(true) {
            guard.db.save_persistent(&guard.db_path).map_err(|e| {
                McpError::internal_error(format!("failed to persist snapshot: {e:?}"), None)
            })?;
        }
        Ok(Json(response))
    }

    #[tool(
        name = "nucleusdb_query",
        description = "Query a single key and return value with proof verification status."
    )]
    pub async fn query(
        &self,
        Parameters(req): Parameters<QueryRequest>,
    ) -> Result<Json<QueryResultRow>, McpError> {
        let guard = self.state.lock().await;
        let idx =
            guard.db.keymap.get(&req.key).ok_or_else(|| {
                McpError::invalid_params(format!("unknown key '{}'", req.key), None)
            })?;
        let row = Self::query_row(&guard.db, &req.key, idx)
            .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(row))
    }

    #[tool(
        name = "nucleusdb_query_range",
        description = "Query keys matching an exact key or prefix pattern like 'prefix%'."
    )]
    pub async fn query_range(
        &self,
        Parameters(req): Parameters<QueryRangeRequest>,
    ) -> Result<Json<QueryRangeResponse>, McpError> {
        let guard = self.state.lock().await;
        let mut rows = Vec::new();
        for (key, idx) in guard.db.keymap.keys_matching(&req.pattern) {
            if let Ok(row) = Self::query_row(&guard.db, &key, idx) {
                rows.push(row);
            }
        }
        Ok(Json(QueryRangeResponse {
            pattern: req.pattern,
            rows,
        }))
    }

    #[tool(
        name = "nucleusdb_verify",
        description = "Verify a key's proof against current state root, with optional expected value check."
    )]
    pub async fn verify(
        &self,
        Parameters(req): Parameters<VerifyRequest>,
    ) -> Result<Json<VerifyResponse>, McpError> {
        let guard = self.state.lock().await;
        let Some(idx) = guard.db.keymap.get(&req.key) else {
            return Ok(Json(VerifyResponse {
                key: req.key,
                verified: false,
                reason: "unknown_key".to_string(),
                value: None,
                expected_value: req.expected_value,
                state_root: None,
            }));
        };
        let Some((value, proof, root)) = guard.db.query(idx) else {
            return Ok(Json(VerifyResponse {
                key: req.key,
                verified: false,
                reason: "missing_value".to_string(),
                value: None,
                expected_value: req.expected_value,
                state_root: None,
            }));
        };
        let proof_ok = guard.db.verify_query(idx, value, &proof, root);
        let expected_ok = req.expected_value.map(|v| v == value).unwrap_or(true);
        let verified = proof_ok && expected_ok;
        let reason = if verified {
            "ok"
        } else if !proof_ok {
            "proof_verification_failed"
        } else {
            "unexpected_value"
        };
        Ok(Json(VerifyResponse {
            key: req.key,
            verified,
            reason: reason.to_string(),
            value: Some(value),
            expected_value: req.expected_value,
            state_root: Some(hex_encode(&root)),
        }))
    }

    #[tool(
        name = "nucleusdb_status",
        description = "Return current DB status including backend, sizes, and Signed Tree Head metadata."
    )]
    pub async fn status(&self) -> Result<Json<StatusResponse>, McpError> {
        let guard = self.state.lock().await;
        let (sth_tree_size, sth_root, sth_timestamp) = match guard.db.current_sth() {
            Some(sth) => (
                sth.tree_size,
                hex_encode(&sth.root_hash),
                sth.timestamp_unix_secs,
            ),
            None => (0, String::new(), 0),
        };
        Ok(Json(StatusResponse {
            db_path: guard.db_path.display().to_string(),
            wal_path: guard.wal_path.display().to_string(),
            backend: Self::backend_label(&guard.db.backend).to_string(),
            state_len: guard.db.state.values.len(),
            entries: guard.db.entries.len(),
            key_count: guard.db.keymap.len(),
            sth_tree_size,
            sth_root,
            sth_timestamp,
        }))
    }

    #[tool(
        name = "nucleusdb_history",
        description = "List commit history from newest to oldest with backend and witness metadata."
    )]
    pub async fn history(
        &self,
        Parameters(req): Parameters<HistoryRequest>,
    ) -> Result<Json<HistoryResponse>, McpError> {
        let guard = self.state.lock().await;
        let mut entries = guard
            .db
            .entries
            .iter()
            .map(|e| HistoryEntryResponse {
                height: e.height,
                state_root: hex_encode(&e.state_root),
                tree_size: e.sth.tree_size,
                timestamp: e.sth.timestamp_unix_secs,
                backend: e.vc_backend_id.clone(),
                witness_algorithm: e.witness_signature_algorithm.clone(),
            })
            .collect::<Vec<_>>();
        entries.reverse();
        if let Some(limit) = req.limit {
            entries.truncate(limit);
        }
        Ok(Json(HistoryResponse { entries }))
    }

    #[tool(
        name = "nucleusdb_export",
        description = "Export current key/value state as pretty JSON."
    )]
    pub async fn export(&self) -> Result<Json<ExportResponse>, McpError> {
        let guard = self.state.lock().await;
        let mut payload = std::collections::BTreeMap::<String, u64>::new();
        for (key, idx) in guard.db.keymap.all_keys() {
            let value = guard.db.state.values.get(idx).copied().unwrap_or(0);
            payload.insert(key.to_string(), value);
        }
        let json = serde_json::to_string_pretty(&payload).map_err(|e| {
            McpError::internal_error(format!("failed to encode export JSON: {e}"), None)
        })?;
        Ok(Json(ExportResponse {
            key_count: payload.len(),
            json,
        }))
    }

    #[tool(
        name = "nucleusdb_checkpoint",
        description = "Persist a snapshot and atomically truncate WAL for the active database."
    )]
    pub async fn checkpoint(
        &self,
        Parameters(req): Parameters<CheckpointRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        let mut guard = self.state.lock().await;
        let db_path = req
            .db_path
            .map(PathBuf::from)
            .unwrap_or_else(|| guard.db_path.clone());
        let wal_path = req
            .wal_path
            .map(PathBuf::from)
            .unwrap_or_else(|| guard.wal_path.clone());
        guard.db.save_persistent(&db_path).map_err(|e| {
            McpError::internal_error(format!("failed to save snapshot: {e:?}"), None)
        })?;
        truncate_wal(&wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to truncate WAL: {e:?}"), None)
        })?;
        guard.db_path = db_path.clone();
        guard.wal_path = wal_path.clone();
        Ok(Json(OperationStatus {
            ok: true,
            message: "checkpoint completed".to_string(),
            db_path: db_path.display().to_string(),
            wal_path: wal_path.display().to_string(),
            backend: Self::backend_label(&guard.db.backend).to_string(),
        }))
    }
}
