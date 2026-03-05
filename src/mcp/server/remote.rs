//! Remote MCP server over Streamable HTTP transport.
//!
//! Exposes the full NucleusDB MCP tool surface at a single `/mcp` endpoint
//! using the MCP Streamable HTTP specification (2025-03-26).
//!
//! Also exposes a `/didcomm` endpoint for receiving DIDComm v2 encrypted
//! envelopes from mesh peers (Part 2 sovereign comms).
//!
//! Supports dual authentication: CAB-as-bearer-token and OAuth 2.1 JWT.

use crate::mcp::server::auth::AuthConfig;
use crate::mcp::tools::{
    ExecuteSqlRequest, ExportRequest, HistoryRequest, NucleusDbMcpService, QueryRangeRequest,
    QueryRequest, VerifyRequest,
};
use axum::Router;
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::tower::{
    StreamableHttpServerConfig, StreamableHttpService,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

const DID_KEY_PREFIX: &str = "did:key:";
const MULTICODEC_ED25519_PUB: &[u8; 2] = &[0xed, 0x01];
const TYPE_ED25519: &str = "Ed25519VerificationKey2020";

#[derive(Clone)]
struct DidcommRouteState {
    mcp_endpoint: String,
    db_path: String,
    agent_identity: Option<Arc<crate::halo::did::DIDIdentity>>,
    mesh_registry_path: String,
}

/// Configuration for the remote MCP HTTP server.
#[derive(Debug, Clone)]
pub struct RemoteServerConfig {
    /// Database path for NucleusDB state.
    pub db_path: String,
    /// Listen address (e.g., "0.0.0.0:8443" or "127.0.0.1:3000").
    pub listen_addr: SocketAddr,
    /// Authentication configuration.
    pub auth: AuthConfig,
    /// MCP endpoint path (default: "/mcp").
    pub endpoint_path: String,
}

impl Default for RemoteServerConfig {
    fn default() -> Self {
        Self {
            db_path: "nucleusdb.ndb".to_string(),
            listen_addr: SocketAddr::from(([127, 0, 0, 1], 3000)),
            auth: AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        }
    }
}

/// Run the NucleusDB MCP server over Streamable HTTP.
pub async fn run_remote_mcp_server(config: RemoteServerConfig) -> Result<(), String> {
    let db_path = config.db_path.clone();
    let auth_config = Arc::new(config.auth.clone());
    let mesh_registry_path = std::env::var("NUCLEUSDB_MESH_REGISTRY")
        .unwrap_or_else(|_| crate::container::mesh::MESH_REGISTRY_PATH.to_string());
    let didcomm_state = DidcommRouteState {
        mcp_endpoint: format!("http://{}{}", config.listen_addr, config.endpoint_path),
        db_path: config.db_path.clone(),
        agent_identity: load_agent_identity().ok().map(Arc::new),
        mesh_registry_path,
    };

    // StreamableHttpService takes a factory closure that creates a fresh service
    // per MCP session. Each session gets its own NucleusDbMcpService sharing
    // the same DB path (state is file-backed, not in-memory per session).
    let mcp_service = StreamableHttpService::new(
        move || NucleusDbMcpService::new(&db_path).map_err(std::io::Error::other),
        LocalSessionManager::default().into(),
        StreamableHttpServerConfig::default(),
    );

    // CORS: restrict to localhost and configured origins. In production,
    // set NUCLEUSDB_CORS_ORIGINS env var to a comma-separated list of allowed origins.
    let cors = {
        let origins_env = std::env::var("NUCLEUSDB_CORS_ORIGINS").unwrap_or_default();
        if origins_env.is_empty() {
            // Default: localhost only.
            CorsLayer::new()
                .allow_origin([
                    "http://localhost:3000".parse().unwrap(),
                    "http://127.0.0.1:3000".parse().unwrap(),
                ])
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::ACCEPT,
                ])
        } else {
            let origins: Vec<axum::http::HeaderValue> = origins_env
                .split(',')
                .filter_map(|s| s.trim().parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::header::AUTHORIZATION,
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::ACCEPT,
                ])
        }
    };

    // Rate limiting: concurrency limit per server (default 64 concurrent requests).
    let max_concurrent: usize = std::env::var("NUCLEUSDB_MAX_CONCURRENT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);

    let app = Router::new()
        .nest_service(&config.endpoint_path, mcp_service)
        .layer(axum::middleware::from_fn_with_state(
            auth_config.clone(),
            super::auth::auth_middleware,
        ))
        .layer(tower::limit::ConcurrencyLimitLayer::new(max_concurrent))
        .layer(cors)
        .route("/health", axum::routing::get(health_handler))
        .route(
            "/.well-known/nucleus-pod",
            axum::routing::get(nucleus_pod_handler),
        )
        .route("/didcomm", axum::routing::post(didcomm_receive_handler))
        .route(
            "/auth/info",
            axum::routing::get(move || {
                let ac = auth_config.clone();
                async move { auth_info_handler(ac).await }
            }),
        )
        .with_state(didcomm_state);

    let listener = tokio::net::TcpListener::bind(config.listen_addr)
        .await
        .map_err(|e| format!("failed to bind {}: {e}", config.listen_addr))?;

    eprintln!(
        "NucleusDB MCP server listening on http://{}{}",
        config.listen_addr, config.endpoint_path
    );
    eprintln!(
        "  Auth: {}",
        if config.auth.enabled {
            "enabled (CAB + OAuth)"
        } else {
            "disabled (dev mode)"
        }
    );
    eprintln!("  Health: http://{}/health", config.listen_addr);
    eprintln!(
        "  Discovery: http://{}/.well-known/nucleus-pod",
        config.listen_addr
    );
    eprintln!("  DIDComm: http://{}/didcomm", config.listen_addr);

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}

/// Receive a DIDComm v2 encrypted envelope from a mesh peer.
///
/// Dispatches based on message type:
/// - McpToolCall → executes local MCP tool via internal JSON-RPC dispatch
/// - EnvelopeExchange → accepts proof envelope
/// - CapabilityGrant → accepts capability
/// - Heartbeat → returns ack
///
/// The envelope is verified and decrypted using the local agent's DID identity.
/// If no identity is loaded (e.g., no NUCLEUSDB_AGENT_PRIVATE_KEY), returns 503.
async fn didcomm_receive_handler(
    axum::extract::State(state): axum::extract::State<DidcommRouteState>,
    axum::Json(envelope): axum::Json<crate::comms::didcomm::DIDCommEnvelope>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    use crate::comms::didcomm::{decrypt_message, encrypt_message, DIDCommMessage, MessageType};

    // Prefer startup-captured identity and fallback to env only if unavailable.
    let identity = match state
        .agent_identity
        .clone()
        .map(Ok)
        .unwrap_or_else(|| load_agent_identity().map(Arc::new))
    {
        Ok(id) => id,
        Err(e) => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                axum::Json(serde_json::json!({
                    "error": format!("agent identity not available: {e}"),
                })),
            );
        }
    };

    // Resolve sender's DID document. For now, use a simple in-process resolver
    // that looks up mesh peers. In production this would query a DID registry.
    let sender_did = envelope.sender_did.clone();
    let registry_path = state.mesh_registry_path.clone();
    let sender_doc =
        match tokio::task::spawn_blocking(move || resolve_sender_did(&sender_did, &registry_path))
            .await
        {
            Ok(Ok(doc)) => doc,
            Ok(Err(e)) => {
                return (
                    axum::http::StatusCode::BAD_REQUEST,
                    axum::Json(serde_json::json!({
                        "error": format!("cannot resolve sender DID: {e}"),
                    })),
                );
            }
            Err(e) => {
                return (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(serde_json::json!({
                        "error": format!("sender DID resolution task failed: {e}"),
                    })),
                );
            }
        };

    // Decrypt and verify in a blocking worker so heavyweight crypto does not
    // consume async runtime stack budget.
    let decrypt_identity = identity.clone();
    let decrypt_sender_doc = sender_doc.clone();
    let message = match tokio::task::spawn_blocking(move || {
        decrypt_message(decrypt_identity.as_ref(), &decrypt_sender_doc, &envelope)
    })
    .await
    {
        Ok(Ok(msg)) => msg,
        Ok(Err(e)) => {
            return (
                axum::http::StatusCode::FORBIDDEN,
                axum::Json(serde_json::json!({
                    "error": format!("envelope verification/decryption failed: {e}"),
                })),
            );
        }
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                axum::Json(serde_json::json!({
                    "error": format!("decrypt task failed: {e}"),
                })),
            );
        }
    };

    if message.is_expired() {
        return (
            axum::http::StatusCode::GONE,
            axum::Json(serde_json::json!({
                "error": "message expired",
            })),
        );
    }

    // Dispatch based on message type and return encrypted DIDComm response.
    let response_envelope = match message.type_ {
        MessageType::Heartbeat => {
            let reply = DIDCommMessage {
                id: crate::comms::envelope::generate_message_id(),
                type_: MessageType::Heartbeat,
                from: identity.did.clone(),
                to: vec![sender_doc.id.clone()],
                created_time: crate::pod::now_unix(),
                expires_time: Some(crate::pod::now_unix() + 300),
                body: serde_json::json!({
                    "status": "ack",
                    "reply_to": message.id,
                    "agent_id": identity.did.clone(),
                }),
                thid: Some(message.id.clone()),
                pthid: message.thid.clone(),
            };
            match encrypt_message(identity.as_ref(), &sender_doc, &reply) {
                Ok(envelope) => envelope,
                Err(e) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({
                            "error": format!("encrypt heartbeat reply failed: {e}"),
                        })),
                    );
                }
            }
        }
        MessageType::EnvelopeExchange => {
            let reply = DIDCommMessage {
                id: crate::comms::envelope::generate_message_id(),
                type_: MessageType::EnvelopeExchange,
                from: identity.did.clone(),
                to: vec![sender_doc.id.clone()],
                created_time: crate::pod::now_unix(),
                expires_time: Some(crate::pod::now_unix() + 300),
                body: serde_json::json!({
                    "status": "accepted",
                    "reply_to": message.id,
                    "message_type": "envelope_exchange",
                }),
                thid: Some(message.id.clone()),
                pthid: message.thid.clone(),
            };
            match encrypt_message(identity.as_ref(), &sender_doc, &reply) {
                Ok(envelope) => envelope,
                Err(e) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({
                            "error": format!("encrypt envelope-exchange reply failed: {e}"),
                        })),
                    );
                }
            }
        }
        MessageType::CapabilityGrant => {
            let capability_result = persist_received_capability_grant(&identity.did, &message.body);
            let reply = DIDCommMessage {
                id: crate::comms::envelope::generate_message_id(),
                type_: MessageType::CapabilityAccept,
                from: identity.did.clone(),
                to: vec![sender_doc.id.clone()],
                created_time: crate::pod::now_unix(),
                expires_time: Some(crate::pod::now_unix() + 300),
                body: match capability_result {
                    Ok(payload) => payload,
                    Err(error) => serde_json::json!({
                        "status": "rejected",
                        "reply_to": message.id,
                        "message_type": "capability_grant",
                        "error": error,
                    }),
                },
                thid: Some(message.id.clone()),
                pthid: message.thid.clone(),
            };
            match encrypt_message(identity.as_ref(), &sender_doc, &reply) {
                Ok(envelope) => envelope,
                Err(e) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({
                            "error": format!("encrypt capability reply failed: {e}"),
                        })),
                    );
                }
            }
        }
        MessageType::McpToolCall => {
            let tool_name = match message.body.get("tool_name").and_then(|v| v.as_str()) {
                Some(name) if !name.trim().is_empty() => name,
                _ => {
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({
                            "error": "McpToolCall body missing non-empty tool_name",
                        })),
                    );
                }
            };
            let arguments = message
                .body
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            let result_payload =
                match authorize_didcomm_tool_call(&identity.did, &message.from, tool_name) {
                    Ok(()) => match dispatch_mcp_tool_call(&state, tool_name, arguments).await {
                        Ok(result) => serde_json::json!({
                            "status": "completed",
                            "reply_to": message.id,
                            "message_type": "mcp_tool_call",
                            "tool_name": tool_name,
                            "result": result,
                        }),
                        Err(e) => serde_json::json!({
                            "status": "failed",
                            "reply_to": message.id,
                            "message_type": "mcp_tool_call",
                            "tool_name": tool_name,
                            "error": format!("local MCP dispatch failed: {e}"),
                        }),
                    },
                    Err(e) => serde_json::json!({
                        "status": "forbidden",
                        "reply_to": message.id,
                        "message_type": "mcp_tool_call",
                        "tool_name": tool_name,
                        "error": e,
                    }),
                };
            match crate::comms::envelope::wrap_mcp_response(
                identity.as_ref(),
                &sender_doc,
                tool_name,
                result_payload,
                Some(&message.id),
            ) {
                Ok(envelope) => envelope,
                Err(e) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({
                            "error": format!("encrypt McpToolResponse failed: {e}"),
                        })),
                    );
                }
            }
        }
        other => {
            let reply = DIDCommMessage {
                id: crate::comms::envelope::generate_message_id(),
                type_: other.clone(),
                from: identity.did.clone(),
                to: vec![sender_doc.id.clone()],
                created_time: crate::pod::now_unix(),
                expires_time: Some(crate::pod::now_unix() + 300),
                body: serde_json::json!({
                    "status": "received",
                    "reply_to": message.id,
                    "message_type": format!("{other:?}"),
                }),
                thid: Some(message.id.clone()),
                pthid: message.thid.clone(),
            };
            match encrypt_message(identity.as_ref(), &sender_doc, &reply) {
                Ok(envelope) => envelope,
                Err(e) => {
                    return (
                        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                        axum::Json(serde_json::json!({
                            "error": format!("encrypt DIDComm reply failed: {e}"),
                        })),
                    );
                }
            }
        }
    };

    match serde_json::to_value(response_envelope) {
        Ok(value) => (axum::http::StatusCode::OK, axum::Json(value)),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({
                "error": format!("serialize DIDComm response envelope failed: {e}"),
            })),
        ),
    }
}

async fn dispatch_mcp_tool_call(
    state: &DidcommRouteState,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let db_path = state.db_path.clone();
    let tool_name = tool_name.to_string();
    let task = tokio::task::spawn_blocking(move || {
        let worker = std::thread::Builder::new()
            .name("didcomm-mcp-dispatch".to_string())
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("build local dispatch runtime: {e}"))?;
                runtime.block_on(dispatch_mcp_tool_call_inner(
                    &db_path, &tool_name, arguments,
                ))
            })
            .map_err(|e| format!("spawn DIDComm dispatch worker: {e}"))?;
        worker
            .join()
            .map_err(|_| "DIDComm dispatch worker panicked".to_string())?
    });

    task.await
        .map_err(|e| format!("join DIDComm dispatch task: {e}"))?
}

async fn dispatch_mcp_tool_call_inner(
    db_path: &str,
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, String> {
    fn parse_args<T: serde::de::DeserializeOwned>(
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<T, String> {
        serde_json::from_value(arguments)
            .map_err(|e| format!("deserialize arguments for {tool_name}: {e}"))
    }

    let service = NucleusDbMcpService::new(db_path)
        .map_err(|e| format!("construct local MCP service: {e}"))?;
    match tool_name {
        "nucleusdb_help" => {
            let rmcp::Json(response) = service
                .help()
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response).map_err(|e| format!("serialize help response: {e}"))
        }
        "nucleusdb_status" => {
            let rmcp::Json(response) = service
                .status()
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response).map_err(|e| format!("serialize status response: {e}"))
        }
        "nucleusdb_query" => {
            let req: QueryRequest = parse_args(tool_name, arguments)?;
            let rmcp::Json(response) = service
                .query(rmcp::handler::server::wrapper::Parameters(req))
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response).map_err(|e| format!("serialize query response: {e}"))
        }
        "nucleusdb_query_range" => {
            let req: QueryRangeRequest = parse_args(tool_name, arguments)?;
            let rmcp::Json(response) = service
                .query_range(rmcp::handler::server::wrapper::Parameters(req))
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response)
                .map_err(|e| format!("serialize query_range response: {e}"))
        }
        "nucleusdb_verify" => {
            let req: VerifyRequest = parse_args(tool_name, arguments)?;
            let rmcp::Json(response) = service
                .verify(rmcp::handler::server::wrapper::Parameters(req))
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response).map_err(|e| format!("serialize verify response: {e}"))
        }
        "nucleusdb_history" => {
            let req: HistoryRequest = parse_args(tool_name, arguments)?;
            let rmcp::Json(response) = service
                .history(rmcp::handler::server::wrapper::Parameters(req))
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response).map_err(|e| format!("serialize history response: {e}"))
        }
        "nucleusdb_export" => {
            let req: ExportRequest = parse_args(tool_name, arguments)?;
            let rmcp::Json(response) = service
                .export(rmcp::handler::server::wrapper::Parameters(req))
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response).map_err(|e| format!("serialize export response: {e}"))
        }
        "nucleusdb_execute_sql" => {
            let req: ExecuteSqlRequest = parse_args(tool_name, arguments)?;
            let rmcp::Json(response) = service
                .execute_sql(rmcp::handler::server::wrapper::Parameters(req))
                .await
                .map_err(|e| format!("{e:?}"))?;
            serde_json::to_value(response)
                .map_err(|e| format!("serialize execute_sql response: {e}"))
        }
        other => Err(format!(
            "DIDComm MCP dispatch currently supports tools: nucleusdb_help, nucleusdb_status, nucleusdb_query, nucleusdb_query_range, nucleusdb_verify, nucleusdb_history, nucleusdb_export, nucleusdb_execute_sql (got `{other}`)"
        )),
    }
}

/// Load the local agent's DID identity from environment-configured seed.
fn load_agent_identity() -> Result<crate::halo::did::DIDIdentity, String> {
    let key_hex = std::env::var("NUCLEUSDB_AGENT_PRIVATE_KEY")
        .map_err(|_| "NUCLEUSDB_AGENT_PRIVATE_KEY not set".to_string())?;
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
    crate::halo::did::did_from_genesis_seed(&seed)
}

fn decode_did_key_ed25519_public(did: &str) -> Result<[u8; 32], String> {
    let encoded = did
        .strip_prefix(DID_KEY_PREFIX)
        .ok_or_else(|| "DID is not a did:key identifier".to_string())?;
    let (_, decoded) = multibase::decode(encoded)
        .map_err(|e| format!("multibase decode failed for did:key identifier: {e}"))?;
    if decoded.len() != MULTICODEC_ED25519_PUB.len() + 32 {
        return Err("did:key payload must be Ed25519 multicodec + 32-byte key".to_string());
    }
    if !decoded.starts_with(MULTICODEC_ED25519_PUB) {
        return Err("did:key payload must use Ed25519 multicodec prefix".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded[MULTICODEC_ED25519_PUB.len()..]);
    Ok(out)
}

fn decode_document_ed25519_public(
    did_document: &crate::halo::did::DIDDocument,
) -> Result<[u8; 32], String> {
    let method = did_document
        .verification_method
        .iter()
        .find(|method| method.type_ == TYPE_ED25519)
        .ok_or_else(|| "DID document missing Ed25519 verification method".to_string())?;
    let (_, decoded) = multibase::decode(&method.public_key_multibase)
        .map_err(|e| format!("multibase decode failed for DID Ed25519 key: {e}"))?;
    if decoded.len() != MULTICODEC_ED25519_PUB.len() + 32 {
        return Err("DID Ed25519 key must include multicodec + 32-byte key".to_string());
    }
    if !decoded.starts_with(MULTICODEC_ED25519_PUB) {
        return Err("DID Ed25519 key has unexpected multicodec prefix".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded[MULTICODEC_ED25519_PUB.len()..]);
    Ok(out)
}

fn verify_did_document_binding(
    sender_did: &str,
    did_document: &crate::halo::did::DIDDocument,
) -> Result<(), String> {
    if did_document.id != sender_did {
        return Err("sender DID document id does not match sender DID".to_string());
    }
    let did_key_ed25519 = decode_did_key_ed25519_public(sender_did)?;
    let document_ed25519 = decode_document_ed25519_public(did_document)?;
    if did_key_ed25519 != document_ed25519 {
        return Err(
            "sender DID document Ed25519 key does not match did:key identifier".to_string(),
        );
    }
    Ok(())
}

/// Resolve a sender's DID document by looking up their peer info from the mesh registry.
fn resolve_sender_did(
    sender_did: &str,
    registry_path: &str,
) -> Result<crate::halo::did::DIDDocument, String> {
    use crate::container::mesh::PeerRegistry;
    use std::path::Path;

    let registry = PeerRegistry::load(Path::new(&registry_path)).unwrap_or_default();

    if let Some(peer) = registry.find_by_did(sender_did) {
        // Discovered peer — fetch their DID document from their discovery endpoint.
        let url =
            peer.mcp_endpoint.trim_end_matches("/mcp").to_string() + "/.well-known/nucleus-pod";
        let resp =
            crate::halo::http_client::get_with_timeout(&url, std::time::Duration::from_secs(5))?
                .call()
                .map_err(|e| format!("fetch DID document from {url}: {e}"))?;
        let body: serde_json::Value = resp
            .into_body()
            .read_json()
            .map_err(|e| format!("parse DID document response: {e}"))?;
        if let Some(doc_val) = body.get("did_document") {
            let doc: crate::halo::did::DIDDocument = serde_json::from_value(doc_val.clone())
                .map_err(|e| format!("parse DID document: {e}"))?;
            verify_did_document_binding(sender_did, &doc)?;
            return Ok(doc);
        }
    }

    Err(format!(
        "cannot resolve DID document for `{sender_did}` — peer not in mesh registry"
    ))
}

fn persist_received_capability_grant(
    local_did: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let token_value = body
        .get("capability_token")
        .ok_or_else(|| "CapabilityGrant message missing capability_token".to_string())?
        .clone();
    let token: crate::pod::capability::CapabilityToken =
        serde_json::from_value(token_value).map_err(|e| format!("parse capability_token: {e}"))?;
    if token.grantee_did != local_did {
        return Err(format!(
            "capability grantee `{}` does not match local DID `{}`",
            token.grantee_did, local_did
        ));
    }
    crate::pod::capability::verify_capability(&token, crate::halo::util::now_unix_secs())?;
    let path = crate::halo::config::capability_store_path();
    let mut store = crate::pod::capability::CapabilityStore::load_or_default(&path)?;
    let exists = store.tokens.iter().any(|t| t.token_id == token.token_id);
    if !exists {
        store.create(token.clone());
        store.save(&path)?;
    }
    Ok(serde_json::json!({
        "status": "accepted",
        "message_type": "capability_grant",
        "capability_token_id": hex::encode(token.token_id),
        "already_present": exists,
    }))
}

fn authorize_didcomm_tool_call(
    local_did: &str,
    sender_did: &str,
    tool_name: &str,
) -> Result<(), String> {
    if sender_did == local_did {
        return Ok(());
    }
    let path = crate::halo::config::capability_store_path();
    let store = crate::pod::capability::CapabilityStore::load_or_default(&path)?;
    let now = crate::halo::util::now_unix_secs();
    if crate::pod::capability::store_authorizes_tool_call(&store, sender_did, tool_name, now) {
        return Ok(());
    }
    Err(format!(
        "DIDComm authorization denied for sender `{sender_did}` on tool `{tool_name}`"
    ))
}

async fn health_handler() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "status": "ok",
        "server": "nucleusdb-mcp",
        "version": env!("CARGO_PKG_VERSION"),
        "transport": "streamable-http",
        "protocol": "mcp/2025-03-26"
    }))
}

async fn nucleus_pod_handler(
    axum::extract::State(state): axum::extract::State<DidcommRouteState>,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    match load_agent_identity() {
        Ok(identity) => (
            axum::http::StatusCode::OK,
            axum::Json(serde_json::json!({
                "agent_id": std::env::var("NUCLEUSDB_MESH_AGENT_ID").unwrap_or_default(),
                "agent_did": identity.did,
                "did_document": identity.did_document,
                "mcp_endpoint": state.mcp_endpoint,
            })),
        ),
        Err(e) => (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "error": format!("agent identity not available: {e}"),
            })),
        ),
    }
}

async fn auth_info_handler(config: Arc<AuthConfig>) -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({
        "auth_enabled": config.enabled,
        "methods": {
            "cab": {
                "description": "CAB-as-bearer-token: Hardware-anchored agent identity",
                "header": "Authorization: Bearer cab:<base64(json)>",
                "payload_fields": ["agent_address", "contract_address", "rpc_url"],
                "tier_scopes": {
                    "1_consumer": ["read", "trust:verify"],
                    "2_server": ["read", "trust:verify"],
                    "3_server_tpm": ["read", "trust:verify", "write"],
                    "4_dgx": ["read", "trust:verify", "write", "trust:attest", "container"],
                }
            },
            "oauth": {
                "description": "OAuth 2.1 JWT: Standard bearer token",
                "header": "Authorization: Bearer <jwt>",
                "algorithm": "HS256",
                "required_claims": ["sub"],
                "scope_claim": "scope (space-separated)",
                "available_scopes": ["read", "trust:verify", "write", "trust:attest", "container"],
            }
        },
        "tool_scopes": {
            "read": [
                "nucleusdb_help", "nucleusdb_status", "nucleusdb_query",
                "nucleusdb_query_range", "nucleusdb_verify", "nucleusdb_export",
                "nucleusdb_history", "abraxas_query_records", "abraxas_record_status",
                "abraxas_merge_status", "abraxas_workspace_diff", "mesh_peers", "mesh_ping"
            ],
            "trust:verify": [
                "nucleusdb_verify_agent", "verify_agent_multichain", "register_chain"
            ],
            "write": [
                "nucleusdb_execute_sql", "nucleusdb_create_database",
                "nucleusdb_open_database", "nucleusdb_checkpoint",
                "abraxas_submit_record", "abraxas_resolve_conflict",
                "abraxas_export_git", "abraxas_workspace_init", "abraxas_workspace_submit",
                "mesh_call", "mesh_exchange_envelope"
            ],
            "trust:attest": [
                "nucleusdb_agent_register", "submit_composite_attestation", "mesh_grant"
            ],
            "container": [
                "nucleusdb_container_launch",
                "nucleusdb_container_list",
                "nucleusdb_container_status",
                "nucleusdb_container_stop",
                "nucleusdb_container_logs"
            ]
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::mesh::{PeerInfo, PeerRegistry};
    use crate::test_support::env_lock;
    use axum::{routing::get, Json, Router};
    use std::time::Duration;
    use tempfile::tempdir;

    fn random_local_addr() -> SocketAddr {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind ephemeral port")
            .local_addr()
            .expect("local addr")
    }

    async fn start_discovery_server(
        addr: SocketAddr,
        did_document: crate::halo::did::DIDDocument,
    ) -> tokio::task::JoinHandle<()> {
        let app = Router::new().route(
            "/.well-known/nucleus-pod",
            get(move || {
                let doc = did_document.clone();
                async move { Json(serde_json::json!({ "did_document": doc })) }
            }),
        );
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .expect("bind discovery server");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        })
    }

    #[test]
    fn default_config_is_sane() {
        let config = RemoteServerConfig::default();
        assert_eq!(config.endpoint_path, "/mcp");
        assert!(config.auth.enabled);
        assert_eq!(config.listen_addr.port(), 3000);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn didcomm_mcp_tool_call_executes_local_tool_under_default_auth() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let workspace = tempdir().expect("tempdir");
        let db_path = workspace.path().join("mesh_didcomm_exec.ndb");
        let registry_path = workspace.path().join("peers.json");
        std::env::set_var("AGENTHALO_HOME", workspace.path().display().to_string());

        let alice_seed = [0x41u8; 64];
        let bob_seed = [0x42u8; 64];
        let alice = crate::halo::did::did_from_genesis_seed(&alice_seed).expect("alice identity");
        let bob = crate::halo::did::did_from_genesis_seed(&bob_seed).expect("bob identity");

        let discovery_addr = random_local_addr();
        let discovery_handle =
            start_discovery_server(discovery_addr, alice.did_document.clone()).await;

        let mut registry = PeerRegistry::new();
        let now = crate::pod::now_unix();
        registry.register(PeerInfo {
            agent_id: "agent-alice".to_string(),
            container_name: "alice".to_string(),
            did_uri: Some(alice.did.clone()),
            mcp_endpoint: format!("http://{}/mcp", discovery_addr),
            discovery_endpoint: format!("http://{}/.well-known/nucleus-pod", discovery_addr),
            registered_at: now,
            last_seen: now,
        });
        registry.save(&registry_path).expect("save mesh registry");

        std::env::set_var(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.display().to_string(),
        );
        std::env::set_var("NUCLEUSDB_AGENT_PRIVATE_KEY", hex::encode(bob_seed));
        std::env::remove_var("NUCLEUSDB_INTERNAL_AUTH_KEY");

        let now = crate::halo::util::now_unix_secs();
        let token = crate::pod::capability::create_capability(
            &bob,
            &alice.did,
            crate::pod::capability::AgentClass::Specific {
                did_uri: alice.did.clone(),
            },
            &["nucleusdb_*".to_string()],
            &[crate::pod::capability::AccessMode::Read],
            now.saturating_sub(10),
            now.saturating_add(600),
            false,
        )
        .expect("create capability token");
        let mut cap_store = crate::pod::capability::CapabilityStore::load_or_default(
            &crate::halo::config::capability_store_path(),
        )
        .expect("load capability store");
        cap_store.create(token);
        cap_store
            .save(&crate::halo::config::capability_store_path())
            .expect("save capability store");

        let listen_addr = random_local_addr();
        let server_handle = tokio::spawn(run_remote_mcp_server(RemoteServerConfig {
            db_path: db_path.display().to_string(),
            listen_addr,
            auth: AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        }));
        tokio::time::sleep(Duration::from_millis(200)).await;

        let envelope = crate::comms::envelope::wrap_mcp_call(
            &alice,
            &bob.did_document,
            "nucleusdb_status",
            serde_json::json!({}),
        )
        .expect("wrap didcomm call");

        let didcomm_url = format!("http://{listen_addr}/didcomm");
        let response: serde_json::Value =
            tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
                crate::halo::http_client::post_with_timeout(&didcomm_url, Duration::from_secs(20))
                    .map_err(|e| format!("didcomm request builder: {e}"))?
                    .send_json(&envelope)
                    .map_err(|e| format!("send didcomm envelope: {e}"))?
                    .into_body()
                    .read_json()
                    .map_err(|e| format!("parse didcomm response: {e}"))
            })
            .await
            .expect("join didcomm request task")
            .expect("didcomm request must succeed");

        let response_envelope: crate::comms::didcomm::DIDCommEnvelope =
            serde_json::from_value(response).expect("response should be DIDComm envelope");
        let (response_tool, response_payload) = crate::comms::envelope::unwrap_mcp_response(
            &alice,
            &bob.did_document,
            &response_envelope,
        )
        .expect("unwrap mcp response");
        assert_eq!(response_tool, "nucleusdb_status");
        assert_eq!(response_payload["status"], "completed");
        assert_eq!(response_payload["tool_name"], "nucleusdb_status");
        assert!(response_payload.get("result").is_some());

        server_handle.abort();
        discovery_handle.abort();
        std::env::remove_var("AGENTHALO_HOME");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn didcomm_mcp_tool_call_without_capability_returns_forbidden_payload() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let workspace = tempdir().expect("tempdir");
        let db_path = workspace.path().join("mesh_didcomm_forbidden.ndb");
        let registry_path = workspace.path().join("peers.json");
        std::env::set_var("AGENTHALO_HOME", workspace.path().display().to_string());

        let alice_seed = [0x44u8; 64];
        let bob_seed = [0x45u8; 64];
        let alice = crate::halo::did::did_from_genesis_seed(&alice_seed).expect("alice identity");
        let bob = crate::halo::did::did_from_genesis_seed(&bob_seed).expect("bob identity");

        let discovery_addr = random_local_addr();
        let discovery_handle =
            start_discovery_server(discovery_addr, alice.did_document.clone()).await;

        let mut registry = PeerRegistry::new();
        let now = crate::pod::now_unix();
        registry.register(PeerInfo {
            agent_id: "agent-alice".to_string(),
            container_name: "alice".to_string(),
            did_uri: Some(alice.did.clone()),
            mcp_endpoint: format!("http://{}/mcp", discovery_addr),
            discovery_endpoint: format!("http://{}/.well-known/nucleus-pod", discovery_addr),
            registered_at: now,
            last_seen: now,
        });
        registry.save(&registry_path).expect("save mesh registry");

        std::env::set_var(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.display().to_string(),
        );
        std::env::set_var("NUCLEUSDB_AGENT_PRIVATE_KEY", hex::encode(bob_seed));
        std::env::remove_var("NUCLEUSDB_INTERNAL_AUTH_KEY");

        let listen_addr = random_local_addr();
        let server_handle = tokio::spawn(run_remote_mcp_server(RemoteServerConfig {
            db_path: db_path.display().to_string(),
            listen_addr,
            auth: AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        }));
        tokio::time::sleep(Duration::from_millis(200)).await;

        let envelope = crate::comms::envelope::wrap_mcp_call(
            &alice,
            &bob.did_document,
            "nucleusdb_status",
            serde_json::json!({}),
        )
        .expect("wrap didcomm call");

        let didcomm_url = format!("http://{listen_addr}/didcomm");
        let response: serde_json::Value =
            tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
                crate::halo::http_client::post_with_timeout(&didcomm_url, Duration::from_secs(20))
                    .map_err(|e| format!("didcomm request builder: {e}"))?
                    .send_json(&envelope)
                    .map_err(|e| format!("send didcomm envelope: {e}"))?
                    .into_body()
                    .read_json()
                    .map_err(|e| format!("parse didcomm response: {e}"))
            })
            .await
            .expect("join didcomm request task")
            .expect("didcomm request must succeed");
        let response_envelope: crate::comms::didcomm::DIDCommEnvelope =
            serde_json::from_value(response).expect("response should be DIDComm envelope");
        let (response_tool, response_payload) = crate::comms::envelope::unwrap_mcp_response(
            &alice,
            &bob.did_document,
            &response_envelope,
        )
        .expect("unwrap mcp response");
        assert_eq!(response_tool, "nucleusdb_status");
        assert_eq!(response_payload["status"], "forbidden");
        assert!(response_payload["error"]
            .as_str()
            .unwrap_or_default()
            .contains("authorization denied"));

        server_handle.abort();
        discovery_handle.abort();
        std::env::remove_var("AGENTHALO_HOME");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn didcomm_sender_did_binding_mismatch_is_rejected() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let workspace = tempdir().expect("tempdir");
        let db_path = workspace.path().join("mesh_didcomm_binding.ndb");
        let registry_path = workspace.path().join("peers.json");

        let alice_seed = [0x51u8; 64];
        let bob_seed = [0x52u8; 64];
        let mismatch_seed = [0x53u8; 64];
        let alice = crate::halo::did::did_from_genesis_seed(&alice_seed).expect("alice identity");
        let bob = crate::halo::did::did_from_genesis_seed(&bob_seed).expect("bob identity");
        let mismatch =
            crate::halo::did::did_from_genesis_seed(&mismatch_seed).expect("mismatch identity");

        let discovery_addr = random_local_addr();
        let discovery_handle =
            start_discovery_server(discovery_addr, mismatch.did_document.clone()).await;

        let mut registry = PeerRegistry::new();
        let now = crate::pod::now_unix();
        registry.register(PeerInfo {
            agent_id: "agent-alice".to_string(),
            container_name: "alice".to_string(),
            did_uri: Some(alice.did.clone()),
            mcp_endpoint: format!("http://{}/mcp", discovery_addr),
            discovery_endpoint: format!("http://{}/.well-known/nucleus-pod", discovery_addr),
            registered_at: now,
            last_seen: now,
        });
        registry.save(&registry_path).expect("save mesh registry");

        std::env::set_var(
            "NUCLEUSDB_MESH_REGISTRY",
            registry_path.display().to_string(),
        );
        std::env::set_var("NUCLEUSDB_AGENT_PRIVATE_KEY", hex::encode(bob_seed));
        std::env::remove_var("NUCLEUSDB_INTERNAL_AUTH_KEY");

        let listen_addr = random_local_addr();
        let server_handle = tokio::spawn(run_remote_mcp_server(RemoteServerConfig {
            db_path: db_path.display().to_string(),
            listen_addr,
            auth: AuthConfig::default(),
            endpoint_path: "/mcp".to_string(),
        }));
        tokio::time::sleep(Duration::from_millis(200)).await;

        let envelope = crate::comms::envelope::wrap_mcp_call(
            &alice,
            &bob.did_document,
            "nucleusdb_status",
            serde_json::json!({}),
        )
        .expect("wrap didcomm call");

        let didcomm_url = format!("http://{listen_addr}/didcomm");
        let err = tokio::task::spawn_blocking(move || {
            crate::halo::http_client::post_with_timeout(&didcomm_url, Duration::from_secs(20))
                .expect("didcomm request builder")
                .send_json(&envelope)
                .expect_err("mismatched did-document binding must fail")
        })
        .await
        .expect("join didcomm request task");
        match err {
            ureq::Error::StatusCode(code) => assert_eq!(code, 400),
            other => panic!("expected HTTP status error, got {other:?}"),
        }

        server_handle.abort();
        discovery_handle.abort();
    }
}
