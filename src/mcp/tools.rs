use crate::cli::{default_witness_cfg, parse_backend};
use crate::container::launcher::{
    container_logs as launcher_container_logs, container_status as launcher_container_status,
    launch_container, list_sessions as list_container_sessions,
    stop_container as launcher_stop_container, MeshConfig, RunConfig,
};
use crate::pcn::{channel_snapshot, ChannelSnapshot, SettlementOp};
use crate::persistence::{init_wal, load_wal, persist_snapshot_and_sync_wal, truncate_wal};
use crate::protocol::{NucleusDb, QueryProof, VcBackend};
use crate::sql::executor::{SqlExecutor, SqlResult};
use crate::state::State;
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
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug)]
struct ServiceState {
    db: NucleusDb,
    db_path: PathBuf,
    wal_path: PathBuf,
    work_record_store: WorkRecordStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    LegacyV1,
    TypedV2,
}

#[derive(Clone, Debug)]
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
    /// If true, request gVisor (`runsc`) runtime.
    pub runtime_runsc: Option<bool>,
    /// Optional host socket path override for sidecar communication.
    pub host_sock: Option<String>,
    /// Optional environment variables injected into the container.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Optional mesh settings for inter-container communication.
    #[serde(default)]
    pub mesh: Option<ContainerMeshRequest>,
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
                website_url: Some("https://github.com/Abraxas1010/nucleusdb".to_string()),
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
        crate::persistence::default_wal_path(db_path)
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
            work_record_store: WorkRecordStore::new(),
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
            work_record_store: WorkRecordStore::new(),
        })
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
        description = "Launch a monitored container session. Supports mesh networking and env injection. Example: {\"image\":\"nucleusdb-agent:latest\",\"agent_id\":\"agent-a\",\"command\":[\"/bin/sh\",\"-lc\",\"echo hello\"],\"env\":{\"AGENTHALO_MCP_SECRET\":\"...\"},\"mesh\":{\"enabled\":true,\"mcp_port\":8420,\"registry_volume\":\"/tmp/nucleusdb-mesh\"}}"
    )]
    pub async fn container_launch(
        &self,
        Parameters(req): Parameters<ContainerLaunchRequest>,
    ) -> Result<Json<ContainerLaunchResponse>, McpError> {
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
            let mut mesh_cfg = MeshConfig::default();
            mesh_cfg.enabled = cfg.enabled.unwrap_or(true);
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

    #[tool(
        name = "nucleusdb_container_list",
        description = "List tracked container sessions launched by NucleusDB tooling."
    )]
    pub async fn container_list(&self) -> Result<Json<ContainerListResponse>, McpError> {
        let sessions = list_container_sessions().map_err(|e| McpError::internal_error(e, None))?;
        let views = sessions
            .into_iter()
            .map(|session| ContainerSessionView {
                session_id: session.session_id,
                container_id: session.container_id,
                image: session.image,
                agent_id: session.agent_id,
                host_sock: session.host_sock.display().to_string(),
                started_at_unix: session.started_at_unix,
                mesh_port: session.mesh_port,
            })
            .collect::<Vec<_>>();
        Ok(Json(ContainerListResponse {
            count: views.len(),
            sessions: views,
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
        let auth_token = std::env::var("NUCLEUSDB_MESH_AUTH_TOKEN").ok();
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
        let auth_token = std::env::var("NUCLEUSDB_MESH_AUTH_TOKEN").ok();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;
    use crate::typed_value::{TypeTag, TypedValue};
    use serde_json::json;
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

    #[test]
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
            "runtime_runsc": true,
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
}
