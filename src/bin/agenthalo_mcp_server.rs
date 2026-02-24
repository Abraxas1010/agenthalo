use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use nucleusdb::halo::agentpmt::{self, AgentPmtClient};
use nucleusdb::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use nucleusdb::halo::audit::{
    audit_contract_file, audit_contract_source, save_audit_result, AuditRequest, AuditSize,
};
use nucleusdb::halo::config;
use nucleusdb::halo::schema::PaidOperation;
use nucleusdb::halo::trace::{now_unix_secs, TraceWriter};
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
        "phase": "1-live"
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
            "serverInfo": {"name": "agenthalo-mcp-server", "version": "0.1.0-phase1"},
            "capabilities": {"tools": {}}
        }),
        "tools/list" => json!({
            "tools": [
                {"name":"attest","description":"AgentHALO Merkle attestation (paid)"},
                {"name":"sign_pq","description":"AgentHALO post-quantum signing (Phase 0 stub)"},
                {"name":"audit_contract","description":"AgentHALO Solidity static audit (paid)"},
                {"name":"trust_query","description":"AgentHALO trust query (Phase 0 stub)"},
                {"name":"vote","description":"AgentHALO DAO vote (Phase 0 stub)"},
                {"name":"sync","description":"AgentHALO cloud sync (Phase 0 stub)"},
                {"name":"privacy_pool_create","description":"AgentHALO privacy pool create (Phase 0 stub)"},
                {"name":"privacy_pool_withdraw","description":"AgentHALO privacy pool withdraw (Phase 0 stub)"},
                {"name":"pq_bridge_transfer","description":"AgentHALO PQ bridge transfer (Phase 0 stub)"}
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
        "sign_pq" => Ok(json!({
            "status": "stub",
            "message": "PQ signing not yet implemented (Phase 1)"
        })),
        "audit_contract" => tool_audit_contract(arguments),
        "trust_query" => Ok(json!({
            "status": "stub",
            "score": 0.5,
            "attestation_count": 0
        })),
        "vote" => Ok(json!({
            "status": "stub",
            "message": "DAO voting not yet implemented (Phase 1)"
        })),
        "sync" => Ok(json!({
            "status": "stub",
            "message": "Cloud sync not yet implemented (Phase 2)"
        })),
        "privacy_pool_create" => Ok(json!({
            "status": "stub",
            "message": "Privacy pool create not yet implemented (Phase 2)"
        })),
        "privacy_pool_withdraw" => Ok(json!({
            "status": "stub",
            "message": "Privacy pool withdraw not yet implemented (Phase 2)"
        })),
        "pq_bridge_transfer" => Ok(json!({
            "status": "stub",
            "message": "PQ bridge transfer not yet implemented (Phase 2)"
        })),
        other => Err(format!("unknown tool: {other}")),
    }
}

fn tool_attest(arguments: Value) -> Result<Value, String> {
    let client = require_agentpmt()?;
    let anonymous = arguments
        .get("anonymous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let requested_session_id = arguments
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let op = if anonymous { "attest_anon" } else { "attest" };
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
        Ok(result) => {
            let saved_path = save_attestation(&session_id, &result)?;
            record_paid_operation(
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
                "attestation": result
            }))
        }
        Err(e) => {
            let _ = record_paid_operation(op, cost, Some(session_id), None, false, Some(e.clone()));
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
            record_paid_operation(
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
            let _ = record_paid_operation(op, cost, None, None, false, Some(e.clone()));
            Err(format!("audit failed after credit deduction: {e}"))
        }
    }
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

fn record_paid_operation(
    operation_type: &str,
    credits_spent: u64,
    session_id: Option<String>,
    result_digest: Option<String>,
    success: bool,
    error: Option<String>,
) -> Result<(), String> {
    let db_path = config::db_path();
    let mut writer = TraceWriter::new(&db_path)?;
    writer.record_paid_operation(PaidOperation {
        operation_id: uuid::Uuid::new_v4().to_string(),
        timestamp: now_unix_secs(),
        operation_type: operation_type.to_string(),
        credits_spent,
        usd_equivalent: (credits_spent as f64) * 0.01,
        session_id,
        result_digest,
        success,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_tool_sets_error_flag() {
        let out = tool_call_response("does_not_exist", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn known_tool_clears_error_flag() {
        let out = tool_call_response("sync", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(false));
    }
}
