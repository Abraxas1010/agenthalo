use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use nucleusdb::halo::addons;
use nucleusdb::halo::agentpmt;
use nucleusdb::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use nucleusdb::halo::audit::{
    audit_contract_file, audit_contract_source, save_audit_result, AuditRequest, AuditSize,
};
use nucleusdb::halo::circuit::{
    load_or_setup_attestation_keys_with_policy, proof_words_json_array, prove_attestation,
    public_inputs_json_array, verify_attestation_proof,
};
use nucleusdb::halo::config;
use nucleusdb::halo::onchain::{load_onchain_config_or_default, post_attestation};
use nucleusdb::halo::pq::{has_wallet, sign_pq_payload};
use nucleusdb::halo::trace::{
    list_sessions, now_unix_secs, record_paid_operation_for_halo, session_summary,
};
use nucleusdb::halo::trust::query_trust_score;
use nucleusdb::halo::util::digest_json;
use nucleusdb::halo::viewer;
use nucleusdb::halo::x402;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
struct AppState {
    secret: String,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    result: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcErrorEnvelope {
    jsonrpc: &'static str,
    id: Value,
    error: JsonRpcError,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let port: u16 = std::env::var("AGENTHALO_MCP_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8390);
    let host = std::env::var("AGENTHALO_MCP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let secret = std::env::var("AGENTHALO_MCP_SECRET").unwrap_or_else(|_| {
        eprintln!(
            "warning: AGENTHALO_MCP_SECRET not set; using dev default secret (set this in non-local environments)"
        );
        "agenthalo-dev-secret".to_string()
    });

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("invalid bind address {host}:{port}: {e}"))?;

    let state = Arc::new(AppState { secret });

    let app = Router::new()
        .route("/health", get(health))
        .route("/mcp", post(mcp))
        .with_state(state);

    println!("agenthalo-mcp-server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind listener {addr}: {e}"))?;
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve axum: {e}"))?;
    Ok(())
}

async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "agenthalo-mcp-server",
        "phase": "6-agentpmt-fixed"
    }))
}

async fn mcp(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, (StatusCode, Json<JsonRpcErrorEnvelope>)> {
    if !authorized(&headers, &state.secret) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(error_envelope(
                req.id.clone(),
                -32001,
                "unauthorized: expected Authorization: Bearer <AGENTHALO_MCP_SECRET>",
            )),
        ));
    }

    if req.jsonrpc.as_deref() != Some("2.0") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(error_envelope(
                req.id.clone(),
                -32600,
                "invalid request: jsonrpc must be \"2.0\"",
            )),
        ));
    }

    let id = req.id.unwrap_or(json!(null));
    let result = match req.method.as_str() {
        "initialize" => json!({
            "protocolVersion": "2025-03-26",
            "serverInfo": {"name": "agenthalo-mcp-server", "version": "0.3.0"},
            "capabilities": {"tools": {}}
        }),
        "tools/list" => {
            let mut tools = vec![
                json!({
                    "name": "attest",
                    "description": "Create a tamper-evident attestation of an agent session. Supports local Merkle proofs and on-chain Groth16 verification.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": {"type": "string", "description": "Session ID to attest. If omitted, attests the most recent session."},
                            "anonymous": {"type": "boolean", "description": "If true, strip identifying metadata from the attestation.", "default": false},
                            "onchain": {"type": "boolean", "description": "If true, generate Groth16 proof and post to smart contract.", "default": false},
                            "dry_run": {"type": "boolean", "description": "If true with onchain, generate proof without submitting transaction.", "default": false}
                        }
                    }
                }),
                json!({
                    "name": "sign_pq",
                    "description": "Create a post-quantum detached signature (ML-DSA / Dilithium) over a message or file.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "message": {"type": "string", "description": "Text message to sign. Provide either message or file_path, not both."},
                            "file_path": {"type": "string", "description": "Path to file to sign. Provide either message or file_path, not both."}
                        }
                    }
                }),
                json!({
                    "name": "audit_contract",
                    "description": "Run static analysis on Solidity source code. Returns findings categorized by severity.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "contract_source": {"type": "string", "description": "Inline Solidity source code to audit."},
                            "contract_path": {"type": "string", "description": "Path to a .sol file to audit. Used as label if contract_source is also provided."},
                            "size": {"type": "string", "enum": ["small", "medium", "large"], "description": "Audit depth tier.", "default": "small"}
                        }
                    }
                }),
                json!({
                    "name": "trust_query",
                    "description": "Query the computed trust score for a session, based on attestation, proof integrity, and behavioral signals.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": {"type": "string", "description": "Session ID to score. If omitted, scores the most recent session."}
                        }
                    }
                }),
                json!({
                    "name": "vote",
                    "description": "Record a governance vote intent locally. On-chain submission is not yet implemented.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "proposal_id": {"type": "string", "description": "Proposal identifier to vote on."},
                            "choice": {"type": "string", "enum": ["yes", "no", "abstain"], "description": "Vote choice."},
                            "reason": {"type": "string", "description": "Optional justification for the vote."}
                        },
                        "required": ["proposal_id", "choice"]
                    }
                }),
                json!({
                    "name": "sync",
                    "description": "Record a cloud sync intent locally. Sync transport is not yet implemented.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "target": {"type": "string", "description": "Sync target (e.g. 'cloudflare').", "default": "cloudflare"}
                        }
                    }
                }),
                json!({
                    "name": "privacy_pool_create",
                    "description": "Record a privacy pool creation intent. Requires agentpmt-workflows add-on. On-chain deployment is not yet implemented.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "chain": {"type": "string", "description": "Target chain.", "default": "base-sepolia"},
                            "asset": {"type": "string", "description": "Token asset.", "default": "USDC"},
                            "denomination": {"type": "integer", "description": "Pool denomination in token base units."}
                        },
                        "required": ["denomination"]
                    }
                }),
                json!({
                    "name": "privacy_pool_withdraw",
                    "description": "Record a privacy pool withdrawal intent. Requires agentpmt-workflows add-on. On-chain execution is not yet implemented.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pool_id": {"type": "string", "description": "Pool identifier to withdraw from."},
                            "recipient": {"type": "string", "description": "Recipient address."},
                            "amount": {"type": "integer", "description": "Amount to withdraw in token base units.", "default": 1}
                        },
                        "required": ["pool_id", "recipient"]
                    }
                }),
                json!({
                    "name": "pq_bridge_transfer",
                    "description": "Record a PQ bridge cross-chain transfer intent. Requires p2pclaw add-on. Execution is not yet implemented.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "from_chain": {"type": "string", "description": "Source chain identifier."},
                            "to_chain": {"type": "string", "description": "Destination chain identifier."},
                            "asset": {"type": "string", "description": "Token asset to transfer."},
                            "amount": {"type": "integer", "description": "Amount in token base units."},
                            "recipient": {"type": "string", "description": "Recipient address on destination chain."}
                        },
                        "required": ["from_chain", "to_chain", "asset", "amount", "recipient"]
                    }
                }),
                json!({
                    "name": "x402_check",
                    "description": "Parse and validate an x402direct payment request (HTTP 402 response body). Returns structured validation with chain/token verification and warnings.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "body": {"type": "string", "description": "The JSON body from an HTTP 402 response containing x402direct payment options."}
                        },
                        "required": ["body"]
                    }
                }),
                json!({
                    "name": "x402_pay",
                    "description": "Execute an x402direct USDC payment on Base. Validates the request, checks wallet balance, transfers on-chain, and returns a payment proof.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "body": {"type": "string", "description": "The JSON body from an HTTP 402 response containing x402direct payment options."},
                            "payment_option_id": {"type": "string", "description": "Specific payment option ID to use. If omitted, auto-selects the first option on a known network with known USDC."}
                        },
                        "required": ["body"]
                    }
                }),
                json!({
                    "name": "x402_balance",
                    "description": "Check USDC wallet balance for x402 payments. Returns wallet address and balance on the configured network.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "halo_traces",
                    "description": "List recorded agent sessions or get full detail for a specific session by ID. Supports filtering by agent type and model.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": {"type": "string", "description": "If provided, returns full session detail including events. Otherwise returns a list of session summaries."},
                            "limit": {"type": "integer", "description": "Maximum number of sessions to return when listing.", "default": 20},
                            "agent": {"type": "string", "description": "Filter sessions by agent type (case-insensitive substring match, e.g. 'claude', 'codex')."},
                            "model": {"type": "string", "description": "Filter sessions by model name (case-insensitive substring match, e.g. 'opus', 'gpt-5')."}
                        }
                    }
                }),
                json!({
                    "name": "halo_costs",
                    "description": "Show agent cost summary bucketed by time period. Includes token usage, session counts, and estimated USD cost.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "monthly": {"type": "boolean", "description": "If true, bucket by month instead of day.", "default": false},
                            "include_paid": {"type": "boolean", "description": "If true, include x402 and other paid operations in the summary.", "default": false}
                        }
                    }
                }),
                json!({
                    "name": "halo_status",
                    "description": "Show AgentHALO system status: session count, total cost, latest session, authentication state, and tool proxy configuration.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "halo_export",
                    "description": "Export a complete session as standalone JSON with metadata, all events, and summary statistics.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "session_id": {"type": "string", "description": "The session ID to export."}
                        },
                        "required": ["session_id"]
                    }
                }),
                json!({
                    "name": "x402_summary",
                    "description": "Unified x402 spending dashboard: budget, total spent, remaining balance, and breakdown by operation type.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "halo_capabilities",
                    "description": "Discover current AgentHALO capabilities: which features are enabled, what add-ons are available, and configuration status for x402, PQ wallet, tool proxy, and attestation.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
            ];
            // Merge AgentPMT proxied tools when tool proxy is enabled.
            let proxied = agentpmt::proxied_tools_for_listing();
            tools.extend(proxied);
            json!({"tools": tools})
        }
        "tools/call" => {
            let name = req
                .params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let arguments = req
                .params
                .as_ref()
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            tool_call_response(name, arguments)
        }
        "notifications/initialized" => json!({}),
        other => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(error_envelope(
                    Some(id),
                    -32601,
                    &format!("method not found: {other}"),
                )),
            ));
        }
    };

    Ok(Json(JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result,
    }))
}

fn tool_call_response(name: &str, arguments: Value) -> Value {
    match tool_call(name, arguments) {
        Ok(payload) => json!({
            "content": [
                {
                    "type": "text",
                    "text": payload.to_string()
                }
            ],
            "isError": false
        }),
        Err(err) => json!({
            "content": [
                {
                    "type": "text",
                    "text": json!({"status":"error","message":err}).to_string()
                }
            ],
            "isError": true
        }),
    }
}

fn tool_call(name: &str, arguments: Value) -> Result<Value, String> {
    // Check if this is an AgentPMT proxied tool (agentpmt/* prefix).
    if let Some(pmt_tool) = agentpmt::is_proxied_tool(name) {
        return tool_agentpmt_proxy(&pmt_tool, arguments);
    }
    match name {
        "attest" => tool_attest(arguments),
        "sign_pq" => tool_sign_pq(arguments),
        "audit_contract" => tool_audit_contract(arguments),
        "trust_query" => tool_trust_query(arguments),
        "vote" => tool_vote(arguments),
        "sync" => tool_sync(arguments),
        "privacy_pool_create" => tool_privacy_pool_create(arguments),
        "privacy_pool_withdraw" => tool_privacy_pool_withdraw(arguments),
        "pq_bridge_transfer" => tool_pq_bridge_transfer(arguments),
        "x402_check" => tool_x402_check(arguments),
        "x402_pay" => tool_x402_pay(arguments),
        "x402_balance" => tool_x402_balance(arguments),
        "x402_summary" => tool_x402_summary(arguments),
        "halo_traces" => tool_halo_traces(arguments),
        "halo_costs" => tool_halo_costs(arguments),
        "halo_status" => tool_halo_status(arguments),
        "halo_export" => tool_halo_export(arguments),
        "halo_capabilities" => tool_halo_capabilities(arguments),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Proxy a tool call to AgentPMT and record it in the trace.
///
/// AgentPMT tool proxy forwarding is not yet implemented. This
/// returns an honest error so agents know the call was NOT executed.
fn tool_agentpmt_proxy(tool_name: &str, _arguments: Value) -> Result<Value, String> {
    if !agentpmt::is_tool_proxy_enabled() {
        return Err(
            "AgentPMT tool proxy is not enabled. Enable it via the halo_capabilities tool or CLI."
                .to_string(),
        );
    }
    let catalog = agentpmt::load_tool_catalog();
    if !catalog.has_tool(tool_name) {
        return Err(format!(
            "unknown AgentPMT tool: {tool_name}. Available tools are listed in the tool catalog."
        ));
    }

    // Record the attempted call for observability.
    let _ = record_paid_operation_for_halo(
        &format!("agentpmt/{tool_name}"),
        0,
        None,
        None,
        false,
        Some("proxy_not_implemented".to_string()),
    );

    Err(format!(
        "AgentPMT tool proxy is not yet implemented: '{}' was recognized but the call was NOT \
         executed. Direct AgentPMT MCP forwarding will be available in a future release.",
        tool_name
    ))
}

fn tool_attest(arguments: Value) -> Result<Value, String> {
    let anonymous = arguments
        .get("anonymous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let onchain = arguments
        .get("onchain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let requested_session_id = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let op = match (onchain, anonymous) {
        (true, true) => "attest_onchain_anon",
        (true, false) => "attest_onchain",
        (false, true) => "attest_anon",
        (false, false) => "attest",
    };

    let db_path = config::db_path();
    let session_id = resolve_session_id(&db_path, requested_session_id.as_deref())?;
    let attestation = attest_session(
        &db_path,
        AttestationRequest {
            session_id: session_id.clone(),
            anonymous,
        },
    );
    match attestation {
        Ok(mut result) => {
            let mut onchain_payload = None;
            if onchain {
                let cfg = load_onchain_config_or_default();
                let (pk, vk, key_info) =
                    load_or_setup_attestation_keys_with_policy(None, cfg.circuit_policy.clone())?;
                let proof_bundle = prove_attestation(&pk, &result)?;
                if !verify_attestation_proof(&vk, &proof_bundle)? {
                    return Err("local Groth16 verification failed".to_string());
                }
                result.proof_type = "groth16-bn254".to_string();
                result.groth16_proof = Some(proof_words_json_array(&proof_bundle));
                result.groth16_public_inputs = Some(public_inputs_json_array(&proof_bundle));
                if dry_run {
                    onchain_payload = Some(serde_json::json!({
                        "dry_run": true,
                        "policy": key_info.policy.as_str(),
                        "metadata_path": key_info.metadata_path,
                        "proof_schema_version": proof_bundle.public_input_schema_version,
                        "proof_word_count": proof_bundle.proof_words.len(),
                        "public_input_count": proof_bundle.public_inputs.len()
                    }));
                } else {
                    let posted = post_attestation(&cfg, &proof_bundle, anonymous)?;
                    result.tx_hash = Some(posted.tx_hash.clone());
                    result.contract_address = Some(posted.contract_address.clone());
                    result.block_number = posted.block_number;
                    result.chain = Some(posted.chain.clone());
                    onchain_payload = Some(
                        serde_json::to_value(posted)
                            .map_err(|e| format!("serialize onchain payload: {e}"))?,
                    );
                }
            }
            let saved_path = save_attestation(&session_id, &result)?;
            record_paid_operation_for_halo(
                op,
                0,
                Some(session_id),
                Some(result.attestation_digest.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "attestation_path": saved_path.display().to_string(),
                "attestation": result,
                "onchain": onchain_payload
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(
                op,
                0,
                Some(session_id),
                None,
                false,
                Some(e.clone()),
            );
            Err(format!("attestation failed: {e}"))
        }
    }
}

fn tool_audit_contract(arguments: Value) -> Result<Value, String> {
    let size_name = arguments
        .get("size")
        .and_then(|v| v.as_str())
        .unwrap_or("small");
    let size = AuditSize::parse(size_name)?;
    let op = match size {
        AuditSize::Small => "audit_small",
        AuditSize::Medium => "audit_medium",
        AuditSize::Large => "audit_large",
    };

    let result = if let Some(source) = arguments.get("contract_source").and_then(|v| v.as_str()) {
        let contract_path = arguments
            .get("contract_path")
            .and_then(|v| v.as_str())
            .unwrap_or("<inline>");
        let request = AuditRequest {
            contract_path: contract_path.to_string(),
            size,
        };
        audit_contract_source(source, request)
    } else if let Some(path) = arguments.get("contract_path").and_then(|v| v.as_str()) {
        audit_contract_file(Path::new(path), size)
    } else {
        return Err("audit_contract requires contract_source or contract_path".to_string());
    };

    match result {
        Ok(result) => {
            let saved_path = save_audit_result(&result)?;
            record_paid_operation_for_halo(
                op,
                0,
                None,
                Some(result.contract_hash.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "audit_path": saved_path.display().to_string(),
                "audit": result
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, 0, None, None, false, Some(e.clone()));
            Err(format!("audit failed: {e}"))
        }
    }
}

fn tool_sign_pq(arguments: Value) -> Result<Value, String> {
    if !has_wallet() {
        return Err("no PQ wallet found. Generate one via the CLI: agenthalo keygen --pq (this requires terminal access).".to_string());
    }
    let message_arg = arguments
        .get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.as_bytes().to_vec());
    let file_arg = arguments
        .get("file_path")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let (payload, payload_kind, payload_hint) = match (message_arg, file_arg) {
        (Some(_), Some(_)) => return Err("provide only one of message or file_path".to_string()),
        (Some(bytes), None) => (bytes, "message".to_string(), Some("inline".to_string())),
        (None, Some(path)) => {
            let bytes =
                std::fs::read(&path).map_err(|e| format!("read signature payload file: {e}"))?;
            (bytes, "file".to_string(), Some(path))
        }
        (None, None) => return Err("sign_pq requires message or file_path".to_string()),
    };

    let op = "sign_pq";
    match sign_pq_payload(&payload, &payload_kind, payload_hint) {
        Ok((envelope, save_path)) => {
            record_paid_operation_for_halo(
                op,
                0,
                None,
                Some(envelope.signature_digest.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "signature_path": save_path.display().to_string(),
                "signature": envelope
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, 0, None, None, false, Some(e.clone()));
            Err(format!("signing failed: {e}"))
        }
    }
}

fn tool_trust_query(arguments: Value) -> Result<Value, String> {
    let op = "trust_query";
    let requested_session = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let db_path = config::db_path();
    match query_trust_score(&db_path, requested_session.as_deref()) {
        Ok(score) => {
            record_paid_operation_for_halo(
                op,
                0,
                requested_session,
                Some(score.digest.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "score": score
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(
                op,
                0,
                requested_session,
                None,
                false,
                Some(e.clone()),
            );
            Err(format!("trust query failed: {e}"))
        }
    }
}

fn tool_vote(arguments: Value) -> Result<Value, String> {
    let proposal_id = arguments
        .get("proposal_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "vote requires proposal_id".to_string())?
        .to_string();
    let choice = arguments
        .get("choice")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "vote requires choice".to_string())?
        .to_string();
    if !matches!(choice.as_str(), "yes" | "no" | "abstain") {
        return Err("choice must be yes, no, or abstain".to_string());
    }
    let reason = arguments
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let op = "vote";
    let vote = json!({
        "vote_id": uuid::Uuid::new_v4().to_string(),
        "proposal_id": proposal_id,
        "choice": choice,
        "reason": reason,
        "timestamp": now_unix_secs()
    });
    let digest = digest_json("agenthalo.vote.v1", &vote)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "recorded_locally",
        "note": "Vote intent recorded locally with digest. On-chain governance submission is not yet implemented.",
        "result_digest": digest,
        "vote": vote
    }))
}

fn tool_sync(arguments: Value) -> Result<Value, String> {
    let target = arguments
        .get("target")
        .and_then(|v| v.as_str())
        .unwrap_or("cloudflare")
        .to_string();
    let op = "sync";
    let db_path = config::db_path();
    let sync = json!({
        "sync_id": uuid::Uuid::new_v4().to_string(),
        "target": target,
        "sessions_considered": list_sessions(&db_path)?.len(),
        "timestamp": now_unix_secs(),
        "mode": "delta-sync"
    });
    let digest = digest_json("agenthalo.sync.v1", &sync)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "recorded_locally",
        "note": "Sync intent recorded locally. Cloud sync transport is not yet implemented.",
        "result_digest": digest,
        "sync": sync
    }))
}

fn tool_privacy_pool_create(arguments: Value) -> Result<Value, String> {
    if !addons::is_enabled("agentpmt-workflows")? {
        return Err(
            "agentpmt-workflows add-on is required. Enable it via the halo_capabilities tool or CLI."
                .to_string(),
        );
    }
    let chain = arguments
        .get("chain")
        .and_then(|v| v.as_str())
        .unwrap_or("base-sepolia")
        .to_string();
    let asset = arguments
        .get("asset")
        .and_then(|v| v.as_str())
        .unwrap_or("USDC")
        .to_string();
    let denomination = arguments
        .get("denomination")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "privacy_pool_create requires denomination".to_string())?;

    let op = "privacy_pool_create";
    let pool = json!({
        "pool_id": format!("pool-{}", uuid::Uuid::new_v4()),
        "chain": chain,
        "asset": asset,
        "denomination": denomination,
        "timestamp": now_unix_secs(),
        "status": "created"
    });
    let digest = digest_json("agenthalo.privacy_pool.create.v1", &pool)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "recorded_locally",
        "note": "Privacy pool creation recorded locally. On-chain pool deployment is not yet implemented.",
        "result_digest": digest,
        "pool": pool
    }))
}

fn tool_privacy_pool_withdraw(arguments: Value) -> Result<Value, String> {
    if !addons::is_enabled("agentpmt-workflows")? {
        return Err(
            "agentpmt-workflows add-on is required. Enable it via the halo_capabilities tool or CLI."
                .to_string(),
        );
    }
    let pool_id = arguments
        .get("pool_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "privacy_pool_withdraw requires pool_id".to_string())?
        .to_string();
    let recipient = arguments
        .get("recipient")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "privacy_pool_withdraw requires recipient".to_string())?
        .to_string();
    let amount = arguments
        .get("amount")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);

    let op = "privacy_pool_withdraw";
    let withdrawal = json!({
        "withdrawal_id": format!("wd-{}", uuid::Uuid::new_v4()),
        "pool_id": pool_id,
        "recipient": recipient,
        "amount": amount,
        "timestamp": now_unix_secs(),
        "status": "submitted"
    });
    let digest = digest_json("agenthalo.privacy_pool.withdraw.v1", &withdrawal)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "recorded_locally",
        "note": "Withdrawal intent recorded locally. On-chain withdrawal execution is not yet implemented.",
        "result_digest": digest,
        "withdrawal": withdrawal
    }))
}

fn tool_pq_bridge_transfer(arguments: Value) -> Result<Value, String> {
    if !addons::is_enabled("p2pclaw")? {
        return Err(
            "p2pclaw add-on is required. Enable it via the halo_capabilities tool or CLI."
                .to_string(),
        );
    }
    let from_chain = arguments
        .get("from_chain")
        .or_else(|| arguments.get("from"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pq_bridge_transfer requires from_chain".to_string())?
        .to_string();
    let to_chain = arguments
        .get("to_chain")
        .or_else(|| arguments.get("to"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pq_bridge_transfer requires to_chain".to_string())?
        .to_string();
    let asset = arguments
        .get("asset")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pq_bridge_transfer requires asset".to_string())?
        .to_string();
    let amount = arguments
        .get("amount")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "pq_bridge_transfer requires amount".to_string())?;
    let recipient = arguments
        .get("recipient")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pq_bridge_transfer requires recipient".to_string())?
        .to_string();

    let op = "pq_bridge_transfer";
    let transfer = json!({
        "transfer_id": format!("xfer-{}", uuid::Uuid::new_v4()),
        "from_chain": from_chain,
        "to_chain": to_chain,
        "asset": asset,
        "amount": amount,
        "recipient": recipient,
        "timestamp": now_unix_secs(),
        "status": "submitted"
    });
    let digest = digest_json("agenthalo.pq_bridge.transfer.v1", &transfer)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "recorded_locally",
        "note": "PQ bridge transfer intent recorded locally. Cross-chain execution is not yet implemented.",
        "result_digest": digest,
        "transfer": transfer
    }))
}

fn tool_x402_check(arguments: Value) -> Result<Value, String> {
    let body = arguments
        .get("body")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            "x402_check requires 'body' (the JSON body of an HTTP 402 response)".to_string()
        })?;
    let req = x402::parse_x402_response(body)?;
    let validation = x402::validate_payment_request(&req);
    let cfg = x402::load_x402_config();
    Ok(json!({
        "status": if validation.valid { "ok" } else { "invalid" },
        "validation": validation,
        "x402_enabled": cfg.enabled,
        "preferred_network": cfg.preferred_network,
        "supported_networks": [
            {"name": "base", "caip2": "eip155:8453", "usdc": x402::BASE_MAINNET.usdc_address},
            {"name": "base-sepolia", "caip2": "eip155:84532", "usdc": x402::BASE_SEPOLIA.usdc_address}
        ]
    }))
}

fn tool_x402_pay(arguments: Value) -> Result<Value, String> {
    let body_str = arguments
        .get("body")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            "x402_pay requires 'body' (the JSON body of an HTTP 402 response)".to_string()
        })?;
    let option_id = arguments.get("payment_option_id").and_then(|v| v.as_str());

    let req = x402::parse_x402_response(body_str)?;
    let cfg = x402::load_x402_config();
    let op = "x402_pay";

    // Check if this exact payment was already executed (by looking in paid operations).
    // We match on the x402 protocol nonce from the payment request — each 402 response
    // has a unique nonce, so paying the same nonce twice is always a duplicate.
    let db_path = config::db_path();
    let nonce_tag = format!("x402_nonce_{}", req.nonce);
    if let Ok(ops) = nucleusdb::halo::trace::paid_operations(&db_path) {
        for op_record in &ops {
            if op_record.success && op_record.operation_type == "x402_pay" {
                if let Some(ref digest) = op_record.result_digest {
                    // Match if the recorded digest contains our nonce tag (set below on success).
                    if digest.contains(&nonce_tag) {
                        return Err(format!(
                            "duplicate payment detected: x402 nonce {} has already been paid (tx: {}). \
                             Use halo_traces to find the original transaction.",
                            req.nonce, digest
                        ));
                    }
                }
            }
        }
    }

    match x402::execute_payment(&cfg, &req, option_id) {
        Ok(result) => {
            // Record with nonce tag + tx_hash so duplicate detection works.
            let digest = format!("{}|{}", nonce_tag, result.transaction_hash);
            record_paid_operation_for_halo(op, result.amount, None, Some(digest), true, None)?;
            // Build submission instructions for the agent to re-access the resource.
            let submit_instructions = json!({
                "method": result.proof.x402version.clone(),
                "step_1": "Include the payment proof as JSON in the X-PAYMENT header of your re-request.",
                "step_2": format!("Re-request the resource at '{}' using {} with the X-PAYMENT header.", req.resource, req.access_method),
                "header_name": "X-PAYMENT",
                "header_value": serde_json::to_string(&result.proof).unwrap_or_default(),
            });
            Ok(json!({
                "status": "ok",
                "payment": result,
                "submit": submit_instructions,
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, 0, None, None, false, Some(e.clone()));
            Err(e)
        }
    }
}

fn tool_x402_balance(_arguments: Value) -> Result<Value, String> {
    let cfg = x402::load_x402_config();
    if !cfg.enabled {
        return Err("x402 payments are disabled. Use the halo_capabilities tool to check configuration, or enable via CLI: agenthalo x402 enable".to_string());
    }
    let (address, balance) = x402::check_usdc_balance(&cfg)?;
    let balance_human = format!("{:.6}", balance as f64 / 1_000_000.0);
    Ok(json!({
        "status": "ok",
        "wallet_address": address,
        "balance_base_units": balance,
        "balance_usdc": balance_human,
        "network": cfg.preferred_network,
    }))
}

fn tool_halo_traces(arguments: Value) -> Result<Value, String> {
    let db_path = config::db_path();
    let session_id = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;
    let agent_filter = arguments
        .get("agent")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());
    let model_filter = arguments
        .get("model")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    if let Some(sid) = session_id {
        let export = viewer::export_session_json(&db_path, &sid)?;
        Ok(export)
    } else {
        let sessions = list_sessions(&db_path)?;
        let items: Vec<Value> = sessions
            .into_iter()
            .filter(|s| {
                if let Some(ref af) = agent_filter {
                    if !s.agent.to_lowercase().contains(af) {
                        return false;
                    }
                }
                if let Some(ref mf) = model_filter {
                    if let Some(ref model) = s.model {
                        if !model.to_lowercase().contains(mf) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            })
            .take(limit)
            .map(|s| {
                let summary = session_summary(&db_path, &s.session_id).ok().flatten();
                json!({
                    "session": s,
                    "summary": summary,
                })
            })
            .collect();
        Ok(json!({
            "sessions": items,
            "count": items.len(),
        }))
    }
}

fn tool_halo_costs(arguments: Value) -> Result<Value, String> {
    let monthly = arguments
        .get("monthly")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let include_paid = arguments
        .get("include_paid")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let db_path = config::db_path();

    let rows = nucleusdb::halo::trace::cost_buckets(&db_path, monthly)?;
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

    let mut result = json!({
        "buckets": items,
        "total_sessions": total_sessions,
        "total_tokens": total_tokens,
        "total_cost_usd": total_cost,
        "granularity": if monthly { "monthly" } else { "daily" },
    });

    if include_paid {
        let paid_ops = nucleusdb::halo::trace::paid_breakdown_by_operation_type(&db_path)?;
        let paid_items: Vec<Value> = paid_ops
            .iter()
            .map(|(kind, count, credits, usd)| {
                json!({
                    "operation_type": kind,
                    "count": count,
                    "credits_spent": credits,
                    "usd_spent": usd,
                })
            })
            .collect();
        let paid_total_usd: f64 = paid_ops.iter().map(|(_, _, _, usd)| usd).sum();
        result["paid_operations"] = json!(paid_items);
        result["paid_total_usd"] = json!(paid_total_usd);
    }

    Ok(result)
}

fn tool_halo_status(_arguments: Value) -> Result<Value, String> {
    let db_path = config::db_path();
    let creds_path = config::credentials_path();
    let has_auth = nucleusdb::halo::auth::is_authenticated(&creds_path)
        || nucleusdb::halo::auth::resolve_api_key(&creds_path).is_some();
    let pmt_cfg = agentpmt::load_or_default();

    let sessions = list_sessions(&db_path).unwrap_or_default();
    let session_count = sessions.len();
    let latest = sessions.first().cloned();
    let mut total_cost = 0.0f64;
    let mut total_tokens = 0u64;
    for s in &sessions {
        if let Ok(Some(summary)) = session_summary(&db_path, &s.session_id) {
            total_cost += summary.estimated_cost_usd;
            total_tokens += summary.total_input_tokens + summary.total_output_tokens;
        }
    }
    Ok(json!({
        "authenticated": has_auth,
        "tool_proxy_enabled": pmt_cfg.enabled,
        "session_count": session_count,
        "total_cost_usd": total_cost,
        "total_tokens": total_tokens,
        "latest_session": latest,
        "db_path": db_path.to_string_lossy(),
        "version": "0.3.0",
    }))
}

fn tool_halo_export(arguments: Value) -> Result<Value, String> {
    let session_id = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "halo_export requires session_id".to_string())?;
    let db_path = config::db_path();
    viewer::export_session_json(&db_path, session_id)
}

fn tool_x402_summary(_arguments: Value) -> Result<Value, String> {
    let cfg = x402::load_x402_config();
    let db_path = config::db_path();

    // Gather x402 paid operations.
    let ops = nucleusdb::halo::trace::paid_operations(&db_path).unwrap_or_default();
    let x402_ops: Vec<_> = ops
        .iter()
        .filter(|o| o.operation_type == "x402_pay" && o.success)
        .collect();
    let total_spent: u64 = x402_ops.iter().map(|o| o.credits_spent).sum();
    let total_spent_usd: f64 = x402_ops.iter().map(|o| o.usd_equivalent).sum();
    let payment_count = x402_ops.len();

    // Check current balance if enabled.
    let balance_info = if cfg.enabled {
        match x402::check_usdc_balance(&cfg) {
            Ok((address, balance)) => Some(json!({
                "wallet_address": address,
                "balance_base_units": balance,
                "balance_usdc": format!("{:.6}", balance as f64 / 1_000_000.0),
            })),
            Err(e) => Some(json!({"error": e})),
        }
    } else {
        None
    };

    Ok(json!({
        "x402_enabled": cfg.enabled,
        "preferred_network": cfg.preferred_network,
        "max_auto_approve": cfg.max_auto_approve,
        "max_auto_approve_usdc": format!("{:.6}", cfg.max_auto_approve as f64 / 1_000_000.0),
        "total_payments": payment_count,
        "total_spent_base_units": total_spent,
        "total_spent_usd": total_spent_usd,
        "wallet": balance_info,
    }))
}

fn tool_halo_capabilities(_arguments: Value) -> Result<Value, String> {
    let db_path = config::db_path();
    let creds_path = config::credentials_path();

    let has_auth = nucleusdb::halo::auth::is_authenticated(&creds_path)
        || nucleusdb::halo::auth::resolve_api_key(&creds_path).is_some();

    let pmt_cfg = agentpmt::load_or_default();
    let x402_cfg = x402::load_x402_config();
    let has_pq = nucleusdb::halo::pq::has_wallet();

    let addons_available = ["agentpmt-workflows", "p2pclaw"];
    let addons_status: Vec<Value> = addons_available
        .iter()
        .map(|name| {
            let enabled = addons::is_enabled(name).unwrap_or(false);
            json!({"name": name, "enabled": enabled})
        })
        .collect();

    Ok(json!({
        "version": "0.3.0",
        "authenticated": has_auth,
        "features": {
            "attestation": {
                "available": true,
                "local_merkle": true,
                "onchain_groth16": true,
            },
            "pq_signing": {
                "available": has_pq,
                "wallet_present": has_pq,
                "note": if has_pq { "Ready" } else { "Generate wallet via CLI: agenthalo keygen --pq" },
            },
            "contract_audit": {
                "available": true,
                "tiers": ["small", "medium", "large"],
            },
            "trust_query": {
                "available": true,
            },
            "x402_payments": {
                "enabled": x402_cfg.enabled,
                "preferred_network": x402_cfg.preferred_network,
                "max_auto_approve_usdc": format!("{:.6}", x402_cfg.max_auto_approve as f64 / 1_000_000.0),
                "note": if x402_cfg.enabled { "Ready" } else { "Enable via CLI: agenthalo x402 enable" },
            },
            "tool_proxy": {
                "enabled": pmt_cfg.enabled,
                "budget_tag": pmt_cfg.budget_tag,
                "note": "AgentPMT tool proxy forwards calls to third-party tools. Proxy execution is not yet implemented.",
            },
        },
        "addons": addons_status,
        "db_path": db_path.to_string_lossy(),
    }))
}

fn error_envelope(id: Option<Value>, code: i64, message: &str) -> JsonRpcErrorEnvelope {
    JsonRpcErrorEnvelope {
        jsonrpc: "2.0",
        id: id.unwrap_or(json!(null)),
        error: JsonRpcError {
            code,
            message: message.to_string(),
        },
    }
}

fn authorized(headers: &HeaderMap, expected_secret: &str) -> bool {
    let Some(raw) = headers.get("authorization") else {
        return false;
    };
    let Ok(raw) = raw.to_str() else {
        return false;
    };
    let Some(token) = raw.strip_prefix("Bearer ") else {
        return false;
    };
    ct_eq(token.as_bytes(), expected_secret.as_bytes())
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleusdb::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
    use nucleusdb::halo::trace::{now_unix_secs, TraceWriter};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn unknown_tool_sets_error_flag() {
        let out = tool_call_response("does_not_exist", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn known_tool_clears_error_flag() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_test_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let out = tool_call_response("sync", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(false));

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn attest_dry_run_returns_payload_without_tx_side_effects() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_attest_dry_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let db_path = nucleusdb::halo::config::db_path();
        let mut writer = TraceWriter::new(&db_path).expect("trace writer");
        let sid = format!("sess-attest-dry-{}", now_unix_secs());
        writer
            .start_session(SessionMetadata {
                session_id: sid.clone(),
                agent: "codex".to_string(),
                model: Some("gpt-5".to_string()),
                started_at: now_unix_secs(),
                ended_at: None,
                prompt: Some("test".to_string()),
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })
            .expect("start");
        writer
            .write_event(TraceEvent {
                seq: 0,
                timestamp: now_unix_secs(),
                event_type: EventType::Assistant,
                content: json!({"text":"hello"}),
                input_tokens: Some(1),
                output_tokens: Some(1),
                cache_read_tokens: Some(0),
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })
            .expect("event");
        writer
            .end_session(SessionStatus::Completed)
            .expect("end session");

        let payload = tool_attest(json!({
            "session_id": sid,
            "onchain": true,
            "dry_run": true
        }))
        .expect("attest dry-run");

        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["onchain"]["dry_run"], true);
        assert!(payload["attestation"]["tx_hash"].is_null());
        assert!(payload["attestation"]["contract_address"].is_null());

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }
}
