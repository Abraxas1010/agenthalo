use crate::halo::did::DIDIdentity;
use crate::halo::didcomm::{
    extract_x25519_public_key_from_doc, message_types, pack_authcrypt, unpack_with_resolver,
    DIDCommMessage,
};
use crate::halo::didcomm_handler::DIDCommHandler;
use crate::halo::p2p_discovery::AgentCapability;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aAgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    pub provider: A2aProvider,
    pub capabilities: A2aCapabilities,
    pub skills: Vec<A2aSkill>,
    pub security_schemes: HashMap<String, A2aSecurityScheme>,
    pub security: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aProvider {
    pub organization: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aCapabilities {
    pub streaming: bool,
    pub push_notifications: bool,
    pub state_transition_history: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct A2aSecurityScheme {
    #[serde(rename = "type")]
    pub type_: String,
    pub did: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Submitted,
    Working,
    Completed,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TaskRecord {
    task_id: String,
    status: TaskStatus,
    created_at: u64,
    updated_at: u64,
    task_type: String,
    payload: Value,
    result: Option<Value>,
    error: Option<String>,
}

#[derive(Clone)]
struct BridgeState {
    card: A2aAgentCard,
    identity: Arc<DIDIdentity>,
    didcomm: Arc<DIDCommHandler>,
    tasks: Arc<RwLock<HashMap<String, TaskRecord>>>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: Option<String>,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct TaskSendParams {
    #[serde(default)]
    recipient_did: Option<String>,
    #[serde(default)]
    task_type: Option<String>,
    #[serde(default)]
    payload: Value,
}

#[derive(Debug, Deserialize)]
struct TaskGetParams {
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct TaskCancelParams {
    task_id: String,
}

pub fn generate_agent_card(
    identity: &DIDIdentity,
    base_url: &str,
    skills: &[AgentCapability],
) -> A2aAgentCard {
    A2aAgentCard {
        name: std::env::var("AGENT_NAME").unwrap_or_else(|_| "AgentHalo".to_string()),
        description: std::env::var("AGENT_DESCRIPTION")
            .unwrap_or_else(|_| "Sovereign AI agent with privacy-first communication".to_string()),
        url: base_url.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        provider: A2aProvider {
            organization: "Self-Sovereign".to_string(),
            url: identity.did.clone(),
        },
        capabilities: A2aCapabilities {
            // Disabled until SSE/WebSocket streaming routes are implemented.
            streaming: false,
            push_notifications: false,
            state_transition_history: true,
        },
        skills: skills
            .iter()
            .map(|capability| A2aSkill {
                id: capability.id.clone(),
                name: capability.name.clone(),
                description: capability.description.clone(),
                tags: Vec::new(),
                examples: Vec::new(),
            })
            .collect(),
        security_schemes: HashMap::from([(
            "didAuth".to_string(),
            A2aSecurityScheme {
                type_: "didcomm".to_string(),
                did: identity.did.clone(),
                description: "DIDComm v2 authenticated messaging".to_string(),
            },
        )]),
        security: vec!["didAuth".to_string()],
    }
}

pub async fn start_a2a_bridge(
    identity: Arc<DIDIdentity>,
    port: u16,
    skills: Vec<AgentCapability>,
) -> Result<(), String> {
    if port == 0 {
        return Ok(());
    }

    let base_url = format!("http://127.0.0.1:{port}");
    let mut didcomm = DIDCommHandler::new(identity.clone());
    didcomm.register_builtin_handlers();

    let state = BridgeState {
        card: generate_agent_card(&identity, &base_url, &skills),
        identity,
        didcomm: Arc::new(didcomm),
        tasks: Arc::new(RwLock::new(HashMap::new())),
    };

    let app = Router::new()
        .route(
            "/.well-known/agent.json",
            get(|State(state): State<BridgeState>| async move { Json(state.card) }),
        )
        .route(
            "/",
            post(
                |State(state): State<BridgeState>, Json(body): Json<Value>| async move {
                    Json(handle_jsonrpc(state, body).await)
                },
            ),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .map_err(|e| format!("bind A2A bridge on port {port}: {e}"))?;

    eprintln!("[AgentHalo/A2A] bridge active on {base_url}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve A2A bridge: {e}"))
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn jsonrpc_result(id: Value, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut err = serde_json::json!({
        "code": code,
        "message": message,
    });
    if let Some(extra) = data {
        err["data"] = extra;
    }
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": err,
    })
}

fn is_terminal(status: &TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Canceled
    )
}

async fn run_task_via_didcomm(
    state: &BridgeState,
    task_id: &str,
    recipient_did: &str,
    task_type: &str,
    payload: &Value,
) -> Result<Option<DIDCommMessage>, String> {
    if recipient_did != state.identity.did {
        return Err(format!(
            "A2A bridge only supports local DIDComm routing in Phase 2; unknown recipient `{recipient_did}`"
        ));
    }

    let recipient_key = extract_x25519_public_key_from_doc(&state.identity.did_document)?;
    let outbound = DIDCommMessage::new(
        message_types::TASK_SEND,
        Some(&state.identity.did),
        vec![recipient_did.to_string()],
        serde_json::json!({
            "task_id": task_id,
            "task_type": task_type,
            "payload": payload,
        }),
    );

    let packed = pack_authcrypt(&outbound, &state.identity, &recipient_key)?;
    let packed_response = state
        .didcomm
        .handle_incoming(&packed, |did| {
            if did == state.identity.did {
                Some(state.identity.did_document.clone())
            } else {
                None
            }
        })
        .await?;

    let Some(packed_response) = packed_response else {
        return Ok(None);
    };

    let (response, _) = unpack_with_resolver(&packed_response, &state.identity, |did| {
        if did == state.identity.did {
            Some(state.identity.did_document.clone())
        } else {
            None
        }
    })?;

    Ok(Some(response))
}

async fn update_task_status(
    tasks: Arc<RwLock<HashMap<String, TaskRecord>>>,
    task_id: &str,
    status: TaskStatus,
    result: Option<Value>,
    error: Option<String>,
) {
    let mut guard = tasks.write().await;
    if let Some(record) = guard.get_mut(task_id) {
        if is_terminal(&record.status) {
            return;
        }
        record.status = status;
        record.updated_at = now_unix_secs();
        if let Some(result) = result {
            record.result = Some(result);
        }
        if let Some(error) = error {
            record.error = Some(error);
        }
    }
}

async fn tasks_send(state: BridgeState, params: TaskSendParams) -> Result<Value, String> {
    let recipient_did = params
        .recipient_did
        .unwrap_or_else(|| state.identity.did.clone());
    let task_type = params
        .task_type
        .unwrap_or_else(|| "generic".to_string())
        .trim()
        .to_string();
    if task_type.is_empty() {
        return Err("tasks/send requires non-empty task_type".to_string());
    }

    let task_id = uuid::Uuid::new_v4().to_string();
    let now = now_unix_secs();
    let initial = TaskRecord {
        task_id: task_id.clone(),
        status: TaskStatus::Submitted,
        created_at: now,
        updated_at: now,
        task_type: task_type.clone(),
        payload: params.payload.clone(),
        result: None,
        error: None,
    };
    state.tasks.write().await.insert(task_id.clone(), initial);

    match run_task_via_didcomm(
        &state,
        &task_id,
        &recipient_did,
        &task_type,
        &params.payload,
    )
    .await
    {
        Ok(response) => {
            let didcomm_status = response
                .as_ref()
                .and_then(|msg| msg.body.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("submitted")
                .to_string();

            let tasks = state.tasks.clone();
            let task_id_for_worker = task_id.clone();
            let task_type_for_worker = task_type.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(150)).await;
                update_task_status(
                    tasks.clone(),
                    &task_id_for_worker,
                    TaskStatus::Working,
                    None,
                    None,
                )
                .await;

                tokio::time::sleep(Duration::from_millis(150)).await;
                update_task_status(
                    tasks,
                    &task_id_for_worker,
                    TaskStatus::Completed,
                    Some(serde_json::json!({
                        "task_type": task_type_for_worker,
                        "status": "completed",
                    })),
                    None,
                )
                .await;
            });

            Ok(serde_json::json!({
                "task_id": task_id,
                "status": "submitted",
                "didcomm_status": didcomm_status,
            }))
        }
        Err(error) => {
            update_task_status(
                state.tasks.clone(),
                &task_id,
                TaskStatus::Failed,
                None,
                Some(error.clone()),
            )
            .await;
            Err(error)
        }
    }
}

async fn tasks_get(state: BridgeState, params: TaskGetParams) -> Result<Value, String> {
    let guard = state.tasks.read().await;
    let Some(record) = guard.get(&params.task_id) else {
        return Err(format!("unknown task_id `{}`", params.task_id));
    };
    serde_json::to_value(record).map_err(|e| format!("serialize task record: {e}"))
}

async fn tasks_cancel(state: BridgeState, params: TaskCancelParams) -> Result<Value, String> {
    let mut guard = state.tasks.write().await;
    let Some(record) = guard.get_mut(&params.task_id) else {
        return Err(format!("unknown task_id `{}`", params.task_id));
    };

    if is_terminal(&record.status) {
        return Ok(serde_json::json!({
            "task_id": record.task_id,
            "status": record.status,
            "already_terminal": true,
        }));
    }

    record.status = TaskStatus::Canceled;
    record.updated_at = now_unix_secs();

    let recipient_did = state.identity.did.clone();
    let recipient_key = extract_x25519_public_key_from_doc(&state.identity.did_document)?;
    let cancel_message = DIDCommMessage::new(
        message_types::TASK_CANCEL,
        Some(&state.identity.did),
        vec![recipient_did],
        serde_json::json!({ "task_id": record.task_id }),
    );
    let packed = pack_authcrypt(&cancel_message, &state.identity, &recipient_key)?;
    let _ = state
        .didcomm
        .handle_incoming(&packed, |did| {
            if did == state.identity.did {
                Some(state.identity.did_document.clone())
            } else {
                None
            }
        })
        .await?;

    Ok(serde_json::json!({
        "task_id": record.task_id,
        "status": "canceled",
    }))
}

async fn handle_jsonrpc(state: BridgeState, body: Value) -> Value {
    let parsed: JsonRpcRequest = match serde_json::from_value(body) {
        Ok(request) => request,
        Err(e) => {
            return jsonrpc_error(
                Value::Null,
                -32700,
                "parse error",
                Some(serde_json::json!({"detail": e.to_string()})),
            )
        }
    };

    let id = parsed.id.unwrap_or(Value::Null);
    if parsed.jsonrpc.as_deref() != Some("2.0") {
        return jsonrpc_error(id, -32600, "invalid request: jsonrpc must be `2.0`", None);
    }

    let Some(method) = parsed.method else {
        return jsonrpc_error(id, -32600, "invalid request: missing method", None);
    };

    match method.as_str() {
        "tasks/send" => {
            let params: TaskSendParams = match serde_json::from_value(parsed.params) {
                Ok(v) => v,
                Err(e) => {
                    return jsonrpc_error(
                        id,
                        -32602,
                        "invalid params",
                        Some(serde_json::json!({"detail": e.to_string()})),
                    )
                }
            };
            match tasks_send(state, params).await {
                Ok(result) => jsonrpc_result(id, result),
                Err(error) => jsonrpc_error(
                    id,
                    -32001,
                    "task dispatch failed",
                    Some(serde_json::json!({"detail": error})),
                ),
            }
        }
        "tasks/get" => {
            let params: TaskGetParams = match serde_json::from_value(parsed.params) {
                Ok(v) => v,
                Err(e) => {
                    return jsonrpc_error(
                        id,
                        -32602,
                        "invalid params",
                        Some(serde_json::json!({"detail": e.to_string()})),
                    )
                }
            };
            match tasks_get(state, params).await {
                Ok(result) => jsonrpc_result(id, result),
                Err(error) => jsonrpc_error(
                    id,
                    -32004,
                    "task lookup failed",
                    Some(serde_json::json!({"detail": error})),
                ),
            }
        }
        "tasks/cancel" => {
            let params: TaskCancelParams = match serde_json::from_value(parsed.params) {
                Ok(v) => v,
                Err(e) => {
                    return jsonrpc_error(
                        id,
                        -32602,
                        "invalid params",
                        Some(serde_json::json!({"detail": e.to_string()})),
                    )
                }
            };
            match tasks_cancel(state, params).await {
                Ok(result) => jsonrpc_result(id, result),
                Err(error) => jsonrpc_error(
                    id,
                    -32005,
                    "task cancel failed",
                    Some(serde_json::json!({"detail": error})),
                ),
            }
        }
        _ => jsonrpc_error(
            id,
            -32601,
            "method not found",
            Some(serde_json::json!({"method": method})),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> BridgeState {
        let identity =
            Arc::new(crate::halo::did::did_from_genesis_seed(&[0x55; 64]).expect("identity"));
        let card = generate_agent_card(&identity, "http://127.0.0.1:9300", &[]);
        let mut handler = DIDCommHandler::new(identity.clone());
        handler.register_builtin_handlers();
        BridgeState {
            card,
            identity,
            didcomm: Arc::new(handler),
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[test]
    fn agent_card_uses_did_and_skills() {
        let identity = crate::halo::did::did_from_genesis_seed(&[0x55; 64]).expect("identity");
        let card = generate_agent_card(
            &identity,
            "http://127.0.0.1:9300",
            &[AgentCapability {
                id: "coding".to_string(),
                name: "Coding".to_string(),
                description: "Writes code".to_string(),
                input_types: vec!["text/plain".to_string()],
                output_types: vec!["text/plain".to_string()],
            }],
        );
        assert_eq!(card.security, vec!["didAuth".to_string()]);
        assert_eq!(card.skills.len(), 1);
        assert_eq!(card.provider.url, identity.did);
        assert!(!card.capabilities.streaming);
    }

    #[tokio::test]
    async fn jsonrpc_tasks_send_get_cancel() {
        let state = test_state();
        let send_resp = handle_jsonrpc(
            state.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tasks/send",
                "params": {
                    "task_type": "proof",
                    "payload": {"goal": "⊢ P -> P"}
                }
            }),
        )
        .await;
        let task_id = send_resp["result"]["task_id"]
            .as_str()
            .expect("task id")
            .to_string();

        let get_resp = handle_jsonrpc(
            state.clone(),
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tasks/get",
                "params": { "task_id": task_id }
            }),
        )
        .await;
        assert_eq!(get_resp["result"]["task_type"], "proof");

        let cancel_resp = handle_jsonrpc(
            state,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 3,
                "method": "tasks/cancel",
                "params": { "task_id": task_id }
            }),
        )
        .await;
        assert_eq!(cancel_resp["result"]["status"], "canceled");
    }

    #[tokio::test]
    async fn jsonrpc_rejects_unknown_method() {
        let state = test_state();
        let response = handle_jsonrpc(
            state,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 10,
                "method": "tasks/unknown",
                "params": {}
            }),
        )
        .await;
        assert_eq!(response["error"]["code"], -32601);
    }
}
