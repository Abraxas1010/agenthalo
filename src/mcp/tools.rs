use crate::config;
use crate::container::launcher::{
    container_logs as launcher_container_logs, container_status as launcher_container_status,
    launch_container, list_sessions as list_container_sessions,
    stop_container as launcher_stop_container, MeshConfig, RunConfig,
};
use crate::container::{
    current_container_id, mesh_auth_token, AgentHookup, AgentHookupKind, AgentResponse,
    ApiAgentHookup, CliAgentHookup, ContainerAgentLock, LocalModelHookup, ReusePolicy,
};
use crate::discord::status as discord_status;
use crate::orchestrator::subsidiary_registry::{
    SubsidiaryRecord, SubsidiaryRegistry, SubsidiaryTaskRecord,
};
use crate::orchestrator::{
    ContainerHookupRequest, LaunchAgentRequest as OrchLaunchRequest, Orchestrator,
    SendTaskRequest as OrchSendTaskRequest, StopRequest as OrchStopRequest,
};
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
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;

struct ServiceState {
    db: NucleusDb,
    db_path: PathBuf,
    wal_path: PathBuf,
    discord_db_path: PathBuf,
    orchestrator: Orchestrator,
    _vault: Option<Arc<crate::halo::vault::Vault>>,
    pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
    _governor_registry: Arc<crate::halo::governor_registry::GovernorRegistry>,
    container_runtime: Arc<tokio::sync::Mutex<ContainerRuntime>>,
}

struct ContainerRuntime {
    active: Option<ActiveContainerHookup>,
}

static GLOBAL_CONTAINER_RUNTIME: OnceLock<Arc<tokio::sync::Mutex<ContainerRuntime>>> =
    OnceLock::new();

fn shared_container_runtime() -> Arc<tokio::sync::Mutex<ContainerRuntime>> {
    GLOBAL_CONTAINER_RUNTIME
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(ContainerRuntime { active: None })))
        .clone()
}

struct ActiveContainerHookup {
    hookup: Arc<dyn AgentHookup>,
}

struct SharedServiceRuntime {
    vault: Option<Arc<crate::halo::vault::Vault>>,
    pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
    governor_registry: Arc<crate::halo::governor_registry::GovernorRegistry>,
    orchestrator: Orchestrator,
}

const CONTAINER_LIST_LOCK_STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const SUBSIDIARY_PEER_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);
const SUBSIDIARY_PEER_REGISTRATION_POLL_INTERVAL: Duration = Duration::from_millis(250);

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
pub struct ContainerLaunchRequest {
    pub image: String,
    pub agent_id: String,
    pub command: Vec<String>,
    pub runtime_runsc: Option<bool>,
    pub host_sock: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub mesh: Option<ContainerMeshRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerMeshRequest {
    pub enabled: Option<bool>,
    pub mcp_port: Option<u16>,
    pub registry_volume: Option<String>,
    pub agent_did: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLaunchResponse {
    pub session_id: String,
    pub container_id: String,
    pub image: String,
    pub agent_id: String,
    pub host_sock: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerListResponse {
    pub count: usize,
    pub sessions: Vec<ContainerSessionView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerSessionView {
    pub session_id: String,
    pub container_id: String,
    pub image: String,
    pub agent_id: String,
    pub host_sock: String,
    pub started_at_unix: u64,
    pub mesh_port: Option<u16>,
    #[serde(default)]
    pub lock_state: Option<String>,
    #[serde(default)]
    pub reuse_policy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ContainerLockStatusView {
    state: String,
    reuse_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerStatusRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerStatusResponse {
    pub session_id: String,
    pub status: String,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerInitializeRequest {
    pub hookup: ContainerHookupRequest,
    #[serde(default)]
    pub reuse_policy: Option<ReusePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerInitializeResponse {
    pub container_id: String,
    pub state: String,
    pub agent_id: String,
    pub trace_session_id: Option<String>,
    pub reuse_policy: ReusePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerAgentPromptRequest {
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ContainerDeinitializeRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerDeinitializeResponse {
    pub container_id: String,
    pub state: String,
    pub trace_session_id: Option<String>,
    pub reuse_policy: ReusePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ContainerLockStatusRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLockStatusResponse {
    pub container_id: String,
    pub state: String,
    pub reuse_policy: ReusePolicy,
    pub lock: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerStopRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerStopResponse {
    pub session_id: String,
    pub stopped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLogsRequest {
    pub session_id: String,
    pub follow: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLogsResponse {
    pub session_id: String,
    pub follow: bool,
    pub logs: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryProvisionRequest {
    pub operator_agent_id: String,
    pub image: String,
    pub agent_id: String,
    #[serde(default)]
    pub command: Vec<String>,
    pub runtime_runsc: Option<bool>,
    pub host_sock: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub mesh: Option<ContainerMeshRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryProvisionResponse {
    pub operator_agent_id: String,
    pub session_id: String,
    pub container_id: String,
    pub agent_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryInitializeRequest {
    pub operator_agent_id: String,
    pub session_id: String,
    pub hookup: ContainerHookupRequest,
    #[serde(default)]
    pub reuse_policy: Option<ReusePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryInitializeResponse {
    pub operator_agent_id: String,
    pub session_id: String,
    pub container_id: String,
    pub state: String,
    pub agent_id: String,
    pub trace_session_id: Option<String>,
    pub reuse_policy: ReusePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiarySendTaskRequest {
    pub operator_agent_id: String,
    pub session_id: String,
    pub prompt: String,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryGetResultRequest {
    pub operator_agent_id: String,
    pub task_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryTaskResponse {
    pub task_id: String,
    pub operator_agent_id: String,
    pub session_id: String,
    pub status: String,
    pub model: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub trace_session_id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryDeinitializeRequest {
    pub operator_agent_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryDeinitializeResponse {
    pub operator_agent_id: String,
    pub session_id: String,
    pub state: String,
    pub trace_session_id: Option<String>,
    pub reuse_policy: ReusePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryDestroyRequest {
    pub operator_agent_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryDestroyResponse {
    pub operator_agent_id: String,
    pub session_id: String,
    pub destroyed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryListRequest {
    pub operator_agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryListResponse {
    pub operator_agent_id: String,
    pub count: usize,
    pub subsidiaries: Vec<SubsidiaryView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubsidiaryView {
    pub session_id: String,
    pub container_id: String,
    pub peer_agent_id: String,
    pub agent_lock_state: String,
    pub agent_kind: Option<AgentHookupKind>,
    pub initialized_agent_id: Option<String>,
    pub trace_session_id: Option<String>,
    pub reuse_policy: Option<ReusePolicy>,
    pub provisioned_at_unix: u64,
    pub initialized_at_unix: Option<u64>,
    pub container_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolResponse {
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

impl NucleusDbMcpService {
    pub fn new(db_path: &str) -> Result<Self, String> {
        Self::new_with_shared_runtime(db_path, None)
    }

    pub fn new_with_runtime(
        db_path: impl AsRef<Path>,
        vault: Option<Arc<crate::halo::vault::Vault>>,
        pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
        governor_registry: Arc<crate::halo::governor_registry::GovernorRegistry>,
        orchestrator: Orchestrator,
    ) -> Result<Self, String> {
        Self::new_with_shared_runtime(
            db_path,
            Some(SharedServiceRuntime {
                vault,
                pty_manager,
                governor_registry,
                orchestrator,
            }),
        )
    }

    fn new_with_shared_runtime(
        db_path: impl AsRef<Path>,
        shared_runtime: Option<SharedServiceRuntime>,
    ) -> Result<Self, String> {
        let db_path = db_path.as_ref().to_path_buf();
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
        let (vault, governor_registry, pty_manager, orchestrator) =
            Self::resolve_runtime(&db_path, shared_runtime);
        let discord_db_path = std::env::var("NUCLEUSDB_DISCORD_DB_PATH")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(|| db_path.clone());
        Ok(Self {
            state: Arc::new(Mutex::new(ServiceState {
                db,
                db_path,
                wal_path,
                discord_db_path,
                orchestrator,
                _vault: vault,
                pty_manager,
                _governor_registry: governor_registry,
                container_runtime: shared_container_runtime(),
            })),
            tool_router: Self::tool_router(),
        })
    }

    fn resolve_runtime(
        db_path: &Path,
        shared_runtime: Option<SharedServiceRuntime>,
    ) -> (
        Option<Arc<crate::halo::vault::Vault>>,
        Arc<crate::halo::governor_registry::GovernorRegistry>,
        Arc<crate::cockpit::pty_manager::PtyManager>,
        Orchestrator,
    ) {
        if let Some(shared) = shared_runtime {
            crate::halo::governor_registry::install_global_registry(
                shared.governor_registry.clone(),
            );
            return (
                shared.vault,
                shared.governor_registry,
                shared.pty_manager,
                shared.orchestrator,
            );
        }
        let vault = crate::halo::vault::Vault::open(
            &crate::halo::config::pq_wallet_path(),
            &crate::halo::config::vault_path(),
        )
        .ok()
        .map(Arc::new);
        let governor_registry = crate::halo::governor_registry::build_default_registry();
        crate::halo::governor_registry::install_global_registry(governor_registry.clone());
        let pty_manager = Arc::new(
            crate::cockpit::pty_manager::PtyManager::with_governor_registry(
                24,
                Some(governor_registry.clone()),
            ),
        );
        let orchestrator =
            Orchestrator::new(pty_manager.clone(), vault.clone(), db_path.to_path_buf());
        (vault, governor_registry, pty_manager, orchestrator)
    }

    pub async fn sync_orchestrator(&self, orchestrator: Orchestrator) {
        let mut state = self.state.lock().await;
        state.orchestrator = orchestrator;
    }

    fn build_container_hookup(
        hookup: &ContainerHookupRequest,
        pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
        db_path: &Path,
    ) -> Result<Arc<dyn AgentHookup>, String> {
        match hookup {
            ContainerHookupRequest::Cli { cli_name, model } => {
                Ok(Arc::new(CliAgentHookup::with_trace_path(
                    cli_name.clone(),
                    pty_manager,
                    model.clone(),
                    db_path,
                )?))
            }
            ContainerHookupRequest::Api {
                provider,
                model,
                api_key_source,
                base_url_override,
            } => Ok(Arc::new(ApiAgentHookup::with_base_url(
                provider.clone(),
                model.clone(),
                api_key_source.clone(),
                base_url_override.clone(),
                db_path,
            )?)),
            ContainerHookupRequest::LocalModel {
                model_id,
                vllm_port,
                base_url_override,
            } => Ok(Arc::new(LocalModelHookup::with_base_url(
                model_id.clone(),
                vllm_port.unwrap_or(8000),
                base_url_override.clone(),
                db_path,
            )?)),
        }
    }

    fn lock_trace_session_id(lock: &ContainerAgentLock) -> Option<String> {
        match &lock.state {
            crate::container::ContainerAgentState::Locked {
                trace_session_id, ..
            }
            | crate::container::ContainerAgentState::Deinitializing {
                trace_session_id, ..
            } => trace_session_id.clone(),
            crate::container::ContainerAgentState::Empty => None,
        }
    }

    fn session_view_with_lock(
        session: crate::container::launcher::SessionInfo,
        lock_state: Option<String>,
        reuse_policy: Option<String>,
    ) -> ContainerSessionView {
        ContainerSessionView {
            session_id: session.session_id,
            container_id: session.container_id,
            image: session.image,
            agent_id: session.agent_id,
            host_sock: session.host_sock.display().to_string(),
            started_at_unix: session.started_at_unix,
            mesh_port: session.mesh_port,
            lock_state,
            reuse_policy,
        }
    }

    fn decode_remote_tool_value<T: serde::de::DeserializeOwned>(
        value: serde_json::Value,
        context: &str,
    ) -> Result<T, McpError> {
        let is_error = value
            .get("isError")
            .or_else(|| value.get("is_error"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_error {
            return Err(McpError::internal_error(
                format!("remote tool returned error for {context}"),
                None,
            ));
        }
        let payload = value
            .get("structuredContent")
            .or_else(|| value.get("structured_content"))
            .cloned()
            .unwrap_or(value);
        serde_json::from_value(payload.clone()).map_err(|e| {
            McpError::internal_error(
                format!(
                    "decode {context}: {e}; payload={}",
                    serde_json::to_string(&payload).unwrap_or_else(|_| "<unserializable>".into())
                ),
                None,
            )
        })
    }

    async fn fetch_container_lock_status_view(
        peer: crate::container::PeerInfo,
    ) -> (Option<String>, Option<String>) {
        let timeout = CONTAINER_LIST_LOCK_STATUS_TIMEOUT;
        let auth_token = crate::container::mesh_auth_token();
        let remote_peer = peer.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::container::call_remote_tool_with_timeout(
                &remote_peer,
                "nucleusdb_container_lock_status",
                serde_json::json!({}),
                auth_token.as_deref(),
                timeout,
            )
        })
        .await;
        match result {
            Ok(Ok(value)) => match Self::decode_remote_tool_value::<ContainerLockStatusView>(
                value,
                "container lock status",
            ) {
                Ok(view) => (Some(view.state), Some(view.reuse_policy)),
                Err(_) => (None, None),
            },
            _ => (None, None),
        }
    }

    async fn container_session_views(
        sessions: Vec<crate::container::launcher::SessionInfo>,
        registry: crate::container::PeerRegistry,
    ) -> Result<Vec<ContainerSessionView>, McpError> {
        let mut join_set = tokio::task::JoinSet::new();
        let total = sessions.len();
        for (idx, session) in sessions.into_iter().enumerate() {
            let peer = registry.find(&session.agent_id).cloned();
            join_set.spawn(async move {
                let (lock_state, reuse_policy) = match peer {
                    Some(peer) => Self::fetch_container_lock_status_view(peer).await,
                    None => (None, None),
                };
                (
                    idx,
                    Self::session_view_with_lock(session, lock_state, reuse_policy),
                )
            });
        }
        let mut ordered = vec![None; total];
        while let Some(joined) = join_set.join_next().await {
            let (idx, view) = joined.map_err(|e| {
                McpError::internal_error(format!("join container session view task: {e}"), None)
            })?;
            ordered[idx] = Some(view);
        }
        Ok(ordered.into_iter().flatten().collect())
    }

    async fn require_operator_capability(&self, operator_agent_id: &str) -> Result<(), McpError> {
        let orchestrator = self.state.lock().await.orchestrator.clone();
        orchestrator
            .require_capability(operator_agent_id, "operator")
            .await
            .map_err(|e| McpError::invalid_params(e, None))
    }

    fn load_operator_registry_locked(
        operator_agent_id: &str,
    ) -> Result<
        (
            crate::orchestrator::subsidiary_registry::SubsidiaryRegistryLock,
            SubsidiaryRegistry,
        ),
        McpError,
    > {
        SubsidiaryRegistry::load_or_create_locked(operator_agent_id)
            .map_err(|e| McpError::internal_error(e, None))
    }

    fn subsidiary_kind_from_hookup(hookup: &ContainerHookupRequest) -> AgentHookupKind {
        match hookup {
            ContainerHookupRequest::Cli { cli_name, .. } => AgentHookupKind::Cli {
                cli_name: cli_name.clone(),
            },
            ContainerHookupRequest::Api { provider, .. } => AgentHookupKind::Api {
                provider: provider.clone(),
            },
            ContainerHookupRequest::LocalModel { model_id, .. } => AgentHookupKind::LocalModel {
                model_id: model_id.clone(),
            },
        }
    }

    fn peer_for_subsidiary_immediate(
        record: &SubsidiaryRecord,
    ) -> Result<Option<crate::container::PeerInfo>, McpError> {
        let registry =
            crate::container::PeerRegistry::load(&crate::container::mesh_registry_path())
                .map_err(|e| McpError::internal_error(e, None))?;
        Ok(registry.find(&record.peer_agent_id).cloned())
    }

    async fn peer_for_subsidiary_with_timeout(
        record: &SubsidiaryRecord,
        timeout: Duration,
    ) -> Result<crate::container::PeerInfo, McpError> {
        let started = tokio::time::Instant::now();
        loop {
            if let Some(peer) = Self::peer_for_subsidiary_immediate(record)? {
                return Ok(peer);
            }
            if started.elapsed() >= timeout {
                return Err(McpError::invalid_params(
                    format!(
                        "subsidiary peer `{}` is not present in the mesh registry",
                        record.peer_agent_id
                    ),
                    None,
                ));
            }
            tokio::time::sleep(SUBSIDIARY_PEER_REGISTRATION_POLL_INTERVAL).await;
        }
    }

    fn subsidiary_view(
        record: &SubsidiaryRecord,
        sessions: &BTreeMap<String, crate::container::launcher::SessionInfo>,
    ) -> SubsidiaryView {
        let container_status = sessions
            .get(&record.session_id)
            .and_then(|_| launcher_container_status(&record.session_id).ok());
        SubsidiaryView {
            session_id: record.session_id.clone(),
            container_id: record.container_id.clone(),
            peer_agent_id: record.peer_agent_id.clone(),
            agent_lock_state: record.agent_lock_state.clone(),
            agent_kind: record.agent_kind.clone(),
            initialized_agent_id: record.initialized_agent_id.clone(),
            trace_session_id: record.trace_session_id.clone(),
            reuse_policy: record.reuse_policy,
            provisioned_at_unix: record.provisioned_at_unix,
            initialized_at_unix: record.initialized_at_unix,
            container_status,
        }
    }

    fn subsidiary_task_response(task: &SubsidiaryTaskRecord) -> SubsidiaryTaskResponse {
        SubsidiaryTaskResponse {
            task_id: task.task_id.clone(),
            operator_agent_id: task.operator_agent_id.clone(),
            session_id: task.session_id.clone(),
            status: task.status.clone(),
            model: task.model.clone(),
            result: task.result.clone(),
            error: task.error.clone(),
            trace_session_id: task.trace_session_id.clone(),
            input_tokens: task.input_tokens,
            output_tokens: task.output_tokens,
            cost_usd: task.cost_usd,
        }
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

    #[tool(description = "Provision a new EMPTY container ready for a later initialize step.")]
    pub async fn nucleusdb_container_provision(
        &self,
        Parameters(req): Parameters<ContainerLaunchRequest>,
    ) -> Result<Json<ContainerLaunchResponse>, McpError> {
        let host_sock = req
            .host_sock
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);
        let env_vars = req.env.into_iter().collect::<Vec<(String, String)>>();
        let mesh = req.mesh.map(|cfg| {
            let mut mesh_cfg = MeshConfig {
                enabled: cfg.enabled.unwrap_or(true),
                ..MeshConfig::default()
            };
            if let Some(port) = cfg.mcp_port {
                mesh_cfg.mcp_port = port;
            }
            if let Some(path) = cfg
                .registry_volume
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                mesh_cfg.registry_volume = PathBuf::from(path);
            }
            mesh_cfg.agent_did = cfg
                .agent_did
                .map(|did| did.trim().to_string())
                .filter(|did| !did.is_empty());
            mesh_cfg
        });
        let default_mcp_port = mesh
            .as_ref()
            .map(|cfg| cfg.mcp_port)
            .unwrap_or(crate::container::mesh::DEFAULT_MCP_PORT);
        let info = launch_container(RunConfig {
            image: req.image.clone(),
            agent_id: req.agent_id.clone(),
            command: if req.command.is_empty() {
                vec![
                    "nucleusdb-mcp".to_string(),
                    "/data/nucleusdb.ndb".to_string(),
                    "--transport".to_string(),
                    "http".to_string(),
                    "--host".to_string(),
                    "0.0.0.0".to_string(),
                    "--port".to_string(),
                    default_mcp_port.to_string(),
                ]
            } else {
                req.command
            },
            use_gvisor: req.runtime_runsc.unwrap_or(false),
            host_sock,
            env_vars,
            mesh,
        })
        .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(ContainerLaunchResponse {
            session_id: info.session_id,
            container_id: info.container_id,
            image: info.image,
            agent_id: info.agent_id,
            host_sock: info.host_sock.display().to_string(),
        }))
    }

    #[tool(description = "List tracked container sessions launched by NucleusDB tooling.")]
    pub async fn nucleusdb_container_list(&self) -> Result<Json<ContainerListResponse>, McpError> {
        let sessions = list_container_sessions().map_err(|e| McpError::internal_error(e, None))?;
        let registry =
            crate::container::PeerRegistry::load(&crate::container::mesh_registry_path())
                .unwrap_or_default();
        let views = Self::container_session_views(sessions, registry).await?;
        Ok(Json(ContainerListResponse {
            count: views.len(),
            sessions: views,
        }))
    }

    #[tool(description = "Get runtime status for a tracked container session.")]
    pub async fn nucleusdb_container_status(
        &self,
        Parameters(req): Parameters<ContainerStatusRequest>,
    ) -> Result<Json<ContainerStatusResponse>, McpError> {
        let status = launcher_container_status(&req.session_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        let running = matches!(status.as_str(), "running" | "restarting" | "created");
        Ok(Json(ContainerStatusResponse {
            session_id: req.session_id,
            status,
            running,
        }))
    }

    #[tool(description = "Return the current container agent lock state for this runtime.")]
    pub async fn nucleusdb_container_lock_status(
        &self,
        _: Parameters<ContainerLockStatusRequest>,
    ) -> Result<Json<ContainerLockStatusResponse>, McpError> {
        let container_id = current_container_id();
        let lock = ContainerAgentLock::load_or_create(&container_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(ContainerLockStatusResponse {
            container_id: lock.container_id.clone(),
            state: lock.state_label().to_string(),
            reuse_policy: lock.reuse_policy,
            lock: serde_json::to_value(lock).map_err(|e| {
                McpError::internal_error(format!("serialize container lock: {e}"), None)
            })?,
        }))
    }

    #[tool(description = "Initialize the current EMPTY container with an agent hookup.")]
    pub async fn nucleusdb_container_initialize(
        &self,
        Parameters(req): Parameters<ContainerInitializeRequest>,
    ) -> Result<Json<ContainerInitializeResponse>, McpError> {
        let (container_id, pty_manager, db_path, runtime) = {
            let guard = self.state.lock().await;
            (
                current_container_id(),
                guard.pty_manager.clone(),
                guard.db_path.clone(),
                guard.container_runtime.clone(),
            )
        };
        let hookup = Self::build_container_hookup(&req.hookup, pty_manager, &db_path)
            .map_err(|e| McpError::invalid_params(e, None))?;
        let mut lock = ContainerAgentLock::load_or_create(&container_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        lock.reuse_policy = req.reuse_policy.unwrap_or(ReusePolicy::Reusable);
        let agent_id = hookup
            .start(&mut lock)
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;
        {
            let mut container_runtime = runtime.lock().await;
            container_runtime.active = Some(ActiveContainerHookup { hookup });
        }
        Ok(Json(ContainerInitializeResponse {
            container_id: lock.container_id.clone(),
            state: lock.state_label().to_string(),
            agent_id,
            trace_session_id: Self::lock_trace_session_id(&lock),
            reuse_policy: lock.reuse_policy,
        }))
    }

    #[tool(description = "Send a prompt to the initialized agent hookup in this container.")]
    pub async fn nucleusdb_container_agent_prompt(
        &self,
        Parameters(req): Parameters<ContainerAgentPromptRequest>,
    ) -> Result<Json<AgentResponse>, McpError> {
        if req.prompt.trim().is_empty() {
            return Err(McpError::invalid_params("prompt must be non-empty", None));
        }
        let runtime = { self.state.lock().await.container_runtime.clone() };
        let hookup = {
            let container_runtime = runtime.lock().await;
            container_runtime
                .active
                .as_ref()
                .map(|active| active.hookup.clone())
        }
        .ok_or_else(|| McpError::invalid_params("container agent is not initialized", None))?;
        let response = hookup
            .send_prompt(&req.prompt)
            .await
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(response))
    }

    #[tool(
        description = "Deinitialize the current container agent hookup and return the lock to EMPTY."
    )]
    pub async fn nucleusdb_container_deinitialize(
        &self,
        _: Parameters<ContainerDeinitializeRequest>,
    ) -> Result<Json<ContainerDeinitializeResponse>, McpError> {
        let (container_id, runtime) = {
            let guard = self.state.lock().await;
            (current_container_id(), guard.container_runtime.clone())
        };
        let hookup = {
            let mut container_runtime = runtime.lock().await;
            container_runtime.active.take()
        }
        .ok_or_else(|| McpError::invalid_params("container agent is not initialized", None))?;
        hookup
            .hookup
            .stop()
            .await
            .map_err(|e| McpError::internal_error(e, None))?;
        let lock = ContainerAgentLock::load_or_create(&container_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(ContainerDeinitializeResponse {
            container_id: lock.container_id.clone(),
            state: lock.state_label().to_string(),
            trace_session_id: Self::lock_trace_session_id(&lock),
            reuse_policy: lock.reuse_policy,
        }))
    }

    #[tool(description = "Stop a running container session.")]
    pub async fn nucleusdb_container_stop(
        &self,
        Parameters(req): Parameters<ContainerStopRequest>,
    ) -> Result<Json<ContainerStopResponse>, McpError> {
        launcher_stop_container(&req.session_id).map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(ContainerStopResponse {
            session_id: req.session_id,
            stopped: true,
        }))
    }

    #[tool(description = "Fetch container logs for a tracked session.")]
    pub async fn nucleusdb_container_logs(
        &self,
        Parameters(req): Parameters<ContainerLogsRequest>,
    ) -> Result<Json<ContainerLogsResponse>, McpError> {
        let follow = req.follow.unwrap_or(false);
        let logs = launcher_container_logs(&req.session_id, follow)
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(ContainerLogsResponse {
            session_id: req.session_id,
            follow,
            logs,
        }))
    }

    #[tool(
        description = "Operator-only: provision a new EMPTY subsidiary container and register ownership."
    )]
    pub async fn nucleusdb_subsidiary_provision(
        &self,
        Parameters(req): Parameters<SubsidiaryProvisionRequest>,
    ) -> Result<Json<SubsidiaryProvisionResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        let Json(provisioned) = self
            .nucleusdb_container_provision(Parameters(ContainerLaunchRequest {
                image: req.image,
                agent_id: req.agent_id,
                command: req.command,
                runtime_runsc: req.runtime_runsc,
                host_sock: req.host_sock,
                env: req.env,
                mesh: req.mesh,
            }))
            .await?;
        let (_lock, mut registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        registry.register_provision(
            provisioned.session_id.clone(),
            provisioned.container_id.clone(),
            provisioned.agent_id.clone(),
        );
        registry
            .save()
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(SubsidiaryProvisionResponse {
            operator_agent_id: req.operator_agent_id,
            session_id: provisioned.session_id,
            container_id: provisioned.container_id,
            agent_id: provisioned.agent_id,
            status: "empty".to_string(),
        }))
    }

    #[tool(
        description = "Operator-only: initialize an owned EMPTY subsidiary container with an agent hookup."
    )]
    pub async fn nucleusdb_subsidiary_initialize(
        &self,
        Parameters(req): Parameters<SubsidiaryInitializeRequest>,
    ) -> Result<Json<SubsidiaryInitializeResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        let (_lock, mut registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        let owned = registry
            .assert_owned(&req.session_id)
            .map_err(|e| McpError::invalid_params(e, None))?
            .clone();
        let peer =
            Self::peer_for_subsidiary_with_timeout(&owned, SUBSIDIARY_PEER_REGISTRATION_TIMEOUT)
                .await?;
        let auth_token = mesh_auth_token();
        let hookup = req.hookup.clone();
        let reuse_policy = req.reuse_policy.unwrap_or(ReusePolicy::Reusable);
        let value = tokio::task::spawn_blocking(move || {
            crate::container::call_remote_tool_with_timeout(
                &peer,
                "nucleusdb_container_initialize",
                serde_json::json!({
                    "hookup": hookup,
                    "reuse_policy": reuse_policy,
                }),
                auth_token.as_deref(),
                Duration::from_secs(30),
            )
        })
        .await
        .map_err(|e| McpError::internal_error(format!("join subsidiary initialize: {e}"), None))?
        .map_err(|e| McpError::internal_error(e, None))?;
        let initialized: ContainerInitializeResponse =
            Self::decode_remote_tool_value(value, "subsidiary initialize")?;
        registry
            .register_initialize(
                &req.session_id,
                Self::subsidiary_kind_from_hookup(&req.hookup),
                initialized.agent_id.clone(),
                initialized.trace_session_id.clone(),
                initialized.reuse_policy,
            )
            .map_err(|e| McpError::internal_error(e, None))?;
        registry
            .save()
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(SubsidiaryInitializeResponse {
            operator_agent_id: req.operator_agent_id,
            session_id: req.session_id,
            container_id: initialized.container_id,
            state: initialized.state,
            agent_id: initialized.agent_id,
            trace_session_id: initialized.trace_session_id,
            reuse_policy: initialized.reuse_policy,
        }))
    }

    #[tool(
        description = "Operator-only: send a prompt to an owned subsidiary agent and persist the result."
    )]
    pub async fn nucleusdb_subsidiary_send_task(
        &self,
        Parameters(req): Parameters<SubsidiarySendTaskRequest>,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        if req.prompt.trim().is_empty() {
            return Err(McpError::invalid_params("prompt must be non-empty", None));
        }
        let (_lock, mut registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        let owned = registry
            .assert_owned(&req.session_id)
            .map_err(|e| McpError::invalid_params(e, None))?
            .clone();
        let peer =
            Self::peer_for_subsidiary_with_timeout(&owned, SUBSIDIARY_PEER_REGISTRATION_TIMEOUT)
                .await?;
        let prompt = req.prompt.clone();
        let auth_token = mesh_auth_token();
        let timeout_secs = req.timeout_secs.unwrap_or(30).clamp(1, 600);
        let value = tokio::task::spawn_blocking(move || {
            crate::container::call_remote_tool_with_timeout(
                &peer,
                "nucleusdb_container_agent_prompt",
                serde_json::json!({ "prompt": prompt }),
                auth_token.as_deref(),
                Duration::from_secs(timeout_secs),
            )
        })
        .await;
        let task = match value {
            Ok(Ok(value)) => {
                let response: AgentResponse =
                    Self::decode_remote_tool_value(value, "subsidiary prompt")?;
                registry
                    .record_task(
                        &req.session_id,
                        req.prompt,
                        "complete".to_string(),
                        Some(response.model.clone()),
                        Some(response.content.clone()),
                        None,
                        owned.trace_session_id.clone(),
                        Some(response.input_tokens),
                        Some(response.output_tokens),
                        Some(response.cost_usd),
                    )
                    .map_err(|e| McpError::internal_error(e, None))?
            }
            Ok(Err(err)) => registry
                .record_task(
                    &req.session_id,
                    req.prompt,
                    "failed".to_string(),
                    None,
                    None,
                    Some(err.clone()),
                    owned.trace_session_id.clone(),
                    None,
                    None,
                    None,
                )
                .map_err(|e| McpError::internal_error(e, None))?,
            Err(err) => registry
                .record_task(
                    &req.session_id,
                    req.prompt,
                    "failed".to_string(),
                    None,
                    None,
                    Some(format!("join subsidiary send_task: {err}")),
                    owned.trace_session_id.clone(),
                    None,
                    None,
                    None,
                )
                .map_err(|e| McpError::internal_error(e, None))?,
        };
        registry
            .save()
            .map_err(|e| McpError::internal_error(e, None))?;
        if task.status == "failed" {
            return Err(McpError::internal_error(
                task.error
                    .clone()
                    .unwrap_or_else(|| "subsidiary task failed".to_string()),
                None,
            ));
        }
        Ok(Json(Self::subsidiary_task_response(&task)))
    }

    #[tool(description = "Operator-only: fetch a persisted subsidiary task result by task id.")]
    pub async fn nucleusdb_subsidiary_get_result(
        &self,
        Parameters(req): Parameters<SubsidiaryGetResultRequest>,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        let (_lock, registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        let task = registry.task(&req.task_id).ok_or_else(|| {
            McpError::invalid_params(
                format!("unknown subsidiary task_id `{}`", req.task_id),
                None,
            )
        })?;
        if task.operator_agent_id != req.operator_agent_id {
            return Err(McpError::invalid_params(
                "subsidiary task does not belong to this operator",
                None,
            ));
        }
        Ok(Json(Self::subsidiary_task_response(task)))
    }

    #[tool(
        description = "Operator-only: deinitialize an owned subsidiary agent and return it to EMPTY."
    )]
    pub async fn nucleusdb_subsidiary_deinitialize(
        &self,
        Parameters(req): Parameters<SubsidiaryDeinitializeRequest>,
    ) -> Result<Json<SubsidiaryDeinitializeResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        let (_lock, mut registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        let owned = registry
            .assert_owned(&req.session_id)
            .map_err(|e| McpError::invalid_params(e, None))?
            .clone();
        let peer =
            Self::peer_for_subsidiary_with_timeout(&owned, SUBSIDIARY_PEER_REGISTRATION_TIMEOUT)
                .await?;
        let auth_token = mesh_auth_token();
        let value = tokio::task::spawn_blocking(move || {
            crate::container::call_remote_tool_with_timeout(
                &peer,
                "nucleusdb_container_deinitialize",
                serde_json::json!({}),
                auth_token.as_deref(),
                Duration::from_secs(30),
            )
        })
        .await
        .map_err(|e| McpError::internal_error(format!("join subsidiary deinitialize: {e}"), None))?
        .map_err(|e| McpError::internal_error(e, None))?;
        let response: ContainerDeinitializeResponse =
            Self::decode_remote_tool_value(value, "subsidiary deinitialize")?;
        registry
            .register_deinitialize(&req.session_id, response.reuse_policy)
            .map_err(|e| McpError::internal_error(e, None))?;
        registry
            .save()
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(SubsidiaryDeinitializeResponse {
            operator_agent_id: req.operator_agent_id,
            session_id: req.session_id,
            state: response.state,
            trace_session_id: response.trace_session_id,
            reuse_policy: response.reuse_policy,
        }))
    }

    #[tool(
        description = "Operator-only: destroy an owned subsidiary container and remove it from the operator registry."
    )]
    pub async fn nucleusdb_subsidiary_destroy(
        &self,
        Parameters(req): Parameters<SubsidiaryDestroyRequest>,
    ) -> Result<Json<SubsidiaryDestroyResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        let (_lock, mut registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        registry
            .assert_owned(&req.session_id)
            .map_err(|e| McpError::invalid_params(e, None))?;
        crate::container::destroy_container(&req.session_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        registry
            .remove_subsidiary(&req.session_id)
            .map_err(|e| McpError::internal_error(e, None))?;
        registry
            .save()
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(SubsidiaryDestroyResponse {
            operator_agent_id: req.operator_agent_id,
            session_id: req.session_id,
            destroyed: true,
        }))
    }

    #[tool(
        description = "Operator-only: list subsidiaries owned by the operator with current status."
    )]
    pub async fn nucleusdb_subsidiary_list(
        &self,
        Parameters(req): Parameters<SubsidiaryListRequest>,
    ) -> Result<Json<SubsidiaryListResponse>, McpError> {
        self.require_operator_capability(&req.operator_agent_id)
            .await?;
        let (_lock, registry) = Self::load_operator_registry_locked(&req.operator_agent_id)?;
        let sessions = list_container_sessions()
            .map_err(|e| McpError::internal_error(e, None))?
            .into_iter()
            .map(|session| (session.session_id.clone(), session))
            .collect::<BTreeMap<_, _>>();
        let subsidiaries = registry
            .subsidiaries
            .iter()
            .map(|record| Self::subsidiary_view(record, &sessions))
            .collect::<Vec<_>>();
        Ok(Json(SubsidiaryListResponse {
            operator_agent_id: req.operator_agent_id,
            count: subsidiaries.len(),
            subsidiaries,
        }))
    }

    #[tool(description = "Launch an orchestrator-managed agent using PTY or container dispatch.")]
    async fn orchestrator_launch(
        &self,
        Parameters(req): Parameters<OrchLaunchRequest>,
    ) -> Result<Json<crate::orchestrator::agent_pool::ManagedAgent>, McpError> {
        let orchestrator = self.state.lock().await.orchestrator.clone();
        orchestrator
            .launch_agent(req)
            .await
            .map(Json)
            .map_err(|e| McpError::internal_error(e, None))
    }

    #[tool(description = "Send a task through the orchestrator.")]
    async fn orchestrator_send_task(
        &self,
        Parameters(req): Parameters<OrchSendTaskRequest>,
    ) -> Result<Json<crate::orchestrator::task::Task>, McpError> {
        let orchestrator = self.state.lock().await.orchestrator.clone();
        orchestrator
            .send_task(req)
            .await
            .map(Json)
            .map_err(|e| McpError::internal_error(e, None))
    }

    #[tool(description = "Stop an orchestrator-managed agent.")]
    async fn orchestrator_stop(
        &self,
        Parameters(req): Parameters<OrchStopRequest>,
    ) -> Result<Json<crate::orchestrator::StopResult>, McpError> {
        let orchestrator = self.state.lock().await.orchestrator.clone();
        orchestrator
            .stop_agent(req)
            .await
            .map(Json)
            .map_err(|e| McpError::internal_error(e, None))
    }

    #[tool(description = "List orchestrator-managed agents.")]
    async fn orchestrator_status(
        &self,
    ) -> Result<Json<crate::orchestrator::OrchestratorStatus>, McpError> {
        let orchestrator = self.state.lock().await.orchestrator.clone();
        Ok(Json(orchestrator.status().await))
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
