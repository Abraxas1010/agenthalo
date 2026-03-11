use crate::config;
use crate::discord::status as discord_status;
use crate::persistence::{default_wal_path, init_wal, load_wal, persist_snapshot_and_sync_wal};
use crate::protocol::{NucleusDb, VcBackend};
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::{cli::default_witness_cfg, cli::parse_backend};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

struct ServiceState {
    db: NucleusDb,
    db_path: PathBuf,
    wal_path: PathBuf,
    discord_db_path: PathBuf,
}

#[derive(Clone)]
pub struct NucleusDbMcpService {
    state: Arc<Mutex<ServiceState>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    pub db_path: String,
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
pub struct ExportRequest {
    pub format: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscordSearchRequest {
    pub query: String,
    pub channel_id: Option<String>,
    pub limit: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscordVerifyRequest {
    pub message_id: String,
    pub channel_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DiscordExportRequest {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResponse {
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

impl NucleusDbMcpService {
    pub fn new(db_path: &str) -> Result<Self, String> {
        let db_path = PathBuf::from(db_path);
        let discord_db_path = std::env::var("NUCLEUSDB_DISCORD_DB_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| db_path.clone());
        let wal_path = default_wal_path(&db_path);
        let cfg = default_witness_cfg();
        let db = if db_path.exists() {
            NucleusDb::load_persistent(&db_path, cfg)
                .map_err(|e| format!("load snapshot {}: {e:?}", db_path.display()))?
        } else {
            let db = NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, cfg);
            db.save_persistent(&db_path)
                .map_err(|e| format!("save snapshot {}: {e:?}", db_path.display()))?;
            init_wal(&wal_path, &db)
                .map_err(|e| format!("init WAL {}: {e:?}", wal_path.display()))?;
            db
        };
        Ok(Self {
            state: Arc::new(Mutex::new(ServiceState {
                db,
                db_path,
                wal_path,
                discord_db_path,
            })),
            tool_router: Self::tool_router(),
        })
    }

    fn key_to_index(db: &NucleusDb, key: &str) -> Result<usize, McpError> {
        db.keymap
            .get(key)
            .ok_or_else(|| McpError::invalid_params("unknown key", None))
    }

    fn render_sql(out: SqlResult) -> serde_json::Value {
        match out {
            SqlResult::Rows { columns, rows } => {
                serde_json::json!({"kind": "rows", "columns": columns, "rows": rows})
            }
            SqlResult::Ok { message } => serde_json::json!({"kind": "ok", "message": message}),
            SqlResult::Error { message } => {
                serde_json::json!({"kind": "error", "message": message})
            }
        }
    }

    fn json_response(value: serde_json::Value) -> Result<Json<ToolResponse>, McpError> {
        let serde_json::Value::Object(map) = value else {
            return Err(McpError::internal_error(
                "tool response must serialize to a JSON object".to_string(),
                None,
            ));
        };
        Ok(Json(ToolResponse {
            fields: map.into_iter().collect(),
        }))
    }

    async fn discord_recorder(&self) -> crate::discord::recorder::DiscordRecorder {
        let state = self.state.lock().await;
        crate::discord::recorder::DiscordRecorder::new(state.discord_db_path.clone())
    }
}

#[tool_router(router = tool_router)]
impl NucleusDbMcpService {
    #[tool(description = "List the available NucleusDB and Discord tools.")]
    async fn help(&self) -> Result<Json<ToolResponse>, McpError> {
        Self::json_response(serde_json::json!({
            "tools": [
                "create_database","open_database","execute_sql","query","query_range","verify","status","history","export","checkpoint","help",
                "discord_status","discord_search","discord_verify","discord_integrity","discord_export"
            ]
        }))
    }

    #[tool(description = "Create a new NucleusDB database.")]
    async fn create_database(
        &self,
        Parameters(req): Parameters<CreateDatabaseRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let backend = req.backend.as_deref().unwrap_or("merkle");
        let backend = parse_backend(backend).map_err(|e| McpError::invalid_params(e, None))?;
        let db_path = PathBuf::from(&req.db_path);
        let wal_path = req
            .wal_path
            .map(PathBuf::from)
            .unwrap_or_else(|| default_wal_path(&db_path));
        let db = NucleusDb::new(State::new(vec![]), backend, default_witness_cfg());
        db.save_persistent(&db_path)
            .map_err(|e| McpError::internal_error(format!("save snapshot: {e:?}"), None))?;
        init_wal(&wal_path, &db)
            .map_err(|e| McpError::internal_error(format!("init WAL: {e:?}"), None))?;
        Self::json_response(
            serde_json::json!({"ok": true, "db_path": db_path, "wal_path": wal_path}),
        )
    }

    #[tool(description = "Open an existing database and make it the active MCP target.")]
    async fn open_database(
        &self,
        Parameters(req): Parameters<OpenDatabaseRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let mut state = self.state.lock().await;
        let db_path = req
            .db_path
            .map(PathBuf::from)
            .unwrap_or_else(|| state.db_path.clone());
        let wal_path = req
            .wal_path
            .map(PathBuf::from)
            .unwrap_or_else(|| default_wal_path(&db_path));
        let cfg = default_witness_cfg();
        let db = if req.prefer_wal.unwrap_or(false) && wal_path.exists() {
            load_wal(&wal_path, cfg)
                .map_err(|e| McpError::internal_error(format!("load WAL: {e:?}"), None))?
        } else {
            NucleusDb::load_persistent(&db_path, cfg)
                .map_err(|e| McpError::internal_error(format!("load snapshot: {e:?}"), None))?
        };
        state.db = db;
        state.db_path = db_path.clone();
        state.wal_path = wal_path.clone();
        Self::json_response(
            serde_json::json!({"ok": true, "db_path": db_path, "wal_path": wal_path}),
        )
    }

    #[tool(description = "Execute SQL against the active database.")]
    async fn execute_sql(
        &self,
        Parameters(req): Parameters<ExecuteSqlRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let mut state = self.state.lock().await;
        let mut exec = SqlExecutor::new(&mut state.db);
        let out = exec.execute(&req.sql);
        let committed = exec.committed();
        if committed || req.persist.unwrap_or(true) {
            persist_snapshot_and_sync_wal(&state.db_path, &state.wal_path, &state.db)
                .map_err(|e| McpError::internal_error(format!("persist: {e:?}"), None))?;
        }
        Self::json_response(Self::render_sql(out))
    }

    #[tool(description = "Query an exact key and return its proof.")]
    async fn query(
        &self,
        Parameters(req): Parameters<QueryRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        let idx = Self::key_to_index(&state.db, &req.key)?;
        let (value, proof, root) = state
            .db
            .query(idx)
            .ok_or_else(|| McpError::invalid_params("query returned no result", None))?;
        Self::json_response(
            serde_json::json!({"key": req.key, "index": idx, "value": value, "root": crate::transparency::ct6962::hex_encode(&root), "proof": proof}),
        )
    }

    #[tool(description = "Query keys by prefix pattern, for example msg:123:%.")]
    async fn query_range(
        &self,
        Parameters(req): Parameters<QueryRangeRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        let pattern = req.pattern.trim_end_matches('%');
        let rows: Vec<_> = state
            .db
            .keymap
            .all_keys()
            .filter(|(key, _)| key.starts_with(pattern))
            .map(|(key, idx)| {
                let value = state.db.state.values.get(idx).copied().unwrap_or(0);
                serde_json::json!({"key": key, "index": idx, "value": value})
            })
            .collect();
        Self::json_response(serde_json::json!({"count": rows.len(), "rows": rows}))
    }

    #[tool(description = "Verify an exact key proof, optionally against an expected value.")]
    async fn verify(
        &self,
        Parameters(req): Parameters<VerifyRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        let idx = Self::key_to_index(&state.db, &req.key)?;
        let (value, proof, root) = state
            .db
            .query(idx)
            .ok_or_else(|| McpError::invalid_params("query returned no result", None))?;
        let verified = state.db.verify_query(idx, value, &proof, root);
        let expected_match = req.expected_value.map(|v| v == value);
        Self::json_response(
            serde_json::json!({"key": req.key, "value": value, "verified": verified, "expected_match": expected_match}),
        )
    }

    #[tool(description = "Return database status.")]
    async fn status(&self) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        Self::json_response(serde_json::json!({
            "db_path": state.db_path,
            "wal_path": state.wal_path,
            "backend": format!("{:?}", state.db.backend),
            "entries": state.db.entries.len(),
            "keys": state.db.keymap.len(),
            "write_mode": format!("{:?}", state.db.write_mode()),
            "seals": state.db.monotone_seals().len()
        }))
    }

    #[tool(description = "Return recent commit history.")]
    async fn history(
        &self,
        Parameters(req): Parameters<HistoryRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        let limit = req.limit.unwrap_or(25);
        let rows: Vec<_> = state
            .db
            .entries
            .iter()
            .rev()
            .take(limit)
            .map(|e| {
                serde_json::json!({
                    "height": e.height,
                    "root": crate::transparency::ct6962::hex_encode(&e.state_root),
                    "tree_size": e.sth.tree_size,
                    "timestamp": e.sth.timestamp_unix_secs
                })
            })
            .collect();
        Self::json_response(serde_json::json!({"count": rows.len(), "rows": rows}))
    }

    #[tool(description = "Export the active database as JSON.")]
    async fn export(
        &self,
        Parameters(_req): Parameters<ExportRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        let rows: Vec<_> = state
            .db
            .keymap
            .all_keys()
            .map(|(key, idx)| {
                serde_json::json!({
                    "key": key,
                    "index": idx,
                    "value": state.db.state.values.get(idx).copied().unwrap_or(0),
                    "type": state.db.type_map.get(key).as_str(),
                })
            })
            .collect();
        Self::json_response(serde_json::json!({"entries": rows}))
    }

    #[tool(description = "Persist a checkpoint of the active database.")]
    async fn checkpoint(&self) -> Result<Json<ToolResponse>, McpError> {
        let state = self.state.lock().await;
        let ts = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
        let dir = config::discord_export_dir();
        std::fs::create_dir_all(&dir)
            .map_err(|e| McpError::internal_error(format!("create export dir: {e}"), None))?;
        let path = dir.join(format!("checkpoint_{ts}.ndb"));
        state
            .db
            .save_persistent(&path)
            .map_err(|e| McpError::internal_error(format!("save checkpoint: {e:?}"), None))?;
        Self::json_response(serde_json::json!({"ok": true, "path": path}))
    }

    #[tool(description = "Return Discord bot status and channel counts.")]
    async fn discord_status(&self) -> Result<Json<ToolResponse>, McpError> {
        let status =
            discord_status::load_status().map_err(|e| McpError::internal_error(e, None))?;
        Self::json_response(
            serde_json::to_value(status)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
        )
    }

    #[tool(description = "Search recorded Discord messages by content.")]
    async fn discord_search(
        &self,
        Parameters(req): Parameters<DiscordSearchRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let recorder = self.discord_recorder().await;
        let rows = recorder
            .search(
                &req.query,
                req.channel_id.as_deref(),
                req.limit.unwrap_or(25),
            )
            .map_err(|e| McpError::internal_error(e, None))?;
        Self::json_response(serde_json::json!({"count": rows.len(), "rows": rows}))
    }

    #[tool(description = "Verify a specific Discord message record.")]
    async fn discord_verify(
        &self,
        Parameters(req): Parameters<DiscordVerifyRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let recorder = self.discord_recorder().await;
        match recorder
            .verify_message(&req.channel_id, &req.message_id)
            .map_err(|e| McpError::internal_error(e, None))?
        {
            Some((verified, value)) => {
                let key = format!("msg:{}:{}", req.channel_id, req.message_id);
                Self::json_response(
                    serde_json::json!({"key": key, "verified": verified, "value": value}),
                )
            }
            None => Err(McpError::invalid_params("message not found", None)),
        }
    }

    #[tool(description = "Verify the full Discord append-only seal chain.")]
    async fn discord_integrity(&self) -> Result<Json<ToolResponse>, McpError> {
        let recorder = self.discord_recorder().await;
        let (append_only, seal_count) = recorder
            .integrity_summary()
            .map_err(|e| McpError::internal_error(e, None))?;
        Self::json_response(
            serde_json::json!({"ok": append_only && seal_count > 0, "seal_count": seal_count, "write_mode": "AppendOnly"}),
        )
    }

    #[tool(description = "Export all Discord records for a channel.")]
    async fn discord_export(
        &self,
        Parameters(req): Parameters<DiscordExportRequest>,
    ) -> Result<Json<ToolResponse>, McpError> {
        let recorder = self.discord_recorder().await;
        let rows = recorder
            .export_channel(&req.channel_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        Self::json_response(
            serde_json::json!({"channel_id": req.channel_id, "count": rows.len(), "rows": rows}),
        )
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for NucleusDbMcpService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "nucleusdb".to_string(),
                title: Some("NucleusDB MCP Server".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some(
                    "Standalone NucleusDB MCP server with Discord-recording tools.".to_string(),
                ),
                icons: None,
                website_url: Some("https://github.com/Abraxas1010/nucleusdb".to_string()),
            },
            instructions: Some(
                "Use help first to discover the standalone NucleusDB and Discord tool surface."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}
