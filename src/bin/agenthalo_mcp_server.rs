use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use nucleusdb::halo::addons;
use nucleusdb::halo::agentpmt::{self, AgentPmtClient};
use nucleusdb::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use nucleusdb::halo::audit::{
    audit_contract_file, audit_contract_source, save_audit_result, AuditRequest, AuditSize,
};
use nucleusdb::halo::circuit::{
    load_or_setup_attestation_keys, proof_words_json_array, prove_attestation,
    public_inputs_json_array, verify_attestation_proof,
};
use nucleusdb::halo::config;
use nucleusdb::halo::onchain::{load_onchain_config_or_default, post_attestation};
use nucleusdb::halo::pq::{has_wallet, sign_pq_payload};
use nucleusdb::halo::trace::{list_sessions, now_unix_secs, record_paid_operation_for_halo};
use nucleusdb::halo::trust::query_trust_score;
use nucleusdb::halo::util::digest_json;
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
        "phase": "3-live"
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
            "serverInfo": {"name": "agenthalo-mcp-server", "version": "0.1.0-phase3"},
            "capabilities": {"tools": {}}
        }),
        "tools/list" => json!({
            "tools": [
                {"name":"attest","description":"AgentHALO attestation (merkle local or Groth16 onchain with onchain=true)"},
                {"name":"sign_pq","description":"AgentHALO post-quantum detached signing (paid)"},
                {"name":"audit_contract","description":"AgentHALO Solidity static audit (paid)"},
                {"name":"trust_query","description":"AgentHALO trust score query (paid)"},
                {"name":"vote","description":"AgentHALO governance vote operation (paid)"},
                {"name":"sync","description":"AgentHALO cloud sync operation (paid)"},
                {"name":"privacy_pool_create","description":"AgentHALO privacy pool create operation (paid; workflows add-on)"},
                {"name":"privacy_pool_withdraw","description":"AgentHALO privacy pool withdraw operation (paid; workflows add-on)"},
                {"name":"pq_bridge_transfer","description":"AgentHALO PQ bridge transfer operation (paid; p2pclaw add-on)"}
            ]
        }),
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
        other => Err(format!("unknown tool: {other}")),
    }
}

fn tool_attest(arguments: Value) -> Result<Value, String> {
    let client = require_agentpmt()?;
    let anonymous = arguments
        .get("anonymous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let onchain = arguments
        .get("onchain")
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

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
                let (pk, vk, _key_info) = load_or_setup_attestation_keys(None)?;
                let proof_bundle = prove_attestation(&pk, &result)?;
                if !verify_attestation_proof(&vk, &proof_bundle)? {
                    return Err("local Groth16 verification failed".to_string());
                }
                let cfg = load_onchain_config_or_default();
                let posted = post_attestation(&cfg, &proof_bundle, anonymous)?;
                result.proof_type = "groth16-bn254".to_string();
                result.groth16_proof = Some(proof_words_json_array(&proof_bundle));
                result.groth16_public_inputs = Some(public_inputs_json_array(&proof_bundle));
                result.tx_hash = Some(posted.tx_hash.clone());
                result.contract_address = Some(posted.contract_address.clone());
                result.block_number = posted.block_number;
                result.chain = Some(posted.chain.clone());
                onchain_payload = Some(posted);
            }
            let saved_path = save_attestation(&session_id, &result)?;
            record_paid_operation_for_halo(
                op,
                cost,
                Some(session_id),
                Some(result.attestation_digest.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "remaining_credits": deducted.remaining_credits,
                "attestation_path": saved_path.display().to_string(),
                "attestation": result,
                "onchain": onchain_payload
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(
                op,
                cost,
                Some(session_id),
                None,
                false,
                Some(e.clone()),
            );
            Err(format!("attestation failed after credit deduction: {e}"))
        }
    }
}

fn tool_audit_contract(arguments: Value) -> Result<Value, String> {
    let client = require_agentpmt()?;
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err("insufficient credits".to_string());
    }

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
                cost,
                None,
                Some(result.contract_hash.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "remaining_credits": deducted.remaining_credits,
                "audit_path": saved_path.display().to_string(),
                "audit": result
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, cost, None, None, false, Some(e.clone()));
            Err(format!("audit failed after credit deduction: {e}"))
        }
    }
}

fn tool_sign_pq(arguments: Value) -> Result<Value, String> {
    if !has_wallet() {
        return Err("no PQ wallet found. Run: agenthalo keygen --pq".to_string());
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

    match sign_pq_payload(&payload, &payload_kind, payload_hint) {
        Ok((envelope, save_path)) => {
            record_paid_operation_for_halo(
                op,
                cost,
                None,
                Some(envelope.signature_digest.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "remaining_credits": deducted.remaining_credits,
                "signature_path": save_path.display().to_string(),
                "signature": envelope
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(op, cost, None, None, false, Some(e.clone()));
            Err(format!("signing failed after credit deduction: {e}"))
        }
    }
}

fn tool_trust_query(arguments: Value) -> Result<Value, String> {
    let op = "trust_query";
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

    let requested_session = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let db_path = config::db_path();
    match query_trust_score(&db_path, requested_session.as_deref()) {
        Ok(score) => {
            record_paid_operation_for_halo(
                op,
                cost,
                requested_session,
                Some(score.digest.clone()),
                true,
                None,
            )?;
            Ok(json!({
                "status": "ok",
                "remaining_credits": deducted.remaining_credits,
                "score": score
            }))
        }
        Err(e) => {
            let _ = record_paid_operation_for_halo(
                op,
                cost,
                requested_session,
                None,
                false,
                Some(e.clone()),
            );
            Err(format!("trust query failed after credit deduction: {e}"))
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

    let vote = json!({
        "vote_id": uuid::Uuid::new_v4().to_string(),
        "proposal_id": proposal_id,
        "choice": choice,
        "reason": reason,
        "timestamp": now_unix_secs()
    });
    let digest = digest_json("agenthalo.vote.v1", &vote)?;
    record_paid_operation_for_halo(op, cost, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "ok",
        "remaining_credits": deducted.remaining_credits,
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

    let db_path = config::db_path();
    let sync = json!({
        "sync_id": uuid::Uuid::new_v4().to_string(),
        "target": target,
        "sessions_considered": list_sessions(&db_path)?.len(),
        "timestamp": now_unix_secs(),
        "mode": "delta-sync"
    });
    let digest = digest_json("agenthalo.sync.v1", &sync)?;
    record_paid_operation_for_halo(op, cost, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "ok",
        "remaining_credits": deducted.remaining_credits,
        "result_digest": digest,
        "sync": sync
    }))
}

fn tool_privacy_pool_create(arguments: Value) -> Result<Value, String> {
    if !addons::is_enabled("agentpmt-workflows")? {
        return Err(
            "agentpmt-workflows add-on is required. Run: agenthalo addon enable agentpmt-workflows"
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

    let pool = json!({
        "pool_id": format!("pool-{}", uuid::Uuid::new_v4()),
        "chain": chain,
        "asset": asset,
        "denomination": denomination,
        "timestamp": now_unix_secs(),
        "status": "created"
    });
    let digest = digest_json("agenthalo.privacy_pool.create.v1", &pool)?;
    record_paid_operation_for_halo(op, cost, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "ok",
        "remaining_credits": deducted.remaining_credits,
        "result_digest": digest,
        "pool": pool
    }))
}

fn tool_privacy_pool_withdraw(arguments: Value) -> Result<Value, String> {
    if !addons::is_enabled("agentpmt-workflows")? {
        return Err(
            "agentpmt-workflows add-on is required. Run: agenthalo addon enable agentpmt-workflows"
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

    let withdrawal = json!({
        "withdrawal_id": format!("wd-{}", uuid::Uuid::new_v4()),
        "pool_id": pool_id,
        "recipient": recipient,
        "amount": amount,
        "timestamp": now_unix_secs(),
        "status": "submitted"
    });
    let digest = digest_json("agenthalo.privacy_pool.withdraw.v1", &withdrawal)?;
    record_paid_operation_for_halo(op, cost, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "ok",
        "remaining_credits": deducted.remaining_credits,
        "result_digest": digest,
        "withdrawal": withdrawal
    }))
}

fn tool_pq_bridge_transfer(arguments: Value) -> Result<Value, String> {
    if !addons::is_enabled("p2pclaw")? {
        return Err("p2pclaw add-on is required. Run: agenthalo addon enable p2pclaw".to_string());
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
    let cost = agentpmt::operation_cost(op).unwrap_or(0);
    let client = require_agentpmt()?;
    let deducted = client.deduct(op, 1)?;
    if !deducted.success {
        return Err(format!(
            "insufficient credits. Have: {}, need: {}",
            deducted.remaining_credits, cost
        ));
    }

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
    record_paid_operation_for_halo(op, cost, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "ok",
        "remaining_credits": deducted.remaining_credits,
        "result_digest": digest,
        "transfer": transfer
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

fn require_agentpmt() -> Result<AgentPmtClient, String> {
    AgentPmtClient::from_config().ok_or_else(|| {
        "not connected to AgentPMT. Run: agenthalo config set-agentpmt-key <key>".to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucleusdb::halo::agentpmt::{agentpmt_config_path, save_agentpmt_config, AgentPmtConfig};
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
        std::env::set_var("AGENTHALO_AGENTPMT_STUB", "1");
        save_agentpmt_config(
            &agentpmt_config_path(),
            &AgentPmtConfig {
                api_key: "test".to_string(),
                cached_balance: Some(10_000),
                balance_refreshed_at: None,
                history: vec![],
            },
        )
        .expect("seed agentpmt config");

        let out = tool_call_response("sync", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(false));

        std::env::remove_var("AGENTHALO_AGENTPMT_STUB");
        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }
}
