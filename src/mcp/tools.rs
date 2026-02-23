use crate::cli::{default_witness_cfg, parse_backend};
use crate::container::launcher::{launch_container, RunConfig};
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
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, Json, ServerHandler,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
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
pub struct HistoryRequest {
    /// Optional max number of entries (newest first).
    pub limit: Option<usize>,
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
                    "Verifiable immutable database tools over MCP (stdio transport).".to_string(),
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
        let out = Command::new("cast")
            .args(args)
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
            .map_err(|e| McpError::invalid_params(e, None))?;
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
        description = "Export current key/value state as pretty JSON payload."
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

    #[tool(
        name = "nucleusdb_open_channel",
        description = "Open a PCN channel as append-only records. Example: {\"p1\":\"0xabc\",\"p2\":\"0xdef\",\"capacity\":1000}"
    )]
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

    #[tool(
        name = "nucleusdb_update_channel",
        description = "Append a channel balance update with conservation enforcement. Example: {\"p1\":\"0xabc\",\"p2\":\"0xdef\",\"balance1\":700,\"balance2\":300}"
    )]
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

    #[tool(
        name = "nucleusdb_close_channel",
        description = "Append a channel close operation. Example: {\"p1\":\"0xabc\",\"p2\":\"0xdef\"}"
    )]
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

    #[tool(
        name = "nucleusdb_query_channel",
        description = "Return current channel record plus append-only op history. Example: {\"p1\":\"0xabc\",\"p2\":\"0xdef\"}"
    )]
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
        description = "Launch a monitored container session. Example: {\"image\":\"nucleusdb-agent:latest\",\"agent_id\":\"agent-a\",\"command\":[\"/bin/sh\",\"-lc\",\"echo hello\"]}"
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
        let info = launch_container(RunConfig {
            image: req.image.clone(),
            agent_id: req.agent_id.clone(),
            command,
            use_gvisor: req.runtime_runsc.unwrap_or(false),
            host_sock: None,
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

    #[tool(
        name = "nucleusdb_agent_reattest",
        description = "Evaluate attestation freshness and optionally submit re-attestation. Example: {\"contract_address\":\"0x...\",\"rpc_url\":\"https://...\",\"agent_address\":\"0x...\",\"stale_after_secs\":3600,\"submit_onchain\":false}"
    )]
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

        let metadata_hash =
            Self::normalize_metadata_hash(req.chain_id, req.verifier_address.trim(), req.metadata_hash.as_deref());
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

    #[tool(
        name = "generate_composite_cab",
        description = "Generate a composite CAB payload for multi-chain attestation. Example: {\"chain_ids\":[8453,1,42161]}"
    )]
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
        let placeholder = generator.build_placeholder();
        let composite_cab_hash = placeholder.composite_cab_hash_hex();
        Ok(Json(GenerateCompositeCabResponse {
            ok: true,
            chain_ids: placeholder.chain_ids.clone(),
            replay_seq: placeholder.replay_seq,
            composite_cab_hash,
            proof_hex: placeholder.proof_hex,
            public_signals: placeholder.public_signals,
            note: "placeholder payload generated; wire proof backend to replace proof_hex".to_string(),
        }))
    }

    #[tool(
        name = "submit_composite_attestation",
        description = "Submit composite CAB proof to TrustVerifierMultiChain. Example: {\"contract_address\":\"0x...\",\"rpc_url\":\"https://...\",\"proof_hex\":\"0x...\",\"chain_ids\":[8453,1]}"
    )]
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

    #[tool(
        name = "query_cross_chain_attestation",
        description = "Prepare (or submit) a cross-chain attestation query payload using ICrossChainAttestationQuery."
    )]
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

    #[tool(
        name = "list_registered_chains",
        description = "List all registered chain IDs from TrustVerifierMultiChain."
    )]
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
                "Trust tools: nucleusdb_agent_register, nucleusdb_verify_agent, nucleusdb_agent_reattest."
                    .to_string(),
                "Multichain tools: register_chain, generate_composite_cab, submit_composite_attestation, verify_agent_multichain, query_cross_chain_attestation, list_registered_chains."
                    .to_string(),
                "On-chain submit paths accept keystore_path (+ optional keystore_password_file) or private_key_env."
                    .to_string(),
            ],
        }))
    }
}
