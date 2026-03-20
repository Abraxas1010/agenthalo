use crate::cli::{default_witness_cfg, parse_backend};
use crate::cockpit::deploy;
use crate::container::launcher::{
    container_logs as launcher_container_logs, container_status as launcher_container_status,
    launch_container, list_sessions as list_container_sessions,
    stop_container as launcher_stop_container, MeshConfig, RunConfig,
};
use crate::container::{
    current_container_id, mesh_auth_token, AgentHookup, AgentHookupKind, AgentResponse,
    ApiAgentHookup, CliAgentHookup, ContainerAgentLock, LocalModelHookup, ReusePolicy,
};
use crate::halo::admission::{evaluate_launch_admission, AdmissionMode};
use crate::halo::governor_registry::{
    build_default_registry, install_global_registry, GovernorRegistry,
};
use crate::immutable::WriteMode;
use crate::orchestrator::subsidiary_registry::{
    SubsidiaryRecord, SubsidiaryRegistry, SubsidiaryTaskRecord,
};
use crate::orchestrator::{
    ContainerHookupRequest, LaunchAgentRequest as OrchLaunchRequest, Orchestrator,
    PipeRequest as OrchPipeRequest, SendTaskRequest as OrchSendTaskRequest,
    StopRequest as OrchStopRequest,
};
use crate::pcn::{channel_snapshot, ChannelSnapshot, SettlementOp};
use crate::persistence::{init_wal, load_wal, persist_snapshot_and_sync_wal, truncate_wal};
use crate::protocol::{NucleusDb, QueryProof, VcBackend};
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
use crate::swarm::chunk_engine::{chunk_data, reassemble_chunks};
use crate::swarm::chunk_store::ChunkStore;
use crate::swarm::config::{ChunkParams, SwarmConfig};
use crate::swarm::manifest::{verify_manifest, ManifestBuilder};
use crate::swarm::types::{AssetType, ManifestId};
use crate::transparency::ct6962::hex_encode;
use crate::trust::composite_cab::CompositeCabGenerator;
use crate::trust::onchain::{
    build_attest_command_preview_with_keystore, build_attest_command_preview_with_private_key_env,
    load_private_key_env, now_unix_secs, prepare_attestation_bundle, send_attestation,
    verify_agent_onchain, CastSigner,
};
use crate::vcs::{
    analyze_records, export_state_to_worktree, git_status_porcelain, hash_hex as vcs_hash_hex,
    parse_hash_hex as vcs_parse_hash_hex, work_records_from_workspace,
    QueryFilter as VcsQueryFilter, WorkRecord, WorkRecordInput, WorkRecordStore, WorkRecordView,
};
use base64::Engine;
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler,
};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

struct ServiceState {
    db: NucleusDb,
    db_path: PathBuf,
    wal_path: PathBuf,
    work_record_store: WorkRecordStore,
    memory_store: crate::memory::MemoryStore,
    swarm_store: ChunkStore,
    swarm_config: SwarmConfig,
    bitswap_runtime: crate::swarm::bitswap::BitswapRuntime,
    orchestrator: Orchestrator,
    vault: Option<Arc<crate::halo::vault::Vault>>,
    pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
    governor_registry: Arc<GovernorRegistry>,
    container_runtime: Arc<tokio::sync::Mutex<ContainerRuntime>>,
}

struct ContainerRuntime {
    active: Option<ActiveContainerHookup>,
}

struct ActiveContainerHookup {
    hookup: Arc<dyn AgentHookup>,
}

struct SharedServiceRuntime {
    vault: Option<Arc<crate::halo::vault::Vault>>,
    pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
    governor_registry: Arc<GovernorRegistry>,
    orchestrator: Orchestrator,
}

const CONTAINER_LIST_LOCK_STATUS_TIMEOUT: Duration = Duration::from_secs(2);
const SUBSIDIARY_PEER_REGISTRATION_TIMEOUT: Duration = Duration::from_secs(30);
const SUBSIDIARY_PEER_REGISTRATION_POLL_INTERVAL: Duration = Duration::from_millis(250);
const SUBSIDIARY_PEER_HEALTH_TIMEOUT: Duration = Duration::from_secs(60);

fn inspect_container_mesh_ip(session_id: &str) -> Result<Option<String>, String> {
    let session = crate::container::launcher::load_session(session_id)?;
    if session.mesh_port.is_some() {
        Ok(Some("127.0.0.1".to_string()))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    LegacyV1,
    TypedV2,
}

#[derive(Clone)]
pub struct NucleusDbMcpService {
    state: Arc<Mutex<ServiceState>>,
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CreateDatabaseRequest {
    /// Snapshot path to create, for example `/tmp/nucleusdb.ndb`.
    pub db_path: String,
    /// Backend id: `binary_merkle` (recommended), `ipa`, or `kzg`.
    pub backend: Option<String>,
    /// Optional WAL path. Defaults to `<db_path>.wal` when omitted.
    pub wal_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenDatabaseRequest {
    /// Snapshot path to open. If omitted, uses the current server path.
    pub db_path: Option<String>,
    /// WAL path to pair with the snapshot. Defaults to `<db_path>.wal`.
    pub wal_path: Option<String>,
    /// If true, opens from WAL replay when WAL exists; otherwise snapshot-first.
    pub prefer_wal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ExecuteSqlRequest {
    /// SQL text in the NucleusDB dialect (INSERT/SELECT/UPDATE/DELETE/SHOW/COMMIT/VERIFY/EXPORT).
    pub sql: String,
    /// Persist snapshot+WAL after successful execution. Defaults to true.
    pub persist: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryRequest {
    /// Exact key to query.
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryRangeRequest {
    /// Exact key or prefix pattern, for example `acct:%`.
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyRequest {
    /// Exact key to verify.
    pub key: String,
    /// Optional expected value check in addition to proof verification.
    pub expected_value: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallRequest {
    /// Natural-language query describing what memory context you need.
    pub query: String,
    /// Number of memory fragments to return. Defaults to 5, max 20.
    pub k: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryStoreRequest {
    /// Memory text to store and embed.
    pub text: String,
    /// Optional source label (e.g. session id, user note).
    pub source: Option<String>,
    /// Optional session ID for contextual embedding enrichment.
    pub session_id: Option<String>,
    /// Optional agent ID for contextual embedding enrichment.
    pub agent_id: Option<String>,
    /// Optional TTL in seconds. After expiry, memory is excluded from recall.
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryIngestRequest {
    /// Structured document text to chunk and ingest.
    pub document: String,
    /// Optional source label.
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryRecallResponse {
    pub query: String,
    pub k: usize,
    pub count: usize,
    pub results: Vec<crate::memory::MemoryRecallRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryStoreResponse {
    pub key: String,
    pub source: Option<String>,
    pub created: String,
    pub dims: usize,
    pub sealed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemoryIngestResponse {
    pub chunks: usize,
    pub keys: Vec<String>,
    pub source: Option<String>,
    pub sealed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmPublishRequest {
    /// UTF-8 text or base64-encoded bytes depending on `encoding`.
    pub data: String,
    /// `utf8` (default) or `base64`.
    pub encoding: Option<String>,
    /// Optional asset type label.
    pub asset_type: Option<AssetType>,
    /// Optional creator DID. Defaults to `did:key:local`.
    pub creator_did: Option<String>,
    /// Optional chunk size override.
    pub chunk_size_bytes: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmFetchRequest {
    pub manifest_id: String,
    /// `utf8` or `base64` (default).
    pub encoding: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmRemoteFetchRequest {
    pub manifest_id: String,
    /// `utf8` or `base64` (default) for the eventual fetched payload.
    pub encoding: Option<String>,
    /// Optional per-request timeout hint for the future live Bitswap implementation.
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmPublishResponse {
    pub manifest_id: String,
    pub chunk_count: usize,
    pub total_size: usize,
    pub root_hash: String,
    pub proof_attached: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmFetchResponse {
    pub manifest_id: String,
    pub chunk_count: usize,
    pub total_size: usize,
    pub encoding: String,
    pub data: String,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmRemoteFetchResponse {
    pub manifest_id: String,
    pub implemented: bool,
    pub message: String,
    pub tracking_issue: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SwarmStatusResponse {
    pub total_chunks: usize,
    pub total_bytes: usize,
    pub manifest_count: usize,
    pub active_transfers: usize,
    pub peer_count: usize,
    pub bitswap_enabled: bool,
    pub chunk_credit_cost: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceCombineRequest {
    /// Optional prior probability P(H). If omitted, use prior_odds_false_over_true.
    pub prior_probability_true: Option<f64>,
    /// Optional prior odds P(¬H)/P(H). Used when prior_probability_true is omitted.
    pub prior_odds_false_over_true: Option<f64>,
    /// Tool evidence sequence in update order.
    pub evidence: Vec<crate::halo::evidence::ToolEvidence>,
    /// Optional output uncertainty framework for the combined confidence.
    pub output_kind: Option<crate::halo::uncertainty::UncertaintyKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceCombineResponse {
    pub posterior_probability_true: f64,
    pub posterior_odds_false_over_true: f64,
    pub translated_confidence: Option<f64>,
    pub output_kind: Option<crate::halo::uncertainty::UncertaintyKind>,
    pub steps: Vec<crate::halo::evidence::EvidenceStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UncertaintyTranslateRequest {
    pub from: crate::halo::uncertainty::UncertaintyKind,
    pub to: crate::halo::uncertainty::UncertaintyKind,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UncertaintyTranslateResponse {
    pub value: f64,
    pub from_balanced: bool,
    pub to_balanced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorLaunchRequest {
    /// Agent kind: claude | codex | gemini | shell
    pub agent: String,
    /// Human-friendly label for this launched instance.
    pub agent_name: String,
    /// Optional working directory for task execution.
    pub working_dir: Option<String>,
    /// Environment variables. Values prefixed with "vault:" resolve from Vault.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Per-task timeout for this agent.
    pub timeout_secs: Option<u64>,
    /// Optional model selector for supported CLIs (for example `claude` / `codex`).
    pub model: Option<String>,
    /// Enable HALO trace capture for this agent's tasks.
    pub trace: Option<bool>,
    /// Capability set granted to this agent.
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub dispatch_mode: Option<crate::orchestrator::DispatchMode>,
    #[serde(default)]
    pub container_hookup: Option<crate::orchestrator::ContainerHookupRequest>,
    /// AETHER admission mode: warn | block | force.
    pub admission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorLaunchResponse {
    pub agent_id: String,
    pub session_id: Option<String>,
    pub status: String,
    pub agent: String,
    pub agent_name: String,
    pub capabilities: Vec<String>,
    pub model: Option<String>,
    #[serde(default)]
    pub admission_mode: Option<String>,
    pub admission_forced: bool,
    #[serde(default)]
    pub admission_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorSendTaskRequest {
    pub agent_id: String,
    pub task: String,
    /// Optional response format hint (currently passthrough).
    pub format: Option<String>,
    pub timeout_secs: Option<u64>,
    pub wait: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorScheduleTaskRequest {
    pub agent_id: String,
    pub task: String,
    pub timeout_secs: Option<u64>,
    pub delay_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorTaskResponse {
    pub task_id: String,
    pub agent_id: String,
    pub status: String,
    pub answer: Option<String>,
    /// Backward-compatible alias of `result`.
    #[serde(default)]
    pub output: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub trace_session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorTasksResponse {
    pub tasks: Vec<OrchestratorTaskResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorGraphResponse {
    pub graph: serde_json::Value,
    /// Number of task nodes in `graph.nodes`.
    #[serde(default)]
    pub node_count: usize,
    /// Number of edges in `graph.edges`.
    #[serde(default)]
    pub edge_count: usize,
    /// Container shape for `graph.nodes` (`object_map`).
    #[serde(default)]
    pub nodes_shape: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct OrchestratorMeshStatusRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorGetResultRequest {
    pub task_id: String,
    pub wait: Option<bool>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorPipeRequest {
    pub source_task_id: String,
    pub target_agent_id: String,
    pub transform: Option<String>,
    pub task_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorPipeResponse {
    pub source_task_id: String,
    pub target_agent_id: String,
    pub status: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorListResponse {
    pub agents: Vec<OrchestratorAgentView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorAgentView {
    pub agent_id: String,
    pub agent_name: String,
    pub agent_type: String,
    pub status: String,
    pub tasks_completed: u32,
    pub total_cost_usd: f64,
    pub capabilities: Vec<String>,
    pub launched_at: u64,
    pub working_dir: Option<String>,
    pub container_session_id: Option<String>,
    pub container_id: Option<String>,
    pub lock_state: Option<String>,
    pub peer_agent_id: Option<String>,
    pub trace_session_id: Option<String>,
    pub agent_home: Option<String>,
    pub identity_fingerprint: Option<String>,
    pub identity_digest: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorStopRequest {
    pub agent_id: String,
    pub force: Option<bool>,
    pub purge: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratorStopResponse {
    pub agent_id: String,
    pub status: String,
    pub trace_session_id: Option<String>,
    pub attestation_ready: bool,
    pub purged: bool,
}

// ── Mesh tool request/response types ────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshPingRequest {
    /// Agent ID of the peer to ping.
    pub agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshCallRequest {
    /// Agent ID of the target peer.
    pub peer_agent_id: String,
    /// Name of the MCP tool to invoke on the remote peer.
    pub tool_name: String,
    /// Arguments to pass to the remote tool.
    #[serde(default)]
    pub arguments: serde_json::Value,
    /// When true, wrap the MCP call in a DIDComm v2 encrypted envelope.
    /// Requires NUCLEUSDB_AGENT_PRIVATE_KEY and peer must have a DID URI.
    /// Falls back to raw HTTP if DIDComm is not available.
    #[serde(default)]
    pub use_didcomm: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshExchangeEnvelopeRequest {
    /// Agent ID of the target peer.
    pub peer_agent_id: String,
    /// ProofEnvelope JSON to send.
    pub envelope: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshGrantRequest {
    /// Agent ID of the target peer.
    pub peer_agent_id: String,
    /// DID URI of the peer to grant access to.
    pub peer_did: String,
    /// Resource key patterns to grant access to.
    pub resource_patterns: Vec<String>,
    /// Access modes (read, write, append, control).
    pub modes: Vec<String>,
    /// Duration of the grant in seconds.
    pub duration_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshPeersResponse {
    pub mesh_enabled: bool,
    pub network: String,
    pub self_agent_id: String,
    pub peer_count: usize,
    pub peers: Vec<MeshPeerView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshPeerView {
    pub agent_id: String,
    pub did_uri: Option<String>,
    pub mcp_endpoint: String,
    pub status: String,
    pub latency_ms: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshPingResponse {
    pub agent_id: String,
    pub reachable: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshCallResponse {
    pub peer_agent_id: String,
    pub tool_name: String,
    pub result: serde_json::Value,
    pub auth_method: String,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshExchangeEnvelopeResponse {
    pub peer_agent_id: String,
    pub accepted: bool,
    pub verification: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshGrantResponse {
    pub capability_token_id: String,
    pub granted_to: String,
    pub resource_patterns: Vec<String>,
    pub modes: Vec<String>,
    pub expires_at: u64,
    pub peer_agent_id: String,
}

// ── End mesh types ──────────────────────────────────────────────────

// ── CLI agent & harness types ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CliDetectRequest {
    /// Agent CLI to detect: "claude", "codex", or "gemini".
    pub agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CliDetectResponse {
    pub agent: String,
    pub installed: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CliInstallRequest {
    /// Agent CLI to install: "claude", "codex", or "gemini".
    pub agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CliInstallResponse {
    pub agent: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

// ── End CLI agent types ─────────────────────────────────────────────

fn parse_mesh_access_modes(
    modes: &[String],
) -> Result<Vec<crate::pod::capability::AccessMode>, String> {
    use crate::pod::capability::AccessMode;
    if modes.is_empty() {
        return Err("modes must include at least one of read|write|append|control".to_string());
    }
    modes
        .iter()
        .map(|m| match m.trim().to_ascii_lowercase().as_str() {
            "read" => Ok(AccessMode::Read),
            "write" => Ok(AccessMode::Write),
            "append" => Ok(AccessMode::Append),
            "control" => Ok(AccessMode::Control),
            other => Err(format!(
                "unknown access mode: {other} (expected read|write|append|control)"
            )),
        })
        .collect()
}

/// DIDComm-wrapped MCP call: encrypt tool request, send to peer's /didcomm endpoint,
/// parse the response. Returns (result, auth_method).
fn mesh_call_didcomm(
    peer: &crate::container::mesh::PeerInfo,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<(serde_json::Value, String), String> {
    // Load local agent identity.
    let key_hex = std::env::var("NUCLEUSDB_AGENT_PRIVATE_KEY")
        .map_err(|_| "DIDComm requires NUCLEUSDB_AGENT_PRIVATE_KEY".to_string())?;
    let key_bytes =
        hex::decode(key_hex.trim()).map_err(|e| format!("decode agent private key: {e}"))?;
    if key_bytes.len() < 64 {
        return Err(format!(
            "agent private key too short: {} bytes (need 64)",
            key_bytes.len()
        ));
    }
    let mut seed = [0u8; 64];
    seed.copy_from_slice(&key_bytes[..64]);
    let local_identity = crate::halo::did::did_from_genesis_seed(&seed)?;

    // Resolve peer's DID document from their discovery endpoint.
    let peer_did = peer
        .did_uri
        .as_deref()
        .ok_or_else(|| format!("peer {} has no DID URI — cannot use DIDComm", peer.agent_id))?;
    let discovery_url =
        peer.mcp_endpoint.trim_end_matches("/mcp").to_string() + "/.well-known/nucleus-pod";
    let resp = crate::halo::http_client::get_with_timeout(
        &discovery_url,
        std::time::Duration::from_secs(5),
    )?
    .call()
    .map_err(|e| format!("fetch peer DID document: {e}"))?;
    let body: serde_json::Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse peer discovery response: {e}"))?;
    let peer_doc: crate::halo::did::DIDDocument = body
        .get("did_document")
        .ok_or_else(|| format!("peer {peer_did} discovery response missing did_document"))?
        .clone()
        .pipe_deser()?;

    // Encrypt the MCP call as a DIDComm envelope.
    let didcomm_envelope =
        crate::comms::envelope::wrap_mcp_call(&local_identity, &peer_doc, tool_name, arguments)?;

    // Send the DIDComm envelope to the peer's /didcomm endpoint.
    let didcomm_url = peer.mcp_endpoint.trim_end_matches("/mcp").to_string() + "/didcomm";
    let resp = crate::halo::http_client::post_with_timeout(
        &didcomm_url,
        std::time::Duration::from_secs(30),
    )?
    .send_json(&didcomm_envelope)
    .map_err(|e| format!("send DIDComm envelope to {}: {e}", peer.agent_id))?;
    let result: serde_json::Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse DIDComm response: {e}"))?;
    if let Ok(response_envelope) =
        serde_json::from_value::<crate::comms::didcomm::DIDCommEnvelope>(result.clone())
    {
        let (response_tool, response_payload) = crate::comms::envelope::unwrap_mcp_response(
            &local_identity,
            &peer_doc,
            &response_envelope,
        )?;
        if response_tool != tool_name {
            return Err(format!(
                "DIDComm response tool mismatch: expected `{tool_name}`, got `{response_tool}`"
            ));
        }
        if let Some(status) = response_payload.get("status").and_then(|v| v.as_str()) {
            if matches!(status, "failed" | "forbidden" | "rejected") {
                let detail = response_payload
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("remote DIDComm tool call failed");
                return Err(format!("remote DIDComm tool call rejected: {detail}"));
            }
        }
        return Ok((response_payload, "didcomm-v2".to_string()));
    }

    // Backward compatibility with peers still returning plaintext JSON.
    // Set AGENTHALO_DIDCOMM_STRICT=true to reject non-envelope responses.
    if std::env::var("AGENTHALO_DIDCOMM_STRICT")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
    {
        return Err(
            "peer returned non-DIDComm response and AGENTHALO_DIDCOMM_STRICT is enabled"
                .to_string(),
        );
    }
    eprintln!(
        "[AgentHalo/DIDComm] WARNING: peer returned plaintext (non-envelope) response; \
         set AGENTHALO_DIDCOMM_STRICT=true to reject"
    );
    Ok((result, "didcomm-v2-legacy-plaintext".to_string()))
}

/// Helper to deserialize a serde_json::Value into a concrete type.
trait PipeDeser: Sized {
    fn pipe_deser<T: serde::de::DeserializeOwned>(self) -> Result<T, String>;
}

impl PipeDeser for serde_json::Value {
    fn pipe_deser<T: serde::de::DeserializeOwned>(self) -> Result<T, String> {
        serde_json::from_value(self).map_err(|e| format!("deserialize: {e}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasSubmitRecordRequest {
    /// WorkRecord JSON payload matching WorkRecordInput schema.
    pub record_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasQueryRecordsRequest {
    /// Optional exact record hash (0x + 64 hex).
    pub hash: Option<String>,
    /// Optional author PUF digest (0x + 64 hex).
    pub author_puf: Option<String>,
    /// Optional path prefix filter.
    pub path_prefix: Option<String>,
    /// Optional lower bound timestamp (inclusive).
    pub start_timestamp: Option<u64>,
    /// Optional upper bound timestamp (inclusive).
    pub end_timestamp: Option<u64>,
    /// Optional max result size.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasSubmitRecordResponse {
    pub hash: String,
    pub proof_ref: u64,
    pub commit_height: u64,
    pub state_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasQueryRecordsResponse {
    pub count: usize,
    pub records: Vec<WorkRecordView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasRecordStatusResponse {
    pub record_count: usize,
    pub latest_hash: Option<String>,
    pub latest_timestamp: Option<u64>,
    pub sth_tree_size: u64,
    pub sth_root: Option<String>,
    pub sth_timestamp: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasMergeStatusResponse {
    pub record_count: usize,
    pub merged_count: usize,
    pub conflict_count: usize,
    pub head_hash: Option<String>,
    pub conflicts: Vec<AbraxasConflictView>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasConflictView {
    pub path: String,
    pub left_hash: String,
    pub right_hash: String,
    pub left_timestamp: u64,
    pub right_timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasExportGitRequest {
    pub repo_path: String,
    pub commit_message: Option<String>,
    pub dry_run: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasExportGitResponse {
    pub exported_files: usize,
    pub deleted_files: usize,
    pub final_paths: usize,
    pub committed: bool,
    pub commit_sha: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasResolveConflictRequest {
    pub path: String,
    pub preferred_hash: String,
    pub author_puf: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasResolveConflictResponse {
    pub resolved: bool,
    pub hash: Option<String>,
    pub proof_ref: Option<u64>,
    pub commit_height: Option<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasWorkspaceInitRequest {
    pub workspace_path: String,
    pub init_git: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasWorkspaceInitResponse {
    pub workspace_path: String,
    pub git_initialized: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasWorkspaceDiffRequest {
    pub workspace_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasWorkspaceDiffResponse {
    pub workspace_path: String,
    pub changed_count: usize,
    pub porcelain_lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasWorkspaceSubmitRequest {
    pub workspace_path: String,
    pub author_puf: String,
    pub timestamp: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AbraxasWorkspaceSubmitResponse {
    pub submitted_count: usize,
    pub hashes: Vec<String>,
    pub commit_heights: Vec<u64>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HistoryRequest {
    /// Optional max number of entries (newest first).
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ExportRequest {
    /// Export format.
    /// - `legacy_v1` (default): JSON object map of `key -> raw_u64_cell`.
    /// - `typed_v2`: JSON array of `{key, value, type}` decoded entries.
    pub format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointRequest {
    /// Optional snapshot target path. Defaults to active path.
    pub db_path: Option<String>,
    /// Optional WAL path. Defaults to active WAL path.
    pub wal_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OpenChannelRequest {
    /// Participant 1 identifier/address.
    pub p1: String,
    /// Participant 2 identifier/address.
    pub p2: String,
    /// Total channel capacity.
    pub capacity: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UpdateChannelRequest {
    /// Participant 1 identifier/address.
    pub p1: String,
    /// Participant 2 identifier/address.
    pub p2: String,
    /// Updated participant 1 balance.
    pub balance1: u64,
    /// Updated participant 2 balance.
    pub balance2: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloseChannelRequest {
    /// Participant 1 identifier/address.
    pub p1: String,
    /// Participant 2 identifier/address.
    pub p2: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryChannelRequest {
    /// Participant 1 identifier/address.
    pub p1: String,
    /// Participant 2 identifier/address.
    pub p2: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLaunchRequest {
    /// OCI image reference, for example `nucleusdb-agent:latest`.
    pub image: String,
    /// Agent identity for monitoring records.
    pub agent_id: String,
    /// Runtime command args.
    pub command: Vec<String>,
    /// Optional host socket path override for sidecar communication.
    pub host_sock: Option<String>,
    /// Optional environment variables injected into the container.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Optional mesh settings for inter-container communication.
    #[serde(default)]
    pub mesh: Option<ContainerMeshRequest>,
    /// AETHER admission mode: warn | block | force.
    pub admission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerMeshRequest {
    /// Enable mesh networking for this container.
    pub enabled: Option<bool>,
    /// MCP port exposed on the mesh network. Defaults to 3000.
    pub mcp_port: Option<u16>,
    /// Shared host directory mounted at `/data/mesh` for peer registry.
    pub registry_volume: Option<String>,
    /// Optional DID URI to advertise in the peer registry.
    pub agent_did: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ContainerLaunchResponse {
    pub session_id: String,
    pub container_id: String,
    pub image: String,
    pub agent_id: String,
    pub host_sock: String,
    #[serde(default)]
    pub admission_mode: Option<String>,
    pub admission_forced: bool,
    #[serde(default)]
    pub admission_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GovernorStatusRequest {
    /// Optional specific instance id. When omitted, returns all registered runtime governors plus gov-memory aggregate.
    pub instance_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GovernorResetToolRequest {
    /// Instance id to reset. Use `gov-memory` for the storage-local governors or omit with `all=true`.
    pub instance_id: Option<String>,
    /// Reset all runtime governors plus gov-memory.
    pub all: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeployPreflightToolRequest {
    pub agent_id: String,
    pub admission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeployLaunchToolRequest {
    pub agent_id: String,
    pub mode: String,
    pub working_dir: Option<String>,
    pub admission_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeployStatusToolRequest {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GovernorStatusToolResponse {
    pub instances: Vec<serde_json::Value>,
    pub memory: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeployPreflightToolResponse {
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeployLaunchToolResponse {
    pub result: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DeployStatusToolResponse {
    pub id: String,
    pub status: String,
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
pub struct SubsidiaryProvisionRequest {
    pub operator_agent_id: String,
    pub image: String,
    pub agent_id: String,
    #[serde(default)]
    pub command: Vec<String>,
    pub host_sock: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub mesh: Option<ContainerMeshRequest>,
    pub admission_mode: Option<String>,
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
pub struct AgentRegisterRequest {
    /// Optional on-chain contract address for `attestAndPay`.
    pub contract_address: Option<String>,
    /// Optional EVM RPC URL.
    pub rpc_url: Option<String>,
    /// Groth16 proof bytes as 0x-prefixed hex for on-chain submission.
    pub proof_hex: Option<String>,
    /// Submit transaction immediately when true; otherwise prepare payload only.
    pub submit_onchain: Option<bool>,
    /// Environment variable name containing private key. Defaults to NUCLEUSDB_AGENT_PRIVATE_KEY.
    pub private_key_env: Option<String>,
    /// Optional keystore path for cast signer mode (preferred over raw private key).
    pub keystore_path: Option<String>,
    /// Optional password-file path for keystore signer mode.
    pub keystore_password_file: Option<String>,
    /// Optional tier override (1..4). Defaults to detected PUF tier mapping.
    pub tier_override: Option<u8>,
    /// Optional agent address to verify after submission.
    pub agent_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentVerifyRequest {
    /// On-chain TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL.
    pub rpc_url: String,
    /// Agent wallet address to verify.
    pub agent_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentReattestRequest {
    /// On-chain TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL.
    pub rpc_url: String,
    /// Agent wallet address to evaluate for re-attestation.
    pub agent_address: String,
    /// Re-attest when last attestation age exceeds this threshold. Defaults to 3600.
    pub stale_after_secs: Option<u64>,
    /// Force re-attestation regardless of freshness.
    pub force: Option<bool>,
    /// Submit transaction immediately when true; otherwise prepare payload only.
    pub submit_onchain: Option<bool>,
    /// Groth16 proof bytes as 0x-prefixed hex for on-chain submission.
    pub proof_hex: Option<String>,
    /// Environment variable name containing private key. Defaults to NUCLEUSDB_AGENT_PRIVATE_KEY.
    pub private_key_env: Option<String>,
    /// Optional keystore path for cast signer mode (preferred over raw private key).
    pub keystore_path: Option<String>,
    /// Optional password-file path for keystore signer mode.
    pub keystore_password_file: Option<String>,
    /// Optional tier override (1..4). Defaults to detected PUF tier mapping.
    pub tier_override: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentVerifyResponse {
    pub verified: bool,
    pub active: Option<bool>,
    pub puf_digest: Option<String>,
    pub tier: Option<u8>,
    pub last_attestation: Option<u64>,
    pub last_replay_seq: Option<u64>,
    pub raw_verify: String,
    pub raw_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentRegisterResponse {
    pub prepared: bool,
    pub submitted: bool,
    pub tx_hash: Option<String>,
    pub puf_digest: String,
    pub puf_tier: u8,
    pub puf_tier_label: String,
    pub replay_seq: u64,
    pub feasibility_root: String,
    pub public_signals: Vec<String>,
    pub compliance_inputs_json: String,
    pub command_preview: String,
    pub verify_after_submit: Option<AgentVerifyResponse>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AgentReattestResponse {
    pub agent_address: String,
    pub should_reattest: bool,
    pub reason: String,
    pub submitted: bool,
    pub tx_hash: Option<String>,
    pub next_public_signals: Option<Vec<String>>,
    pub next_replay_seq: Option<u64>,
    pub command_preview: Option<String>,
    pub verify_after_submit: Option<AgentVerifyResponse>,
    pub current_status: AgentVerifyResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RegisterChainRequest {
    /// Multi-chain TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL.
    pub rpc_url: String,
    /// Chain ID to register.
    pub chain_id: u64,
    /// Chain-local verifier address metadata.
    pub verifier_address: String,
    /// Optional metadata hash (0x + 64 hex). Defaults to sha256(chain_id|verifier_address).
    pub metadata_hash: Option<String>,
    /// Optional per-chain fee override.
    pub chain_fee: Option<u64>,
    /// Optional default fee override.
    pub default_fee: Option<u64>,
    /// Environment variable name containing private key. Defaults to NUCLEUSDB_AGENT_PRIVATE_KEY.
    pub private_key_env: Option<String>,
    /// Optional keystore path for cast signer mode.
    pub keystore_path: Option<String>,
    /// Optional password-file path for keystore signer mode.
    pub keystore_password_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RegisterChainResponse {
    pub registered: bool,
    pub chain_id: u64,
    pub register_tx_hash: Option<String>,
    pub chain_fee_tx_hash: Option<String>,
    pub default_fee_tx_hash: Option<String>,
    pub command_preview: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerateCompositeCabRequest {
    /// Chain IDs included in the composite attestation.
    pub chain_ids: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GenerateCompositeCabResponse {
    pub ok: bool,
    pub chain_ids: Vec<u64>,
    pub replay_seq: u64,
    pub composite_cab_hash: String,
    pub proof_hex: String,
    pub public_signals: Vec<String>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubmitCompositeAttestationRequest {
    /// Multi-chain TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL.
    pub rpc_url: String,
    /// Composite proof bytes as 0x-prefixed hex.
    pub proof_hex: String,
    /// Chain IDs to verify in this submission.
    pub chain_ids: Vec<u64>,
    /// Optional explicit public signals; when omitted, derived from current DB state.
    pub public_signals: Option<Vec<String>>,
    /// Environment variable name containing private key. Defaults to NUCLEUSDB_AGENT_PRIVATE_KEY.
    pub private_key_env: Option<String>,
    /// Optional keystore path for cast signer mode.
    pub keystore_path: Option<String>,
    /// Optional password-file path for keystore signer mode.
    pub keystore_password_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubmitCompositeAttestationResponse {
    pub submitted: bool,
    pub tx_hash: Option<String>,
    pub chain_ids: Vec<u64>,
    pub public_signals: Vec<String>,
    pub command_preview: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyAgentMultichainRequest {
    /// Multi-chain TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL.
    pub rpc_url: String,
    /// Agent wallet address.
    pub agent_address: String,
    /// Required chain IDs to validate.
    pub required_chains: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PerChainVerification {
    pub chain_id: u64,
    pub verified: bool,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VerifyAgentMultichainResponse {
    pub agent_address: String,
    pub verified_all_required: bool,
    pub required_chains: Vec<u64>,
    pub per_chain: Vec<PerChainVerification>,
    pub raw_multichain: String,
    pub base_status: AgentVerifyResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryCrossChainAttestationRequest {
    /// Optional deployed query contract implementing ICrossChainAttestationQuery.
    pub query_contract: Option<String>,
    /// Optional EVM RPC URL when submitting on-chain.
    pub rpc_url: Option<String>,
    /// Source chain ID.
    pub source_chain_id: u64,
    /// Target chain ID.
    pub target_chain_id: u64,
    /// Agent wallet address to query.
    pub agent_address: String,
    /// Optional request nonce.
    pub request_nonce: Option<u64>,
    /// Submit transaction when true; otherwise dry-run payload generation.
    pub submit_onchain: Option<bool>,
    /// Environment variable name containing private key. Defaults to NUCLEUSDB_AGENT_PRIVATE_KEY.
    pub private_key_env: Option<String>,
    /// Optional keystore path for cast signer mode.
    pub keystore_path: Option<String>,
    /// Optional password-file path for keystore signer mode.
    pub keystore_password_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryCrossChainAttestationResponse {
    pub request_id: String,
    pub source_chain_id: u64,
    pub target_chain_id: u64,
    pub submitted: bool,
    pub tx_hash: Option<String>,
    pub calldata_preview: String,
    pub command_preview: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListRegisteredChainsRequest {
    /// Multi-chain TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL.
    pub rpc_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChainRegistrationView {
    pub chain_id: u64,
    pub raw_chain_info: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListRegisteredChainsResponse {
    pub count: usize,
    pub chains: Vec<ChainRegistrationView>,
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
    /// Raw u64 cell value (content hash for blob types).
    pub value: u64,
    /// Human-readable typed value (decoded from blob store when applicable).
    pub typed_value: serde_json::Value,
    /// Display string for the typed value.
    pub display: String,
    /// Type tag: null, integer, float, bool, text, json, bytes, vector.
    #[serde(rename = "type")]
    pub type_tag: String,
    pub verified: bool,
    pub proof_kind: String,
    pub state_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QueryRangeResponse {
    pub pattern: String,
    pub count: usize,
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
    /// Selected format: `legacy_v1` or `typed_v2`.
    pub format: String,
    /// Schema/version tag for the selected format.
    pub format_version: String,
    pub json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HelpResponse {
    pub server: String,
    pub version: String,
    pub backends: Vec<String>,
    pub policy_profiles: Vec<String>,
    pub sql_reference: Vec<String>,
    pub notes: Vec<String>,
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
                    "Verifiable immutable database with trust attestation tools over MCP."
                        .to_string(),
                ),
                icons: None,
                website_url: Some("https://github.com/Abraxas1010/agenthalo".to_string()),
            },
            instructions: Some(
                "Use nucleusdb_help first to discover SQL syntax, backend ids, and safe defaults."
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
        Self::new_with_shared_runtime(db_path, None)
    }

    pub fn new_with_runtime(
        db_path: impl AsRef<Path>,
        vault: Option<Arc<crate::halo::vault::Vault>>,
        pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
        governor_registry: Arc<GovernorRegistry>,
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
        let wal_path = Self::default_wal_path(&db_path);
        let state = if db_path.exists() {
            Self::load_state(db_path, wal_path, false, shared_runtime)?
        } else {
            Self::create_state(db_path, wal_path, VcBackend::BinaryMerkle, shared_runtime)?
        };
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            tool_router: Self::tool_router(),
        })
    }

    fn default_wal_path(db_path: &Path) -> PathBuf {
        crate::persistence::default_wal_path(db_path)
    }

    pub async fn sync_orchestrator(&self, orchestrator: Orchestrator) {
        let mut state = self.state.lock().await;
        state.orchestrator = orchestrator;
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

    fn verify_agent_bridge(
        rpc_url: &str,
        contract_address: &str,
        agent_address: &str,
    ) -> Result<AgentVerifyResponse, McpError> {
        let status = verify_agent_onchain(rpc_url, contract_address, agent_address)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(AgentVerifyResponse {
            verified: status.verified,
            active: status.active,
            puf_digest: status.puf_digest,
            tier: status.tier,
            last_attestation: status.last_attestation,
            last_replay_seq: status.last_replay_seq,
            raw_verify: status.raw_verify,
            raw_status: status.raw_status,
        })
    }

    fn cast_array_u64(values: &[u64]) -> String {
        let inner = values
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!("[{inner}]")
    }

    fn extract_hash(raw: &str) -> Option<String> {
        raw.split(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ':' | '(' | ')' | '"' | '\'' | ';')
        })
        .find(|tok| tok.starts_with("0x") && tok.len() == 66)
        .map(ToOwned::to_owned)
    }

    fn parse_bool_output(raw: &str) -> Result<bool, McpError> {
        let t = raw.trim().to_ascii_lowercase();
        if t.contains("true") {
            return Ok(true);
        }
        if t.contains("false") {
            return Ok(false);
        }
        if let Some(hex) = t.strip_prefix("0x") {
            let nz = hex.chars().any(|c| c != '0');
            return Ok(nz);
        }
        if let Ok(v) = t.parse::<u64>() {
            return Ok(v != 0);
        }
        Err(McpError::invalid_params(
            format!("boolean output expected, got `{raw}`"),
            None,
        ))
    }

    fn parse_u64_output(raw: &str) -> Result<u64, McpError> {
        for tok in raw.split(|c: char| c.is_whitespace() || matches!(c, ',' | '(' | ')')) {
            if tok.is_empty() {
                continue;
            }
            if let Ok(v) = tok.parse::<u64>() {
                return Ok(v);
            }
            if let Some(hex) = tok.strip_prefix("0x") {
                if let Ok(v) = u64::from_str_radix(hex, 16) {
                    return Ok(v);
                }
            }
        }
        Err(McpError::invalid_params(
            format!("u64 output expected, got `{raw}`"),
            None,
        ))
    }

    fn decode_remote_tool_value<T: DeserializeOwned>(
        value: serde_json::Value,
        context: &str,
    ) -> Result<T, McpError> {
        let is_error = value
            .get("isError")
            .or_else(|| value.get("is_error"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if is_error {
            let message = value
                .get("structuredContent")
                .or_else(|| value.get("structured_content"))
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| {
                    value
                        .get("content")
                        .and_then(|v| v.as_array())
                        .and_then(|items| items.first())
                        .and_then(|item| {
                            item.as_str()
                                .or_else(|| item.get("text").and_then(|v| v.as_str()))
                        })
                        .map(str::to_string)
                })
                .unwrap_or_else(|| format!("remote tool returned error for {context}"));
            return Err(McpError::internal_error(message, None));
        }
        let payload = value
            .get("structuredContent")
            .or_else(|| value.get("structured_content"))
            .cloned()
            .unwrap_or(value);
        serde_json::from_value(payload)
            .map_err(|e| McpError::internal_error(format!("decode {context}: {e}"), None))
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

    async fn fetch_container_lock_status_view(
        peer: crate::container::PeerInfo,
    ) -> (Option<String>, Option<String>) {
        let timeout = CONTAINER_LIST_LOCK_STATUS_TIMEOUT;
        let auth_token = crate::container::mesh_auth_token();
        let remote_peer = peer.clone();
        let result = tokio::task::spawn_blocking(move || {
            crate::container::mesh::call_remote_tool_with_timeout(
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
                Err(_) => {
                    let reachable = crate::container::mesh::peer_endpoint_reachable(&peer, timeout);
                    if reachable {
                        (Some("unknown".to_string()), None)
                    } else {
                        (None, None)
                    }
                }
            },
            _ => {
                let reachable = crate::container::mesh::peer_endpoint_reachable(&peer, timeout);
                if reachable {
                    (Some("unknown".to_string()), None)
                } else {
                    (None, None)
                }
            }
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

    async fn require_operator_capability_with(
        &self,
        operator_agent_id: &str,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<(), McpError> {
        let orchestrator = match orchestrator_override {
            Some(orchestrator) => orchestrator,
            None => self.state.lock().await.orchestrator.clone(),
        };
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
        if let Some(peer) = registry.find(&record.peer_agent_id).cloned() {
            return Ok(Some(peer));
        }

        let session = match crate::container::launcher::load_session(&record.session_id) {
            Ok(session) => session,
            Err(_) => return Ok(None),
        };
        let mesh_port = match session.mesh_port {
            Some(port) => port,
            None => return Ok(None),
        };
        let host = inspect_container_mesh_ip(&session.session_id)
            .ok()
            .flatten()
            .unwrap_or_else(|| session.session_id.clone());
        let now = crate::pod::now_unix();
        Ok(Some(crate::container::PeerInfo {
            agent_id: record.peer_agent_id.clone(),
            container_name: session.session_id.clone(),
            did_uri: None,
            mcp_endpoint: format!("http://{host}:{mesh_port}/mcp"),
            discovery_endpoint: format!("http://{host}:{mesh_port}/.well-known/nucleus-pod"),
            registered_at: now,
            last_seen: now,
        }))
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

    async fn wait_for_subsidiary_peer_health(
        record: &SubsidiaryRecord,
        timeout: Duration,
    ) -> Result<(), McpError> {
        let started = tokio::time::Instant::now();
        loop {
            let Some(peer) = Self::peer_for_subsidiary_immediate(record)? else {
                if started.elapsed() >= timeout {
                    return Err(McpError::internal_error(
                        format!(
                            "subsidiary peer `{}` did not become discoverable within {}s",
                            record.peer_agent_id,
                            timeout.as_secs()
                        ),
                        None,
                    ));
                }
                tokio::time::sleep(SUBSIDIARY_PEER_REGISTRATION_POLL_INTERVAL).await;
                continue;
            };
            let health_peer = peer.clone();
            let healthy =
                tokio::task::spawn_blocking(move || crate::container::ping_peer(&health_peer))
                    .await
                    .map_err(|e| {
                        McpError::internal_error(
                            format!("join subsidiary peer health check: {e}"),
                            None,
                        )
                    })?
                    .map_err(|e| McpError::internal_error(e, None))?;
            if healthy {
                return Ok(());
            }
            if started.elapsed() >= timeout {
                return Err(McpError::internal_error(
                    format!(
                        "subsidiary peer `{}` did not become healthy within {}s",
                        record.peer_agent_id,
                        timeout.as_secs()
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

    fn run_cast(args: &[String]) -> Result<String, McpError> {
        let mut cmd = Command::new("cast");
        cmd.args(args);
        crate::halo::nym::apply_proxy_env_for_cast(&mut cmd, args).map_err(|e| {
            McpError::internal_error(format!("cast privacy routing failed: {e}"), None)
        })?;
        let out = cmd
            .output()
            .map_err(|e| McpError::internal_error(format!("failed to run cast: {e}"), None))?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !out.status.success() {
            return Err(McpError::invalid_params(
                format!(
                    "cast failed (status={}): stdout=`{}` stderr=`{}`",
                    out.status, stdout, stderr
                ),
                None,
            ));
        }
        Ok(if stdout.is_empty() {
            stderr
        } else if stderr.is_empty() {
            stdout
        } else {
            format!("{stdout}\n{stderr}")
        })
    }

    fn run_git(repo_path: &str, args: &[&str]) -> Result<String, McpError> {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .args(args)
            .output()
            .map_err(|e| McpError::internal_error(format!("failed to run git: {e}"), None))?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        if !out.status.success() {
            return Err(McpError::invalid_params(
                format!(
                    "git failed (status={}): stdout=`{}` stderr=`{}`",
                    out.status, stdout, stderr
                ),
                None,
            ));
        }
        Ok(if stdout.is_empty() {
            stderr
        } else if stderr.is_empty() {
            stdout
        } else {
            format!("{stdout}\n{stderr}")
        })
    }

    fn cast_send(
        rpc_url: &str,
        contract: &str,
        selector: &str,
        args: Vec<String>,
        signer: &CastSigner,
    ) -> Result<Option<String>, McpError> {
        let mut cmd = vec![
            "send".to_string(),
            "--async".to_string(),
            "--rpc-url".to_string(),
            rpc_url.to_string(),
        ];
        match signer {
            CastSigner::PrivateKey(key) => {
                cmd.push("--private-key".to_string());
                cmd.push(key.clone());
            }
            CastSigner::Keystore {
                path,
                password_file,
            } => {
                cmd.push("--keystore".to_string());
                cmd.push(path.clone());
                if let Some(p) = password_file {
                    cmd.push("--password-file".to_string());
                    cmd.push(p.clone());
                }
            }
        }
        cmd.push(contract.to_string());
        cmd.push(selector.to_string());
        cmd.extend(args);
        let raw = Self::run_cast(&cmd)?;
        Ok(Self::extract_hash(&raw))
    }

    fn resolve_signer(
        private_key_env: Option<&str>,
        keystore_path: Option<&str>,
        keystore_password_file: Option<&str>,
    ) -> Result<CastSigner, McpError> {
        let keystore = keystore_path.unwrap_or("").trim();
        if !keystore.is_empty() {
            return Ok(CastSigner::Keystore {
                path: keystore.to_string(),
                password_file: keystore_password_file
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToOwned::to_owned),
            });
        }
        let env_name = private_key_env
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("NUCLEUSDB_AGENT_PRIVATE_KEY");
        let private_key = load_private_key_env(env_name)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        Ok(CastSigner::PrivateKey(private_key))
    }

    fn normalize_metadata_hash(chain_id: u64, verifier_address: &str, raw: Option<&str>) -> String {
        if let Some(v) = raw {
            let t = v.trim();
            if !t.is_empty() {
                return t.to_string();
            }
        }
        let mut h = Sha256::new();
        h.update(b"nucleusdb.multichain.registry.v1|");
        h.update(chain_id.to_le_bytes());
        h.update(verifier_address.as_bytes());
        format!("0x{}", hex_encode(&h.finalize()))
    }

    fn decode_swarm_payload(data: &str, encoding: Option<&str>) -> Result<Vec<u8>, McpError> {
        match encoding
            .unwrap_or("utf8")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "utf8" | "text" => Ok(data.as_bytes().to_vec()),
            "base64" => base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| {
                    McpError::invalid_params(format!("invalid base64 swarm payload: {e}"), None)
                }),
            other => Err(McpError::invalid_params(
                format!("unsupported swarm encoding `{other}`"),
                None,
            )),
        }
    }

    fn encode_swarm_payload(
        data: &[u8],
        encoding: Option<&str>,
    ) -> Result<(String, String), McpError> {
        match encoding
            .unwrap_or("base64")
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "utf8" | "text" => String::from_utf8(data.to_vec())
                .map(|value| (value, "utf8".to_string()))
                .map_err(|e| {
                    McpError::invalid_params(format!("payload is not valid UTF-8: {e}"), None)
                }),
            "base64" => Ok((
                base64::engine::general_purpose::STANDARD.encode(data),
                "base64".to_string(),
            )),
            other => Err(McpError::invalid_params(
                format!("unsupported swarm encoding `{other}`"),
                None,
            )),
        }
    }

    fn create_state(
        db_path: PathBuf,
        wal_path: PathBuf,
        backend: VcBackend,
        shared_runtime: Option<SharedServiceRuntime>,
    ) -> Result<ServiceState, String> {
        let cfg = default_witness_cfg();
        let db = NucleusDb::new(State::new(vec![]), backend, cfg);
        db.save_persistent(&db_path)
            .map_err(|e| format!("failed to save snapshot {}: {e:?}", db_path.display()))?;
        init_wal(&wal_path, &db)
            .map_err(|e| format!("failed to initialize WAL {}: {e:?}", wal_path.display()))?;
        let (vault, governor_registry, pty_manager, orchestrator) =
            Self::resolve_runtime(&db_path, shared_runtime);
        let swarm_store = ChunkStore::load_from_db(&db);
        let swarm_config = SwarmConfig::from_env();
        let mut bitswap_runtime = crate::swarm::bitswap::BitswapRuntime::default();
        bitswap_runtime.register_local_chunks(&swarm_store.all_chunks());
        bitswap_runtime.set_require_grants(swarm_config.require_grants);
        bitswap_runtime.set_grants(crate::pod::acl::GrantStore::load_or_default(
            &db_path.with_extension("pod_grants.json"),
        ));
        Ok(ServiceState {
            db,
            orchestrator,
            db_path,
            wal_path,
            work_record_store: WorkRecordStore::new(),
            memory_store: crate::memory::MemoryStore::default(),
            swarm_store,
            swarm_config,
            bitswap_runtime,
            vault,
            pty_manager,
            governor_registry,
            container_runtime: Arc::new(tokio::sync::Mutex::new(ContainerRuntime { active: None })),
        })
    }

    fn load_state(
        db_path: PathBuf,
        wal_path: PathBuf,
        prefer_wal: bool,
        shared_runtime: Option<SharedServiceRuntime>,
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
        let (vault, governor_registry, pty_manager, orchestrator) =
            Self::resolve_runtime(&db_path, shared_runtime);
        let swarm_store = ChunkStore::load_from_db(&db);
        let swarm_config = SwarmConfig::from_env();
        let mut bitswap_runtime = crate::swarm::bitswap::BitswapRuntime::default();
        bitswap_runtime.register_local_chunks(&swarm_store.all_chunks());
        bitswap_runtime.set_require_grants(swarm_config.require_grants);
        bitswap_runtime.set_grants(crate::pod::acl::GrantStore::load_or_default(
            &db_path.with_extension("pod_grants.json"),
        ));
        Ok(ServiceState {
            db,
            orchestrator,
            db_path,
            wal_path,
            work_record_store: WorkRecordStore::new(),
            memory_store: crate::memory::MemoryStore::default(),
            swarm_store,
            swarm_config,
            bitswap_runtime,
            vault,
            pty_manager,
            governor_registry,
            container_runtime: Arc::new(tokio::sync::Mutex::new(ContainerRuntime { active: None })),
        })
    }

    fn resolve_runtime(
        db_path: &Path,
        shared_runtime: Option<SharedServiceRuntime>,
    ) -> (
        Option<Arc<crate::halo::vault::Vault>>,
        Arc<GovernorRegistry>,
        Arc<crate::cockpit::pty_manager::PtyManager>,
        Orchestrator,
    ) {
        if let Some(shared) = shared_runtime {
            install_global_registry(shared.governor_registry.clone());
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
        let governor_registry = build_default_registry();
        install_global_registry(governor_registry.clone());
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

    fn build_container_hookup(
        hookup: &ContainerHookupRequest,
        pty_manager: Arc<crate::cockpit::pty_manager::PtyManager>,
        db_path: &Path,
    ) -> Result<Arc<dyn AgentHookup>, String> {
        match hookup {
            ContainerHookupRequest::Cli { cli_name, model } => {
                let normalized = ContainerHookupRequest::Cli {
                    cli_name: cli_name.clone(),
                    model: model.clone(),
                }
                .normalized();
                Ok(Arc::new(CliAgentHookup::with_trace_path(
                    cli_name.clone(),
                    pty_manager,
                    normalized.model(),
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

    fn parse_export_format(raw: Option<&str>) -> Result<ExportFormat, McpError> {
        let normalized = raw
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("legacy_v1")
            .to_ascii_lowercase();
        match normalized.as_str() {
            "legacy_v1" | "legacy" | "v1" | "map" => Ok(ExportFormat::LegacyV1),
            "typed_v2" | "typed" | "v2" => Ok(ExportFormat::TypedV2),
            other => Err(McpError::invalid_params(
                format!("invalid export format '{other}'. expected one of: legacy_v1, typed_v2"),
                None,
            )),
        }
    }

    fn decode_typed_value(
        db: &NucleusDb,
        key: &str,
        cell: u64,
    ) -> Result<crate::typed_value::TypedValue, String> {
        let tag = db.type_map.get(key);
        let blob = db.blob_store.get(key);
        crate::typed_value::TypedValue::decode(tag, cell, blob)
            .map_err(|e| format!("typed decode failed for key '{key}': {e}"))
    }

    fn export_legacy_map(db: &NucleusDb) -> BTreeMap<String, u64> {
        let mut payload = BTreeMap::<String, u64>::new();
        for (key, idx) in db.keymap.all_keys() {
            let cell = db.state.values.get(idx).copied().unwrap_or(0);
            payload.insert(key.to_string(), cell);
        }
        payload
    }

    fn export_typed_entries(db: &NucleusDb) -> Result<Vec<serde_json::Value>, String> {
        let mut entries = Vec::new();
        for (key, idx) in db.keymap.all_keys() {
            let cell = db.state.values.get(idx).copied().unwrap_or(0);
            let tag = db.type_map.get(key);
            let typed = Self::decode_typed_value(db, key, cell)?;
            entries.push(serde_json::json!({
                "key": key,
                "value": typed.to_json_value(),
                "type": tag.as_str(),
            }));
        }
        Ok(entries)
    }

    fn query_row(db: &NucleusDb, key: &str, idx: usize) -> Result<QueryResultRow, String> {
        let Some((value, proof, root)) = db.query(idx) else {
            return Err(format!("no value for key '{key}'"));
        };
        let verified = db.verify_query(idx, value, &proof, root);

        // Decode typed value for human-readable output.
        let tag = db.type_map.get(key);
        let typed = Self::decode_typed_value(db, key, value)?;
        let typed_value = typed.to_json_value();
        let display = typed.display_string();

        Ok(QueryResultRow {
            key: key.to_string(),
            index: idx,
            value,
            typed_value,
            display,
            type_tag: tag.as_str().to_string(),
            verified,
            proof_kind: Self::proof_kind_name(&proof).to_string(),
            state_root: hex_encode(&root),
        })
    }

    fn parse_optional_hash(raw: Option<&str>, label: &str) -> Result<Option<[u8; 32]>, McpError> {
        raw.map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| {
                vcs_parse_hash_hex(v)
                    .map_err(|e| McpError::invalid_params(format!("invalid {label}: {e}"), None))
            })
            .transpose()
    }

    #[tool(
        name = "nucleusdb_create_database",
        description = "Create a new NucleusDB snapshot and WAL with selected backend. Example: {\"db_path\":\"/tmp/app.ndb\",\"backend\":\"binary_merkle\"}"
    )]
    pub async fn create_database(
        &self,
        Parameters(req): Parameters<CreateDatabaseRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        if req.db_path.trim().is_empty() {
            return Err(McpError::invalid_params(
                "db_path must be non-empty (example: /tmp/nucleusdb.ndb)",
                None,
            ));
        }
        let db_path = PathBuf::from(req.db_path.trim());
        let wal_path = req
            .wal_path
            .map(PathBuf::from)
            .unwrap_or_else(|| Self::default_wal_path(&db_path));
        let backend = parse_backend(req.backend.as_deref().unwrap_or("merkle"))
            .map_err(|e| McpError::invalid_params(e, None))?;
        let state = Self::create_state(db_path.clone(), wal_path.clone(), backend.clone(), None)
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
        description = "Open an existing snapshot (or WAL) and switch active state. Example: {\"db_path\":\"/tmp/app.ndb\",\"prefer_wal\":true}"
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
        let state = Self::load_state(db_path.clone(), wal_path.clone(), prefer_wal, None)
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
        description = "Execute SQL in the active DB. Example: {\"sql\":\"INSERT INTO data (key, value) VALUES ('acct:1', 42); COMMIT;\"}"
    )]
    pub async fn execute_sql(
        &self,
        Parameters(req): Parameters<ExecuteSqlRequest>,
    ) -> Result<Json<SqlExecutionResponse>, McpError> {
        let mut guard = self.state.lock().await;
        let (response, committed) = {
            let mut exec = SqlExecutor::new(&mut guard.db);
            let result = exec.execute(&req.sql);
            let committed = exec.committed();
            let resp = match result {
                SqlResult::Rows { columns, rows } => SqlExecutionResponse {
                    status: "rows".to_string(),
                    message: format!("returned {} row(s)", rows.len()),
                    columns,
                    rows,
                },
                SqlResult::Ok { message } => SqlExecutionResponse {
                    status: "ok".to_string(),
                    message,
                    columns: Vec::new(),
                    rows: Vec::new(),
                },
                SqlResult::Error { message } => SqlExecutionResponse {
                    status: "error".to_string(),
                    message,
                    columns: Vec::new(),
                    rows: Vec::new(),
                },
            };
            (resp, committed)
        };
        if committed && req.persist.unwrap_or(true) {
            persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                |e| {
                    McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
                },
            )?;
        }
        Ok(Json(response))
    }

    #[tool(
        name = "nucleusdb_query",
        description = "Query a single key and return value plus proof verification status. Example: {\"key\":\"acct:1\"}"
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
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(row))
    }

    #[tool(
        name = "nucleusdb_query_range",
        description = "Query keys by exact match or prefix pattern like 'acct:%'. Example: {\"pattern\":\"acct:%\"}"
    )]
    pub async fn query_range(
        &self,
        Parameters(req): Parameters<QueryRangeRequest>,
    ) -> Result<Json<QueryRangeResponse>, McpError> {
        let guard = self.state.lock().await;
        let mut rows = Vec::new();
        for (key, idx) in guard.db.keymap.keys_matching(&req.pattern) {
            let row = Self::query_row(&guard.db, &key, idx).map_err(|e| {
                McpError::internal_error(format!("query_range failed for key '{key}': {e}"), None)
            })?;
            rows.push(row);
        }
        Ok(Json(QueryRangeResponse {
            pattern: req.pattern,
            count: rows.len(),
            rows,
        }))
    }

    #[tool(
        name = "nucleusdb_verify",
        description = "Verify a key proof against current root, optionally assert expected value. Example: {\"key\":\"acct:1\",\"expected_value\":42}"
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
        name = "agenthalo_memory_recall",
        description = "Recall relevant memory fragments using semantic vector search over mem:chunk:* embeddings."
    )]
    pub async fn memory_recall(
        &self,
        Parameters(req): Parameters<MemoryRecallRequest>,
    ) -> Result<Json<MemoryRecallResponse>, McpError> {
        let query = req.query.trim().to_string();
        if query.is_empty() {
            return Err(McpError::invalid_params(
                "query must be non-empty for memory recall",
                None,
            ));
        }
        let k = req.k.unwrap_or(5).clamp(1, 20);
        let state = self.state.clone();
        let query_for_task = query.clone();
        let results = tokio::task::spawn_blocking(move || {
            let mut guard = state.blocking_lock();
            let memory_store = guard.memory_store.clone();
            let results = memory_store
                .recall(&mut guard.db, &query_for_task, k)
                .map_err(|e| McpError::invalid_params(e, None))?;
            persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                |e| {
                    McpError::internal_error(
                        format!("persist memory recall telemetry failed: {e:?}"),
                        None,
                    )
                },
            )?;
            Ok(results)
        })
        .await
        .map_err(|e| {
            McpError::internal_error(format!("memory_recall task join failed: {e}"), None)
        })??;
        Ok(Json(MemoryRecallResponse {
            query,
            k,
            count: results.len(),
            results,
        }))
    }

    #[tool(
        name = "agenthalo_memory_store",
        description = "Store a memory fragment, embed it, and commit it to NucleusDB with seal/witness evidence."
    )]
    pub async fn memory_store(
        &self,
        Parameters(req): Parameters<MemoryStoreRequest>,
    ) -> Result<Json<MemoryStoreResponse>, McpError> {
        let text = req.text.trim();
        if text.is_empty() {
            return Err(McpError::invalid_params("text must be non-empty", None));
        }
        let text_owned = text.to_string();
        let source = req.source.clone();
        let ctx = crate::memory::MemoryContext {
            session_id: req.session_id.clone(),
            agent_id: req.agent_id.clone(),
            ttl_secs: req.ttl_secs,
        };
        let state = self.state.clone();
        let response = tokio::task::spawn_blocking(move || {
            let mut guard = state.blocking_lock();
            let memory_store = guard.memory_store.clone();
            let record = memory_store
                .store_memory_ctx(&mut guard.db, &text_owned, source.as_deref(), &ctx)
                .map_err(|e| McpError::invalid_params(e, None))?;
            persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                |e| McpError::internal_error(format!("persist memory store failed: {e:?}"), None),
            )?;
            Ok(MemoryStoreResponse {
                key: record.key,
                source: record.source,
                created: record.created,
                dims: memory_store.embedding_model().dims(),
                sealed: matches!(guard.db.write_mode(), WriteMode::AppendOnly),
            })
        })
        .await
        .map_err(|e| {
            McpError::internal_error(format!("memory_store task join failed: {e}"), None)
        })??;
        Ok(Json(response))
    }

    #[tool(
        name = "agenthalo_memory_ingest",
        description = "Ingest a structured document into chunked semantic memory fragments and seal them into NucleusDB."
    )]
    pub async fn memory_ingest(
        &self,
        Parameters(req): Parameters<MemoryIngestRequest>,
    ) -> Result<Json<MemoryIngestResponse>, McpError> {
        let document = req.document.trim();
        if document.is_empty() {
            return Err(McpError::invalid_params("document must be non-empty", None));
        }
        let document_owned = document.to_string();
        let source = req.source.clone();
        let state = self.state.clone();
        let response = tokio::task::spawn_blocking(move || {
            let mut guard = state.blocking_lock();
            let memory_store = guard.memory_store.clone();
            let records = memory_store
                .ingest_document(&mut guard.db, &document_owned, source.as_deref())
                .map_err(|e| McpError::invalid_params(e, None))?;
            persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                |e| McpError::internal_error(format!("persist memory ingest failed: {e:?}"), None),
            )?;
            Ok(MemoryIngestResponse {
                chunks: records.len(),
                keys: records.into_iter().map(|r| r.key).collect(),
                source,
                sealed: matches!(guard.db.write_mode(), WriteMode::AppendOnly),
            })
        })
        .await
        .map_err(|e| {
            McpError::internal_error(format!("memory_ingest task join failed: {e}"), None)
        })??;
        Ok(Json(response))
    }

    // ── Library tools (read-only agent access to persistent Library) ──

    #[tool(
        name = "library_search",
        description = "Search the persistent Library for knowledge from past agent sessions. Full-text search across all Library records (traces, summaries, tool calls). Returns matching entries with relevance scores. The Library accumulates data from all completed sessions across all projects."
    )]
    pub async fn library_search(
        &self,
        Parameters(req): Parameters<crate::halo::library_mcp::LibrarySearchRequest>,
    ) -> Result<Json<crate::halo::library_mcp::LibrarySearchResponse>, McpError> {
        tokio::task::spawn_blocking(move || {
            crate::halo::library_mcp::tool_search(req)
                .map(Json)
                .map_err(|e| McpError::internal_error(e, None))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("library_search join: {e}"), None))?
    }

    #[tool(
        name = "library_browse",
        description = "Browse persistent Library records by key prefix. Use prefix 'lib:session:' for session metadata, 'lib:summary:' for summaries, 'lib:evt:' for events. Returns key-value pairs with content previews."
    )]
    pub async fn library_browse(
        &self,
        Parameters(req): Parameters<crate::halo::library_mcp::LibraryBrowseRequest>,
    ) -> Result<Json<crate::halo::library_mcp::LibraryBrowseResponse>, McpError> {
        tokio::task::spawn_blocking(move || {
            crate::halo::library_mcp::tool_browse(req)
                .map(Json)
                .map_err(|e| McpError::internal_error(e, None))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("library_browse join: {e}"), None))?
    }

    #[tool(
        name = "library_session_lookup",
        description = "Look up a specific past agent session in the persistent Library by session ID. Returns session metadata, summary (tokens, cost, tool calls), and event count."
    )]
    pub async fn library_session_lookup(
        &self,
        Parameters(req): Parameters<crate::halo::library_mcp::LibrarySessionLookupRequest>,
    ) -> Result<Json<crate::halo::library_mcp::LibrarySessionLookupResponse>, McpError> {
        tokio::task::spawn_blocking(move || {
            crate::halo::library_mcp::tool_session_lookup(req)
                .map(Json)
                .map_err(|e| McpError::internal_error(e, None))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("library_session_lookup join: {e}"), None))?
    }

    #[tool(
        name = "library_sessions",
        description = "List all past agent sessions stored in the persistent Library. Returns session metadata (agent, model, start/end time, status) sorted by most recent first."
    )]
    pub async fn library_sessions(
        &self,
    ) -> Result<Json<crate::halo::library_mcp::LibrarySessionsResponse>, McpError> {
        tokio::task::spawn_blocking(move || {
            crate::halo::library_mcp::tool_sessions()
                .map(Json)
                .map_err(|e| McpError::internal_error(e, None))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("library_sessions join: {e}"), None))?
    }

    #[tool(
        name = "library_status",
        description = "Check the persistent Library health: whether it is initialized, total sessions, total keys, storage size, and push history count."
    )]
    pub async fn library_status_tool(
        &self,
    ) -> Result<Json<crate::halo::library_mcp::LibraryStatusResponse>, McpError> {
        tokio::task::spawn_blocking(move || {
            crate::halo::library_mcp::tool_status()
                .map(Json)
                .map_err(|e| McpError::internal_error(e, None))
        })
        .await
        .map_err(|e| McpError::internal_error(format!("library_status join: {e}"), None))?
    }

    #[tool(
        name = "swarm_publish",
        description = "Chunk data into BLAKE3-addressed swarm pieces, persist them in NucleusDB, and emit a manifest. Example: {\"data\":\"hello\",\"encoding\":\"utf8\"}"
    )]
    pub async fn swarm_publish(
        &self,
        Parameters(req): Parameters<SwarmPublishRequest>,
    ) -> Result<Json<SwarmPublishResponse>, McpError> {
        let raw = Self::decode_swarm_payload(&req.data, req.encoding.as_deref())?;
        let state = self.state.clone();
        let asset_type = req.asset_type.unwrap_or(AssetType::Binary);
        let creator_did = req
            .creator_did
            .unwrap_or_else(|| "did:key:local".to_string());
        let chunk_size_bytes = req.chunk_size_bytes;
        let response = tokio::task::spawn_blocking(move || {
            let mut guard = state.blocking_lock();
            let params = ChunkParams {
                chunk_size_bytes: chunk_size_bytes
                    .unwrap_or(guard.swarm_config.chunk_params.chunk_size_bytes),
            };
            let chunks = chunk_data(&raw, &params);
            let mut swarm_store = std::mem::take(&mut guard.swarm_store);
            swarm_store
                .store_chunks(&mut guard.db, &chunks)
                .map_err(|e| McpError::internal_error(e, None))?;
            guard.bitswap_runtime.register_local_chunks(&chunks);
            swarm_store.set_active_transfers(guard.bitswap_runtime.active_transfers());
            let builder = ManifestBuilder {
                asset_type,
                creator_did,
                params,
            };
            let manifest = builder
                .build(&chunks)
                .map_err(|e| McpError::invalid_params(e, None))?;
            swarm_store
                .store_manifest(&mut guard.db, manifest.clone())
                .map_err(|e| McpError::internal_error(e, None))?;
            let proof_attached = swarm_store
                .get_manifest(&manifest.manifest_id)
                .and_then(|stored| stored.proof.as_ref())
                .is_some();
            guard.swarm_store = swarm_store;
            persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                |e| McpError::internal_error(format!("persist swarm publish failed: {e:?}"), None),
            )?;
            Ok(SwarmPublishResponse {
                manifest_id: manifest.manifest_id.to_string(),
                chunk_count: chunks.len(),
                total_size: manifest.total_size,
                root_hash: manifest.root_hash,
                proof_attached,
            })
        })
        .await
        .map_err(|e| {
            McpError::internal_error(format!("swarm_publish task join failed: {e}"), None)
        })??;
        Ok(Json(response))
    }

    #[tool(
        name = "swarm_fetch",
        description = "Resolve a locally cached swarm manifest, verify its chunks, and reassemble the asset. Local-only; this tool does not contact peers. Example: {\"manifest_id\":\"<hex>\",\"encoding\":\"base64\"}"
    )]
    pub async fn swarm_fetch(
        &self,
        Parameters(req): Parameters<SwarmFetchRequest>,
    ) -> Result<Json<SwarmFetchResponse>, McpError> {
        let manifest_id = req
            .manifest_id
            .parse::<ManifestId>()
            .map_err(|e| McpError::invalid_params(e, None))?;
        let state = self.state.clone();
        let encoding = req.encoding.clone();
        let response = tokio::task::spawn_blocking(move || {
            let guard = state.blocking_lock();
            let manifest = guard
                .swarm_store
                .get_manifest(&manifest_id)
                .cloned()
                .ok_or_else(|| McpError::invalid_params("unknown manifest id", None))?;
            let chunks = manifest
                .chunk_hashes
                .iter()
                .map(|chunk_id| {
                    guard
                        .swarm_store
                        .get_chunk(chunk_id)
                        .cloned()
                        .ok_or_else(|| {
                            McpError::invalid_params(
                                format!("missing local chunk {chunk_id}"),
                                None,
                            )
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let verification = verify_manifest(&manifest, &chunks);
            if !verification.accepted {
                return Err(McpError::internal_error(
                    "manifest verification failed",
                    None,
                ));
            }
            let rebuilt = reassemble_chunks(&chunks).map_err(|e| {
                McpError::internal_error(format!("reassemble swarm asset: {e}"), None)
            })?;
            let (data, resolved_encoding) =
                Self::encode_swarm_payload(&rebuilt, encoding.as_deref())?;
            Ok(SwarmFetchResponse {
                manifest_id: manifest.manifest_id.to_string(),
                chunk_count: chunks.len(),
                total_size: rebuilt.len(),
                encoding: resolved_encoding,
                data,
                verified: verification.accepted,
            })
        })
        .await
        .map_err(|e| {
            McpError::internal_error(format!("swarm_fetch task join failed: {e}"), None)
        })??;
        Ok(Json(response))
    }

    #[tool(
        name = "swarm_remote_fetch",
        description = "Reserved control-plane stub for future remote Bitswap retrieval. Returns a deferred status until a live P2P node handle is threaded into MCP service state."
    )]
    pub async fn swarm_remote_fetch(
        &self,
        Parameters(req): Parameters<SwarmRemoteFetchRequest>,
    ) -> Result<Json<SwarmRemoteFetchResponse>, McpError> {
        Ok(Json(SwarmRemoteFetchResponse {
            manifest_id: req.manifest_id,
            implemented: false,
            message: "swarm_remote_fetch is not yet implemented; MCP service state does not yet own a live P2P node handle for outbound Bitswap requests".to_string(),
            tracking_issue: "WIP/bitswap_remote_fetch_tracking_2026-03-08.md".to_string(),
        }))
    }

    #[tool(
        name = "swarm_status",
        description = "Report local swarm chunk/manifest inventory plus Bitswap runtime status."
    )]
    pub async fn swarm_status(&self) -> Result<Json<SwarmStatusResponse>, McpError> {
        let guard = self.state.lock().await;
        let store_stats = guard.swarm_store.stats();
        let bitswap_status = guard.bitswap_runtime.status();
        Ok(Json(SwarmStatusResponse {
            total_chunks: store_stats.total_chunks,
            total_bytes: store_stats.total_bytes,
            manifest_count: store_stats.manifest_count,
            active_transfers: bitswap_status.active_transfers,
            peer_count: bitswap_status.peer_count,
            bitswap_enabled: guard.swarm_config.bitswap_enabled,
            chunk_credit_cost: guard.swarm_config.chunk_credit_cost,
        }))
    }

    #[tool(
        name = "agenthalo_evidence_combine",
        description = "Combine multiple tool evidence records using Heyting-style vUpdate Bayesian odds with optional uncertainty translation."
    )]
    pub async fn evidence_combine(
        &self,
        Parameters(req): Parameters<EvidenceCombineRequest>,
    ) -> Result<Json<EvidenceCombineResponse>, McpError> {
        let prior_odds = if let Some(prior_probability_true) = req.prior_probability_true {
            if !prior_probability_true.is_finite() {
                return Err(McpError::invalid_params(
                    "prior_probability_true must be finite",
                    None,
                ));
            }
            crate::halo::evidence::probability_true_to_odds_false_true(prior_probability_true)
        } else {
            req.prior_odds_false_over_true.unwrap_or(1.0)
        };

        if !prior_odds.is_finite() || prior_odds < 0.0 {
            return Err(McpError::invalid_params(
                "prior odds must be finite and non-negative",
                None,
            ));
        }

        let combined = crate::halo::evidence::combine_evidence(prior_odds, &req.evidence);
        let translated_confidence = req.output_kind.map(|kind| {
            crate::halo::uncertainty::translate_uncertainty(
                crate::halo::uncertainty::UncertaintyKind::Probability,
                kind,
                combined.posterior_probability_true,
            )
        });

        Ok(Json(EvidenceCombineResponse {
            posterior_probability_true: combined.posterior_probability_true,
            posterior_odds_false_over_true: combined.posterior_odds_false_over_true,
            translated_confidence,
            output_kind: req.output_kind,
            steps: combined.steps,
        }))
    }

    #[tool(
        name = "agenthalo_uncertainty_translate",
        description = "Translate confidence values across uncertainty frameworks (probability, certainty_factor, possibility, binary)."
    )]
    pub async fn uncertainty_translate(
        &self,
        Parameters(req): Parameters<UncertaintyTranslateRequest>,
    ) -> Result<Json<UncertaintyTranslateResponse>, McpError> {
        use crate::halo::uncertainty::UncertaintyTranslator;

        let value = crate::halo::uncertainty::translate_uncertainty(req.from, req.to, req.value);
        Ok(Json(UncertaintyTranslateResponse {
            value,
            from_balanced: req.from.is_balanced(),
            to_balanced: req.to.is_balanced(),
        }))
    }

    #[tool(
        name = "nucleusdb_status",
        description = "Return backend, state sizes, and Signed Tree Head metadata for the active DB."
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
        description = "List commit history newest-first. Example: {\"limit\":20}"
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
        description = "Export current key/value state. Default format is legacy_v1 key->u64 map. Set format='typed_v2' for decoded typed entries."
    )]
    pub async fn export(
        &self,
        Parameters(req): Parameters<ExportRequest>,
    ) -> Result<Json<ExportResponse>, McpError> {
        let guard = self.state.lock().await;
        let format = Self::parse_export_format(req.format.as_deref())?;
        let key_count = guard.db.keymap.len();
        let (format_name, format_version, json) = match format {
            ExportFormat::LegacyV1 => {
                let payload = Self::export_legacy_map(&guard.db);
                let json = serde_json::to_string_pretty(&payload).map_err(|e| {
                    McpError::internal_error(
                        format!("failed to encode legacy export JSON: {e}"),
                        None,
                    )
                })?;
                (
                    "legacy_v1".to_string(),
                    "nucleusdb_export/legacy_v1".to_string(),
                    json,
                )
            }
            ExportFormat::TypedV2 => {
                let entries = Self::export_typed_entries(&guard.db)
                    .map_err(|e| McpError::internal_error(e, None))?;
                let json = serde_json::to_string_pretty(&entries).map_err(|e| {
                    McpError::internal_error(
                        format!("failed to encode typed export JSON: {e}"),
                        None,
                    )
                })?;
                (
                    "typed_v2".to_string(),
                    "nucleusdb_export/typed_v2".to_string(),
                    json,
                )
            }
        };
        Ok(Json(ExportResponse {
            key_count,
            format: format_name,
            format_version,
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

    // Kept as an internal helper (not exposed in the constrained 25-tool surface).
    pub async fn open_channel(
        &self,
        Parameters(req): Parameters<OpenChannelRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        let mut guard = self.state.lock().await;
        SettlementOp::Open {
            p1: req.p1.clone(),
            p2: req.p2.clone(),
            capacity: req.capacity,
        }
        .apply(&mut guard.db)
        .map_err(|e| McpError::invalid_params(format!("open channel failed: {e:?}"), None))?;
        persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
        })?;
        Ok(Json(OperationStatus {
            ok: true,
            message: "channel opened".to_string(),
            db_path: guard.db_path.display().to_string(),
            wal_path: guard.wal_path.display().to_string(),
            backend: Self::backend_label(&guard.db.backend).to_string(),
        }))
    }

    // Kept as an internal helper (not exposed in the constrained 25-tool surface).
    pub async fn update_channel(
        &self,
        Parameters(req): Parameters<UpdateChannelRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        let mut guard = self.state.lock().await;
        SettlementOp::Update {
            p1: req.p1.clone(),
            p2: req.p2.clone(),
            balance1: req.balance1,
            balance2: req.balance2,
        }
        .apply(&mut guard.db)
        .map_err(|e| McpError::invalid_params(format!("update channel failed: {e:?}"), None))?;
        persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
        })?;
        Ok(Json(OperationStatus {
            ok: true,
            message: "channel updated".to_string(),
            db_path: guard.db_path.display().to_string(),
            wal_path: guard.wal_path.display().to_string(),
            backend: Self::backend_label(&guard.db.backend).to_string(),
        }))
    }

    // Kept as an internal helper (not exposed in the constrained 25-tool surface).
    pub async fn close_channel(
        &self,
        Parameters(req): Parameters<CloseChannelRequest>,
    ) -> Result<Json<OperationStatus>, McpError> {
        let mut guard = self.state.lock().await;
        SettlementOp::Close {
            p1: req.p1.clone(),
            p2: req.p2.clone(),
        }
        .apply(&mut guard.db)
        .map_err(|e| McpError::invalid_params(format!("close channel failed: {e:?}"), None))?;
        persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
        })?;
        Ok(Json(OperationStatus {
            ok: true,
            message: "channel closed".to_string(),
            db_path: guard.db_path.display().to_string(),
            wal_path: guard.wal_path.display().to_string(),
            backend: Self::backend_label(&guard.db.backend).to_string(),
        }))
    }

    // Kept as an internal helper (not exposed in the constrained 25-tool surface).
    pub async fn query_channel(
        &self,
        Parameters(req): Parameters<QueryChannelRequest>,
    ) -> Result<Json<ChannelSnapshot>, McpError> {
        let guard = self.state.lock().await;
        let snapshot = channel_snapshot(&guard.db, &req.p1, &req.p2)
            .ok_or_else(|| McpError::invalid_params("channel not found", None))?;
        Ok(Json(snapshot))
    }

    #[tool(
        name = "nucleusdb_container_launch",
        description = "Launch a monitored native session. Supports mesh networking and env injection. Example: {\"image\":\"nucleusdb-agent:latest\",\"agent_id\":\"agent-a\",\"command\":[\"/bin/sh\",\"-lc\",\"echo hello\"],\"env\":{\"AGENTHALO_MCP_SECRET\":\"...\"},\"mesh\":{\"enabled\":true,\"mcp_port\":8420,\"registry_volume\":\"/tmp/nucleusdb-mesh\"}}"
    )]
    pub async fn container_launch(
        &self,
        Parameters(req): Parameters<ContainerLaunchRequest>,
    ) -> Result<Json<ContainerLaunchResponse>, McpError> {
        let admission_mode = AdmissionMode::parse(req.admission_mode.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let guard = self.state.lock().await;
        let admission =
            evaluate_launch_admission(admission_mode, Some(&guard.governor_registry), None);
        if !admission.allowed {
            let reason = admission
                .issues
                .iter()
                .map(|issue| issue.message.clone())
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(McpError::invalid_params(
                format!("AETHER admission policy blocked container launch: {reason}"),
                None,
            ));
        }
        let command = if req.command.is_empty() {
            vec![
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "echo nucleusdb-sidecar".to_string(),
            ]
        } else {
            req.command
        };
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
        let info = launch_container(RunConfig {
            image: req.image.clone(),
            agent_id: req.agent_id.clone(),
            command,
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
            admission_mode: Some(admission.mode),
            admission_forced: admission.forced,
            admission_issues: admission
                .issues
                .into_iter()
                .map(|issue| issue.message)
                .collect(),
        }))
    }

    #[tool(
        name = "nucleusdb_container_provision",
        description = "Provision a new EMPTY container ready for a later initialize step. If command is omitted, starts the AgentHALO MCP server entrypoint."
    )]
    pub async fn container_provision(
        &self,
        Parameters(req): Parameters<ContainerLaunchRequest>,
    ) -> Result<Json<ContainerLaunchResponse>, McpError> {
        let admission_mode = AdmissionMode::parse(req.admission_mode.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let guard = self.state.lock().await;
        let admission =
            evaluate_launch_admission(admission_mode, Some(&guard.governor_registry), None);
        if !admission.allowed {
            let reason = admission
                .issues
                .iter()
                .map(|issue| issue.message.clone())
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(McpError::invalid_params(
                format!("AETHER admission policy blocked container provision: {reason}"),
                None,
            ));
        }
        drop(guard);
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
        let info = launch_container(RunConfig {
            image: req.image.clone(),
            agent_id: req.agent_id.clone(),
            command: if req.command.is_empty() {
                vec!["agenthalo-mcp-server".to_string()]
            } else {
                req.command
            },
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
            admission_mode: Some(admission.mode),
            admission_forced: admission.forced,
            admission_issues: admission
                .issues
                .into_iter()
                .map(|issue| issue.message)
                .collect(),
        }))
    }

    #[tool(
        name = "nucleusdb_container_list",
        description = "List tracked container sessions launched by NucleusDB tooling."
    )]
    pub async fn container_list(&self) -> Result<Json<ContainerListResponse>, McpError> {
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

    #[tool(
        name = "nucleusdb_subsidiary_provision",
        description = "Operator-only: provision a new EMPTY subsidiary container and register ownership."
    )]
    pub async fn subsidiary_provision(
        &self,
        Parameters(req): Parameters<SubsidiaryProvisionRequest>,
    ) -> Result<Json<SubsidiaryProvisionResponse>, McpError> {
        self.subsidiary_provision_internal(req, None).await
    }

    pub async fn subsidiary_provision_with_orchestrator(
        &self,
        req: SubsidiaryProvisionRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryProvisionResponse>, McpError> {
        self.subsidiary_provision_internal(req, Some(orchestrator))
            .await
    }

    async fn subsidiary_provision_internal(
        &self,
        req: SubsidiaryProvisionRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryProvisionResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        let Json(provisioned) = self
            .container_provision(Parameters(ContainerLaunchRequest {
                image: req.image,
                agent_id: req.agent_id,
                command: req.command,
                host_sock: req.host_sock,
                env: req.env,
                mesh: req.mesh,
                admission_mode: req.admission_mode,
            }))
            .await?;
        let (_registry_lock, mut registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
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
        name = "nucleusdb_subsidiary_initialize",
        description = "Operator-only: initialize an owned EMPTY subsidiary container with an agent hookup."
    )]
    pub async fn subsidiary_initialize(
        &self,
        Parameters(req): Parameters<SubsidiaryInitializeRequest>,
    ) -> Result<Json<SubsidiaryInitializeResponse>, McpError> {
        self.subsidiary_initialize_internal(req, None).await
    }

    pub async fn subsidiary_initialize_with_orchestrator(
        &self,
        req: SubsidiaryInitializeRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryInitializeResponse>, McpError> {
        self.subsidiary_initialize_internal(req, Some(orchestrator))
            .await
    }

    async fn subsidiary_initialize_internal(
        &self,
        req: SubsidiaryInitializeRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryInitializeResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        let (_registry_lock, mut registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
        let owned = registry
            .assert_owned(&req.session_id)
            .map_err(|e| McpError::invalid_params(e, None))?
            .clone();
        let peer =
            Self::peer_for_subsidiary_with_timeout(&owned, SUBSIDIARY_PEER_REGISTRATION_TIMEOUT)
                .await?;
        Self::wait_for_subsidiary_peer_health(&owned, SUBSIDIARY_PEER_HEALTH_TIMEOUT).await?;
        let value = tokio::task::spawn_blocking({
            let hookup = req.hookup.clone();
            let reuse_policy = req.reuse_policy.unwrap_or(ReusePolicy::Reusable);
            let auth_token = mesh_auth_token();
            move || {
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
            }
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
        name = "nucleusdb_subsidiary_send_task",
        description = "Operator-only: send a prompt to an owned subsidiary agent and persist the result in the subsidiary registry."
    )]
    pub async fn subsidiary_send_task(
        &self,
        Parameters(req): Parameters<SubsidiarySendTaskRequest>,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.subsidiary_send_task_internal(req, None).await
    }

    pub async fn subsidiary_send_task_with_orchestrator(
        &self,
        req: SubsidiarySendTaskRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.subsidiary_send_task_internal(req, Some(orchestrator))
            .await
    }

    async fn subsidiary_send_task_internal(
        &self,
        req: SubsidiarySendTaskRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        if req.prompt.trim().is_empty() {
            return Err(McpError::invalid_params("prompt must be non-empty", None));
        }
        let (_registry_lock, mut registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
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

    #[tool(
        name = "nucleusdb_subsidiary_get_result",
        description = "Operator-only: fetch a persisted subsidiary task result by task id."
    )]
    pub async fn subsidiary_get_result(
        &self,
        Parameters(req): Parameters<SubsidiaryGetResultRequest>,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.subsidiary_get_result_internal(req, None).await
    }

    pub async fn subsidiary_get_result_with_orchestrator(
        &self,
        req: SubsidiaryGetResultRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.subsidiary_get_result_internal(req, Some(orchestrator))
            .await
    }

    async fn subsidiary_get_result_internal(
        &self,
        req: SubsidiaryGetResultRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryTaskResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        let (_registry_lock, registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
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
        name = "nucleusdb_subsidiary_deinitialize",
        description = "Operator-only: deinitialize an owned subsidiary agent and return it to EMPTY."
    )]
    pub async fn subsidiary_deinitialize(
        &self,
        Parameters(req): Parameters<SubsidiaryDeinitializeRequest>,
    ) -> Result<Json<SubsidiaryDeinitializeResponse>, McpError> {
        self.subsidiary_deinitialize_internal(req, None).await
    }

    pub async fn subsidiary_deinitialize_with_orchestrator(
        &self,
        req: SubsidiaryDeinitializeRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryDeinitializeResponse>, McpError> {
        self.subsidiary_deinitialize_internal(req, Some(orchestrator))
            .await
    }

    async fn subsidiary_deinitialize_internal(
        &self,
        req: SubsidiaryDeinitializeRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryDeinitializeResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        let (_registry_lock, mut registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
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
        name = "nucleusdb_subsidiary_destroy",
        description = "Operator-only: destroy an owned subsidiary container and remove it from the operator registry."
    )]
    pub async fn subsidiary_destroy(
        &self,
        Parameters(req): Parameters<SubsidiaryDestroyRequest>,
    ) -> Result<Json<SubsidiaryDestroyResponse>, McpError> {
        self.subsidiary_destroy_internal(req, None).await
    }

    pub async fn subsidiary_destroy_with_orchestrator(
        &self,
        req: SubsidiaryDestroyRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryDestroyResponse>, McpError> {
        self.subsidiary_destroy_internal(req, Some(orchestrator))
            .await
    }

    async fn subsidiary_destroy_internal(
        &self,
        req: SubsidiaryDestroyRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryDestroyResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        let (_registry_lock, mut registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
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
        name = "nucleusdb_subsidiary_list",
        description = "Operator-only: list subsidiaries owned by the operator with current status."
    )]
    pub async fn subsidiary_list(
        &self,
        Parameters(req): Parameters<SubsidiaryListRequest>,
    ) -> Result<Json<SubsidiaryListResponse>, McpError> {
        self.subsidiary_list_internal(req, None).await
    }

    pub async fn subsidiary_list_with_orchestrator(
        &self,
        req: SubsidiaryListRequest,
        orchestrator: Orchestrator,
    ) -> Result<Json<SubsidiaryListResponse>, McpError> {
        self.subsidiary_list_internal(req, Some(orchestrator)).await
    }

    async fn subsidiary_list_internal(
        &self,
        req: SubsidiaryListRequest,
        orchestrator_override: Option<Orchestrator>,
    ) -> Result<Json<SubsidiaryListResponse>, McpError> {
        self.require_operator_capability_with(&req.operator_agent_id, orchestrator_override)
            .await?;
        let (_registry_lock, registry) =
            Self::load_operator_registry_locked(&req.operator_agent_id)?;
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

    #[tool(
        name = "agenthalo_governor_status",
        description = "Return AETHER governor status for runtime lanes plus the gov-memory aggregate. Example: {\"instance_id\":\"gov-compute\"}"
    )]
    pub async fn governor_status(
        &self,
        Parameters(req): Parameters<GovernorStatusRequest>,
    ) -> Result<Json<GovernorStatusToolResponse>, McpError> {
        let mut guard = self.state.lock().await;
        if guard.db.aether_maintenance_tick(now_unix_secs()) {
            persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                |e| McpError::internal_error(format!("persist governor maintenance: {e:?}"), None),
            )?;
        }
        let vector_stats = guard.db.vector_index.eviction_stats();
        let blob_stats = guard.db.blob_store.stats();
        let mean_error = (vector_stats.governor_error + blob_stats.governor_error) / 2.0;
        let memory_snapshot = serde_json::json!({
            "instance_id": "gov-memory",
            "epsilon": (vector_stats.governor_epsilon + blob_stats.governor_epsilon) / 2.0,
            "target": (vector_stats.max_entries + blob_stats.max_entries) as f64 / 2.0,
            "measured_signal": (vector_stats.tracked_vectors + blob_stats.tracked_blobs) as f64 / 2.0,
            "error": mean_error,
            "lyapunov": 0.5 * mean_error * mean_error,
            "regime": "engineering aggregate (formal scope exited after first storage observation)",
            "gamma": vector_stats.governor_gamma.max(blob_stats.governor_gamma),
            "contraction_bound": vector_stats
                .governor_contraction_bound
                .max(blob_stats.governor_contraction_bound),
            "stable": vector_stats.governor_stable && blob_stats.governor_stable,
            "oscillating": vector_stats.governor_oscillating || blob_stats.governor_oscillating,
            "gain_violated": vector_stats.governor_gain_violated || blob_stats.governor_gain_violated,
            "clamp_active": vector_stats.governor_clamp_active || blob_stats.governor_clamp_active,
            "formal_basis": "HeytingLean.Bridge.Sharma.AetherGovernor.validatorRegime",
            "sparkline": [vector_stats.governor_epsilon, blob_stats.governor_epsilon],
            "last_updated_unix": now_unix_secs(),
            "warning": "gov-memory aggregates two storage-local engineering governors plus Chebyshev wrapper telemetry; this aggregate is not itself formally verified and exits the single-step scope after the first storage observation."
        });
        let instances = match req
            .instance_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            Some("gov-memory") => Vec::new(),
            Some(instance_id) => vec![guard
                .governor_registry
                .snapshot_one(instance_id)
                .map_err(|e| McpError::invalid_params(e, None))?],
            None => guard.governor_registry.snapshot_all(),
        };
        let instances = serde_json::to_value(instances)
            .map_err(|e| {
                McpError::internal_error(format!("serialize governor instances: {e}"), None)
            })?
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(Json(GovernorStatusToolResponse {
            instances,
            memory: memory_snapshot,
        }))
    }

    #[tool(
        name = "agenthalo_governor_reset",
        description = "Soft-reset a governor instance, gov-memory, or all governors back to from-rest when quiescent. Example: {\"instance_id\":\"gov-compute\"}"
    )]
    pub async fn governor_reset(
        &self,
        Parameters(req): Parameters<GovernorResetToolRequest>,
    ) -> Result<Json<GovernorStatusToolResponse>, McpError> {
        {
            let mut guard = self.state.lock().await;
            let instance_id = req.instance_id.unwrap_or_else(|| "all".to_string());
            let reset_all = req.all.unwrap_or(false) || instance_id == "all";

            if reset_all {
                for runtime_id in [
                    "gov-proxy",
                    "gov-comms",
                    "gov-compute",
                    "gov-cost",
                    "gov-pty",
                ] {
                    guard
                        .governor_registry
                        .soft_reset(runtime_id)
                        .map_err(|e| McpError::invalid_params(e, None))?;
                }
            } else if instance_id != "gov-memory" {
                guard
                    .governor_registry
                    .soft_reset(&instance_id)
                    .map_err(|e| McpError::invalid_params(e, None))?;
            }

            if reset_all || instance_id == "gov-memory" {
                guard.db.soft_reset_aether_memory();
                persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(
                    |e| McpError::internal_error(format!("persist governor reset: {e:?}"), None),
                )?;
            }
        }
        self.governor_status(Parameters(GovernorStatusRequest { instance_id: None }))
            .await
    }

    #[tool(
        name = "agenthalo_deploy_preflight",
        description = "Inspect CLI readiness, topology drift, and AETHER admission before cockpit launch. Example: {\"agent_id\":\"codex\",\"admission_mode\":\"block\"}"
    )]
    pub async fn deploy_preflight(
        &self,
        Parameters(req): Parameters<DeployPreflightToolRequest>,
    ) -> Result<Json<DeployPreflightToolResponse>, McpError> {
        let guard = self.state.lock().await;
        let admission_mode = AdmissionMode::parse(req.admission_mode.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let result = deploy::preflight(
            &req.agent_id,
            guard.vault.as_deref(),
            Some(&guard.db_path),
            Some(&guard.governor_registry),
            admission_mode,
        )
        .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(DeployPreflightToolResponse {
            result: serde_json::to_value(result).map_err(|e| {
                McpError::internal_error(format!("serialize deploy preflight: {e}"), None)
            })?,
        }))
    }

    #[tool(
        name = "agenthalo_deploy_launch",
        description = "Launch a cockpit-managed agent session with AETHER admission control. Example: {\"agent_id\":\"codex\",\"mode\":\"terminal\",\"admission_mode\":\"block\"}"
    )]
    pub async fn deploy_launch(
        &self,
        Parameters(req): Parameters<DeployLaunchToolRequest>,
    ) -> Result<Json<DeployLaunchToolResponse>, McpError> {
        let guard = self.state.lock().await;
        let result = deploy::launch(
            &deploy::LaunchRequest {
                agent_id: req.agent_id,
                mode: req.mode,
                working_dir: req.working_dir,
                admission_mode: req.admission_mode,
                workspace_profile: None,
            },
            &guard.pty_manager,
            guard.vault.as_deref(),
            Some(&guard.db_path),
            Some(&guard.governor_registry),
        )
        .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(DeployLaunchToolResponse {
            result: serde_json::to_value(result).map_err(|e| {
                McpError::internal_error(format!("serialize deploy launch: {e}"), None)
            })?,
        }))
    }

    #[tool(
        name = "agenthalo_deploy_status",
        description = "Return current cockpit deploy session status by session id. Example: {\"session_id\":\"pty-...\"}"
    )]
    pub async fn deploy_status(
        &self,
        Parameters(req): Parameters<DeployStatusToolRequest>,
    ) -> Result<Json<DeployStatusToolResponse>, McpError> {
        let guard = self.state.lock().await;
        let session = guard
            .pty_manager
            .get_session(&req.session_id)
            .ok_or_else(|| McpError::invalid_params("session not found", None))?;
        Ok(Json(DeployStatusToolResponse {
            id: req.session_id,
            status: format!("{:?}", session.status()),
        }))
    }

    #[tool(
        name = "nucleusdb_container_status",
        description = "Get runtime status for a tracked container session. Example: {\"session_id\":\"sess-...\"}"
    )]
    pub async fn container_status(
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

    #[tool(
        name = "nucleusdb_container_lock_status",
        description = "Return the current container agent lock state for this runtime. Useful before container initialize/deinitialize flows."
    )]
    pub async fn container_lock_status(
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

    #[tool(
        name = "nucleusdb_container_initialize",
        description = "Initialize the current EMPTY container with an agent hookup. Example: {\"hookup\":{\"kind\":\"cli\",\"cli_name\":\"claude\",\"model\":\"claude-3.7-sonnet\"},\"reuse_policy\":\"reusable\"}"
    )]
    pub async fn container_initialize(
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

    #[tool(
        name = "nucleusdb_container_agent_prompt",
        description = "Send a prompt to the initialized agent hookup in this container. Example: {\"prompt\":\"Review src/main.rs\"}"
    )]
    pub async fn container_agent_prompt(
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
        name = "nucleusdb_container_deinitialize",
        description = "Deinitialize the current container agent hookup and return the lock to EMPTY. Example: {}"
    )]
    pub async fn container_deinitialize(
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

    #[tool(
        name = "nucleusdb_container_stop",
        description = "Stop a running container session. Example: {\"session_id\":\"sess-...\"}"
    )]
    pub async fn container_stop(
        &self,
        Parameters(req): Parameters<ContainerStopRequest>,
    ) -> Result<Json<ContainerStopResponse>, McpError> {
        launcher_stop_container(&req.session_id).map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(ContainerStopResponse {
            session_id: req.session_id,
            stopped: true,
        }))
    }

    #[tool(
        name = "nucleusdb_container_logs",
        description = "Fetch container logs for a tracked session. Example: {\"session_id\":\"sess-...\",\"follow\":false}"
    )]
    pub async fn container_logs(
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
        name = "nucleusdb_agent_register",
        description = "Prepare or submit an on-chain trust attestation. Example: {\"contract_address\":\"0x...\",\"rpc_url\":\"https://...\",\"proof_hex\":\"0x...\",\"submit_onchain\":false}"
    )]
    pub async fn agent_register(
        &self,
        Parameters(req): Parameters<AgentRegisterRequest>,
    ) -> Result<Json<AgentRegisterResponse>, McpError> {
        let guard = self.state.lock().await;
        let bundle = prepare_attestation_bundle(&guard.db, req.tier_override)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        drop(guard);

        let contract_address = req.contract_address.unwrap_or_default();
        let rpc_url = req.rpc_url.unwrap_or_default();
        let submit_onchain = req.submit_onchain.unwrap_or(false);
        let private_key_env = req
            .private_key_env
            .unwrap_or_else(|| "NUCLEUSDB_AGENT_PRIVATE_KEY".to_string());
        let keystore_path = req.keystore_path.as_deref().map(str::trim).unwrap_or("");
        let keystore_password_file = req
            .keystore_password_file
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let proof_for_preview = req.proof_hex.as_deref().unwrap_or("0x");

        let command_preview = if !contract_address.trim().is_empty() && !rpc_url.trim().is_empty() {
            if !keystore_path.is_empty() {
                build_attest_command_preview_with_keystore(
                    contract_address.trim(),
                    rpc_url.trim(),
                    proof_for_preview,
                    &bundle.public_signals,
                    keystore_path,
                    keystore_password_file,
                )
            } else {
                build_attest_command_preview_with_private_key_env(
                    contract_address.trim(),
                    rpc_url.trim(),
                    proof_for_preview,
                    &bundle.public_signals,
                    &private_key_env,
                )
            }
        } else {
            "set contract_address and rpc_url to generate cast command preview".to_string()
        };

        let mut submitted = false;
        let mut tx_hash = None;
        let mut verify_after_submit = None;
        let message = if submit_onchain {
            if contract_address.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "contract_address is required when submit_onchain=true",
                    None,
                ));
            }
            if rpc_url.trim().is_empty() {
                return Err(McpError::invalid_params(
                    "rpc_url is required when submit_onchain=true",
                    None,
                ));
            }
            let proof_hex = req.proof_hex.ok_or_else(|| {
                McpError::invalid_params("proof_hex is required when submit_onchain=true", None)
            })?;
            if !proof_hex.starts_with("0x") {
                return Err(McpError::invalid_params(
                    "proof_hex must be 0x-prefixed",
                    None,
                ));
            }
            let signer = if !keystore_path.is_empty() {
                CastSigner::Keystore {
                    path: keystore_path.to_string(),
                    password_file: keystore_password_file.map(ToOwned::to_owned),
                }
            } else {
                let private_key = load_private_key_env(&private_key_env)
                    .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
                CastSigner::PrivateKey(private_key)
            };
            tx_hash = send_attestation(
                rpc_url.trim(),
                contract_address.trim(),
                &proof_hex,
                &bundle.public_signals,
                &signer,
            )
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
            submitted = true;
            if let Some(agent_address) = req.agent_address.as_deref() {
                if !agent_address.trim().is_empty() {
                    verify_after_submit = Some(Self::verify_agent_bridge(
                        rpc_url.trim(),
                        contract_address.trim(),
                        agent_address.trim(),
                    )?);
                }
            }
            "attestation submitted".to_string()
        } else {
            "attestation payload prepared (dry run)".to_string()
        };

        Ok(Json(AgentRegisterResponse {
            prepared: true,
            submitted,
            tx_hash,
            puf_digest: bundle.puf_digest_hex,
            puf_tier: bundle.puf_tier,
            puf_tier_label: bundle.puf_tier_label,
            replay_seq: bundle.replay_seq,
            feasibility_root: bundle.feasibility_root_hex,
            public_signals: bundle.public_signals,
            compliance_inputs_json: serde_json::to_string_pretty(&bundle.compliance_inputs)
                .map_err(|e| McpError::internal_error(e.to_string(), None))?,
            command_preview,
            verify_after_submit,
            message,
        }))
    }

    #[tool(
        name = "nucleusdb_verify_agent",
        description = "Verify an on-chain agent attestation and return current TrustVerifier status. Example: {\"contract_address\":\"0x...\",\"rpc_url\":\"https://...\",\"agent_address\":\"0x...\"}"
    )]
    pub async fn verify_agent(
        &self,
        Parameters(req): Parameters<AgentVerifyRequest>,
    ) -> Result<Json<AgentVerifyResponse>, McpError> {
        if req.contract_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "contract_address must be non-empty",
                None,
            ));
        }
        if req.rpc_url.trim().is_empty() {
            return Err(McpError::invalid_params("rpc_url must be non-empty", None));
        }
        if req.agent_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "agent_address must be non-empty",
                None,
            ));
        }
        let resp = Self::verify_agent_bridge(
            req.rpc_url.trim(),
            req.contract_address.trim(),
            req.agent_address.trim(),
        )?;
        Ok(Json(resp))
    }

    // Kept as an internal helper (not exposed in the constrained 25-tool surface).
    pub async fn agent_reattest(
        &self,
        Parameters(req): Parameters<AgentReattestRequest>,
    ) -> Result<Json<AgentReattestResponse>, McpError> {
        if req.contract_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "contract_address must be non-empty",
                None,
            ));
        }
        if req.rpc_url.trim().is_empty() {
            return Err(McpError::invalid_params("rpc_url must be non-empty", None));
        }
        if req.agent_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "agent_address must be non-empty",
                None,
            ));
        }
        let rpc_url = req.rpc_url.trim();
        let contract_address = req.contract_address.trim();
        let agent_address = req.agent_address.trim().to_string();
        let stale_after_secs = req.stale_after_secs.unwrap_or(3600);
        let force = req.force.unwrap_or(false);

        let current_status = Self::verify_agent_bridge(rpc_url, contract_address, &agent_address)?;
        let now = now_unix_secs();
        let (should_reattest, reason) = if force {
            (true, "forced")
        } else if !current_status.verified {
            (true, "verifyAgent returned false")
        } else if current_status.active == Some(false) {
            (true, "agent status inactive")
        } else {
            match current_status.last_attestation {
                None => (true, "no previous attestation timestamp"),
                Some(ts) => {
                    let age = now.saturating_sub(ts);
                    if age > stale_after_secs {
                        (true, "attestation stale")
                    } else {
                        (false, "attestation fresh")
                    }
                }
            }
        };

        if !should_reattest {
            return Ok(Json(AgentReattestResponse {
                agent_address,
                should_reattest: false,
                reason: reason.to_string(),
                submitted: false,
                tx_hash: None,
                next_public_signals: None,
                next_replay_seq: None,
                command_preview: None,
                verify_after_submit: None,
                current_status,
            }));
        }

        let guard = self.state.lock().await;
        let bundle = prepare_attestation_bundle(&guard.db, req.tier_override)
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        drop(guard);

        let private_key_env = req
            .private_key_env
            .unwrap_or_else(|| "NUCLEUSDB_AGENT_PRIVATE_KEY".to_string());
        let keystore_path = req.keystore_path.as_deref().map(str::trim).unwrap_or("");
        let keystore_password_file = req
            .keystore_password_file
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let proof_for_preview = req.proof_hex.as_deref().unwrap_or("0x");
        let command_preview = Some(if !keystore_path.is_empty() {
            build_attest_command_preview_with_keystore(
                contract_address,
                rpc_url,
                proof_for_preview,
                &bundle.public_signals,
                keystore_path,
                keystore_password_file,
            )
        } else {
            build_attest_command_preview_with_private_key_env(
                contract_address,
                rpc_url,
                proof_for_preview,
                &bundle.public_signals,
                &private_key_env,
            )
        });

        let submit_onchain = req.submit_onchain.unwrap_or(false);
        if !submit_onchain {
            return Ok(Json(AgentReattestResponse {
                agent_address,
                should_reattest: true,
                reason: reason.to_string(),
                submitted: false,
                tx_hash: None,
                next_public_signals: Some(bundle.public_signals),
                next_replay_seq: Some(bundle.replay_seq),
                command_preview,
                verify_after_submit: None,
                current_status,
            }));
        }

        let proof_hex = req.proof_hex.ok_or_else(|| {
            McpError::invalid_params("proof_hex is required when submit_onchain=true", None)
        })?;
        if !proof_hex.starts_with("0x") {
            return Err(McpError::invalid_params(
                "proof_hex must be 0x-prefixed",
                None,
            ));
        }
        let signer = if !keystore_path.is_empty() {
            CastSigner::Keystore {
                path: keystore_path.to_string(),
                password_file: keystore_password_file.map(ToOwned::to_owned),
            }
        } else {
            let private_key = load_private_key_env(&private_key_env)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
            CastSigner::PrivateKey(private_key)
        };
        let tx_hash = send_attestation(
            rpc_url,
            contract_address,
            &proof_hex,
            &bundle.public_signals,
            &signer,
        )
        .map_err(|e| McpError::invalid_params(e.to_string(), None))?;

        let verify_after_submit = Some(Self::verify_agent_bridge(
            rpc_url,
            contract_address,
            &agent_address,
        )?);

        Ok(Json(AgentReattestResponse {
            agent_address,
            should_reattest: true,
            reason: reason.to_string(),
            submitted: true,
            tx_hash,
            next_public_signals: Some(bundle.public_signals),
            next_replay_seq: Some(bundle.replay_seq),
            command_preview,
            verify_after_submit,
            current_status,
        }))
    }

    #[tool(
        name = "register_chain",
        description = "Register a chain in TrustVerifierMultiChain and optionally set chain/default fee. Example: {\"contract_address\":\"0x...\",\"rpc_url\":\"https://...\",\"chain_id\":8453,\"verifier_address\":\"0x...\",\"chain_fee\":1}"
    )]
    pub async fn register_chain(
        &self,
        Parameters(req): Parameters<RegisterChainRequest>,
    ) -> Result<Json<RegisterChainResponse>, McpError> {
        if req.contract_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "contract_address must be non-empty",
                None,
            ));
        }
        if req.rpc_url.trim().is_empty() {
            return Err(McpError::invalid_params("rpc_url must be non-empty", None));
        }
        if req.verifier_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "verifier_address must be non-empty",
                None,
            ));
        }

        let env_name = req
            .private_key_env
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("NUCLEUSDB_AGENT_PRIVATE_KEY")
            .to_string();
        let signer = Self::resolve_signer(
            Some(&env_name),
            req.keystore_path.as_deref(),
            req.keystore_password_file.as_deref(),
        )?;

        let metadata_hash = Self::normalize_metadata_hash(
            req.chain_id,
            req.verifier_address.trim(),
            req.metadata_hash.as_deref(),
        );
        if !(metadata_hash.starts_with("0x") && metadata_hash.len() == 66) {
            return Err(McpError::invalid_params(
                "metadata_hash must be 0x-prefixed bytes32",
                None,
            ));
        }

        let register_selector = "registerChain(uint256,address,bytes32)";
        let register_args = vec![
            req.chain_id.to_string(),
            req.verifier_address.trim().to_string(),
            metadata_hash.clone(),
        ];
        let register_tx_hash = Self::cast_send(
            req.rpc_url.trim(),
            req.contract_address.trim(),
            register_selector,
            register_args.clone(),
            &signer,
        )?;

        let mut chain_fee_tx_hash = None;
        let mut default_fee_tx_hash = None;
        if let Some(fee) = req.chain_fee {
            chain_fee_tx_hash = Self::cast_send(
                req.rpc_url.trim(),
                req.contract_address.trim(),
                "setChainFee(uint256,uint256)",
                vec![req.chain_id.to_string(), fee.to_string()],
                &signer,
            )?;
        }
        if let Some(fee) = req.default_fee {
            default_fee_tx_hash = Self::cast_send(
                req.rpc_url.trim(),
                req.contract_address.trim(),
                "setDefaultFee(uint256)",
                vec![fee.to_string()],
                &signer,
            )?;
        }

        let signer_preview = match &signer {
            CastSigner::PrivateKey(_) => format!("--private-key ${env_name}"),
            CastSigner::Keystore {
                path,
                password_file,
            } => {
                let mut v = format!("--keystore {path}");
                if let Some(p) = password_file {
                    v.push_str(&format!(" --password-file {p}"));
                }
                v
            }
        };
        let mut command_preview = vec![format!(
            "cast send --async --rpc-url {} {} {} \"{}\" {} {} {}",
            req.rpc_url.trim(),
            signer_preview,
            req.contract_address.trim(),
            register_selector,
            req.chain_id,
            req.verifier_address.trim(),
            metadata_hash
        )];
        if let Some(fee) = req.chain_fee {
            command_preview.push(format!(
                "cast send --async --rpc-url {} {} {} \"setChainFee(uint256,uint256)\" {} {}",
                req.rpc_url.trim(),
                signer_preview,
                req.contract_address.trim(),
                req.chain_id,
                fee
            ));
        }
        if let Some(fee) = req.default_fee {
            command_preview.push(format!(
                "cast send --async --rpc-url {} {} {} \"setDefaultFee(uint256)\" {}",
                req.rpc_url.trim(),
                signer_preview,
                req.contract_address.trim(),
                fee
            ));
        }

        Ok(Json(RegisterChainResponse {
            registered: true,
            chain_id: req.chain_id,
            register_tx_hash,
            chain_fee_tx_hash,
            default_fee_tx_hash,
            command_preview,
            message: "chain registered".to_string(),
        }))
    }

    // Kept as an internal helper for multichain workflows.
    pub async fn generate_composite_cab(
        &self,
        Parameters(req): Parameters<GenerateCompositeCabRequest>,
    ) -> Result<Json<GenerateCompositeCabResponse>, McpError> {
        if req.chain_ids.is_empty() {
            return Err(McpError::invalid_params(
                "chain_ids must be non-empty",
                None,
            ));
        }
        let guard = self.state.lock().await;
        let generator = CompositeCabGenerator::new(&guard.db, req.chain_ids.clone())
            .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
        let proof = generator
            .generate_proof()
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let composite_cab_hash = proof.composite_cab_hash_hex();
        Ok(Json(GenerateCompositeCabResponse {
            ok: true,
            chain_ids: proof.chain_ids.clone(),
            replay_seq: proof.replay_seq,
            composite_cab_hash,
            proof_hex: proof.proof_hex,
            public_signals: proof.public_signals,
            note: "composite CAB proof generated".to_string(),
        }))
    }

    // Kept as an internal helper (not exposed in the constrained 25-tool surface).
    pub async fn submit_composite_attestation(
        &self,
        Parameters(req): Parameters<SubmitCompositeAttestationRequest>,
    ) -> Result<Json<SubmitCompositeAttestationResponse>, McpError> {
        if req.contract_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "contract_address must be non-empty",
                None,
            ));
        }
        if req.rpc_url.trim().is_empty() {
            return Err(McpError::invalid_params("rpc_url must be non-empty", None));
        }
        if req.chain_ids.is_empty() {
            return Err(McpError::invalid_params(
                "chain_ids must be non-empty",
                None,
            ));
        }
        if !req.proof_hex.starts_with("0x") {
            return Err(McpError::invalid_params(
                "proof_hex must be 0x-prefixed",
                None,
            ));
        }

        let env_name = req
            .private_key_env
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("NUCLEUSDB_AGENT_PRIVATE_KEY")
            .to_string();
        let signer = Self::resolve_signer(
            Some(&env_name),
            req.keystore_path.as_deref(),
            req.keystore_password_file.as_deref(),
        )?;

        let public_signals = if let Some(signals) = req.public_signals.clone() {
            if signals.is_empty() {
                return Err(McpError::invalid_params(
                    "public_signals must be non-empty when provided",
                    None,
                ));
            }
            signals
        } else {
            let guard = self.state.lock().await;
            prepare_attestation_bundle(&guard.db, None)
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?
                .public_signals
        };

        let signals_array = format!("[{}]", public_signals.join(","));
        let chains_array = Self::cast_array_u64(&req.chain_ids);
        let selector = "submitCompositeAttestation(bytes,uint256[],uint256[])";
        let tx_hash = Self::cast_send(
            req.rpc_url.trim(),
            req.contract_address.trim(),
            selector,
            vec![
                req.proof_hex.clone(),
                signals_array.clone(),
                chains_array.clone(),
            ],
            &signer,
        )?;

        let signer_preview = match &signer {
            CastSigner::PrivateKey(_) => format!("--private-key ${env_name}"),
            CastSigner::Keystore {
                path,
                password_file,
            } => {
                let mut v = format!("--keystore {path}");
                if let Some(p) = password_file {
                    v.push_str(&format!(" --password-file {p}"));
                }
                v
            }
        };
        let command_preview = format!(
            "cast send --async --rpc-url {} {} {} \"{}\" {} '{}' '{}'",
            req.rpc_url.trim(),
            signer_preview,
            req.contract_address.trim(),
            selector,
            req.proof_hex,
            signals_array,
            chains_array
        );

        Ok(Json(SubmitCompositeAttestationResponse {
            submitted: true,
            tx_hash,
            chain_ids: req.chain_ids,
            public_signals,
            command_preview,
            message: "composite attestation submitted".to_string(),
        }))
    }

    #[tool(
        name = "verify_agent_multichain",
        description = "Verify if an agent is attested across a required chain set. Example: {\"contract_address\":\"0x...\",\"rpc_url\":\"https://...\",\"agent_address\":\"0x...\",\"required_chains\":[8453,1]}"
    )]
    pub async fn verify_agent_multichain(
        &self,
        Parameters(req): Parameters<VerifyAgentMultichainRequest>,
    ) -> Result<Json<VerifyAgentMultichainResponse>, McpError> {
        if req.contract_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "contract_address must be non-empty",
                None,
            ));
        }
        if req.rpc_url.trim().is_empty() {
            return Err(McpError::invalid_params("rpc_url must be non-empty", None));
        }
        if req.agent_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "agent_address must be non-empty",
                None,
            ));
        }
        let base_status = Self::verify_agent_bridge(
            req.rpc_url.trim(),
            req.contract_address.trim(),
            req.agent_address.trim(),
        )?;

        let required_array = Self::cast_array_u64(&req.required_chains);
        let raw_multichain = Self::run_cast(&[
            "call".to_string(),
            "--rpc-url".to_string(),
            req.rpc_url.trim().to_string(),
            req.contract_address.trim().to_string(),
            "isVerifiedMultiChain(address,uint256[])(bool)".to_string(),
            req.agent_address.trim().to_string(),
            required_array,
        ])?;
        let verified_all_required = Self::parse_bool_output(&raw_multichain)?;

        let mut per_chain = Vec::with_capacity(req.required_chains.len());
        for chain_id in &req.required_chains {
            let raw = Self::run_cast(&[
                "call".to_string(),
                "--rpc-url".to_string(),
                req.rpc_url.trim().to_string(),
                req.contract_address.trim().to_string(),
                "isVerifiedForChain(address,uint256)(bool)".to_string(),
                req.agent_address.trim().to_string(),
                chain_id.to_string(),
            ])?;
            per_chain.push(PerChainVerification {
                chain_id: *chain_id,
                verified: Self::parse_bool_output(&raw)?,
                raw,
            });
        }

        Ok(Json(VerifyAgentMultichainResponse {
            agent_address: req.agent_address,
            verified_all_required,
            required_chains: req.required_chains,
            per_chain,
            raw_multichain,
            base_status,
        }))
    }

    // Kept as an internal helper for multichain workflows.
    pub async fn query_cross_chain_attestation(
        &self,
        Parameters(req): Parameters<QueryCrossChainAttestationRequest>,
    ) -> Result<Json<QueryCrossChainAttestationResponse>, McpError> {
        if req.agent_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "agent_address must be non-empty",
                None,
            ));
        }
        let nonce = req.request_nonce.unwrap_or_else(now_unix_secs);
        let mut h = Sha256::new();
        h.update(b"nucleusdb.cross_chain_query.v1|");
        h.update(req.source_chain_id.to_le_bytes());
        h.update(req.target_chain_id.to_le_bytes());
        h.update(req.agent_address.as_bytes());
        h.update(nonce.to_le_bytes());
        let request_id = format!("0x{}", hex_encode(&h.finalize()));
        let calldata_preview = format!(
            "requestCrossChainVerification({}, {}, {}, {})",
            req.source_chain_id, req.target_chain_id, req.agent_address, request_id
        );

        let submit_onchain = req.submit_onchain.unwrap_or(false);
        if !submit_onchain {
            return Ok(Json(QueryCrossChainAttestationResponse {
                request_id,
                source_chain_id: req.source_chain_id,
                target_chain_id: req.target_chain_id,
                submitted: false,
                tx_hash: None,
                calldata_preview,
                command_preview: None,
                message: "query payload prepared (dry run)".to_string(),
            }));
        }

        let contract = req
            .query_contract
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                McpError::invalid_params(
                    "query_contract is required when submit_onchain=true",
                    None,
                )
            })?;
        let rpc_url = req
            .rpc_url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                McpError::invalid_params("rpc_url is required when submit_onchain=true", None)
            })?;
        let env_name = req
            .private_key_env
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("NUCLEUSDB_AGENT_PRIVATE_KEY")
            .to_string();
        let signer = Self::resolve_signer(
            Some(&env_name),
            req.keystore_path.as_deref(),
            req.keystore_password_file.as_deref(),
        )?;

        let selector = "requestCrossChainVerification(uint256,uint256,address,bytes32)";
        let tx_hash = Self::cast_send(
            rpc_url,
            contract,
            selector,
            vec![
                req.source_chain_id.to_string(),
                req.target_chain_id.to_string(),
                req.agent_address.trim().to_string(),
                request_id.clone(),
            ],
            &signer,
        )?;
        let signer_preview = match &signer {
            CastSigner::PrivateKey(_) => format!("--private-key ${env_name}"),
            CastSigner::Keystore {
                path,
                password_file,
            } => {
                let mut v = format!("--keystore {path}");
                if let Some(p) = password_file {
                    v.push_str(&format!(" --password-file {p}"));
                }
                v
            }
        };
        let command_preview = Some(format!(
            "cast send --async --rpc-url {} {} {} \"{}\" {} {} {} {}",
            rpc_url,
            signer_preview,
            contract,
            selector,
            req.source_chain_id,
            req.target_chain_id,
            req.agent_address.trim(),
            request_id
        ));

        Ok(Json(QueryCrossChainAttestationResponse {
            request_id,
            source_chain_id: req.source_chain_id,
            target_chain_id: req.target_chain_id,
            submitted: true,
            tx_hash,
            calldata_preview,
            command_preview,
            message: "cross-chain query submitted".to_string(),
        }))
    }

    // Kept as an internal helper for multichain workflows.
    pub async fn list_registered_chains(
        &self,
        Parameters(req): Parameters<ListRegisteredChainsRequest>,
    ) -> Result<Json<ListRegisteredChainsResponse>, McpError> {
        if req.contract_address.trim().is_empty() {
            return Err(McpError::invalid_params(
                "contract_address must be non-empty",
                None,
            ));
        }
        if req.rpc_url.trim().is_empty() {
            return Err(McpError::invalid_params("rpc_url must be non-empty", None));
        }
        let len_raw = Self::run_cast(&[
            "call".to_string(),
            "--rpc-url".to_string(),
            req.rpc_url.trim().to_string(),
            req.contract_address.trim().to_string(),
            "registeredChainsLength()(uint256)".to_string(),
        ])?;
        let len = Self::parse_u64_output(&len_raw)?;
        let mut chains = Vec::with_capacity(len as usize);
        for idx in 0..len {
            let chain_raw = Self::run_cast(&[
                "call".to_string(),
                "--rpc-url".to_string(),
                req.rpc_url.trim().to_string(),
                req.contract_address.trim().to_string(),
                "registeredChains(uint256)(uint256)".to_string(),
                idx.to_string(),
            ])?;
            let chain_id = Self::parse_u64_output(&chain_raw)?;
            let info_raw = Self::run_cast(&[
                "call".to_string(),
                "--rpc-url".to_string(),
                req.rpc_url.trim().to_string(),
                req.contract_address.trim().to_string(),
                "chainInfo(uint256)(bool,address,bytes32,uint64,uint256)".to_string(),
                chain_id.to_string(),
            ])?;
            chains.push(ChainRegistrationView {
                chain_id,
                raw_chain_info: info_raw,
            });
        }
        Ok(Json(ListRegisteredChainsResponse {
            count: chains.len(),
            chains,
        }))
    }

    #[tool(
        name = "abraxas_submit_record",
        description = "Submit an Abraxas WorkRecord JSON payload into append-only NucleusDB storage. Returns record hash and proof reference."
    )]
    pub async fn abraxas_submit_record(
        &self,
        Parameters(req): Parameters<AbraxasSubmitRecordRequest>,
    ) -> Result<Json<AbraxasSubmitRecordResponse>, McpError> {
        if req.record_json.trim().is_empty() {
            return Err(McpError::invalid_params(
                "record_json must be non-empty",
                None,
            ));
        }
        let input: WorkRecordInput = serde_json::from_str(req.record_json.trim()).map_err(|e| {
            McpError::invalid_params(format!("invalid WorkRecordInput JSON: {e}"), None)
        })?;
        let now = WorkRecordStore::now_unix_secs();
        let record = input
            .into_record(now)
            .map_err(|e| McpError::invalid_params(e, None))?;

        let mut guard = self.state.lock().await;
        let store = guard.work_record_store.clone();
        let submitted = store
            .submit_record(&mut guard.db, record)
            .map_err(|e| McpError::internal_error(e, None))?;
        persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
        })?;

        Ok(Json(AbraxasSubmitRecordResponse {
            hash: vcs_hash_hex(&submitted.hash),
            proof_ref: submitted.proof_ref,
            commit_height: submitted.commit_height,
            state_root: hex_encode(&submitted.state_root),
        }))
    }

    #[tool(
        name = "abraxas_query_records",
        description = "Query Abraxas WorkRecords by hash, author_puf, path prefix, or timestamp range."
    )]
    pub async fn abraxas_query_records(
        &self,
        Parameters(req): Parameters<AbraxasQueryRecordsRequest>,
    ) -> Result<Json<AbraxasQueryRecordsResponse>, McpError> {
        let hash = Self::parse_optional_hash(req.hash.as_deref(), "hash")?;
        let author_puf = Self::parse_optional_hash(req.author_puf.as_deref(), "author_puf")?;
        let filter = VcsQueryFilter {
            hash,
            author_puf,
            path_prefix: req
                .path_prefix
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            start_timestamp: req.start_timestamp,
            end_timestamp: req.end_timestamp,
            limit: req.limit,
        };

        let guard = self.state.lock().await;
        let records = guard.work_record_store.query_records(&guard.db, &filter);
        let views = records.iter().map(WorkRecordView::from).collect::<Vec<_>>();
        Ok(Json(AbraxasQueryRecordsResponse {
            count: views.len(),
            records: views,
        }))
    }

    #[tool(
        name = "abraxas_record_status",
        description = "Return Abraxas WorkRecord store status: count, latest hash/timestamp, and current transparency STH."
    )]
    pub async fn abraxas_record_status(
        &self,
    ) -> Result<Json<AbraxasRecordStatusResponse>, McpError> {
        let guard = self.state.lock().await;
        let status = guard.work_record_store.status(&guard.db);
        Ok(Json(AbraxasRecordStatusResponse {
            record_count: status.record_count,
            latest_hash: status.latest_hash.as_ref().map(vcs_hash_hex),
            latest_timestamp: status.latest_timestamp,
            sth_tree_size: status.sth_tree_size,
            sth_root: status.sth_root.as_ref().map(|h| hex_encode(h)),
            sth_timestamp: status.sth_timestamp,
        }))
    }

    #[tool(
        name = "abraxas_merge_status",
        description = "Return Abraxas merge-agent status, including conflict candidates discovered from concurrent same-path records."
    )]
    pub async fn abraxas_merge_status(&self) -> Result<Json<AbraxasMergeStatusResponse>, McpError> {
        let guard = self.state.lock().await;
        let records = guard
            .work_record_store
            .query_records(&guard.db, &VcsQueryFilter::default());
        let snapshot = analyze_records(&records);
        let conflicts = snapshot
            .conflicts
            .into_iter()
            .map(|c| AbraxasConflictView {
                path: c.path,
                left_hash: vcs_hash_hex(&c.left_hash),
                right_hash: vcs_hash_hex(&c.right_hash),
                left_timestamp: c.left_timestamp,
                right_timestamp: c.right_timestamp,
            })
            .collect::<Vec<_>>();
        Ok(Json(AbraxasMergeStatusResponse {
            record_count: snapshot.record_count,
            merged_count: snapshot.merged_count,
            conflict_count: snapshot.conflict_count,
            head_hash: snapshot.head_hash.as_ref().map(vcs_hash_hex),
            conflicts,
        }))
    }

    #[tool(
        name = "abraxas_resolve_conflict",
        description = "Resolve a same-path conflict by writing a deterministic preferred-hash Modify record."
    )]
    pub async fn abraxas_resolve_conflict(
        &self,
        Parameters(req): Parameters<AbraxasResolveConflictRequest>,
    ) -> Result<Json<AbraxasResolveConflictResponse>, McpError> {
        if req.path.trim().is_empty() {
            return Err(McpError::invalid_params("path must be non-empty", None));
        }
        let preferred_hash = vcs_parse_hash_hex(req.preferred_hash.trim())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let author_puf = match req.author_puf.as_deref() {
            Some(v) if !v.trim().is_empty() => {
                vcs_parse_hash_hex(v.trim()).map_err(|e| McpError::invalid_params(e, None))?
            }
            _ => [0u8; 32],
        };

        let record = WorkRecord {
            hash: [0u8; 32],
            parents: vec![],
            author_puf,
            timestamp: WorkRecordStore::now_unix_secs(),
            op: crate::vcs::FileOp::Modify {
                path: req.path.trim().to_string(),
                old_hash: preferred_hash,
                new_hash: preferred_hash,
                patch: None,
            },
            proof_ref: None,
        };

        let mut guard = self.state.lock().await;
        let store = guard.work_record_store.clone();
        let submitted = store
            .submit_record(&mut guard.db, record)
            .map_err(|e| McpError::internal_error(e, None))?;
        persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
        })?;

        Ok(Json(AbraxasResolveConflictResponse {
            resolved: true,
            hash: Some(vcs_hash_hex(&submitted.hash)),
            proof_ref: Some(submitted.proof_ref),
            commit_height: Some(submitted.commit_height),
            message: "conflict resolution record committed".to_string(),
        }))
    }

    #[tool(
        name = "abraxas_export_git",
        description = "Export materialized Abraxas canonical state to a git worktree path; optionally commit."
    )]
    pub async fn abraxas_export_git(
        &self,
        Parameters(req): Parameters<AbraxasExportGitRequest>,
    ) -> Result<Json<AbraxasExportGitResponse>, McpError> {
        let repo_path = req.repo_path.trim();
        if repo_path.is_empty() {
            return Err(McpError::invalid_params(
                "repo_path must be non-empty",
                None,
            ));
        }
        let dry_run = req.dry_run.unwrap_or(false);

        let guard = self.state.lock().await;
        let records = guard
            .work_record_store
            .query_records(&guard.db, &VcsQueryFilter::default());
        drop(guard);

        if dry_run {
            let state = crate::vcs::materialize_state(&records);
            let final_paths = state.values().filter(|v| v.is_some()).count();
            return Ok(Json(AbraxasExportGitResponse {
                exported_files: final_paths,
                deleted_files: 0,
                final_paths,
                committed: false,
                commit_sha: None,
                message: "dry run only; no files written".to_string(),
            }));
        }

        let stats = export_state_to_worktree(&records, Path::new(repo_path))
            .map_err(|e| McpError::internal_error(e, None))?;

        let mut committed = false;
        let mut commit_sha = None;
        if Path::new(repo_path).join(".git").exists() {
            let _ = Self::run_git(repo_path, &["add", "-A"])?;
            if Self::run_git(
                repo_path,
                &[
                    "commit",
                    "-m",
                    req.commit_message
                        .as_deref()
                        .unwrap_or("abraxas export: materialized canonical state"),
                ],
            )
            .is_ok()
            {
                committed = true;
                let sha = Self::run_git(repo_path, &["rev-parse", "HEAD"])?;
                commit_sha = Some(sha.trim().to_string());
            }
        }

        Ok(Json(AbraxasExportGitResponse {
            exported_files: stats.written_files,
            deleted_files: stats.deleted_files,
            final_paths: stats.final_paths,
            committed,
            commit_sha,
            message: "git export completed".to_string(),
        }))
    }

    #[tool(
        name = "abraxas_workspace_init",
        description = "Initialize an Abraxas workspace directory; optionally run git init."
    )]
    pub async fn abraxas_workspace_init(
        &self,
        Parameters(req): Parameters<AbraxasWorkspaceInitRequest>,
    ) -> Result<Json<AbraxasWorkspaceInitResponse>, McpError> {
        let workspace = req.workspace_path.trim();
        if workspace.is_empty() {
            return Err(McpError::invalid_params(
                "workspace_path must be non-empty",
                None,
            ));
        }
        std::fs::create_dir_all(workspace)
            .map_err(|e| McpError::internal_error(format!("create workspace: {e}"), None))?;
        let mut git_initialized = false;
        if req.init_git.unwrap_or(true) {
            let _ = Self::run_git(workspace, &["init"])?;
            git_initialized = true;
        }
        Ok(Json(AbraxasWorkspaceInitResponse {
            workspace_path: workspace.to_string(),
            git_initialized,
            message: "workspace initialized".to_string(),
        }))
    }

    #[tool(
        name = "abraxas_workspace_diff",
        description = "Return git porcelain diff lines from a workspace."
    )]
    pub async fn abraxas_workspace_diff(
        &self,
        Parameters(req): Parameters<AbraxasWorkspaceDiffRequest>,
    ) -> Result<Json<AbraxasWorkspaceDiffResponse>, McpError> {
        let workspace = req.workspace_path.trim();
        if workspace.is_empty() {
            return Err(McpError::invalid_params(
                "workspace_path must be non-empty",
                None,
            ));
        }
        let lines = git_status_porcelain(Path::new(workspace))
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(Json(AbraxasWorkspaceDiffResponse {
            workspace_path: workspace.to_string(),
            changed_count: lines.len(),
            porcelain_lines: lines,
        }))
    }

    #[tool(
        name = "abraxas_workspace_submit",
        description = "Convert workspace git-diff changes into Abraxas WorkRecords and commit them append-only."
    )]
    pub async fn abraxas_workspace_submit(
        &self,
        Parameters(req): Parameters<AbraxasWorkspaceSubmitRequest>,
    ) -> Result<Json<AbraxasWorkspaceSubmitResponse>, McpError> {
        let workspace = req.workspace_path.trim();
        if workspace.is_empty() {
            return Err(McpError::invalid_params(
                "workspace_path must be non-empty",
                None,
            ));
        }
        let author_puf = vcs_parse_hash_hex(req.author_puf.trim())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let timestamp = req.timestamp.unwrap_or_else(WorkRecordStore::now_unix_secs);
        let records = work_records_from_workspace(Path::new(workspace), author_puf, timestamp)
            .map_err(|e| McpError::invalid_params(e, None))?;

        let mut guard = self.state.lock().await;
        let store = guard.work_record_store.clone();
        let mut hashes = Vec::with_capacity(records.len());
        let mut heights = Vec::with_capacity(records.len());
        for rec in records {
            let submitted = store
                .submit_record(&mut guard.db, rec)
                .map_err(|e| McpError::internal_error(e, None))?;
            hashes.push(vcs_hash_hex(&submitted.hash));
            heights.push(submitted.commit_height);
        }
        persist_snapshot_and_sync_wal(&guard.db_path, &guard.wal_path, &guard.db).map_err(|e| {
            McpError::internal_error(format!("failed to persist snapshot+wal: {e:?}"), None)
        })?;

        Ok(Json(AbraxasWorkspaceSubmitResponse {
            submitted_count: hashes.len(),
            hashes,
            commit_heights: heights,
            message: "workspace diff committed into Abraxas".to_string(),
        }))
    }

    #[tool(
        name = "nucleusdb_help",
        description = "Return SQL dialect reference, backend ids, and policy profiles for agent-safe usage."
    )]
    pub async fn help(&self) -> Result<Json<HelpResponse>, McpError> {
        Ok(Json(HelpResponse {
            server: "nucleusdb".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            backends: vec![
                "binary_merkle (recommended)".to_string(),
                "ipa".to_string(),
                "kzg".to_string(),
            ],
            policy_profiles: vec!["permissive".to_string(), "production".to_string()],
            sql_reference: vec![
                "INSERT INTO data (key, value) VALUES ('k', 1);".to_string(),
                "SELECT key, value FROM data WHERE key = 'k';".to_string(),
                "SELECT key, value FROM data WHERE key LIKE 'prefix%';".to_string(),
                "UPDATE data SET value = 2 WHERE key = 'k';".to_string(),
                "DELETE FROM data WHERE key = 'k';".to_string(),
                "SHOW STATUS; SHOW HISTORY; SHOW HISTORY 'k';".to_string(),
                "VERIFY 'k'; EXPORT; COMMIT;".to_string(),
            ],
            notes: vec![
                "Use explicit db_path values when creating databases to avoid collisions."
                    .to_string(),
                "persist defaults to true in nucleusdb_execute_sql.".to_string(),
                "prefer_wal defaults to false in nucleusdb_open_database.".to_string(),
                "Trust tools: nucleusdb_agent_register, nucleusdb_verify_agent, verify_agent_multichain."
                    .to_string(),
                "Multichain tools: register_chain, submit_composite_attestation."
                    .to_string(),
                "Abraxas VCS tools: abraxas_submit_record, abraxas_query_records, abraxas_record_status, abraxas_merge_status, abraxas_resolve_conflict, abraxas_export_git, abraxas_workspace_init, abraxas_workspace_diff, abraxas_workspace_submit."
                    .to_string(),
                "On-chain submit paths accept keystore_path (+ optional keystore_password_file) or private_key_env."
                    .to_string(),
                "Mesh tools: mesh_peers, mesh_ping, mesh_call, mesh_exchange_envelope, mesh_grant."
                    .to_string(),
            ],
        }))
    }

    // ── Mesh tools ──────────────────────────────────────────────────────

    #[tool(
        name = "mesh_peers",
        description = "List known peers on the container mesh network. Returns peer agent IDs, DID URIs, endpoints, and online status."
    )]
    pub async fn mesh_peers(&self) -> Result<Json<MeshPeersResponse>, McpError> {
        use crate::container::mesh::{mesh_registry_path, PeerRegistry, MESH_NETWORK_NAME};
        let my_agent_id = std::env::var("NUCLEUSDB_MESH_AGENT_ID").unwrap_or_default();
        let registry_path = mesh_registry_path();
        let registry = PeerRegistry::load(registry_path.as_path()).unwrap_or_default();
        let peers: Vec<MeshPeerView> = registry
            .peers_except(&my_agent_id)
            .iter()
            .map(|p| {
                let (reachable, latency_ms) = crate::container::mesh::ping_peer_with_latency(p);
                MeshPeerView {
                    agent_id: p.agent_id.clone(),
                    did_uri: p.did_uri.clone(),
                    mcp_endpoint: p.mcp_endpoint.clone(),
                    status: if reachable {
                        "online".to_string()
                    } else {
                        "offline".to_string()
                    },
                    latency_ms,
                    last_seen: p.last_seen,
                }
            })
            .collect();
        Ok(Json(MeshPeersResponse {
            mesh_enabled: !my_agent_id.is_empty(),
            network: MESH_NETWORK_NAME.to_string(),
            self_agent_id: my_agent_id,
            peer_count: peers.len(),
            peers,
        }))
    }

    #[tool(
        name = "mesh_ping",
        description = "Ping a specific peer on the mesh network. Returns reachability and latency. Example: {\"agent_id\":\"agent-bob\"}"
    )]
    pub async fn mesh_ping(
        &self,
        Parameters(req): Parameters<MeshPingRequest>,
    ) -> Result<Json<MeshPingResponse>, McpError> {
        use crate::container::mesh::{mesh_registry_path, PeerRegistry};
        let registry_path = mesh_registry_path();
        let registry = PeerRegistry::load(registry_path.as_path()).unwrap_or_default();
        let peer = registry.find(&req.agent_id).ok_or_else(|| {
            McpError::invalid_params(
                format!("peer '{}' not found in mesh registry", req.agent_id),
                None,
            )
        })?;
        let (reachable, latency_ms) = crate::container::mesh::ping_peer_with_latency(peer);
        Ok(Json(MeshPingResponse {
            agent_id: req.agent_id,
            reachable,
            latency_ms,
        }))
    }

    #[tool(
        name = "mesh_call",
        description = "Call a remote peer's MCP tool via the mesh network. Example: {\"peer_agent_id\":\"agent-bob\",\"tool_name\":\"nucleusdb_query\",\"arguments\":{\"key\":\"test\"}}"
    )]
    pub async fn mesh_call(
        &self,
        Parameters(req): Parameters<MeshCallRequest>,
    ) -> Result<Json<MeshCallResponse>, McpError> {
        use crate::container::mesh::{mesh_registry_path, PeerRegistry};
        let registry_path = mesh_registry_path();
        let registry = PeerRegistry::load(registry_path.as_path()).unwrap_or_default();
        let peer = registry.find(&req.peer_agent_id).ok_or_else(|| {
            McpError::invalid_params(
                format!("peer '{}' not found in mesh registry", req.peer_agent_id),
                None,
            )
        })?;
        let start = std::time::Instant::now();

        if req.use_didcomm {
            // DIDComm-wrapped MCP call path.
            let tool_name = req.tool_name.clone();
            let args = req.arguments.clone();
            let peer_owned = peer.clone();
            let (result, auth_method) = tokio::task::spawn_blocking(move || {
                mesh_call_didcomm(&peer_owned, &tool_name, args)
            })
            .await
            .map_err(|e| {
                McpError::internal_error(format!("mesh DIDComm worker task failed: {e}"), None)
            })?
            .map_err(|e| McpError::internal_error(e, None))?;
            let latency_ms = start.elapsed().as_millis() as u64;
            return Ok(Json(MeshCallResponse {
                peer_agent_id: req.peer_agent_id,
                tool_name: req.tool_name,
                result,
                auth_method,
                latency_ms,
            }));
        }

        // Raw HTTP MCP call path (Part 1 behavior).
        let auth_token = mesh_auth_token();
        let result = crate::container::mesh::call_remote_tool(
            peer,
            &req.tool_name,
            req.arguments.clone(),
            auth_token.as_deref(),
        )
        .map_err(|e| McpError::internal_error(e, None))?;
        let latency_ms = start.elapsed().as_millis() as u64;
        Ok(Json(MeshCallResponse {
            peer_agent_id: req.peer_agent_id,
            tool_name: req.tool_name,
            result,
            auth_method: if auth_token.is_some() {
                "bearer".to_string()
            } else {
                "none".to_string()
            },
            latency_ms,
        }))
    }

    #[tool(
        name = "mesh_exchange_envelope",
        description = "Send a ProofEnvelope to a remote peer for verification. Example: {\"peer_agent_id\":\"agent-bob\",\"envelope\":{...}}"
    )]
    pub async fn mesh_exchange_envelope(
        &self,
        Parameters(req): Parameters<MeshExchangeEnvelopeRequest>,
    ) -> Result<Json<MeshExchangeEnvelopeResponse>, McpError> {
        use crate::container::mesh::{mesh_registry_path, PeerRegistry};
        let registry_path = mesh_registry_path();
        let registry = PeerRegistry::load(registry_path.as_path()).unwrap_or_default();
        let peer = registry.find(&req.peer_agent_id).ok_or_else(|| {
            McpError::invalid_params(
                format!("peer '{}' not found in mesh registry", req.peer_agent_id),
                None,
            )
        })?;
        let auth_token = mesh_auth_token();
        let result =
            crate::container::mesh::exchange_envelope(peer, &req.envelope, auth_token.as_deref())
                .map_err(|e| McpError::internal_error(e, None))?;
        let accepted = result
            .get("accepted")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        Ok(Json(MeshExchangeEnvelopeResponse {
            peer_agent_id: req.peer_agent_id,
            accepted,
            verification: result,
        }))
    }

    #[tool(
        name = "mesh_grant",
        description = "Grant a remote peer access to local resources via capability token. Example: {\"peer_agent_id\":\"agent-bob\",\"peer_did\":\"did:key:z6Mk...\",\"resource_patterns\":[\"results/*\"],\"modes\":[\"read\"],\"duration_secs\":3600}"
    )]
    pub async fn mesh_grant(
        &self,
        Parameters(req): Parameters<MeshGrantRequest>,
    ) -> Result<Json<MeshGrantResponse>, McpError> {
        use crate::container::mesh::{mesh_registry_path, PeerRegistry};
        use crate::halo::did::did_from_genesis_seed;
        use crate::halo::genesis_seed;
        use crate::halo::util::hex_encode;
        use crate::pod::capability::{create_capability, AgentClass};

        let seed = genesis_seed::load_seed_bytes()
            .map_err(|e| McpError::internal_error(e, None))?
            .ok_or_else(|| {
                McpError::internal_error(
                    "genesis seed missing; run `agenthalo genesis harvest` first",
                    None,
                )
            })?;
        let grantor =
            did_from_genesis_seed(&seed).map_err(|e| McpError::internal_error(e, None))?;

        let registry_path = mesh_registry_path();
        let registry = PeerRegistry::load(registry_path.as_path()).unwrap_or_default();
        let peer = registry.find(&req.peer_agent_id).ok_or_else(|| {
            McpError::invalid_params(
                format!("peer '{}' not found in mesh registry", req.peer_agent_id),
                None,
            )
        })?;
        let peer_did_from_registry = peer.did_uri.as_deref().unwrap_or("").trim();
        let peer_did_from_request = req.peer_did.trim();
        let effective_peer_did = if !peer_did_from_registry.is_empty() {
            if !peer_did_from_request.is_empty() && peer_did_from_request != peer_did_from_registry
            {
                return Err(McpError::invalid_params(
                    format!(
                        "peer_did mismatch for peer '{}': registry has '{}', request has '{}'",
                        req.peer_agent_id, peer_did_from_registry, peer_did_from_request
                    ),
                    None,
                ));
            }
            peer_did_from_registry.to_string()
        } else if !peer_did_from_request.is_empty() {
            peer_did_from_request.to_string()
        } else {
            return Err(McpError::invalid_params(
                format!(
                    "peer '{}' has no DID in registry and request peer_did is empty",
                    req.peer_agent_id
                ),
                None,
            ));
        };

        let now = crate::pod::now_unix();
        let modes =
            parse_mesh_access_modes(&req.modes).map_err(|e| McpError::invalid_params(e, None))?;

        let token = create_capability(
            &grantor,
            &effective_peer_did,
            AgentClass::Authenticated,
            &req.resource_patterns,
            &modes,
            now,
            now.saturating_add(req.duration_secs),
            false,
        )
        .map_err(|e| McpError::invalid_params(e, None))?;
        let mut store = crate::pod::capability::CapabilityStore::load_or_default(
            &crate::halo::config::capability_store_path(),
        )
        .map_err(|e| McpError::internal_error(e, None))?;
        if !store.tokens.iter().any(|t| t.token_id == token.token_id) {
            store.create(token.clone());
            store
                .save(&crate::halo::config::capability_store_path())
                .map_err(|e| McpError::internal_error(e, None))?;
        }

        Ok(Json(MeshGrantResponse {
            capability_token_id: hex_encode(&token.token_id),
            granted_to: effective_peer_did,
            resource_patterns: req.resource_patterns,
            modes: req.modes,
            expires_at: now.saturating_add(req.duration_secs),
            peer_agent_id: req.peer_agent_id,
        }))
    }

    #[tool(
        name = "orchestrator_launch",
        description = "Launch a managed agent instance for orchestrated tasks. Example: {\"agent\":\"codex\",\"agent_name\":\"reviewer\",\"timeout_secs\":600,\"trace\":true,\"capabilities\":[\"memory_read\",\"memory_write\"]}"
    )]
    pub async fn orchestrator_launch(
        &self,
        Parameters(req): Parameters<OrchestratorLaunchRequest>,
    ) -> Result<Json<OrchestratorLaunchResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied = call_orchestrator_proxy_tool(
                "orchestrator_launch",
                serde_json::to_value(&req).map_err(|e| {
                    McpError::internal_error(
                        format!("serialize orchestrator launch proxy payload: {e}"),
                        None,
                    )
                })?,
            )
            .await?;
            let parsed: OrchestratorLaunchResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator launch proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let admission_mode = AdmissionMode::parse(req.admission_mode.as_deref())
            .map_err(|e| McpError::invalid_params(e, None))?;
        let (orchestrator, admission) = {
            let guard = self.state.lock().await;
            let admission =
                evaluate_launch_admission(admission_mode, Some(&guard.governor_registry), None);
            (guard.orchestrator.clone(), admission)
        };
        if !admission.allowed {
            let reason = admission
                .issues
                .iter()
                .map(|issue| issue.message.clone())
                .collect::<Vec<_>>()
                .join(" | ");
            return Err(McpError::invalid_params(
                format!("AETHER admission policy blocked orchestrator launch: {reason}"),
                None,
            ));
        }
        let launched = orchestrator
            .launch_agent(OrchLaunchRequest {
                agent: req.agent,
                agent_name: req.agent_name,
                working_dir: req.working_dir,
                env: req.env,
                timeout_secs: req.timeout_secs.unwrap_or(600),
                model: req.model,
                trace: req.trace.unwrap_or(true),
                capabilities: req.capabilities,
                dispatch_mode: req.dispatch_mode,
                container_hookup: req.container_hookup,
            })
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(OrchestratorLaunchResponse {
            agent_id: launched.agent_id,
            session_id: launched.pty_session_id,
            status: "idle".to_string(),
            agent: launched.agent_type,
            agent_name: launched.agent_name,
            capabilities: launched.capabilities,
            model: launched.model,
            admission_mode: Some(admission.mode),
            admission_forced: admission.forced,
            admission_issues: admission
                .issues
                .into_iter()
                .map(|issue| issue.message)
                .collect(),
        }))
    }

    #[tool(
        name = "orchestrator_send_task",
        description = "Submit a task to a launched orchestrator agent. Example: {\"agent_id\":\"orch-...\",\"task\":\"Review src/main.rs\",\"wait\":true}"
    )]
    pub async fn orchestrator_send_task(
        &self,
        Parameters(req): Parameters<OrchestratorSendTaskRequest>,
    ) -> Result<Json<OrchestratorTaskResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied = call_orchestrator_proxy_tool(
                "orchestrator_send_task",
                serde_json::to_value(&req).map_err(|e| {
                    McpError::internal_error(
                        format!("serialize orchestrator send_task proxy payload: {e}"),
                        None,
                    )
                })?,
            )
            .await?;
            let parsed: OrchestratorTaskResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator send_task proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        if req.task.trim().is_empty() {
            return Err(McpError::invalid_params("task must be non-empty", None));
        }
        let task = orchestrator
            .send_task(OrchSendTaskRequest {
                agent_id: req.agent_id.clone(),
                task: req.task,
                timeout_secs: req.timeout_secs,
                wait: req.wait.unwrap_or(true),
            })
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(task_to_response(task)))
    }

    #[tool(
        name = "orchestrator_schedule_task",
        description = "Schedule a delayed task for a launched orchestrator agent. Example: {\"agent_id\":\"orch-...\",\"task\":\"Rotate keys\",\"delay_secs\":300}"
    )]
    pub async fn orchestrator_schedule_task(
        &self,
        Parameters(req): Parameters<OrchestratorScheduleTaskRequest>,
    ) -> Result<Json<OrchestratorTaskResponse>, McpError> {
        if req.task.trim().is_empty() {
            return Err(McpError::invalid_params("task must be non-empty", None));
        }
        if req.delay_secs == 0 {
            return Err(McpError::invalid_params("delay_secs must be > 0", None));
        }
        if orchestrator_proxy_enabled() {
            let proxied = call_orchestrator_proxy_tool(
                "orchestrator_schedule_task",
                serde_json::to_value(&req).map_err(|e| {
                    McpError::internal_error(
                        format!("serialize orchestrator schedule_task proxy payload: {e}"),
                        None,
                    )
                })?,
            )
            .await?;
            let parsed: OrchestratorTaskResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator schedule_task proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let task = orchestrator
            .schedule_task(
                OrchSendTaskRequest {
                    agent_id: req.agent_id.clone(),
                    task: req.task,
                    timeout_secs: req.timeout_secs,
                    wait: false,
                },
                req.delay_secs,
            )
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(task_to_response(task)))
    }

    #[tool(
        name = "orchestrator_get_result",
        description = "Get task status/result by task_id. Optionally wait for completion. Example: {\"task_id\":\"task-...\",\"wait\":true,\"timeout_secs\":60}"
    )]
    pub async fn orchestrator_get_result(
        &self,
        Parameters(req): Parameters<OrchestratorGetResultRequest>,
    ) -> Result<Json<OrchestratorTaskResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied = call_orchestrator_proxy_tool(
                "orchestrator_get_result",
                serde_json::to_value(&req).map_err(|e| {
                    McpError::internal_error(
                        format!("serialize orchestrator get_result proxy payload: {e}"),
                        None,
                    )
                })?,
            )
            .await?;
            let parsed: OrchestratorTaskResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator get_result proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let wait = req.wait.unwrap_or(true);
        let timeout = req.timeout_secs.unwrap_or(60).clamp(1, 600);
        let started = std::time::Instant::now();
        loop {
            if let Some(task) = orchestrator.get_task(&req.task_id).await {
                if !wait
                    || matches!(
                        task.status,
                        crate::orchestrator::task::TaskStatus::Complete
                            | crate::orchestrator::task::TaskStatus::Failed
                            | crate::orchestrator::task::TaskStatus::Timeout
                    )
                {
                    return Ok(Json(task_to_response(task)));
                }
            } else {
                return Err(McpError::invalid_params(
                    format!("unknown task_id {}", req.task_id),
                    None,
                ));
            }
            if started.elapsed() >= std::time::Duration::from_secs(timeout) {
                let task = orchestrator
                    .get_task(&req.task_id)
                    .await
                    .ok_or_else(|| McpError::invalid_params("task disappeared", None))?;
                return Ok(Json(task_to_response(task)));
            }
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }
    }

    #[tool(
        name = "orchestrator_pipe",
        description = "Create task-graph pipe from source task output to target agent input. Example: {\"source_task_id\":\"task-a\",\"target_agent_id\":\"orch-b\",\"transform\":\"claude_answer\"}"
    )]
    pub async fn orchestrator_pipe(
        &self,
        Parameters(req): Parameters<OrchestratorPipeRequest>,
    ) -> Result<Json<OrchestratorPipeResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied = call_orchestrator_proxy_tool(
                "orchestrator_pipe",
                serde_json::to_value(&req).map_err(|e| {
                    McpError::internal_error(
                        format!("serialize orchestrator pipe proxy payload: {e}"),
                        None,
                    )
                })?,
            )
            .await?;
            let parsed: OrchestratorPipeResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator pipe proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let submitted = orchestrator
            .pipe(OrchPipeRequest {
                source_task_id: req.source_task_id.clone(),
                target_agent_id: req.target_agent_id.clone(),
                transform: req.transform.clone(),
                task_prefix: req.task_prefix.clone(),
            })
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(OrchestratorPipeResponse {
            source_task_id: req.source_task_id,
            target_agent_id: req.target_agent_id,
            status: submitted
                .as_ref()
                .map(|task| match task.status {
                    crate::orchestrator::task::TaskStatus::Complete => "complete".to_string(),
                    crate::orchestrator::task::TaskStatus::Failed => "failed".to_string(),
                    crate::orchestrator::task::TaskStatus::Timeout => "timeout".to_string(),
                    _ => "running".to_string(),
                })
                .unwrap_or_else(|| "linked".to_string()),
            task_id: submitted.map(|t| t.task_id),
        }))
    }

    #[tool(
        name = "orchestrator_list",
        description = "List launched orchestrator agents and status."
    )]
    pub async fn orchestrator_list(&self) -> Result<Json<OrchestratorListResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied =
                call_orchestrator_proxy_tool("orchestrator_list", serde_json::json!({})).await?;
            let parsed: OrchestratorListResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator list proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let agents = orchestrator.list_agents().await;
        let mut views = Vec::with_capacity(agents.len());
        for a in agents {
            let metadata = orchestrator.container_agent_metadata(&a.agent_id).await;
            views.push(OrchestratorAgentView {
                agent_id: a.agent_id,
                agent_name: a.agent_name,
                agent_type: a.agent_type,
                status: match a.status {
                    crate::orchestrator::agent_pool::AgentStatus::Idle => "idle".to_string(),
                    crate::orchestrator::agent_pool::AgentStatus::Busy { .. } => "busy".to_string(),
                    crate::orchestrator::agent_pool::AgentStatus::Stopped { .. } => {
                        "stopped".to_string()
                    }
                },
                tasks_completed: a.tasks_completed,
                total_cost_usd: a.total_cost_usd,
                capabilities: a.capabilities,
                launched_at: a.launched_at,
                working_dir: a.working_dir,
                container_session_id: metadata.as_ref().map(|meta| meta.session_id.clone()),
                container_id: metadata.as_ref().map(|meta| meta.container_id.clone()),
                lock_state: metadata.as_ref().map(|meta| meta.lock_state.clone()),
                peer_agent_id: metadata.as_ref().map(|meta| meta.peer_agent_id.clone()),
                trace_session_id: metadata
                    .as_ref()
                    .and_then(|meta| meta.trace_session_id.clone()),
                agent_home: metadata.as_ref().and_then(|meta| meta.agent_home.clone()),
                identity_fingerprint: metadata
                    .as_ref()
                    .map(|meta| meta.identity_fingerprint.clone()),
                identity_digest: metadata
                    .as_ref()
                    .map(|meta| meta.identity_fingerprint.clone()), // deprecated: use identity_fingerprint
            });
        }
        Ok(Json(OrchestratorListResponse { agents: views }))
    }

    #[tool(
        name = "orchestrator_tasks",
        description = "List orchestrator tasks and current status."
    )]
    pub async fn orchestrator_tasks(&self) -> Result<Json<OrchestratorTasksResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied =
                call_orchestrator_proxy_tool("orchestrator_tasks", serde_json::json!({})).await?;
            let parsed: OrchestratorTasksResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator tasks proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let tasks = orchestrator.list_tasks().await;
        Ok(Json(OrchestratorTasksResponse {
            tasks: tasks.into_iter().map(task_to_response).collect(),
        }))
    }

    #[tool(
        name = "orchestrator_graph",
        description = "Get current orchestrator task graph snapshot. graph.nodes is an object map keyed by task_id; graph.edges is an array."
    )]
    pub async fn orchestrator_graph(&self) -> Result<Json<OrchestratorGraphResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied =
                call_orchestrator_proxy_tool("orchestrator_graph", serde_json::json!({})).await?;
            let parsed: OrchestratorGraphResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator graph proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let graph = orchestrator.graph_snapshot().await;
        let node_count = graph.nodes.len();
        let edge_count = graph.edges.len();
        Ok(Json(OrchestratorGraphResponse {
            graph: serde_json::to_value(graph).map_err(|e| {
                McpError::internal_error(format!("serialize orchestrator graph: {e}"), None)
            })?,
            node_count,
            edge_count,
            nodes_shape: "object_map".to_string(),
        }))
    }

    #[tool(
        name = "orchestrator_mesh_status",
        description = "Query orchestrator mesh peer topology, reachability, and latency."
    )]
    pub async fn orchestrator_mesh_status(
        &self,
        Parameters(_req): Parameters<OrchestratorMeshStatusRequest>,
    ) -> Result<Json<crate::orchestrator::MeshStatusResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied =
                call_orchestrator_proxy_tool("orchestrator_mesh_status", serde_json::json!({}))
                    .await?;
            let parsed: crate::orchestrator::MeshStatusResponse = serde_json::from_value(proxied)
                .map_err(|e| {
                McpError::internal_error(
                    format!("decode orchestrator mesh status proxy response: {e}"),
                    None,
                )
            })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        Ok(Json(orchestrator.mesh_status_async().await))
    }

    #[tool(
        name = "orchestrator_stop",
        description = "Stop a launched orchestrator agent and finalize its session state."
    )]
    pub async fn orchestrator_stop(
        &self,
        Parameters(req): Parameters<OrchestratorStopRequest>,
    ) -> Result<Json<OrchestratorStopResponse>, McpError> {
        if orchestrator_proxy_enabled() {
            let proxied = call_orchestrator_proxy_tool(
                "orchestrator_stop",
                serde_json::to_value(&req).map_err(|e| {
                    McpError::internal_error(
                        format!("serialize orchestrator stop proxy payload: {e}"),
                        None,
                    )
                })?,
            )
            .await?;
            let parsed: OrchestratorStopResponse =
                serde_json::from_value(proxied).map_err(|e| {
                    McpError::internal_error(
                        format!("decode orchestrator stop proxy response: {e}"),
                        None,
                    )
                })?;
            return Ok(Json(parsed));
        }
        let orchestrator = { self.state.lock().await.orchestrator.clone() };
        let stopped = orchestrator
            .stop_agent(OrchStopRequest {
                agent_id: req.agent_id,
                force: req.force.unwrap_or(false),
                purge: req.purge.unwrap_or(false),
            })
            .await
            .map_err(|e| McpError::invalid_params(e, None))?;
        Ok(Json(OrchestratorStopResponse {
            agent_id: stopped.agent_id,
            status: stopped.status,
            trace_session_id: stopped.trace_session_id,
            attestation_ready: stopped.attestation_ready,
            purged: stopped.purged,
        }))
    }

    // ── CLI agent management tools ─────────────────────────────────

    #[tool(
        name = "cli_detect",
        description = "Detect whether an agent CLI is installed on this host. Supports claude, codex, gemini. Example: {\"agent\":\"claude\"}"
    )]
    pub async fn cli_detect(
        &self,
        Parameters(req): Parameters<CliDetectRequest>,
    ) -> Result<Json<CliDetectResponse>, McpError> {
        let agent = req.agent.trim().to_lowercase();
        let detect_cmd = cli_detect_command(&agent)
            .ok_or_else(|| McpError::invalid_params(format!("unknown agent: {agent}"), None))?;
        let (installed, path) = tokio::task::spawn_blocking(move || {
            let output = Command::new("which")
                .arg(detect_cmd)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    (true, if p.is_empty() { None } else { Some(p) })
                }
                _ => (false, None),
            }
        })
        .await
        .unwrap_or((false, None));
        Ok(Json(CliDetectResponse {
            agent,
            installed,
            path,
        }))
    }

    #[tool(
        name = "cli_install",
        description = "Install an agent CLI via npm. Supports claude (@anthropic-ai/claude-code), codex (@openai/codex), gemini (@google/gemini-cli). Example: {\"agent\":\"claude\"}"
    )]
    pub async fn cli_install(
        &self,
        Parameters(req): Parameters<CliInstallRequest>,
    ) -> Result<Json<CliInstallResponse>, McpError> {
        let agent = req.agent.trim().to_lowercase();
        let npm_package = cli_npm_package(&agent)
            .ok_or_else(|| McpError::invalid_params(format!("unknown agent: {agent}"), None))?;
        let pkg = npm_package.to_string();
        let result = tokio::task::spawn_blocking(move || {
            Command::new("npm")
                .args(["install", "-g", &pkg])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
        })
        .await
        .map_err(|e| McpError::internal_error(format!("join error: {e}"), None))?
        .map_err(|e| McpError::internal_error(format!("install failed: {e}"), None))?;

        Ok(Json(CliInstallResponse {
            agent,
            success: result.status.success(),
            exit_code: result.status.code(),
            stdout: String::from_utf8_lossy(&result.stdout).to_string(),
            stderr: String::from_utf8_lossy(&result.stderr).to_string(),
        }))
    }
}

/// CLI detect command for each supported agent.
fn cli_detect_command(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" => Some("claude"),
        "codex" => Some("codex"),
        "gemini" => Some("gemini"),
        _ => None,
    }
}

fn task_to_response(task: crate::orchestrator::task::Task) -> OrchestratorTaskResponse {
    let result = task.result;
    OrchestratorTaskResponse {
        task_id: task.task_id,
        agent_id: task.agent_id,
        status: match task.status {
            crate::orchestrator::task::TaskStatus::Pending => "pending".to_string(),
            crate::orchestrator::task::TaskStatus::Running => "running".to_string(),
            crate::orchestrator::task::TaskStatus::Complete => "complete".to_string(),
            crate::orchestrator::task::TaskStatus::Failed => "failed".to_string(),
            crate::orchestrator::task::TaskStatus::Timeout => "timeout".to_string(),
        },
        answer: task.answer,
        output: result.clone(),
        result,
        error: task.error,
        exit_code: task.exit_code,
        input_tokens: Some(task.usage.input_tokens),
        output_tokens: Some(task.usage.output_tokens),
        cost_usd: Some(task.usage.estimated_cost_usd),
        trace_session_id: task.trace_session_id,
    }
}

fn orchestrator_proxy_enabled() -> bool {
    crate::halo::orchestrator_proxy::orchestrator_proxy_enabled()
}

async fn call_orchestrator_proxy_tool(
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, McpError> {
    crate::halo::orchestrator_proxy::call_orchestrator_tool(tool_name, arguments)
        .await
        .map_err(|e| McpError::internal_error(e, None))
}

/// npm package name for each supported agent CLI.
fn cli_npm_package(agent: &str) -> Option<&'static str> {
    match agent {
        "claude" => Some("@anthropic-ai/claude-code"),
        "codex" => Some("@openai/codex"),
        "gemini" => Some("@google/gemini-cli"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{env_lock, MockOpenAiServer};
    use crate::typed_value::{TypeTag, TypedValue};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn random_local_addr() -> std::net::SocketAddr {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind ephemeral port")
            .local_addr()
            .expect("local addr")
    }

    fn temp_db_path(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before unix epoch")
            .as_nanos();
        p.push(format!(
            "nucleusdb_mcp_tools_{tag}_{}_{}.ndb",
            std::process::id(),
            nanos
        ));
        p
    }

    fn write_typed(db: &mut NucleusDb, key: &str, value: TypedValue) -> (usize, u64) {
        let (idx, cell) = db.put_typed(key, value).expect("put_typed");
        if idx >= db.state.values.len() {
            db.state.values.resize(idx + 1, 0);
        }
        db.state.values[idx] = cell;
        (idx, cell)
    }

    fn cleanup_db_files(db_path: &Path) {
        let _ = std::fs::remove_file(db_path);
        let wal_path = NucleusDbMcpService::default_wal_path(db_path);
        let _ = std::fs::remove_file(wal_path);
    }

    struct EnvVarRestore {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarRestore {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, prev }
        }

        fn unset(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, prev }
        }
    }

    impl Drop for EnvVarRestore {
        fn drop(&mut self) {
            if let Some(prev) = &self.prev {
                std::env::set_var(self.key, prev);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    struct MockContainerToolServer {
        pub base_url: String,
        shutdown: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl MockContainerToolServer {
        fn spawn() -> Self {
            let listener =
                std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock container server");
            listener
                .set_nonblocking(true)
                .expect("set mock container nonblocking");
            let base_url = format!("http://{}", listener.local_addr().expect("mock local addr"));
            let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let shutdown_flag = shutdown.clone();
            let handle = std::thread::spawn(move || {
                while !shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                            let mut request = [0u8; 4096];
                            let read = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                            let body = String::from_utf8_lossy(&request[..read]);
                            let (result, session) = if body.contains("\"method\":\"initialize\"")
                                || body.contains("\"method\": \"initialize\"")
                            {
                                (
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": 0,
                                        "result": {"protocolVersion": "2025-03-26", "serverInfo": {"name": "mock-sub", "version": "test"}}
                                    }),
                                    Some("mock-session".to_string()),
                                )
                            } else if body.contains("nucleusdb_container_initialize") {
                                (
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": 1,
                                        "result": {
                                            "container_id": "ctr-sub",
                                            "state": "locked",
                                            "agent_id": "sub-agent",
                                            "trace_session_id": "trace-sub",
                                            "reuse_policy": "single_use"
                                        }
                                    }),
                                    None,
                                )
                            } else if body.contains("nucleusdb_container_agent_prompt") {
                                (
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": 1,
                                        "result": {
                                            "content": "subsidiary response",
                                            "model": "openrouter/test-model",
                                            "input_tokens": 3,
                                            "output_tokens": 5,
                                            "cost_usd": 0.25,
                                            "tool_calls": [],
                                            "duration_ms": 12
                                        }
                                    }),
                                    None,
                                )
                            } else if body.contains("nucleusdb_container_deinitialize") {
                                (
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": 1,
                                        "result": {
                                            "container_id": "ctr-sub",
                                            "state": "empty",
                                            "trace_session_id": null,
                                            "reuse_policy": "single_use"
                                        }
                                    }),
                                    None,
                                )
                            } else {
                                (
                                    json!({
                                        "jsonrpc": "2.0",
                                        "id": 1,
                                        "error": {"code": -32601, "message": "unknown mock tool"}
                                    }),
                                    None,
                                )
                            };
                            let payload = serde_json::to_string(&result).expect("encode mock");
                            let mut response = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
                                payload.len()
                            );
                            if let Some(session) = session {
                                response.push_str(&format!("mcp-session-id: {session}\r\n"));
                            }
                            response.push_str("\r\n");
                            let _ = std::io::Write::write_all(
                                &mut stream,
                                format!("{response}{payload}").as_bytes(),
                            );
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(25));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                base_url,
                shutdown,
                handle: Some(handle),
            }
        }
    }

    impl Drop for MockContainerToolServer {
        fn drop(&mut self) {
            self.shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = std::net::TcpStream::connect(
                self.base_url
                    .trim_start_matches("http://")
                    .parse::<std::net::SocketAddr>()
                    .expect("mock socket addr"),
            );
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    #[test]
    #[allow(clippy::await_holding_lock)]
    fn mesh_call_didcomm_executes_remote_tool() {
        let join = std::thread::Builder::new()
            .name("mesh_call_didcomm_executes_remote_tool".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("build tokio runtime");
                rt.block_on(async {
                    let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
                    let sender_seed = [0x61u8; 64];
                    let sender = crate::halo::did::did_from_genesis_seed(&sender_seed)
                        .expect("sender identity");
                    std::env::set_var("NUCLEUSDB_AGENT_PRIVATE_KEY", hex::encode(sender_seed));

                    let remote_db_path = temp_db_path("mesh_didcomm_remote");
                    let remote_addr = random_local_addr();

                    let registry_path = std::env::temp_dir().join(format!(
                        "mesh_registry_{}_{}.json",
                        std::process::id(),
                        crate::pod::now_unix_nanos()
                    ));
                    let mut registry = crate::container::mesh::PeerRegistry::new();
                    let now = crate::pod::now_unix();
                    registry.register(crate::container::mesh::PeerInfo {
                        agent_id: "agent-remote".to_string(),
                        container_name: "remote".to_string(),
                        did_uri: Some(sender.did.clone()),
                        mcp_endpoint: format!("http://{remote_addr}/mcp"),
                        discovery_endpoint: format!("http://{remote_addr}/.well-known/nucleus-pod"),
                        registered_at: now,
                        last_seen: now,
                    });
                    registry.save(&registry_path).expect("save mesh registry");
                    std::env::set_var(
                        "NUCLEUSDB_MESH_REGISTRY",
                        registry_path.display().to_string(),
                    );

                    let remote_server =
                        tokio::spawn(crate::mcp::server::remote::run_remote_mcp_server(
                            crate::mcp::server::remote::RemoteServerConfig {
                                db_path: remote_db_path.display().to_string(),
                                listen_addr: remote_addr,
                                auth: crate::mcp::server::auth::AuthConfig::default(),
                                endpoint_path: "/mcp".to_string(),
                            },
                        ));
                    tokio::time::sleep(Duration::from_millis(200)).await;

                    let local_db_path = temp_db_path("mesh_didcomm_local");
                    let service = NucleusDbMcpService::new(&local_db_path).expect("local service");
                    let Json(resp) = service
                        .mesh_call(Parameters(MeshCallRequest {
                            peer_agent_id: "agent-remote".to_string(),
                            tool_name: "nucleusdb_status".to_string(),
                            arguments: json!({}),
                            use_didcomm: true,
                        }))
                        .await
                        .expect("mesh_call didcomm path");
                    assert_eq!(resp.auth_method, "didcomm-v2");
                    assert_eq!(resp.peer_agent_id, "agent-remote");
                    assert_eq!(resp.tool_name, "nucleusdb_status");
                    assert!(
                        resp.result.get("status").is_some()
                            || resp.result.get("structuredContent").is_some()
                    );

                    remote_server.abort();
                    let _ = std::fs::remove_file(registry_path);
                    cleanup_db_files(&local_db_path);
                    cleanup_db_files(&remote_db_path);
                });
            })
            .expect("spawn large-stack test thread");
        join.join().expect("mesh DIDComm test thread panicked");
    }

    #[test]
    fn parse_export_format_defaults_and_aliases() {
        assert_eq!(
            NucleusDbMcpService::parse_export_format(None).expect("default"),
            ExportFormat::LegacyV1
        );
        assert_eq!(
            NucleusDbMcpService::parse_export_format(Some("typed")).expect("typed alias"),
            ExportFormat::TypedV2
        );
        assert_eq!(
            NucleusDbMcpService::parse_export_format(Some("v2")).expect("v2 alias"),
            ExportFormat::TypedV2
        );
        assert_eq!(
            NucleusDbMcpService::parse_export_format(Some(" map ")).expect("map alias"),
            ExportFormat::LegacyV1
        );
    }

    #[test]
    fn container_launch_request_parses_mesh_and_env_options() {
        let req: ContainerLaunchRequest = serde_json::from_value(json!({
            "image": "nucleusdb-mcp:latest",
            "agent_id": "agent-qa",
            "command": ["/bin/sh", "-lc", "echo ready"],
            "host_sock": "/tmp/agent-qa.sock",
            "env": {
                "AGENTHALO_MCP_SECRET": "secret",
                "NUCLEUSDB_AGENT_PRIVATE_KEY": "abcd"
            },
            "mesh": {
                "enabled": true,
                "mcp_port": 8420,
                "registry_volume": "/tmp/nucleusdb-mesh",
                "agent_did": "did:key:z6MkAgentQa"
            }
        }))
        .expect("deserialize container launch request");

        assert_eq!(req.image, "nucleusdb-mcp:latest");
        assert_eq!(req.agent_id, "agent-qa");
        assert_eq!(
            req.env
                .get("AGENTHALO_MCP_SECRET")
                .map(String::as_str)
                .unwrap_or(""),
            "secret"
        );
        let mesh = req.mesh.expect("mesh config");
        assert_eq!(mesh.enabled, Some(true));
        assert_eq!(mesh.mcp_port, Some(8420));
        assert_eq!(mesh.registry_volume.as_deref(), Some("/tmp/nucleusdb-mesh"));
    }

    #[tokio::test]
    async fn container_lock_status_tool_reports_current_container() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "mcp-container");
        let db_path = temp_db_path("container_lock_status");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(resp) = service
            .container_lock_status(Parameters(ContainerLockStatusRequest::default()))
            .await
            .expect("container lock status");
        assert_eq!(resp.container_id, "mcp-container");
        assert_eq!(resp.state, "empty");
        assert_eq!(resp.reuse_policy, ReusePolicy::Reusable);
        assert_eq!(resp.lock["container_id"], "mcp-container");
        assert_eq!(resp.lock["state"]["state"], "empty");

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn container_initialize_prompt_deinitialize_roundtrip() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "mcp-runtime");
        let server = MockOpenAiServer::spawn("openrouter/test-model", "container api response");
        let db_path = temp_db_path("container_roundtrip");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(initialized) = service
            .container_initialize(Parameters(ContainerInitializeRequest {
                hookup: ContainerHookupRequest::Api {
                    provider: "openrouter".to_string(),
                    model: "openrouter/test-model".to_string(),
                    api_key_source: "unused".to_string(),
                    base_url_override: Some(server.base_url.clone()),
                },
                reuse_policy: Some(ReusePolicy::SingleUse),
            }))
            .await
            .expect("initialize container hookup");
        assert_eq!(initialized.container_id, "mcp-runtime");
        assert_eq!(initialized.state, "locked");
        assert_eq!(initialized.reuse_policy, ReusePolicy::SingleUse);
        assert!(initialized.trace_session_id.is_some());

        let Json(response) = service
            .container_agent_prompt(Parameters(ContainerAgentPromptRequest {
                prompt: "review this module".to_string(),
            }))
            .await
            .expect("container prompt");
        assert_eq!(response.content, "container api response");
        assert_eq!(response.model, "openrouter/test-model");

        let Json(deinitialized) = service
            .container_deinitialize(Parameters(ContainerDeinitializeRequest {}))
            .await
            .expect("deinitialize container hookup");
        assert_eq!(deinitialized.container_id, "mcp-runtime");
        assert_eq!(deinitialized.state, "empty");
        assert_eq!(deinitialized.reuse_policy, ReusePolicy::SingleUse);
        assert!(deinitialized.trace_session_id.is_none());

        let err = service
            .container_agent_prompt(Parameters(ContainerAgentPromptRequest {
                prompt: "should fail after deinit".to_string(),
            }))
            .await
            .err()
            .expect("prompt after deinitialize must fail");
        assert!(format!("{err:?}").contains("not initialized"));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn container_list_bounds_remote_lock_status_lookup_latency() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let registry_dir = tempfile::tempdir().expect("tempdir");
        let registry_path = registry_dir.path().join("peers.json");
        let _registry = EnvVarRestore::set(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.to_str().expect("utf8 registry path"),
        );

        let run_dir = std::env::temp_dir().join("agenthalo-native");
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        // Clean up stale session files from prior test runs to avoid count
        // mismatches — list_sessions() reads every session dir in run_dir.
        for entry in std::fs::read_dir(&run_dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            }
        }
        let session_ids = ["sess-hang-a", "sess-hang-b"];
        let session_paths = [
            run_dir.join(session_ids[0]).join("session.json"),
            run_dir.join(session_ids[1]).join("session.json"),
        ];

        let listeners = (0..2)
            .map(|_| std::net::TcpListener::bind("127.0.0.1:0").expect("bind hanging listener"))
            .collect::<Vec<_>>();
        let addrs = listeners
            .iter()
            .map(|listener| listener.local_addr().expect("listener addr"))
            .collect::<Vec<_>>();
        let handles = listeners
            .into_iter()
            .map(|listener| {
                std::thread::spawn(move || {
                    if let Ok((_stream, _addr)) = listener.accept() {
                        std::thread::sleep(Duration::from_secs(5));
                    }
                })
            })
            .collect::<Vec<_>>();

        for (idx, path) in session_paths.iter().enumerate() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create session dir");
            }
            let session = crate::container::launcher::SessionInfo {
                session_id: session_ids[idx].to_string(),
                container_id: format!("ctr-hang-{idx}"),
                image: "nucleusdb-agent:test".to_string(),
                agent_id: format!("agent-hang-{idx}"),
                host_sock: std::env::temp_dir().join(format!("hang-{idx}.sock")),
                started_at_unix: crate::pod::now_unix(),
                mesh_port: Some(3000 + idx as u16),
                pid: None,
                log_path: None,
                agent_home: Some(std::env::temp_dir().join(format!("hang-home-{idx}"))),
            };
            std::fs::write(
                path,
                serde_json::to_vec_pretty(&session).expect("encode session"),
            )
            .expect("write session");
        }

        let mut registry = crate::container::PeerRegistry::new();
        for (idx, addr) in addrs.iter().enumerate() {
            registry.register(crate::container::PeerInfo {
                agent_id: format!("agent-hang-{idx}"),
                container_name: format!("container-hang-{idx}"),
                did_uri: None,
                mcp_endpoint: format!("http://{addr}/mcp"),
                discovery_endpoint: format!("http://{addr}/.well-known/nucleus-pod"),
                registered_at: crate::pod::now_unix(),
                last_seen: crate::pod::now_unix(),
            });
        }
        registry.save(&registry_path).expect("save registry");

        let db_path = temp_db_path("container_list_timeout");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let started = tokio::time::Instant::now();
        let Json(response) = service.container_list().await.expect("container list");
        let elapsed = started.elapsed();
        let max_elapsed = Duration::from_secs(6);

        let matching = response
            .sessions
            .iter()
            .filter(|view| session_ids.contains(&view.session_id.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(matching.len(), 2);
        assert!(matching
            .iter()
            .all(|view| view.lock_state.as_deref() == Some("unknown")));
        assert!(matching.iter().all(|view| view.reuse_policy.is_none()));
        assert!(elapsed < max_elapsed, "elapsed: {elapsed:?}");

        for path in &session_paths {
            if let Some(parent) = path.parent() {
                let _ = std::fs::remove_dir_all(parent);
            }
        }
        for handle in handles {
            let _ = handle.join();
        }
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn container_list_uses_mesh_auth_token_for_lock_status() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _secret = EnvVarRestore::set("AGENTHALO_MCP_SECRET", "mesh-secret");
        let _mesh_secret = EnvVarRestore::unset("NUCLEUSDB_MESH_AUTH_TOKEN");
        let registry_dir = tempfile::tempdir().expect("tempdir");
        let registry_path = registry_dir.path().join("peers.json");
        let _registry = EnvVarRestore::set(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.to_str().expect("utf8 registry path"),
        );

        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind auth lock-status listener");
        listener
            .set_nonblocking(true)
            .expect("set nonblocking auth listener");
        let addr = listener.local_addr().expect("listener addr");
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_flag = shutdown.clone();
        let handle = std::thread::spawn(move || {
            while !shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let mut request = [0u8; 4096];
                        let read = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                        let body = String::from_utf8_lossy(&request[..read]);
                        let body_lower = body.to_ascii_lowercase();
                        let authorized = body_lower.contains("authorization: bearer mesh-secret");
                        let (status_line, payload, session) = if !authorized {
                            (
                                "HTTP/1.1 401 Unauthorized\r\n",
                                json!({"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"unauthorized"}}),
                                None,
                            )
                        } else if body.contains("\"method\":\"initialize\"")
                            || body.contains("\"method\": \"initialize\"")
                        {
                            (
                                "HTTP/1.1 200 OK\r\n",
                                json!({
                                    "jsonrpc":"2.0",
                                    "id":0,
                                    "result":{"protocolVersion":"2025-03-26","serverInfo":{"name":"mock-auth-lock","version":"test"}}
                                }),
                                Some("auth-session".to_string()),
                            )
                        } else if body.contains("nucleusdb_container_lock_status") {
                            (
                                "HTTP/1.1 200 OK\r\n",
                                json!({
                                    "jsonrpc":"2.0",
                                    "id":1,
                                    "result":{
                                        "content":[{"type":"text","text":"{\"state\":\"empty\",\"reuse_policy\":\"reusable\"}"}],
                                        "isError":false,
                                        "structuredContent":{"state":"empty","reuse_policy":"reusable"}
                                    }
                                }),
                                None,
                            )
                        } else {
                            (
                                "HTTP/1.1 404 Not Found\r\n",
                                json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"unknown mock tool"}}),
                                None,
                            )
                        };
                        let payload = serde_json::to_string(&payload).expect("encode auth mock");
                        let mut response = format!(
                            "{status_line}Content-Type: application/json\r\nContent-Length: {}\r\n",
                            payload.len()
                        );
                        if let Some(session) = session {
                            response.push_str(&format!("mcp-session-id: {session}\r\n"));
                        }
                        response.push_str("\r\n");
                        let _ = std::io::Write::write_all(
                            &mut stream,
                            format!("{response}{payload}").as_bytes(),
                        );
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        });

        let run_dir = std::env::temp_dir().join("agenthalo-native");
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        let session_dir = run_dir.join("sess-auth-lock");
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("session.json");
        std::fs::write(
            &session_path,
            serde_json::to_vec_pretty(&crate::container::launcher::SessionInfo {
                session_id: "sess-auth-lock".to_string(),
                container_id: "ctr-auth-lock".to_string(),
                image: "nucleusdb-agent:test".to_string(),
                agent_id: "agent-auth-lock".to_string(),
                host_sock: std::env::temp_dir().join("auth-lock.sock"),
                started_at_unix: crate::pod::now_unix(),
                mesh_port: Some(3000),
                pid: None,
                log_path: None,
                agent_home: Some(session_dir.join("home")),
            })
            .expect("encode session"),
        )
        .expect("write session");

        let mut registry = crate::container::PeerRegistry::new();
        registry.register(crate::container::PeerInfo {
            agent_id: "agent-auth-lock".to_string(),
            container_name: "container-auth-lock".to_string(),
            did_uri: None,
            mcp_endpoint: format!("http://{addr}/mcp"),
            discovery_endpoint: format!("http://{addr}/.well-known/nucleus-pod"),
            registered_at: crate::pod::now_unix(),
            last_seen: crate::pod::now_unix(),
        });
        registry.save(&registry_path).expect("save registry");

        let db_path = temp_db_path("container_list_mesh_auth");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(response) = service.container_list().await.expect("container list");

        let matching = response
            .sessions
            .iter()
            .find(|view| view.session_id == "sess-auth-lock")
            .expect("auth session present");
        assert_eq!(matching.lock_state.as_deref(), Some("empty"));
        assert_eq!(matching.reuse_policy.as_deref(), Some("reusable"));

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = handle.join();
        let _ = std::fs::remove_dir_all(session_dir);
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn mesh_call_uses_mesh_auth_token_fallback() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _secret = EnvVarRestore::set("AGENTHALO_MCP_SECRET", "mesh-secret");
        let _mesh_secret = EnvVarRestore::unset("NUCLEUSDB_MESH_AUTH_TOKEN");
        let registry_dir = tempfile::tempdir().expect("tempdir");
        let registry_path = registry_dir.path().join("peers.json");
        let _registry = EnvVarRestore::set(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.to_str().expect("utf8 registry path"),
        );

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mesh_call");
        listener
            .set_nonblocking(true)
            .expect("set nonblocking mesh_call");
        let addr = listener.local_addr().expect("listener addr");
        let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shutdown_flag = shutdown.clone();
        let handle = std::thread::spawn(move || {
            while !shutdown_flag.load(std::sync::atomic::Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                        let mut request = [0u8; 4096];
                        let read = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                        let body = String::from_utf8_lossy(&request[..read]);
                        let authorized = body
                            .to_ascii_lowercase()
                            .contains("authorization: bearer mesh-secret");
                        let payload = if !authorized {
                            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"unauthorized"}})
                        } else if body.contains("\"method\":\"initialize\"")
                            || body.contains("\"method\": \"initialize\"")
                        {
                            json!({
                                "jsonrpc":"2.0",
                                "id":0,
                                "result":{"protocolVersion":"2025-03-26","serverInfo":{"name":"mock-mesh-call","version":"test"}}
                            })
                        } else {
                            json!({
                                "jsonrpc":"2.0",
                                "id":1,
                                "result":{
                                    "content":[{"type":"text","text":"{\"ok\":true}"}],
                                    "isError":false,
                                    "structuredContent":{"ok":true}
                                }
                            })
                        };
                        let payload = serde_json::to_string(&payload).expect("encode payload");
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                            payload.len(),
                            payload
                        );
                        let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    Err(_) => break,
                }
            }
        });

        let mut registry = crate::container::PeerRegistry::new();
        registry.register(crate::container::PeerInfo {
            agent_id: "mesh-auth-peer".to_string(),
            container_name: "mesh-auth-peer".to_string(),
            did_uri: None,
            mcp_endpoint: format!("http://{addr}/mcp"),
            discovery_endpoint: format!("http://{addr}/.well-known/nucleus-pod"),
            registered_at: crate::pod::now_unix(),
            last_seen: crate::pod::now_unix(),
        });
        registry.save(&registry_path).expect("save registry");

        let db_path = temp_db_path("mesh_call_auth_fallback");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(response) = service
            .mesh_call(Parameters(MeshCallRequest {
                peer_agent_id: "mesh-auth-peer".to_string(),
                tool_name: "nucleusdb_container_lock_status".to_string(),
                arguments: json!({}),
                use_didcomm: false,
            }))
            .await
            .expect("mesh_call");
        assert_eq!(response.auth_method, "bearer");
        assert_eq!(
            response.result.get("ok").and_then(|v| v.as_bool()),
            Some(true)
        );

        shutdown.store(true, std::sync::atomic::Ordering::Relaxed);
        let _ = handle.join();
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn subsidiary_roundtrip_tracks_only_owned_sessions() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "operator-container");
        let registry_dir = tempfile::tempdir().expect("registry tempdir");
        let registry_path = registry_dir.path().join("peers.json");
        let _registry = EnvVarRestore::set(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.to_str().expect("utf8 registry path"),
        );

        let db_path = temp_db_path("subsidiary_roundtrip");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(operator) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "operator".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["operator".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch operator");

        let run_dir = std::env::temp_dir().join("agenthalo-native");
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        let owned_session_id = "sess-owned-phase4";
        let unowned_session_id = "sess-unowned-phase4";
        for (session_id, agent_id) in [
            (owned_session_id, "peer-owned"),
            (unowned_session_id, "peer-unowned"),
        ] {
            let session_dir = run_dir.join(session_id);
            std::fs::create_dir_all(&session_dir).expect("create session dir");
            let path = session_dir.join("session.json");
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(&crate::container::launcher::SessionInfo {
                    session_id: session_id.to_string(),
                    container_id: format!("ctr-{session_id}"),
                    image: "nucleusdb-agent:test".to_string(),
                    agent_id: agent_id.to_string(),
                    host_sock: std::env::temp_dir().join(format!("{session_id}.sock")),
                    started_at_unix: crate::pod::now_unix(),
                    mesh_port: Some(3000),
                    pid: None,
                    log_path: None,
                    agent_home: Some(session_dir.join("home")),
                })
                .expect("encode session"),
            )
            .expect("write session");
        }

        let mock = MockContainerToolServer::spawn();
        let mut peers = crate::container::PeerRegistry::new();
        peers.register(crate::container::PeerInfo {
            agent_id: "peer-owned".to_string(),
            container_name: "peer-owned".to_string(),
            did_uri: None,
            mcp_endpoint: format!("{}/mcp", mock.base_url),
            discovery_endpoint: format!("{}/.well-known/nucleus-pod", mock.base_url),
            registered_at: crate::pod::now_unix(),
            last_seen: crate::pod::now_unix(),
        });
        peers.save(&registry_path).expect("save peer registry");

        let mut registry =
            SubsidiaryRegistry::load_or_create(&operator.agent_id).expect("subsidiary registry");
        registry.register_provision(
            owned_session_id.to_string(),
            "ctr-sess-owned-phase4".to_string(),
            "peer-owned".to_string(),
        );
        registry.save().expect("save operator registry");

        let Json(initialized) = service
            .subsidiary_initialize(Parameters(SubsidiaryInitializeRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: owned_session_id.to_string(),
                hookup: ContainerHookupRequest::Api {
                    provider: "openrouter".to_string(),
                    model: "openrouter/test-model".to_string(),
                    api_key_source: "unused".to_string(),
                    base_url_override: None,
                },
                reuse_policy: Some(ReusePolicy::SingleUse),
            }))
            .await
            .expect("subsidiary initialize");
        assert_eq!(initialized.state, "locked");
        assert_eq!(initialized.reuse_policy, ReusePolicy::SingleUse);

        let Json(task) = service
            .subsidiary_send_task(Parameters(SubsidiarySendTaskRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: owned_session_id.to_string(),
                prompt: "review this".to_string(),
                timeout_secs: Some(30),
            }))
            .await
            .expect("subsidiary send_task");
        assert_eq!(task.status, "complete");
        assert_eq!(task.result.as_deref(), Some("subsidiary response"));

        let Json(loaded_task) = service
            .subsidiary_get_result(Parameters(SubsidiaryGetResultRequest {
                operator_agent_id: operator.agent_id.clone(),
                task_id: task.task_id.clone(),
            }))
            .await
            .expect("subsidiary get_result");
        assert_eq!(loaded_task.task_id, task.task_id);
        assert_eq!(loaded_task.cost_usd, Some(0.25));

        let Json(listed) = service
            .subsidiary_list(Parameters(SubsidiaryListRequest {
                operator_agent_id: operator.agent_id.clone(),
            }))
            .await
            .expect("subsidiary list");
        assert_eq!(listed.count, 1);
        assert_eq!(listed.subsidiaries[0].session_id, owned_session_id);

        let Json(deinitialized) = service
            .subsidiary_deinitialize(Parameters(SubsidiaryDeinitializeRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: owned_session_id.to_string(),
            }))
            .await
            .expect("subsidiary deinitialize");
        assert_eq!(deinitialized.state, "empty");
        assert_eq!(deinitialized.reuse_policy, ReusePolicy::SingleUse);

        let Json(destroyed) = service
            .subsidiary_destroy(Parameters(SubsidiaryDestroyRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: owned_session_id.to_string(),
            }))
            .await
            .expect("subsidiary destroy");
        assert!(destroyed.destroyed);
        assert!(!run_dir.join(owned_session_id).exists());

        let Json(listed_after) = service
            .subsidiary_list(Parameters(SubsidiaryListRequest {
                operator_agent_id: operator.agent_id.clone(),
            }))
            .await
            .expect("subsidiary list after destroy");
        assert_eq!(listed_after.count, 0);

        let _ = service
            .orchestrator_stop(Parameters(OrchestratorStopRequest {
                agent_id: operator.agent_id.clone(),
                force: Some(true),
                purge: Some(false),
            }))
            .await;
        let _ = std::fs::remove_dir_all(run_dir.join(unowned_session_id));
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn subsidiary_initialize_waits_for_peer_registration() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "operator-container");
        let registry_dir = tempfile::tempdir().expect("registry tempdir");
        let registry_path = registry_dir.path().join("peers.json");
        let _registry = EnvVarRestore::set(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.to_str().expect("utf8 registry path"),
        );

        let db_path = temp_db_path("subsidiary_waits_for_peer");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(operator) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "operator".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["operator".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch operator");

        let run_dir = std::env::temp_dir().join("agenthalo-native");
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        let session_id = "sess-delayed-peer-phase4";
        let session_dir = run_dir.join(session_id);
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("session.json");
        std::fs::write(
            &session_path,
            serde_json::to_vec_pretty(&crate::container::launcher::SessionInfo {
                session_id: session_id.to_string(),
                container_id: "ctr-delayed-peer".to_string(),
                image: "nucleusdb-agent:test".to_string(),
                agent_id: "peer-delayed".to_string(),
                host_sock: std::env::temp_dir().join("sess-delayed-peer.sock"),
                started_at_unix: crate::pod::now_unix(),
                mesh_port: None,
                pid: None,
                log_path: None,
                agent_home: Some(session_dir.join("home")),
            })
            .expect("encode session"),
        )
        .expect("write session");

        let mut registry =
            SubsidiaryRegistry::load_or_create(&operator.agent_id).expect("subsidiary registry");
        registry.register_provision(
            session_id.to_string(),
            "ctr-delayed-peer".to_string(),
            "peer-delayed".to_string(),
        );
        registry.save().expect("save operator registry");

        let mock = MockContainerToolServer::spawn();
        let delayed_registry_path = registry_path.clone();
        let delayed_base_url = mock.base_url.clone();
        let delayed_writer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            let mut peers = crate::container::PeerRegistry::new();
            peers.register(crate::container::PeerInfo {
                agent_id: "peer-delayed".to_string(),
                container_name: "peer-delayed".to_string(),
                did_uri: None,
                mcp_endpoint: format!("{delayed_base_url}/mcp"),
                discovery_endpoint: format!("{delayed_base_url}/.well-known/nucleus-pod"),
                registered_at: crate::pod::now_unix(),
                last_seen: crate::pod::now_unix(),
            });
            peers
                .save(&delayed_registry_path)
                .expect("save delayed peer registry");
        });

        let started = tokio::time::Instant::now();
        let Json(initialized) = service
            .subsidiary_initialize(Parameters(SubsidiaryInitializeRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: session_id.to_string(),
                hookup: ContainerHookupRequest::Api {
                    provider: "openrouter".to_string(),
                    model: "openrouter/test-model".to_string(),
                    api_key_source: "unused".to_string(),
                    base_url_override: None,
                },
                reuse_policy: Some(ReusePolicy::SingleUse),
            }))
            .await
            .expect("subsidiary initialize");
        assert_eq!(initialized.state, "locked");
        assert!(
            started.elapsed() >= Duration::from_millis(400),
            "initialize should wait for delayed mesh registration"
        );

        delayed_writer.join().expect("join delayed writer");
        let _ = service
            .orchestrator_stop(Parameters(OrchestratorStopRequest {
                agent_id: operator.agent_id.clone(),
                force: Some(true),
                purge: Some(false),
            }))
            .await;
        let _ = std::fs::remove_dir_all(session_dir);
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn subsidiary_initialize_falls_back_to_session_mesh_endpoint() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "operator-container");
        let registry_dir = tempfile::tempdir().expect("registry tempdir");
        let registry_path = registry_dir.path().join("peers.json");
        let _registry = EnvVarRestore::set(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.to_str().expect("utf8 registry path"),
        );

        let db_path = temp_db_path("subsidiary_fallback_mesh_endpoint");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(operator) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "operator".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["operator".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch operator");

        let mock = MockContainerToolServer::spawn();
        let port = mock
            .base_url
            .rsplit(':')
            .next()
            .and_then(|v| v.parse::<u16>().ok())
            .expect("mock port");

        let run_dir = std::env::temp_dir().join("agenthalo-native");
        std::fs::create_dir_all(&run_dir).expect("create run dir");
        let session_id = "localhost";
        let session_dir = run_dir.join(session_id);
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        let session_path = session_dir.join("session.json");
        std::fs::write(
            &session_path,
            serde_json::to_vec_pretty(&crate::container::launcher::SessionInfo {
                session_id: session_id.to_string(),
                container_id: "ctr-fallback-peer".to_string(),
                image: "nucleusdb-agent:test".to_string(),
                agent_id: "peer-fallback".to_string(),
                host_sock: std::env::temp_dir().join("sess-fallback-peer.sock"),
                started_at_unix: crate::pod::now_unix(),
                mesh_port: Some(port),
                pid: None,
                log_path: None,
                agent_home: Some(session_dir.join("home")),
            })
            .expect("encode session"),
        )
        .expect("write session");

        let mut registry =
            SubsidiaryRegistry::load_or_create(&operator.agent_id).expect("subsidiary registry");
        registry.register_provision(
            session_id.to_string(),
            "ctr-fallback-peer".to_string(),
            "peer-fallback".to_string(),
        );
        registry.save().expect("save operator registry");

        let Json(initialized) = service
            .subsidiary_initialize(Parameters(SubsidiaryInitializeRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: session_id.to_string(),
                hookup: ContainerHookupRequest::Api {
                    provider: "openrouter".to_string(),
                    model: "openrouter/test-model".to_string(),
                    api_key_source: "unused".to_string(),
                    base_url_override: None,
                },
                reuse_policy: Some(ReusePolicy::SingleUse),
            }))
            .await
            .expect("subsidiary initialize");
        assert_eq!(initialized.state, "locked");
        assert_eq!(initialized.reuse_policy, ReusePolicy::SingleUse);

        let _ = service
            .orchestrator_stop(Parameters(OrchestratorStopRequest {
                agent_id: operator.agent_id.clone(),
                force: Some(true),
                purge: Some(false),
            }))
            .await;
        let _ = std::fs::remove_file(session_path);
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn subsidiary_tools_accept_explicit_orchestrator_override_when_service_state_is_stale() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "operator-container");

        let db_path = temp_db_path("subsidiary_override");
        let stale_governors = build_default_registry();
        let stale_service = NucleusDbMcpService::new_with_shared_runtime(
            &db_path,
            Some(SharedServiceRuntime {
                vault: None,
                pty_manager: Arc::new(crate::cockpit::pty_manager::PtyManager::new(4)),
                governor_registry: stale_governors,
                orchestrator: Orchestrator::new(
                    Arc::new(crate::cockpit::pty_manager::PtyManager::new(4)),
                    None,
                    db_path.clone(),
                ),
            }),
        )
        .expect("service");

        let live_orchestrator = Orchestrator::new(
            Arc::new(crate::cockpit::pty_manager::PtyManager::new(4)),
            None,
            db_path.clone(),
        );
        let launched = live_orchestrator
            .launch_agent(crate::orchestrator::LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "operator".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["operator".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch operator");

        let err = stale_service
            .subsidiary_list(Parameters(SubsidiaryListRequest {
                operator_agent_id: launched.agent_id.clone(),
            }))
            .await
            .err()
            .expect("stale service must reject unknown operator");
        assert!(format!("{err:?}").contains("unknown agent_id"));

        let Json(listed) = stale_service
            .subsidiary_list_with_orchestrator(
                SubsidiaryListRequest {
                    operator_agent_id: launched.agent_id.clone(),
                },
                live_orchestrator.clone(),
            )
            .await
            .expect("explicit orchestrator override should succeed");
        assert_eq!(listed.operator_agent_id, launched.agent_id);
        assert_eq!(listed.count, 0);

        let _ = live_orchestrator
            .stop_agent(crate::orchestrator::StopRequest {
                agent_id: launched.agent_id,
                force: true,
                purge: false,
            })
            .await;
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn subsidiary_tools_reject_unowned_session() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarRestore::set("AGENTHALO_HOME", home.path().to_str().expect("utf8 home"));
        let _container = EnvVarRestore::set("NUCLEUSDB_MESH_AGENT_ID", "operator-container");

        let db_path = temp_db_path("subsidiary_unowned");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(operator) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "operator".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["operator".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch operator");

        let err = service
            .subsidiary_destroy(Parameters(SubsidiaryDestroyRequest {
                operator_agent_id: operator.agent_id.clone(),
                session_id: "sess-unknown".to_string(),
            }))
            .await
            .err()
            .expect("unknown subsidiary must fail");
        assert!(format!("{err:?}").contains("does not own subsidiary session"));

        let _ = service
            .orchestrator_stop(Parameters(OrchestratorStopRequest {
                agent_id: operator.agent_id.clone(),
                force: Some(true),
                purge: Some(false),
            }))
            .await;
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn query_returns_typed_fields_and_fails_closed_on_decode_error() {
        let db_path = temp_db_path("query_contract");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        {
            let mut guard = service.state.lock().await;
            write_typed(
                &mut guard.db,
                "good:text",
                TypedValue::Text("hello".to_string()),
            );
            let (_, bad_cell) = write_typed(&mut guard.db, "bad:text", TypedValue::Integer(7));
            guard.db.type_map.set("bad:text", TypeTag::Text);
            assert_eq!(
                guard.db.state.values[guard.db.keymap.get("bad:text").unwrap()],
                bad_cell
            );
        }

        let Json(good_row) = service
            .query(Parameters(QueryRequest {
                key: "good:text".to_string(),
            }))
            .await
            .expect("good query");
        assert_eq!(good_row.type_tag, "text");
        assert_eq!(good_row.typed_value, json!("hello"));
        assert_eq!(good_row.display, "hello");

        let bad_err = service
            .query(Parameters(QueryRequest {
                key: "bad:text".to_string(),
            }))
            .await
            .err()
            .expect("decode mismatch must fail closed");
        let err_dbg = format!("{bad_err:?}");
        assert!(err_dbg.contains("typed decode failed for key 'bad:text'"));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn query_range_fails_closed_when_any_match_has_decode_error() {
        let db_path = temp_db_path("query_range_contract");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        {
            let mut guard = service.state.lock().await;
            write_typed(
                &mut guard.db,
                "pref:ok",
                TypedValue::Text("value".to_string()),
            );
            write_typed(&mut guard.db, "pref:bad", TypedValue::Integer(11));
            guard.db.type_map.set("pref:bad", TypeTag::Json);
        }

        let err = service
            .query_range(Parameters(QueryRangeRequest {
                pattern: "pref:%".to_string(),
            }))
            .await
            .err()
            .expect("query_range must fail on decode error");
        let err_dbg = format!("{err:?}");
        assert!(err_dbg.contains("query_range failed for key 'pref:bad'"));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn query_range_includes_count_metadata() {
        let db_path = temp_db_path("query_range_count");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        {
            let mut guard = service.state.lock().await;
            write_typed(&mut guard.db, "pref:a", TypedValue::Integer(1));
            write_typed(&mut guard.db, "pref:b", TypedValue::Integer(2));
            write_typed(&mut guard.db, "other:c", TypedValue::Integer(3));
        }

        let Json(resp) = service
            .query_range(Parameters(QueryRangeRequest {
                pattern: "pref:%".to_string(),
            }))
            .await
            .expect("query_range should succeed");
        assert_eq!(resp.pattern, "pref:%");
        assert_eq!(resp.count, 2);
        assert_eq!(resp.rows.len(), 2);
        assert!(resp.rows.iter().all(|row| row.key.starts_with("pref:")));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn export_defaults_to_legacy_v1_and_supports_typed_v2() {
        let db_path = temp_db_path("export_contract");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        {
            let mut guard = service.state.lock().await;
            write_typed(&mut guard.db, "alpha", TypedValue::Integer(7));
            write_typed(&mut guard.db, "beta", TypedValue::Text("hello".to_string()));
        }

        let Json(legacy) = service
            .export(Parameters(ExportRequest::default()))
            .await
            .expect("legacy export");
        assert_eq!(legacy.format, "legacy_v1");
        assert_eq!(legacy.format_version, "nucleusdb_export/legacy_v1");
        assert_eq!(legacy.key_count, 2);
        let legacy_json: serde_json::Value =
            serde_json::from_str(&legacy.json).expect("legacy json parse");
        assert!(legacy_json.is_object());
        assert_eq!(legacy_json["alpha"], json!(7u64));
        assert!(legacy_json["beta"].is_u64());

        let Json(typed) = service
            .export(Parameters(ExportRequest {
                format: Some("typed_v2".to_string()),
            }))
            .await
            .expect("typed export");
        assert_eq!(typed.format, "typed_v2");
        assert_eq!(typed.format_version, "nucleusdb_export/typed_v2");
        assert_eq!(typed.key_count, 2);
        let typed_json: serde_json::Value =
            serde_json::from_str(&typed.json).expect("typed json parse");
        let typed_entries = typed_json.as_array().expect("typed array");
        let beta = typed_entries
            .iter()
            .find(|row| row.get("key") == Some(&json!("beta")))
            .expect("beta entry");
        assert_eq!(beta["type"], json!("text"));
        assert_eq!(beta["value"], json!("hello"));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn export_rejects_invalid_format() {
        let db_path = temp_db_path("export_invalid_format");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let err = service
            .export(Parameters(ExportRequest {
                format: Some("yaml".to_string()),
            }))
            .await
            .err()
            .expect("invalid format must fail");
        let err_dbg = format!("{err:?}");
        assert!(err_dbg.contains("invalid export format"));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn export_typed_v2_fails_closed_on_decode_error() {
        let db_path = temp_db_path("export_typed_decode_error");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        {
            let mut guard = service.state.lock().await;
            write_typed(&mut guard.db, "bad:export", TypedValue::Integer(42));
            guard.db.type_map.set("bad:export", TypeTag::Json);
        }

        let err = service
            .export(Parameters(ExportRequest {
                format: Some("typed_v2".to_string()),
            }))
            .await
            .err()
            .expect("typed export must fail on decode mismatch");
        let err_dbg = format!("{err:?}");
        assert!(err_dbg.contains("typed decode failed for key 'bad:export'"));

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn swarm_publish_fetch_roundtrip() {
        let db_path = temp_db_path("swarm_publish_fetch");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(published) = service
            .swarm_publish(Parameters(SwarmPublishRequest {
                data: "hello swarm".to_string(),
                encoding: Some("utf8".to_string()),
                asset_type: Some(AssetType::Text),
                creator_did: Some("did:key:test".to_string()),
                chunk_size_bytes: Some(4),
            }))
            .await
            .expect("publish");
        assert!(published.chunk_count > 1);
        assert!(published.proof_attached);

        let Json(fetched) = service
            .swarm_fetch(Parameters(SwarmFetchRequest {
                manifest_id: published.manifest_id.clone(),
                encoding: Some("utf8".to_string()),
            }))
            .await
            .expect("fetch");
        assert_eq!(fetched.data, "hello swarm");
        assert!(fetched.verified);

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn swarm_remote_fetch_stub_reports_deferred_status() {
        let db_path = temp_db_path("swarm_remote_fetch_stub");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(response) = service
            .swarm_remote_fetch(Parameters(SwarmRemoteFetchRequest {
                manifest_id: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                    .to_string(),
                encoding: Some("base64".to_string()),
                timeout_secs: Some(30),
            }))
            .await
            .expect("remote fetch stub");
        assert!(!response.implemented);
        assert!(response.message.contains("not yet implemented"));
        assert_eq!(
            response.tracking_issue,
            "WIP/bitswap_remote_fetch_tracking_2026-03-08.md"
        );

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn swarm_status_reports_inventory_after_publish() {
        let db_path = temp_db_path("swarm_status");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let _ = service
            .swarm_publish(Parameters(SwarmPublishRequest {
                data: base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3, 4, 5, 6]),
                encoding: Some("base64".to_string()),
                asset_type: Some(AssetType::Binary),
                creator_did: Some("did:key:test".to_string()),
                chunk_size_bytes: Some(2),
            }))
            .await
            .expect("publish");

        let Json(status) = service.swarm_status().await.expect("status");
        assert_eq!(status.total_bytes, 6);
        assert_eq!(status.total_chunks, 3);
        assert_eq!(status.manifest_count, 1);
        assert!(status.bitswap_enabled);

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn swarm_manifest_proof_survives_service_reload() {
        let db_path = temp_db_path("swarm_manifest_reload");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(published) = service
            .swarm_publish(Parameters(SwarmPublishRequest {
                data: "proof survives restart".to_string(),
                encoding: Some("utf8".to_string()),
                asset_type: Some(AssetType::Text),
                creator_did: Some("did:key:test".to_string()),
                chunk_size_bytes: Some(5),
            }))
            .await
            .expect("publish");
        drop(service);

        let reloaded = NucleusDbMcpService::new(&db_path).expect("reloaded service");
        let manifest_id = published
            .manifest_id
            .parse::<ManifestId>()
            .expect("manifest id");
        let guard = reloaded.state.lock().await;
        let manifest = guard
            .swarm_store
            .get_manifest(&manifest_id)
            .expect("manifest");
        assert!(manifest.proof.is_some());
        drop(guard);

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn swarm_state_loads_grants_from_db_path() {
        let db_path = temp_db_path("swarm_grants");
        let grants_path = db_path.with_extension("pod_grants.json");
        let grants = vec![crate::pod::acl::AccessGrant {
            grant_id: crate::pod::acl::AccessGrant::compute_id(
                &[1u8; 32],
                &[2u8; 32],
                "swarm/chunk/*",
                123,
                1,
            ),
            grantor_puf: [1u8; 32],
            grantee_puf: [2u8; 32],
            key_pattern: "swarm/chunk/*".to_string(),
            permissions: crate::pod::acl::GrantPermissions::read_only(),
            expires_at: None,
            created_at: 123,
            nonce: 1,
            revoked: false,
        }];
        std::fs::write(
            &grants_path,
            serde_json::to_vec(&grants).expect("serialize grants"),
        )
        .expect("write grants");

        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(published) = service
            .swarm_publish(Parameters(SwarmPublishRequest {
                data: "granted chunk".to_string(),
                encoding: Some("utf8".to_string()),
                asset_type: Some(AssetType::Text),
                creator_did: Some("did:key:test".to_string()),
                chunk_size_bytes: Some(256 * 1024),
            }))
            .await
            .expect("publish");
        let guard = service.state.lock().await;
        let peer = libp2p::PeerId::random();
        let manifest_id = published
            .manifest_id
            .parse::<ManifestId>()
            .expect("manifest id");
        let chunk_id = guard
            .swarm_store
            .get_manifest(&manifest_id)
            .expect("manifest")
            .chunk_hashes[0]
            .clone();
        let response = guard.bitswap_runtime.clone().handle_request(
            &peer,
            crate::swarm::bitswap::BitswapMessage::Want(vec![chunk_id]),
        );
        assert_eq!(
            response,
            crate::swarm::bitswap::BitswapMessage::Have(Vec::new())
        );
        drop(guard);

        let _ = std::fs::remove_file(&grants_path);
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn swarm_state_respects_require_grants_env() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _require_grants = EnvVarRestore::set("HALO_BITSWAP_REQUIRE_GRANTS", "1");
        let db_path = temp_db_path("swarm_require_grants");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(published) = service
            .swarm_publish(Parameters(SwarmPublishRequest {
                data: "locked chunk".to_string(),
                encoding: Some("utf8".to_string()),
                asset_type: Some(AssetType::Text),
                creator_did: Some("did:key:test".to_string()),
                chunk_size_bytes: Some(256 * 1024),
            }))
            .await
            .expect("publish");
        let guard = service.state.lock().await;
        let peer = libp2p::PeerId::random();
        let manifest_id = published
            .manifest_id
            .parse::<ManifestId>()
            .expect("manifest id");
        let chunk_id = guard
            .swarm_store
            .get_manifest(&manifest_id)
            .expect("manifest")
            .chunk_hashes[0]
            .clone();
        let response = guard.bitswap_runtime.clone().handle_request(
            &peer,
            crate::swarm::bitswap::BitswapMessage::Want(vec![chunk_id]),
        );
        assert_eq!(
            response,
            crate::swarm::bitswap::BitswapMessage::Have(Vec::new())
        );
        drop(guard);

        cleanup_db_files(&db_path);
    }

    #[test]
    fn mesh_mode_parser_rejects_unknown_values() {
        let ok = parse_mesh_access_modes(&["read".to_string(), "WRITE".to_string()])
            .expect("valid mode parse");
        assert_eq!(ok.len(), 2);
        let err = parse_mesh_access_modes(&["bogus".to_string()]).expect_err("must reject");
        assert!(err.contains("unknown access mode"));
    }

    #[test]
    fn mesh_mode_parser_rejects_empty_list() {
        let err = parse_mesh_access_modes(&[]).expect_err("must reject empty");
        assert!(err.contains("at least one"));
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_launch_and_task_roundtrip_shell() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let db_path = temp_db_path("orchestrator_roundtrip");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(launch) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "unit-shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["memory_read".to_string(), "memory_write".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch shell");
        assert_eq!(launch.status, "idle");
        assert_eq!(launch.agent, "shell");

        let Json(task) = service
            .orchestrator_send_task(Parameters(OrchestratorSendTaskRequest {
                agent_id: launch.agent_id,
                task: "printf 'hello-orchestrator'".to_string(),
                format: None,
                timeout_secs: Some(30),
                wait: Some(true),
            }))
            .await
            .expect("run task");
        assert_eq!(task.status, "complete");
        assert!(task
            .result
            .as_deref()
            .unwrap_or_default()
            .contains("hello-orchestrator"));
        assert_eq!(task.output, task.result);

        cleanup_db_files(&db_path);
    }

    #[test]
    #[allow(clippy::await_holding_lock)]
    fn orchestrator_shell_trace_wait_roundtrip_multiple_tasks() {
        let join = std::thread::Builder::new()
            .name("orchestrator_shell_trace_wait_roundtrip_multiple_tasks".to_string())
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()
                    .expect("build tokio runtime");
                rt.block_on(async {
                    let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
                    let _proxy =
                        EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
                    let db_path = temp_db_path("orchestrator_shell_trace_wait");
                    let service = NucleusDbMcpService::new(&db_path).expect("service");

                    let Json(launch) = service
                        .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                            agent: "shell".to_string(),
                            agent_name: "trace-shell".to_string(),
                            working_dir: None,
                            env: BTreeMap::new(),
                            timeout_secs: Some(30),
                            model: None,
                            trace: Some(true),
                            capabilities: vec!["memory_read".to_string()],
                            dispatch_mode: None,
                            container_hookup: None,
                            admission_mode: None,
                        }))
                        .await
                        .expect("launch shell");
                    assert_eq!(launch.status, "idle");

                    let tasks = [
                        ("true", ""),
                        ("printf 'trace-shell-ok'", "trace-shell-ok"),
                        ("echo trace-shell-done", "trace-shell-done"),
                    ];

                    for (command, expected) in tasks {
                        let Json(task) = service
                            .orchestrator_send_task(Parameters(OrchestratorSendTaskRequest {
                                agent_id: launch.agent_id.clone(),
                                task: command.to_string(),
                                format: None,
                                timeout_secs: Some(20),
                                wait: Some(true),
                            }))
                            .await
                            .expect("run traced shell task");
                        assert_eq!(task.status, "complete", "command `{command}`");
                        if !expected.is_empty() {
                            assert!(
                                task.result
                                    .as_deref()
                                    .unwrap_or_default()
                                    .contains(expected),
                                "command `{command}` output mismatch"
                            );
                        }
                        assert!(task.trace_session_id.is_some(), "command `{command}`");
                        assert_eq!(task.output, task.result);
                    }

                    cleanup_db_files(&db_path);
                });
            })
            .expect("spawn stack-sized test thread");
        join.join().expect("join stack-sized test thread");
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_launch_rejects_unknown_capability() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let db_path = temp_db_path("orchestrator_invalid_cap");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let result = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "unit-shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["bogus_capability".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await;
        match result {
            Ok(_) => panic!("invalid capability should fail"),
            Err(err) => {
                let dbg = format!("{err:?}");
                assert!(dbg.contains("unknown capability"));
            }
        }
        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_pipe_triggers_followup_task() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let db_path = temp_db_path("orchestrator_pipe");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(src_agent) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "src".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch source");
        let Json(dst_agent) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "dst".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(30),
                model: None,
                trace: Some(false),
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch target");

        let Json(source_task) = service
            .orchestrator_send_task(Parameters(OrchestratorSendTaskRequest {
                agent_id: src_agent.agent_id,
                task: "printf '{\"result\":\"hello\"}'".to_string(),
                format: None,
                timeout_secs: Some(30),
                wait: Some(true),
            }))
            .await
            .expect("source task");
        assert_eq!(source_task.status, "complete");

        let Json(pipe_resp) = service
            .orchestrator_pipe(Parameters(OrchestratorPipeRequest {
                source_task_id: source_task.task_id,
                target_agent_id: dst_agent.agent_id,
                transform: Some("json_extract:.result".to_string()),
                task_prefix: Some("echo ".to_string()),
            }))
            .await
            .expect("pipe");
        assert!(matches!(
            pipe_resp.status.as_str(),
            "complete" | "running" | "linked"
        ));
        assert!(pipe_resp.task_id.is_some());

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_timeout_marks_task_timeout() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let db_path = temp_db_path("orchestrator_timeout");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(launch) = service
            .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
                agent: "shell".to_string(),
                agent_name: "slow-shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: Some(1),
                model: None,
                trace: Some(false),
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
                admission_mode: None,
            }))
            .await
            .expect("launch");

        let result = service
            .orchestrator_send_task(Parameters(OrchestratorSendTaskRequest {
                agent_id: launch.agent_id,
                task: "sleep 2; echo done".to_string(),
                format: None,
                timeout_secs: Some(1),
                wait: Some(true),
            }))
            .await;
        match result {
            Ok(Json(task)) => assert_eq!(task.status, "timeout"),
            Err(err) => {
                let dbg = format!("{err:?}");
                assert!(dbg.contains("timeout"));
            }
        }

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_error_path_does_not_leak_secrets() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let db_path = temp_db_path("orchestrator_sanitize");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let result = service
            .orchestrator_send_task(Parameters(OrchestratorSendTaskRequest {
                agent_id: "missing-agent".to_string(),
                task: "echo should-fail".to_string(),
                format: None,
                timeout_secs: Some(5),
                wait: Some(true),
            }))
            .await;
        match result {
            Ok(_) => panic!("send task should fail"),
            Err(err) => {
                let dbg = format!("{err:?}");
                assert!(!dbg.contains("AGENTHALO_MCP_SECRET"));
                assert!(dbg.contains("unknown agent_id"));
            }
        }

        cleanup_db_files(&db_path);
    }

    #[test]
    fn task_to_response_preserves_trace_session_id() {
        let task = crate::orchestrator::task::Task {
            task_id: "task-123".to_string(),
            agent_id: "orch-abc".to_string(),
            prompt: "noop".to_string(),
            status: crate::orchestrator::task::TaskStatus::Complete,
            answer: Some("ok".to_string()),
            result: Some("ok".to_string()),
            error: None,
            exit_code: Some(0),
            usage: crate::orchestrator::task::TaskUsage {
                input_tokens: 12,
                output_tokens: 34,
                estimated_cost_usd: 0.0001,
            },
            started_at: Some(1),
            completed_at: Some(2),
            trace_session_id: Some("orch-trace-task-123".to_string()),
        };
        let response = task_to_response(task);
        assert_eq!(response.status, "complete");
        assert_eq!(response.output.as_deref(), Some("ok"));
        assert_eq!(
            response.trace_session_id.as_deref(),
            Some("orch-trace-task-123")
        );
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_graph_response_reports_shape_and_counts() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let db_path = temp_db_path("orchestrator_graph_shape");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(graph) = service.orchestrator_graph().await.expect("graph snapshot");
        assert_eq!(graph.nodes_shape, "object_map");
        assert!(graph.graph["nodes"].is_object());
        assert!(graph.graph["edges"].is_array());
        assert_eq!(
            graph.node_count,
            graph
                .graph
                .get("nodes")
                .and_then(|v| v.as_object())
                .map(|m| m.len())
                .unwrap_or(0)
        );
        assert_eq!(
            graph.edge_count,
            graph
                .graph
                .get("edges")
                .and_then(|v| v.as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        );

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn orchestrator_mesh_status_returns_disabled_when_not_configured() {
        let _env_guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _proxy = EnvVarRestore::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", "0");
        let _mesh_id = EnvVarRestore::unset("NUCLEUSDB_MESH_AGENT_ID");
        let _mesh_registry = EnvVarRestore::unset("NUCLEUSDB_MESH_REGISTRY");

        let db_path = temp_db_path("orchestrator_mesh_status_disabled");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(status) = service
            .orchestrator_mesh_status(Parameters(OrchestratorMeshStatusRequest::default()))
            .await
            .expect("mesh status");
        assert!(!status.enabled);
        assert!(status.peers.is_empty());

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn evidence_combine_matches_lean_witness() {
        let db_path = temp_db_path("evidence_combine");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(response) = service
            .evidence_combine(Parameters(EvidenceCombineRequest {
                prior_probability_true: None,
                prior_odds_false_over_true: Some(2.0),
                evidence: vec![crate::halo::evidence::ToolEvidence {
                    tool_name: "witness".to_string(),
                    result: serde_json::json!(null),
                    prior_reliability: 1.0,
                    likelihood_given_true: 2.0,
                    likelihood_given_false: 1.0,
                    confidence_value: None,
                    confidence_kind: None,
                }],
                output_kind: Some(crate::halo::uncertainty::UncertaintyKind::Probability),
            }))
            .await
            .expect("combine");

        assert!((response.posterior_odds_false_over_true - 1.0).abs() < 1e-10);
        assert!((response.posterior_probability_true - 0.5).abs() < 1e-10);
        assert_eq!(response.steps.len(), 1);

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn evidence_combine_asymmetric_case_prefers_true_hypothesis() {
        let db_path = temp_db_path("evidence_combine_asymmetric");
        let service = NucleusDbMcpService::new(&db_path).expect("service");
        let Json(response) = service
            .evidence_combine(Parameters(EvidenceCombineRequest {
                prior_probability_true: None,
                prior_odds_false_over_true: Some(1.0),
                evidence: vec![crate::halo::evidence::ToolEvidence {
                    tool_name: "likelihood".to_string(),
                    result: serde_json::json!(null),
                    prior_reliability: 1.0,
                    likelihood_given_true: 0.9,
                    likelihood_given_false: 0.1,
                    confidence_value: None,
                    confidence_kind: None,
                }],
                output_kind: Some(crate::halo::uncertainty::UncertaintyKind::Probability),
            }))
            .await
            .expect("combine");

        assert!((response.posterior_odds_false_over_true - (1.0 / 9.0)).abs() < 1e-10);
        assert!((response.posterior_probability_true - 0.9).abs() < 1e-10);

        cleanup_db_files(&db_path);
    }

    #[tokio::test]
    async fn uncertainty_translate_roundtrip_probability_cf() {
        let db_path = temp_db_path("uncertainty_translate");
        let service = NucleusDbMcpService::new(&db_path).expect("service");

        let Json(to_cf) = service
            .uncertainty_translate(Parameters(UncertaintyTranslateRequest {
                from: crate::halo::uncertainty::UncertaintyKind::Probability,
                to: crate::halo::uncertainty::UncertaintyKind::CertaintyFactor,
                value: 0.75,
            }))
            .await
            .expect("to cf");

        let Json(back) = service
            .uncertainty_translate(Parameters(UncertaintyTranslateRequest {
                from: crate::halo::uncertainty::UncertaintyKind::CertaintyFactor,
                to: crate::halo::uncertainty::UncertaintyKind::Probability,
                value: to_cf.value,
            }))
            .await
            .expect("to probability");

        assert!((back.value - 0.75).abs() < 1e-10);

        cleanup_db_files(&db_path);
    }
}
