use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::net::SocketAddr;
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
    let secret = std::env::var("AGENTHALO_MCP_SECRET")
        .unwrap_or_else(|_| "agenthalo-dev-secret".to_string());

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
        "phase": "0-stub"
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
            "serverInfo": {"name": "agenthalo-mcp-server", "version": "0.1.0-phase0"},
            "capabilities": {"tools": {}}
        }),
        "tools/list" => json!({
            "tools": [
                {"name":"attest","description":"AgentHALO attestation (Phase 0 stub)"},
                {"name":"sign_pq","description":"AgentHALO post-quantum signing (Phase 0 stub)"},
                {"name":"audit_contract","description":"AgentHALO contract audit (Phase 0 stub)"},
                {"name":"trust_query","description":"AgentHALO trust query (Phase 0 stub)"},
                {"name":"vote","description":"AgentHALO DAO vote (Phase 0 stub)"},
                {"name":"sync","description":"AgentHALO cloud sync (Phase 0 stub)"}
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
            tool_call_stub(name, arguments)
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

fn tool_call_stub(name: &str, arguments: Value) -> Value {
    let payload = match name {
        "attest" => json!({
            "status": "stub",
            "message": "Attestation not yet implemented (Phase 1)",
            "session_id": arguments.get("session_id").cloned().unwrap_or(Value::Null),
            "anonymous": arguments.get("anonymous").and_then(|v| v.as_bool()).unwrap_or(false)
        }),
        "sign_pq" => json!({
            "status": "stub",
            "message": "PQ signing not yet implemented (Phase 1)"
        }),
        "audit_contract" => json!({
            "status": "stub",
            "message": "Contract audit not yet implemented (Phase 1)"
        }),
        "trust_query" => json!({
            "status": "stub",
            "score": 0.5,
            "attestation_count": 0
        }),
        "vote" => json!({
            "status": "stub",
            "message": "DAO voting not yet implemented (Phase 1)"
        }),
        "sync" => json!({
            "status": "stub",
            "message": "Cloud sync not yet implemented (Phase 2)"
        }),
        other => json!({
            "status": "error",
            "message": format!("unknown tool: {other}")
        }),
    };

    json!({
        "content": [
            {
                "type": "text",
                "text": payload.to_string()
            }
        ],
        "isError": name.is_empty()
    })
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
