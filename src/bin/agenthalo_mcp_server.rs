use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use bip39::{Language, Mnemonic};
use nucleusdb::container::{deregister_self_from_mesh, mesh_enabled, register_self_in_mesh};
use nucleusdb::halo::addons;
use nucleusdb::halo::agent_auth;
use nucleusdb::halo::agentpmt;
use nucleusdb::halo::attest::{
    attest_session, resolve_session_id, save_attestation, AttestationRequest,
};
use nucleusdb::halo::audit::{
    audit_contract_file, audit_contract_source, save_audit_result, AuditRequest, AuditSize,
};
use nucleusdb::halo::auth::{load_credentials, save_credentials};
use nucleusdb::halo::circuit::{
    load_or_setup_attestation_keys_with_policy, proof_words_json_array, prove_attestation,
    public_inputs_json_array, verify_attestation_proof,
};
use nucleusdb::halo::config;
use nucleusdb::halo::crypto_scope::CryptoScope;
use nucleusdb::halo::encrypted_file;
use nucleusdb::halo::http_client;
use nucleusdb::halo::migration;
use nucleusdb::halo::nym;
use nucleusdb::halo::onchain::{
    load_onchain_config_or_default, onchain_simulation_enabled, post_attestation,
    warn_if_simulation_mode,
};
use nucleusdb::halo::password;
use nucleusdb::halo::pq::{has_wallet, sign_pq_payload};
use nucleusdb::halo::privacy_controller;
use nucleusdb::halo::session_manager::SessionManager;
use nucleusdb::halo::trace::{
    list_sessions, now_unix_secs, record_paid_operation_for_halo, session_summary,
};
use nucleusdb::halo::trust::query_trust_score;
use nucleusdb::halo::util::digest_json;
use nucleusdb::halo::viewer;
use nucleusdb::halo::x402;
use nucleusdb::halo::zk_compute;
use nucleusdb::halo::zk_credential;
use nucleusdb::pod::access_policy::{AccessContext, PolicyStore};
use nucleusdb::pod::capability::{self, AccessMode, CapabilityStore};
use nucleusdb::verifier::gate as proof_gate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use zeroize::{Zeroize, Zeroizing};

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
    warn_if_simulation_mode();
    let port: u16 = std::env::var("AGENTHALO_MCP_PORT")
        .ok()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(8390);
    let host = std::env::var("AGENTHALO_MCP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let secret = resolve_mcp_secret()?;

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|e| format!("invalid bind address {host}:{port}: {e}"))?;

    let state = Arc::new(AppState { secret });

    let mesh_registered = if mesh_enabled() {
        match register_self_in_mesh() {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[mesh] registration failed: {e}");
                false
            }
        }
    } else {
        false
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/mcp", post(mcp))
        .with_state(state);

    println!("agenthalo-mcp-server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind listener {addr}: {e}"))?;
    let serve_res = axum::serve(listener, app)
        .await
        .map_err(|e| format!("serve axum: {e}"));
    if mesh_registered {
        deregister_self_from_mesh();
    }
    serve_res?;
    Ok(())
}

fn is_truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn resolve_mcp_secret() -> Result<String, String> {
    if let Ok(secret) = std::env::var("AGENTHALO_MCP_SECRET") {
        let trimmed = secret.trim();
        if trimmed.is_empty() {
            return Err(
                "AGENTHALO_MCP_SECRET is set but empty; provide a non-empty bearer secret"
                    .to_string(),
            );
        }
        return Ok(trimmed.to_string());
    }

    if is_truthy_env("AGENTHALO_ALLOW_DEV_SECRET") {
        eprintln!(
            "warning: using AGENTHALO_ALLOW_DEV_SECRET=1 fallback secret; localhost dev only"
        );
        return Ok("agenthalo-dev-secret".to_string());
    }

    Err(
        "AGENTHALO_MCP_SECRET is required. Set it (for example: `export AGENTHALO_MCP_SECRET=$(openssl rand -hex 32)`). For localhost-only dev fallback, set AGENTHALO_ALLOW_DEV_SECRET=1.".to_string(),
    )
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
                    "description": "Submit a governance vote (local ledger by default, optional on-chain transaction when submit_onchain=true).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "proposal_id": {"type": "string", "description": "Proposal identifier to vote on."},
                            "choice": {"type": "string", "enum": ["yes", "no", "abstain"], "description": "Vote choice."},
                            "reason": {"type": "string", "description": "Optional justification for the vote."},
                            "submit_onchain": {"type": "boolean", "description": "When true, submit the vote as an on-chain transaction. Defaults to false (local ledger only).", "default": false},
                            "rpc_url": {"type": "string", "description": "Override RPC endpoint for on-chain submission."},
                            "contract_address": {"type": "string", "description": "Override contract address for on-chain submission."},
                            "private_key_env": {"type": "string", "description": "Environment variable name holding the private key for on-chain signing."},
                            "function_signature": {"type": "string", "description": "Override Solidity function signature for the on-chain call."}
                        },
                        "required": ["proposal_id", "choice"]
                    }
                }),
                json!({
                    "name": "sync",
                    "description": "Execute session sync by creating a signed sync artifact and optionally pushing to an HTTP endpoint.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "target": {"type": "string", "description": "Sync target (e.g. 'cloudflare').", "default": "cloudflare"}
                        }
                    }
                }),
                json!({
                    "name": "privacy_pool_create",
                    "description": "Create a privacy pool workflow record and optionally execute an on-chain create transaction.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "chain": {"type": "string", "description": "Target chain.", "default": "base-sepolia"},
                            "asset": {"type": "string", "description": "Token asset.", "default": "USDC"},
                            "denomination": {"type": "integer", "description": "Pool denomination in token base units."},
                            "submit_onchain": {"type": "boolean", "description": "When true, submit the transaction on-chain. Defaults to false (local workflow ledger only).", "default": false},
                            "rpc_url": {"type": "string", "description": "Override RPC endpoint for on-chain submission."},
                            "contract_address": {"type": "string", "description": "Override contract address for on-chain submission."},
                            "private_key_env": {"type": "string", "description": "Environment variable name holding the private key for on-chain signing."},
                            "function_signature": {"type": "string", "description": "Override Solidity function signature for the on-chain call."}
                        },
                        "required": ["denomination"]
                    }
                }),
                json!({
                    "name": "privacy_pool_withdraw",
                    "description": "Execute a privacy pool withdrawal workflow and optionally submit the withdrawal on-chain.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pool_id": {"type": "string", "description": "Pool identifier to withdraw from."},
                            "recipient": {"type": "string", "description": "Recipient address."},
                            "amount": {"type": "integer", "description": "Amount to withdraw in token base units.", "default": 1},
                            "submit_onchain": {"type": "boolean", "description": "When true, submit the transaction on-chain. Defaults to false (local workflow ledger only).", "default": false},
                            "rpc_url": {"type": "string", "description": "Override RPC endpoint for on-chain submission."},
                            "contract_address": {"type": "string", "description": "Override contract address for on-chain submission."},
                            "private_key_env": {"type": "string", "description": "Environment variable name holding the private key for on-chain signing."},
                            "function_signature": {"type": "string", "description": "Override Solidity function signature for the on-chain call."}
                        },
                        "required": ["pool_id", "recipient"]
                    }
                }),
                json!({
                    "name": "pq_bridge_transfer",
                    "description": "Execute a PQ bridge transfer workflow and optionally submit a cross-chain bridge transaction.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "from_chain": {"type": "string", "description": "Source chain identifier."},
                            "from": {"type": "string", "description": "Legacy alias for from_chain."},
                            "to_chain": {"type": "string", "description": "Destination chain identifier."},
                            "to": {"type": "string", "description": "Legacy alias for to_chain."},
                            "asset": {"type": "string", "description": "Token asset to transfer."},
                            "amount": {"type": "integer", "description": "Amount in token base units."},
                            "recipient": {"type": "string", "description": "Recipient address on destination chain."},
                            "submit_onchain": {"type": "boolean", "description": "When true, submit the transaction on-chain. Defaults to false (local workflow ledger only).", "default": false},
                            "rpc_url": {"type": "string", "description": "Override RPC endpoint for on-chain submission."},
                            "contract_address": {"type": "string", "description": "Override contract address for on-chain submission."},
                            "private_key_env": {"type": "string", "description": "Environment variable name holding the private key for on-chain signing."},
                            "function_signature": {"type": "string", "description": "Override Solidity function signature for the on-chain call."}
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
                json!({
                    "name": "identity_status",
                    "description": "Return profile identity, social-login projection, and super-secure state from the immutable identity category.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "profile_get",
                    "description": "Return current profile state (display name, avatar metadata, revision/lock state).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "profile_set",
                    "description": "Update profile fields and append immutable profile update ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "display_name": {"type": "string"},
                            "avatar_type": {"type": "string"},
                            "avatar_data": {"type": "string"},
                            "rename": {"type": "boolean", "default": false}
                        }
                    }
                }),
                json!({
                    "name": "identity_device_scan",
                    "description": "Collect local device identity components and entropy tiers (read-only scan).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "identity_device_save",
                    "description": "Persist selected device identity components and append immutable identity ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "browser_fingerprint": {"type": "string"},
                            "selected_components": {"type": "array", "items": {"type": "string"}}
                        }
                    }
                }),
                json!({
                    "name": "identity_network_probe",
                    "description": "Probe local network identity hints (local IP and MAC where available).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "identity_network_save",
                    "description": "Persist network sharing preferences and hashed identifiers; append immutable identity ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "share_local_ip": {"type": "boolean", "default": false},
                            "share_public_ip": {"type": "boolean", "default": false},
                            "share_mac": {"type": "boolean", "default": false},
                            "local_ip": {"type": "string"},
                            "public_ip": {"type": "string"},
                            "mac_addresses": {"type": "array", "items": {"type": "string"}}
                        }
                    }
                }),
                json!({
                    "name": "identity_tier_set",
                    "description": "Persist identity safety tier (max-safe/less-safe/low-security) and append immutable ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "tier": {"type": "string", "description": "Tier value: max-safe|less-safe|low-security."},
                            "applied_by": {"type": "string", "description": "Audit source tag.", "default": "mcp"},
                            "step_failures": {"type": "integer", "description": "Number of best-effort steps skipped during application.", "default": 0}
                        },
                        "required": ["tier"]
                    }
                }),
                json!({
                    "name": "identity_anonymous_set",
                    "description": "Enable or disable anonymous identity mode; when enabled, clears stored device/network identity fields.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "enabled": {"type": "boolean", "description": "Set true to enable anonymous mode, false to disable."}
                        },
                        "required": ["enabled"]
                    }
                }),
                json!({
                    "name": "identity_social_connect",
                    "description": "Connect a social provider token, persist it securely, and append an immutable ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "provider": {"type": "string", "description": "Provider: google|github|microsoft|discord|apple|facebook."},
                            "token": {"type": "string", "description": "OAuth/provider token to store."},
                            "expires_in_days": {"type": "integer", "description": "Token expiry horizon in days (1..365).", "default": 30},
                            "selected": {"type": "boolean", "description": "Whether this provider is selected in identity preferences.", "default": true},
                            "source": {"type": "string", "description": "Source tag for audit trail.", "default": "mcp"}
                        },
                        "required": ["provider", "token"]
                    }
                }),
                json!({
                    "name": "identity_social_revoke",
                    "description": "Revoke a social provider token and append an immutable revoke event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "provider": {"type": "string", "description": "Provider to revoke."},
                            "reason": {"type": "string", "description": "Reason stored in the immutable ledger.", "default": "operator_requested"}
                        },
                        "required": ["provider"]
                    }
                }),
                json!({
                    "name": "identity_super_secure_set",
                    "description": "Set passkey/security-key/TOTP super-secure flags and append immutable identity ledger update.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "option": {"type": "string", "enum": ["passkey", "security_key", "totp"], "description": "Super-secure option key."},
                            "enabled": {"type": "boolean", "description": "Enable or disable the option."},
                            "label": {"type": "string", "description": "Optional TOTP label metadata."}
                        },
                        "required": ["option", "enabled"]
                    }
                }),
                json!({
                    "name": "identity_pod_share",
                    "description": "Project identity attributes into POD keyspace and return selective share payloads by key pattern (optional grant enforcement).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "key_patterns": {"type": "array", "items": {"type":"string"}, "description": "POD key patterns, e.g. ['identity/profile/*']."},
                            "include_ledger": {"type": "boolean", "description": "Include identity/ledger/* metadata.", "default": false},
                            "grantee_puf_hex": {"type": "string", "description": "32-byte hex grantee PUF used for grant enforcement."},
                            "require_grants": {"type": "boolean", "description": "If true, only return keys granted to grantee_puf_hex.", "default": false}
                        }
                    }
                }),
                json!({
                    "name": "genesis_status",
                    "description": "Return Genesis ceremony completion state, ledger summary, and sealed seed status.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "genesis_harvest",
                    "description": "Run Genesis entropy harvest and append immutable completed event when successful.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "genesis_reset",
                    "description": "Append a Genesis reset event (policy-gated; disabled by default).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "reason": {"type": "string", "description": "Optional reset reason for ledger payload.", "default": "operator_requested"}
                        }
                    }
                }),
                json!({
                    "name": "crypto_status",
                    "description": "Return cryptographic lock state, migration status, and unlocked scopes for agent operations.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "crypto_create_password",
                    "description": "Create the unified cryptographic password (or migrate v1 to v2), set verifier, and unlock scoped session.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "password": {"type": "string", "description": "New vault password."},
                            "confirm": {"type": "string", "description": "Confirmation password (defaults to password)."}
                        },
                        "required": ["password"]
                    }
                }),
                json!({
                    "name": "crypto_unlock",
                    "description": "Unlock cryptographic scopes using password-derived master key with throttling protection.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "password": {"type": "string", "description": "Configured cryptographic password."}
                        },
                        "required": ["password"]
                    }
                }),
                json!({
                    "name": "crypto_lock",
                    "description": "Lock cryptographic session and clear all scoped keys from memory.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "crypto_change_password",
                    "description": "Rotate password and re-encrypt all v2 scoped files and agent encapsulations.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "current_password": {"type": "string"},
                            "new_password": {"type": "string"},
                            "confirm": {"type": "string"}
                        },
                        "required": ["current_password", "new_password", "confirm"]
                    }
                }),
                json!({
                    "name": "agents_list",
                    "description": "List ML-KEM agent credentials currently authorized in the local credential store.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "agents_authorize",
                    "description": "Authorize a new ML-KEM agent with selected scopes and optional expiry; returns one-time secret key.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "label": {"type": "string", "description": "Human-readable label for the credential."},
                            "scopes": {"type": "array", "items": {"type": "string"}, "description": "Scopes: sign|vault|wallet|identity|genesis"},
                            "expires_days": {"type": "integer", "description": "Optional expiration in days."}
                        },
                        "required": ["label", "scopes"]
                    }
                }),
                json!({
                    "name": "agents_revoke",
                    "description": "Revoke a previously authorized agent credential.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "agent_id": {"type": "string", "description": "Credential agent_id to revoke."}
                        },
                        "required": ["agent_id"]
                    }
                }),
                json!({
                    "name": "agentaddress_status",
                    "description": "Return AgentAddress connection state and persisted public address metadata.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "agentaddress_chains",
                    "description": "List EVM-compatible chains exposed by AgentAddress integration.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "agentaddress_generate",
                    "description": "Generate AgentAddress identity externally or derive from local genesis seed, then persist/stash credentials.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "source": {"type": "string", "description": "external|genesis", "default": "external"},
                            "persist_public_address": {"type": "boolean", "description": "Persist public address in identity state.", "default": true}
                        }
                    }
                }),
                json!({
                    "name": "agentaddress_credentials",
                    "description": "Fetch locally stored AgentAddress credentials from encrypted vault. Secrets are redacted unless reveal=true is explicitly provided.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "reveal": {"type": "boolean", "description": "When true, include plaintext private_key and mnemonic. Defaults to false."},
                            "acknowledge_plaintext": {"type": "string", "description": "Required when reveal=true. Must equal I_UNDERSTAND_PLAINTEXT_RISK."}
                        }
                    }
                }),
                json!({
                    "name": "agentaddress_disconnect",
                    "description": "Disconnect persisted AgentAddress identity metadata and append immutable ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "wallet_status",
                    "description": "Return WDK wallet availability, encrypted-seed presence, sidecar state, and lightweight status for agent use.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "wallet_create",
                    "description": "Create a new self-custodial WDK wallet, encrypt seed-at-rest with passphrase, and append immutable wallet-created ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "passphrase": {"type": "string", "description": "Local encryption passphrase (min length 8)."}
                        },
                        "required": ["passphrase"]
                    }
                }),
                json!({
                    "name": "wallet_import",
                    "description": "Import a BIP-39 mnemonic into WDK, encrypt seed-at-rest with passphrase, and append immutable wallet-imported ledger event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "seed": {"type": "string", "description": "12 or 24-word BIP-39 seed phrase."},
                            "passphrase": {"type": "string", "description": "Local encryption passphrase (min length 8)."}
                        },
                        "required": ["seed", "passphrase"]
                    }
                }),
                json!({
                    "name": "wallet_unlock",
                    "description": "Decrypt local encrypted seed, initialize WDK sidecar session, and append immutable wallet-unlocked event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "passphrase": {"type": "string", "description": "Local encryption passphrase used for seed decryption."}
                        },
                        "required": ["passphrase"]
                    }
                }),
                json!({
                    "name": "wallet_accounts",
                    "description": "List derived wallet accounts for supported chains (bitcoin/ethereum/polygon/arbitrum). Wallet must be unlocked.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "wallet_balances",
                    "description": "Query wallet balances for supported chains. Wallet must be unlocked.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "wallet_quote",
                    "description": "Estimate transfer quote/fees for a chain, destination address, and amount. Wallet must be unlocked.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "chain": {"type": "string", "description": "bitcoin|ethereum|polygon|arbitrum"},
                            "to": {"type": "string", "description": "Recipient address for the selected chain."},
                            "amount": {"type": "string", "description": "Positive integer amount in chain base units."}
                        },
                        "required": ["chain", "to", "amount"]
                    }
                }),
                json!({
                    "name": "wallet_send",
                    "description": "Broadcast a wallet transfer on the selected chain. Wallet must be unlocked.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "chain": {"type": "string", "description": "bitcoin|ethereum|polygon|arbitrum"},
                            "to": {"type": "string", "description": "Recipient address for the selected chain."},
                            "amount": {"type": "string", "description": "Positive integer amount in chain base units."}
                        },
                        "required": ["chain", "to", "amount"]
                    }
                }),
                json!({
                    "name": "wallet_fees",
                    "description": "Return current fee model snapshot from WDK sidecar for supported chains. Wallet must be unlocked.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "wallet_lock",
                    "description": "Destroy active WDK sidecar wallet session and append immutable wallet-locked event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "wallet_delete",
                    "description": "Permanently delete encrypted local wallet seed and append immutable wallet-deleted event.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "confirm": {"type": "string", "description": "Must be exactly DELETE."}
                        },
                        "required": ["confirm"]
                    }
                }),
                json!({
                    "name": "access_grant",
                    "description": "Create and persist a DID-signed capability token granting resource access to a remote DID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "grantee_did": {"type": "string"},
                            "pattern": {"type": "string", "description": "Resource key pattern (e.g., results/*)."},
                            "modes": {"type": "array", "items": {"type": "string"}, "description": "Access modes: read|write|append|control."},
                            "ttl_seconds": {"type": "integer", "default": 3600},
                            "delegatable": {"type": "boolean", "default": false}
                        },
                        "required": ["grantee_did", "pattern"]
                    }
                }),
                json!({
                    "name": "access_revoke",
                    "description": "Revoke a capability token by token_id hex.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "token_id_hex": {"type": "string"}
                        },
                        "required": ["token_id_hex"]
                    }
                }),
                json!({
                    "name": "access_list",
                    "description": "List capability tokens, optionally filtered to active tokens and/or grantee DID.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "active_only": {"type": "boolean", "default": false},
                            "grantee_did": {"type": "string"}
                        }
                    }
                }),
                json!({
                    "name": "access_verify",
                    "description": "Verify a capability token payload provided inline.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "token": {"type": "object"}
                        },
                        "required": ["token"]
                    }
                }),
                json!({
                    "name": "access_evaluate",
                    "description": "Evaluate ACP-style policy decision for agent/resource/mode.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "agent_did": {"type": "string"},
                            "resource_key": {"type": "string"},
                            "mode": {"type": "string"},
                            "agent_tier": {"type": "integer"},
                            "agent_puf_hex": {"type": "string"}
                        },
                        "required": ["agent_did", "resource_key", "mode"]
                    }
                }),
                json!({
                    "name": "proof_gate_status",
                    "description": "Show proof-gate configuration and requirement summary.",
                    "inputSchema": {"type": "object", "properties": {}}
                }),
                json!({
                    "name": "proof_gate_verify",
                    "description": "Verify a lean4export proof certificate file.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        },
                        "required": ["path"]
                    }
                }),
                json!({
                    "name": "proof_gate_submit",
                    "description": "Copy a proof certificate into the proof-gate certificate directory.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        },
                        "required": ["path"]
                    }
                }),
                json!({
                    "name": "zk_prove_credential",
                    "description": "Generate a Groth16 credential proof for an access grant without revealing grant metadata.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "grant": {"type": "object", "description": "AccessGrant payload"},
                            "grantee_did": {"type": "string"},
                            "action": {"type": "string", "description": "read|write|append|control"},
                            "current_time": {"type": "integer"}
                        },
                        "required": ["grant", "grantee_did", "action"]
                    }
                }),
                json!({
                    "name": "zk_verify_credential",
                    "description": "Verify a Groth16 credential proof bundle.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "proof_bundle": {"type": "object"}
                        },
                        "required": ["proof_bundle"]
                    }
                }),
                json!({
                    "name": "zk_prove_anonymous_membership",
                    "description": "Generate an anonymous membership credential proof using a Merkle witness.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "grant": {"type": "object", "description": "AccessGrant payload"},
                            "grantee_did": {"type": "string"},
                            "action": {"type": "string", "description": "read|write|append|control"},
                            "witness": {"type": "object", "description": "AnonymousMembershipWitness payload"},
                            "current_time": {"type": "integer"}
                        },
                        "required": ["grant", "grantee_did", "action", "witness"]
                    }
                }),
                json!({
                    "name": "zk_verify_anonymous_membership",
                    "description": "Verify an anonymous credential proof bundle.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "proof_bundle": {"type": "object"}
                        },
                        "required": ["proof_bundle"]
                    }
                }),
                json!({
                    "name": "zk_compute_prove",
                    "description": "Generate a verifiable computation receipt (feature-gated with zk-compute).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "request": {"type": "object", "description": "ComputeRequest payload"}
                        },
                        "required": ["request"]
                    }
                }),
                json!({
                    "name": "zk_compute_verify",
                    "description": "Verify a verifiable computation receipt (feature-gated with zk-compute).",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "receipt": {"type": "object", "description": "ComputeReceipt payload"}
                        },
                        "required": ["receipt"]
                    }
                }),
                json!({
                    "name": "nym_status",
                    "description": "Get current Nym/SOCKS5 privacy transport status.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                }),
                json!({
                    "name": "privacy_classify",
                    "description": "Classify a URL under the privacy controller and show routing expectations.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "url": {"type": "string"}
                        },
                        "required": ["url"]
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
            "structuredContent": payload,
            "isError": false
        }),
        Err(err) => {
            let err_payload = json!({"status":"error","message":err});
            json!({
                "structuredContent": err_payload.clone(),
                "content": [
                    {
                        "type": "text",
                        "text": err_payload.to_string()
                    }
                ],
                "isError": true
            })
        }
    }
}

fn tool_call(name: &str, arguments: Value) -> Result<Value, String> {
    // Check if this is an AgentPMT proxied tool (agentpmt/* prefix).
    if let Some(pmt_tool) = agentpmt::is_proxied_tool(name) {
        return tool_agentpmt_proxy(&pmt_tool, arguments);
    }
    if let Ok(gate_cfg) = proof_gate::load_gate_config() {
        if gate_cfg.has_requirements(name) {
            let gate = gate_cfg.evaluate(name);
            if !gate.passed {
                return Err(format!(
                    "proof gate failed for tool '{}': {}/{} requirements met",
                    name, gate.requirements_met, gate.requirements_checked
                ));
            }
        }
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
        "identity_status" => tool_identity_status(arguments),
        "profile_get" => tool_profile_get(arguments),
        "profile_set" => tool_profile_set(arguments),
        "identity_device_scan" => tool_identity_device_scan(arguments),
        "identity_device_save" => tool_identity_device_save(arguments),
        "identity_network_probe" => tool_identity_network_probe(arguments),
        "identity_network_save" => tool_identity_network_save(arguments),
        "identity_tier_set" => tool_identity_tier_set(arguments),
        "identity_anonymous_set" => tool_identity_anonymous_set(arguments),
        "identity_social_connect" => tool_identity_social_connect(arguments),
        "identity_social_revoke" => tool_identity_social_revoke(arguments),
        "identity_super_secure_set" => tool_identity_super_secure_set(arguments),
        "identity_pod_share" => tool_identity_pod_share(arguments),
        "genesis_status" => tool_genesis_status(arguments),
        "genesis_harvest" => tool_genesis_harvest(arguments),
        "genesis_reset" => tool_genesis_reset(arguments),
        "crypto_status" => tool_crypto_status(arguments),
        "crypto_create_password" => tool_crypto_create_password(arguments),
        "crypto_unlock" => tool_crypto_unlock(arguments),
        "crypto_lock" => tool_crypto_lock(arguments),
        "crypto_change_password" => tool_crypto_change_password(arguments),
        "agents_list" => tool_agents_list(arguments),
        "agents_authorize" => tool_agents_authorize(arguments),
        "agents_revoke" => tool_agents_revoke(arguments),
        "agentaddress_status" => tool_agentaddress_status(arguments),
        "agentaddress_chains" => tool_agentaddress_chains(arguments),
        "agentaddress_generate" => tool_agentaddress_generate(arguments),
        "agentaddress_credentials" => tool_agentaddress_credentials(arguments),
        "agentaddress_disconnect" => tool_agentaddress_disconnect(arguments),
        "wallet_status" => tool_wallet_status(arguments),
        "wallet_create" => tool_wallet_create(arguments),
        "wallet_import" => tool_wallet_import(arguments),
        "wallet_unlock" => tool_wallet_unlock(arguments),
        "wallet_accounts" => tool_wallet_accounts(arguments),
        "wallet_balances" => tool_wallet_balances(arguments),
        "wallet_quote" => tool_wallet_quote(arguments),
        "wallet_send" => tool_wallet_send(arguments),
        "wallet_fees" => tool_wallet_fees(arguments),
        "wallet_lock" => tool_wallet_lock(arguments),
        "wallet_delete" => tool_wallet_delete(arguments),
        "access_grant" => tool_access_grant(arguments),
        "access_revoke" => tool_access_revoke(arguments),
        "access_list" => tool_access_list(arguments),
        "access_verify" => tool_access_verify(arguments),
        "access_evaluate" => tool_access_evaluate(arguments),
        "proof_gate_status" => tool_proof_gate_status(arguments),
        "proof_gate_verify" => tool_proof_gate_verify(arguments),
        "proof_gate_submit" => tool_proof_gate_submit(arguments),
        "zk_prove_credential" => tool_zk_prove_credential(arguments),
        "zk_verify_credential" => tool_zk_verify_credential(arguments),
        "zk_prove_anonymous_membership" => tool_zk_prove_anonymous_membership(arguments),
        "zk_verify_anonymous_membership" => tool_zk_verify_anonymous_membership(arguments),
        "zk_compute_prove" => tool_zk_compute_prove(arguments),
        "zk_compute_verify" => tool_zk_compute_verify(arguments),
        "nym_status" => tool_nym_status(arguments),
        "privacy_classify" => tool_privacy_classify(arguments),
        other => Err(format!("unknown tool: {other}")),
    }
}

/// Proxy a tool call to AgentPMT and record it in the trace.
fn tool_agentpmt_proxy(tool_name: &str, arguments: Value) -> Result<Value, String> {
    if !agentpmt::is_tool_proxy_enabled() {
        return Err(
            "AgentPMT tool proxy is not enabled. Enable it via the halo_capabilities tool or CLI."
                .to_string(),
        );
    }

    let catalog = agentpmt::load_tool_catalog();
    if !catalog.tools.is_empty() && !catalog.has_tool(tool_name) {
        return Err(format!(
            "unknown AgentPMT tool: {tool_name}. Refresh catalog via CLI: agenthalo config tool-proxy refresh"
        ));
    }

    let proxied_tool = format!("agentpmt/{tool_name}");
    let proxied = match agentpmt::call_tool(tool_name, arguments) {
        Ok(v) => v,
        Err(e) => {
            let _ = record_paid_operation_for_halo(
                &proxied_tool,
                0,
                None,
                None,
                false,
                Some(e.clone()),
            );
            return Err(format!("AgentPMT proxy failed: {e}"));
        }
    };

    if let Some(msg) = agentpmt::extract_tool_call_error(&proxied) {
        let _ =
            record_paid_operation_for_halo(&proxied_tool, 0, None, None, false, Some(msg.clone()));
        return Err(format!("AgentPMT tool '{tool_name}' failed: {msg}"));
    }

    record_paid_operation_for_halo(&proxied_tool, 0, None, None, true, None)?;
    Ok(json!({
        "status": "ok",
        "proxied": true,
        "tool": proxied_tool,
        "result": proxied
    }))
}

fn mcp_supported_social_providers() -> &'static [&'static str] {
    &[
        "google",
        "github",
        "microsoft",
        "discord",
        "apple",
        "facebook",
    ]
}

fn mcp_parse_identity_tier(input: &str) -> Option<nucleusdb::halo::identity::IdentitySecurityTier> {
    match input.trim().to_ascii_lowercase().as_str() {
        "max-safe" | "max_safe" | "maxsafe" => {
            Some(nucleusdb::halo::identity::IdentitySecurityTier::MaxSafe)
        }
        "less-safe" | "less_safe" | "lesssafe" | "balanced" | "a_little_rebellious" => {
            Some(nucleusdb::halo::identity::IdentitySecurityTier::LessSafe)
        }
        "low-security" | "low_security" | "low" | "why-bother" => {
            Some(nucleusdb::halo::identity::IdentitySecurityTier::LowSecurity)
        }
        _ => None,
    }
}

fn mcp_identity_tier_label(tier: &nucleusdb::halo::identity::IdentitySecurityTier) -> &'static str {
    match tier {
        nucleusdb::halo::identity::IdentitySecurityTier::MaxSafe => "max-safe",
        nucleusdb::halo::identity::IdentitySecurityTier::LessSafe => "less-safe",
        nucleusdb::halo::identity::IdentitySecurityTier::LowSecurity => "low-security",
    }
}

fn mcp_is_supported_social_provider(provider: &str) -> bool {
    let normalized = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    mcp_supported_social_providers().contains(&normalized.as_str())
}

fn mcp_store_social_token(provider: &str, token: &str) -> Result<String, String> {
    use nucleusdb::halo::vault;

    let normalized = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    let vault_provider = format!("social_{normalized}");
    let env_var = vault::provider_default_env_var(&vault_provider);
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();

    if pq_wallet_path.exists() {
        if let Ok(v) = vault::Vault::open(&pq_wallet_path, &vault_path) {
            v.set_key(&vault_provider, &env_var, token)?;
            return Ok("vault".to_string());
        }
    }

    let creds_path = config::credentials_path();
    let mut creds = load_credentials(&creds_path).unwrap_or_default();
    creds.oauth_provider = Some(normalized);
    creds.oauth_token = Some(token.to_string());
    creds.created_at = now_unix_secs();
    save_credentials(&creds_path, &creds)?;
    Ok("credentials".to_string())
}

fn mcp_clear_social_token(provider: &str) -> Result<(), String> {
    use nucleusdb::halo::vault;

    let normalized = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    let vault_provider = format!("social_{normalized}");
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();

    if pq_wallet_path.exists() {
        if let Ok(v) = vault::Vault::open(&pq_wallet_path, &vault_path) {
            let _ = v.delete_key(&vault_provider);
        }
    }

    let creds_path = config::credentials_path();
    let mut creds = load_credentials(&creds_path).unwrap_or_default();
    if creds.oauth_provider.as_deref() == Some(normalized.as_str()) {
        creds.oauth_provider = None;
        creds.oauth_token = None;
        save_credentials(&creds_path, &creds)?;
    }
    Ok(())
}

#[derive(Debug)]
struct McpCryptoState {
    session: SessionManager,
    migration_status: migration::MigrationStatus,
}

impl McpCryptoState {
    fn new() -> Self {
        Self {
            session: SessionManager::new(),
            migration_status: migration::detect_migration_status(),
        }
    }
}

fn mcp_crypto_mutex() -> &'static Mutex<McpCryptoState> {
    static CRYPTO: OnceLock<Mutex<McpCryptoState>> = OnceLock::new();
    CRYPTO.get_or_init(|| Mutex::new(McpCryptoState::new()))
}

fn with_mcp_crypto_state<T>(
    f: impl FnOnce(&mut McpCryptoState) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = mcp_crypto_mutex()
        .lock()
        .map_err(|e| format!("crypto state lock poisoned: {e}"))?;
    f(&mut guard)
}

fn mcp_migration_status_name(status: &migration::MigrationStatus) -> &'static str {
    match status {
        migration::MigrationStatus::Fresh => "fresh",
        migration::MigrationStatus::NeedsPasswordCreation => "needs_password_creation",
        migration::MigrationStatus::V2Locked => "v2_locked",
        migration::MigrationStatus::V2Unlocked => "v2_unlocked",
    }
}

fn mcp_header_salt_bytes(header: &encrypted_file::CryptoHeader) -> Result<[u8; 32], String> {
    let raw = hex::decode(&header.kdf.salt_hex).map_err(|e| format!("kdf salt decode: {e}"))?;
    if raw.len() != 32 {
        return Err(format!(
            "crypto header salt must be 32 bytes, got {}",
            raw.len()
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&raw);
    Ok(out)
}

fn mcp_crypto_scope_targets() -> Vec<(std::path::PathBuf, CryptoScope)> {
    vec![
        (config::pq_wallet_v2_path(), CryptoScope::Sign),
        (config::vault_v2_path(), CryptoScope::Vault),
        (config::wdk_seed_v2_path(), CryptoScope::Wallet),
        (config::identity_v2_path(), CryptoScope::Identity),
        (config::profile_v2_path(), CryptoScope::Identity),
        (config::genesis_seed_v2_path(), CryptoScope::Genesis),
    ]
}

fn mcp_derive_scope_key_from_master(
    master: &[u8; 32],
    scope: CryptoScope,
) -> Result<[u8; 32], String> {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(b"agenthalo-scope-v2"), master);
    let mut out = [0u8; 32];
    hk.expand(scope.hkdf_info(), &mut out)
        .map_err(|_| "hkdf expand failed".to_string())?;
    Ok(out)
}

fn mcp_verify_master_key(
    header: &encrypted_file::CryptoHeader,
    master: &[u8; 32],
) -> Result<bool, String> {
    if encrypted_file::verify_password_with_header(header, master) {
        return Ok(true);
    }
    let targets = mcp_crypto_scope_targets();
    if !targets.iter().any(|(path, _)| path.exists()) {
        return Ok(false);
    }
    for (path, scope) in targets {
        if !path.exists() {
            continue;
        }
        let mut scope_key = mcp_derive_scope_key_from_master(master, scope)?;
        let file = encrypted_file::EncryptedFileV2::load(&path)?;
        let ok = file.decrypt(&scope_key).is_ok();
        scope_key.zeroize();
        if ok {
            return Ok(true);
        }
    }
    Ok(false)
}

fn mcp_require_scope(scope: CryptoScope) -> Result<(), String> {
    if !encrypted_file::header_exists() {
        return Ok(());
    }
    with_mcp_crypto_state(|crypto| {
        crypto
            .session
            .get_scope_key(scope)
            .map(|_| ())
            .map_err(|_| format!("unlock required (scope: {})", scope.as_str()))
    })
}

fn mcp_genesis_is_completed_status(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "completed" | "sealed" | "committed"
    )
}

fn mcp_genesis_reset_enabled() -> bool {
    std::env::var("AGENTHALO_ENABLE_GENESIS_RESET")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn mcp_normalize_genesis_reset_reason(input: Option<&str>) -> String {
    input
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("operator_requested")
        .to_string()
}

fn tool_crypto_status(_arguments: Value) -> Result<Value, String> {
    let header = encrypted_file::load_header()?;
    with_mcp_crypto_state(|crypto| {
        crypto.session.reap_expired();
        let mut scopes = crypto
            .session
            .active_scopes()
            .into_iter()
            .map(|s| s.as_str().to_string())
            .collect::<Vec<_>>();
        scopes.sort();
        Ok(json!({
            "status": "ok",
            "password_configured": header.is_some(),
            "unlocked": crypto.session.is_unlocked(),
            "active_scopes": scopes,
            "failed_attempts": crypto.session.failed_attempts(),
            "locked_until_unix": crypto.session.locked_until_unix(),
            "migration_status": mcp_migration_status_name(&crypto.migration_status),
        }))
    })
}

fn tool_crypto_create_password(arguments: Value) -> Result<Value, String> {
    let password_raw = arguments
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "password is required".to_string())?;
    let confirm_raw = arguments
        .get("confirm")
        .and_then(|v| v.as_str())
        .unwrap_or(password_raw);
    password::validate_password_pair(password_raw, confirm_raw)?;

    let status = migration::detect_migration_status();
    let mut migrated_files = Vec::new();
    if matches!(status, migration::MigrationStatus::NeedsPasswordCreation) {
        let report = migration::migrate_v1_to_v2(password_raw)?;
        migrated_files = report.files_migrated;
    } else {
        let _ = encrypted_file::create_header_if_missing()?;
    }

    let header = encrypted_file::load_header()?
        .ok_or_else(|| "crypto header missing after password creation".to_string())?;
    let mut verify_master = header.kdf.derive_master_key(password_raw)?;
    let mut updated_header = header.clone();
    updated_header.password_verifier_hex =
        Some(encrypted_file::password_verifier_hex(&verify_master));
    encrypted_file::save_header(&updated_header)?;
    verify_master.zeroize();
    let salt = mcp_header_salt_bytes(&header)?;

    with_mcp_crypto_state(|crypto| {
        crypto.session.unlock_with_password(password_raw, &salt)?;
        crypto.migration_status = migration::MigrationStatus::V2Unlocked;
        Ok(())
    })?;

    let mut scopes = with_mcp_crypto_state(|crypto| {
        Ok(crypto
            .session
            .active_scopes()
            .into_iter()
            .map(|s| s.as_str().to_string())
            .collect::<Vec<_>>())
    })?;
    scopes.sort();

    Ok(json!({
        "status": "ok",
        "migrated_files": migrated_files,
        "active_scopes": scopes,
        "migration_status": "v2_unlocked",
    }))
}

fn tool_crypto_unlock(arguments: Value) -> Result<Value, String> {
    let password = arguments
        .get("password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "password is required".to_string())?;
    let header =
        encrypted_file::load_header()?.ok_or_else(|| "password not configured".to_string())?;

    with_mcp_crypto_state(|crypto| {
        let now = now_unix_secs();
        if let Err((until, _)) = crypto.session.check_throttle(now) {
            return Err(format!(
                "too many failed unlock attempts; retry in {}s",
                until.saturating_sub(now)
            ));
        }
        Ok(())
    })?;

    let mut candidate_master = header.kdf.derive_master_key(password)?;
    let verified = mcp_verify_master_key(&header, &candidate_master)?;
    if !verified {
        with_mcp_crypto_state(|crypto| {
            let now = now_unix_secs();
            if let Err((until, _)) = crypto.session.check_throttle(now) {
                candidate_master.zeroize();
                return Err(format!(
                    "too many failed unlock attempts; retry in {}s",
                    until.saturating_sub(now)
                ));
            }
            candidate_master.zeroize();
            crypto.session.record_failed_attempt(now);
            Err(format!(
                "invalid password; retry in {}s",
                crypto.session.locked_until_unix().saturating_sub(now)
            ))
        })?;
    }

    let verifier_upgrade = if header.password_verifier_hex.is_none() {
        Some(encrypted_file::password_verifier_hex(&candidate_master))
    } else {
        None
    };

    with_mcp_crypto_state(|crypto| {
        let now = now_unix_secs();
        if let Err((until, _)) = crypto.session.check_throttle(now) {
            candidate_master.zeroize();
            return Err(format!(
                "too many failed unlock attempts; retry in {}s",
                until.saturating_sub(now)
            ));
        }
        crypto.session.unlock_with_master_key(candidate_master)?;
        crypto.migration_status = migration::MigrationStatus::V2Unlocked;
        Ok(())
    })?;

    if let Some(verifier) = verifier_upgrade {
        let mut upgraded = header.clone();
        upgraded.password_verifier_hex = Some(verifier);
        if let Err(err) = encrypted_file::save_header(&upgraded) {
            eprintln!(
                "warning: failed to persist password verifier upgrade after MCP unlock: {}",
                err
            );
        }
    }

    let mut scopes = with_mcp_crypto_state(|crypto| {
        Ok(crypto
            .session
            .active_scopes()
            .into_iter()
            .map(|s| s.as_str().to_string())
            .collect::<Vec<_>>())
    })?;
    scopes.sort();

    Ok(json!({
        "status": "ok",
        "mode": "password",
        "unlocked_scopes": scopes,
    }))
}

fn tool_crypto_lock(_arguments: Value) -> Result<Value, String> {
    with_mcp_crypto_state(|crypto| {
        crypto.session.lock();
        crypto.migration_status = if encrypted_file::header_exists() {
            migration::MigrationStatus::V2Locked
        } else {
            migration::detect_migration_status()
        };
        Ok(())
    })?;
    Ok(json!({"status":"ok","locked":true}))
}

fn tool_crypto_change_password(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let current_password = arguments
        .get("current_password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "current_password is required".to_string())?;
    let new_password = arguments
        .get("new_password")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "new_password is required".to_string())?;
    let confirm = arguments
        .get("confirm")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "confirm is required".to_string())?;
    password::validate_password_pair(new_password, confirm)?;

    let old_header =
        encrypted_file::load_header()?.ok_or_else(|| "password not configured".to_string())?;
    let mut old_master = old_header.kdf.derive_master_key(current_password)?;
    let old_verified = mcp_verify_master_key(&old_header, &old_master)?;
    if !old_verified {
        old_master.zeroize();
        return Err("current password is incorrect".to_string());
    }

    let new_kdf = encrypted_file::KdfParams::random_v2();
    let mut new_master = new_kdf.derive_master_key(new_password)?;

    for (path, scope) in mcp_crypto_scope_targets() {
        if !path.exists() {
            continue;
        }
        let mut old_scope_key = mcp_derive_scope_key_from_master(&old_master, scope)?;
        let file = encrypted_file::EncryptedFileV2::load(&path)?;
        let plain = Zeroizing::new(file.decrypt(&old_scope_key)?);
        old_scope_key.zeroize();
        let mut new_scope_key = mcp_derive_scope_key_from_master(&new_master, scope)?;
        let rebuilt = encrypted_file::EncryptedFileV2::encrypt(
            plain.as_slice(),
            &new_scope_key,
            scope,
            &new_kdf,
        )?;
        new_scope_key.zeroize();
        rebuilt.save(&path)?;
    }

    let new_header = encrypted_file::CryptoHeader {
        schema: encrypted_file::CRYPTO_HEADER_SCHEMA.to_string(),
        kdf: new_kdf.clone(),
        created_at: now_unix_secs(),
        password_protected: true,
        password_verifier_hex: Some(encrypted_file::password_verifier_hex(&new_master)),
    };
    encrypted_file::save_header(&new_header)?;
    old_master.zeroize();
    new_master.zeroize();

    let salt = mcp_header_salt_bytes(&new_header)?;
    let agents_reencapsulated = with_mcp_crypto_state(|crypto| {
        crypto.session.unlock_with_password(new_password, &salt)?;
        let reencapsulated = agent_auth::reencapsulate_all_agents(&mut crypto.session).unwrap_or(0);
        crypto.migration_status = migration::MigrationStatus::V2Unlocked;
        Ok(reencapsulated)
    })?;

    Ok(json!({
        "status": "ok",
        "agents_reencapsulated": agents_reencapsulated,
    }))
}

fn tool_agents_list(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let agents = agent_auth::list_agents()?;
    Ok(json!({"status":"ok","agents":agents}))
}

fn tool_agents_authorize(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Sign)?;
    let label = arguments
        .get("label")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "label is required".to_string())?;
    let scopes_json = arguments
        .get("scopes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "scopes must be an array".to_string())?;
    let mut scopes = Vec::new();
    for raw in scopes_json {
        let Some(name) = raw.as_str() else {
            continue;
        };
        if let Some(scope) = CryptoScope::parse(name) {
            if scope != CryptoScope::Admin {
                scopes.push(scope);
            }
        }
    }
    if scopes.is_empty() {
        return Err("at least one valid scope is required".to_string());
    }
    let expires_days = arguments.get("expires_days").and_then(|v| v.as_u64());
    let (cred, secret) = with_mcp_crypto_state(|crypto| {
        agent_auth::authorize_agent(&mut crypto.session, label, &scopes, expires_days)
    })?;

    Ok(json!({
        "status": "ok",
        "agent_id": cred.agent_id,
        "label": cred.label,
        "scopes": cred.scopes.keys().cloned().collect::<Vec<_>>(),
        "expires_at": cred.expires_at,
        "agent_sk": secret.secret_key_hex,
        "algorithm": secret.algorithm,
        "warning": "This secret key is shown once. Copy and store it securely."
    }))
}

fn tool_agents_revoke(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let agent_id = arguments
        .get("agent_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "agent_id is required".to_string())?;
    agent_auth::revoke_agent(agent_id)?;
    Ok(json!({"status":"ok","agent_id":agent_id,"revoked":true}))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum McpAgentAddressSource {
    External,
    Genesis,
}

fn mcp_parse_agentaddress_source(raw: Option<&str>) -> Result<McpAgentAddressSource, String> {
    let Some(source) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(McpAgentAddressSource::External);
    };
    match source.to_ascii_lowercase().as_str() {
        "external" | "agentpmt_external_noauth" | "agentpmt" => Ok(McpAgentAddressSource::External),
        "genesis" | "genesis_derived" => Ok(McpAgentAddressSource::Genesis),
        other => Err(format!(
            "unsupported source '{other}' (expected 'external' or 'genesis')"
        )),
    }
}

fn mcp_agentaddress_supported_chains() -> Vec<&'static str> {
    vec![
        "Ethereum",
        "Base",
        "Arbitrum",
        "Optimism",
        "Polygon",
        "BNB Chain",
        "Avalanche",
        "zkSync",
        "Linea",
        "Scroll",
        "Blast",
        "Mantle",
        "Fantom",
        "Gnosis",
        "Cronos",
        "Celo",
        "Moonbeam",
        "Harmony",
        "Zora",
        "Metis",
        "Aurora",
        "Taiko",
        "Sei",
        "Sepolia",
        "Holesky",
        "Base Sepolia",
        "Arbitrum Sepolia",
        "OP Sepolia",
    ]
}

fn mcp_agentaddress_api_base() -> String {
    std::env::var("AGENTHALO_AGENTADDRESS_API_BASE")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "https://www.agentpmt.com".to_string())
}

fn mcp_is_evm_address(raw: &str) -> bool {
    let trimmed = raw.trim();
    let Some(hex) = trimmed.strip_prefix("0x") else {
        return false;
    };
    hex.len() == 40 && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

fn mcp_open_vault() -> Result<nucleusdb::halo::vault::Vault, String> {
    let pq_wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();
    if !pq_wallet_path.exists() {
        return Err("pq wallet not found; vault unavailable".to_string());
    }
    nucleusdb::halo::vault::Vault::open(&pq_wallet_path, &vault_path)
}

fn tool_agentaddress_status(_arguments: Value) -> Result<Value, String> {
    let identity = nucleusdb::halo::identity::load();
    let entry = identity.agent_address.clone();
    Ok(json!({
        "status": "ok",
        "connected": entry.is_some(),
        "agent_address": entry.as_ref().map(|a| a.evm_address.clone()),
        "generated_at": entry.as_ref().map(|a| a.generated_at.clone()),
        "source": entry.as_ref().and_then(|a| a.source.clone()),
    }))
}

fn tool_agentaddress_chains(_arguments: Value) -> Result<Value, String> {
    let chains = mcp_agentaddress_supported_chains();
    Ok(json!({
        "status": "ok",
        "count": chains.len(),
        "chains": chains,
        "note": "+30 more EVM-compatible networks are supported by AgentAddress."
    }))
}

fn tool_agentaddress_generate(arguments: Value) -> Result<Value, String> {
    let source = mcp_parse_agentaddress_source(arguments.get("source").and_then(|v| v.as_str()))?;
    let (provider, endpoint, source_tag, data, genesis_seed_sha256) = match source {
        McpAgentAddressSource::External => {
            let base = mcp_agentaddress_api_base();
            let endpoint = format!("{}/api/external/agentaddress", base.trim_end_matches('/'));
            let resp = http_client::post(&endpoint)?
                .header("Content-Type", "application/json")
                .send_json(json!({}))
                .map_err(|e| format!("AgentAddress request failed: {e}"))?;
            let payload: Value = resp
                .into_body()
                .read_json()
                .map_err(|e| format!("parse AgentAddress response: {e}"))?;
            let data = payload
                .get("data")
                .cloned()
                .unwrap_or_else(|| payload.clone());
            (
                "AgentAddress",
                endpoint,
                "agentpmt_external_noauth".to_string(),
                data,
                None,
            )
        }
        McpAgentAddressSource::Genesis => {
            mcp_require_scope(CryptoScope::Wallet)?;
            let mnemonic =
                nucleusdb::halo::genesis_seed::derive_wallet_mnemonic()?.ok_or_else(|| {
                    "genesis seed not available; complete Genesis ceremony first".to_string()
                })?;
            let derived = nucleusdb::halo::evm_wallet::derive_from_mnemonic(&mnemonic, None)?;
            let seed_hash = nucleusdb::halo::genesis_seed::load_seed_sha256()?.unwrap_or_default();
            let data = json!({
                "evmAddress": derived.evm_address,
                "evmPrivateKey": derived.private_key_hex,
                "mnemonic": mnemonic,
                "derivationPath": derived.derivation_path,
            });
            (
                "LocalGenesis",
                "local://genesis-seed-derivation".to_string(),
                "genesis_derived".to_string(),
                data,
                Some(seed_hash),
            )
        }
    };
    let address = data
        .get("evmAddress")
        .and_then(Value::as_str)
        .or_else(|| data.get("evm_address").and_then(Value::as_str))
        .unwrap_or("")
        .trim()
        .to_string();
    if !mcp_is_evm_address(&address) {
        return Err("AgentAddress response missing a valid evmAddress".to_string());
    }

    let persist_public = arguments
        .get("persist_public_address")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let private_key = data
        .get("evmPrivateKey")
        .or_else(|| data.get("evm_private_key"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mnemonic = data
        .get("mnemonic")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut vault_stored = false;
    let mut vault_error = None::<String>;
    match mcp_open_vault() {
        Ok(vault) => {
            let mut all_ok = true;
            if !private_key.is_empty() {
                if let Err(e) = vault.set_key(
                    "agent_wallet_private_key",
                    "AGENT_WALLET_PRIVATE_KEY",
                    &private_key,
                ) {
                    vault_error = Some(format!("store private key: {e}"));
                    all_ok = false;
                }
            }
            if !mnemonic.is_empty() {
                if let Err(e) =
                    vault.set_key("agent_wallet_mnemonic", "AGENT_WALLET_MNEMONIC", &mnemonic)
                {
                    let msg = format!("store mnemonic: {e}");
                    vault_error = Some(match vault_error {
                        Some(prev) => format!("{prev}; {msg}"),
                        None => msg,
                    });
                    all_ok = false;
                }
            }
            vault_stored = all_ok && (!private_key.is_empty() || !mnemonic.is_empty());
        }
        Err(e) => {
            vault_error = Some(format!(
                "vault not available — credentials shown but not stored: {e}"
            ));
        }
    }

    let mut ledger_logged = false;
    let mut ledger_error = None::<String>;
    if persist_public {
        let mut identity = nucleusdb::halo::identity::load();
        identity.agent_address = Some(nucleusdb::halo::identity::AgentAddressIdentity {
            evm_address: address.clone(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            source: Some(source_tag.clone()),
        });
        nucleusdb::halo::identity::save(&identity)?;
        let ledger_kind = match source {
            McpAgentAddressSource::Genesis => {
                nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletCreated
            }
            McpAgentAddressSource::External => {
                nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletImported
            }
        };
        match nucleusdb::halo::identity_ledger::append_wallet_event(
            ledger_kind,
            "agent_address_generated",
            json!({
                "provider": "agentaddress",
                "source": source_tag,
                "evm_address": address,
                "vault_stored": vault_stored,
                "genesis_seed_sha256": genesis_seed_sha256,
            }),
        ) {
            Ok(_) => ledger_logged = true,
            Err(e) => ledger_error = Some(e),
        }
    }

    let safe_data = if vault_stored {
        let mut d = data.clone();
        if let Some(obj) = d.as_object_mut() {
            obj.remove("evmPrivateKey");
            obj.remove("evm_private_key");
            obj.remove("mnemonic");
        }
        d
    } else {
        data
    };

    Ok(json!({
        "status": "ok",
        "provider": provider,
        "source": match source {
            McpAgentAddressSource::External => "external",
            McpAgentAddressSource::Genesis => "genesis",
        },
        "endpoint": endpoint,
        "persist_public_address": persist_public,
        "vault_stored": vault_stored,
        "vault_error": vault_error,
        "ledger_logged": ledger_logged,
        "ledger_error": ledger_error,
        "genesis_seed_sha256": genesis_seed_sha256,
        "data": safe_data,
    }))
}

fn tool_agentaddress_credentials(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let reveal = arguments
        .get("reveal")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if reveal {
        let ack = arguments
            .get("acknowledge_plaintext")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if ack != "I_UNDERSTAND_PLAINTEXT_RISK" {
            return Err(
                "reveal=true requires acknowledge_plaintext=I_UNDERSTAND_PLAINTEXT_RISK"
                    .to_string(),
            );
        }
    }
    let identity = nucleusdb::halo::identity::load();
    let address = identity
        .agent_address
        .as_ref()
        .map(|a| a.evm_address.clone())
        .unwrap_or_default();
    let vault = mcp_open_vault()?;
    let private_key = vault.get_key("agent_wallet_private_key").ok();
    let mnemonic = vault.get_key("agent_wallet_mnemonic").ok();
    Ok(json!({
        "status": "ok",
        "address": address,
        "has_private_key": private_key.is_some(),
        "private_key": match (reveal, private_key) {
            (true, value) => value,
            (false, Some(_)) => Some("REDACTED".to_string()),
            (false, None) => None,
        },
        "has_mnemonic": mnemonic.is_some(),
        "mnemonic": match (reveal, mnemonic) {
            (true, value) => value,
            (false, Some(_)) => Some("REDACTED".to_string()),
            (false, None) => None,
        },
        "revealed": reveal,
        "reveal_transport_warning": if reveal {
            Some("plaintext secrets returned over MCP stdio; do not use reveal=true over remote/network transports")
        } else {
            None
        },
    }))
}

fn tool_agentaddress_disconnect(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let mut identity = nucleusdb::halo::identity::load();
    let prev = identity.agent_address.take();
    nucleusdb::halo::identity::save(&identity)?;
    let address = prev
        .as_ref()
        .map(|x| x.evm_address.clone())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = nucleusdb::halo::identity_ledger::append_wallet_event(
        nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletDeleted,
        "agent_address_disconnected",
        json!({
            "provider": "agentaddress",
            "evm_address": address,
        }),
    );
    Ok(json!({"status":"ok","disconnected":true}))
}

fn tool_identity_status(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let profile = nucleusdb::halo::profile::load();
    let identity = nucleusdb::halo::identity::load();
    let ledger = nucleusdb::halo::identity_ledger::project_ledger_status(now_unix_secs())?;
    Ok(json!({
        "status": "ok",
        "profile": profile,
        "identity": identity,
        "ledger": ledger,
    }))
}

fn tool_profile_get(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let profile = nucleusdb::halo::profile::load();
    Ok(json!({
        "status": "ok",
        "profile": profile,
    }))
}

fn tool_profile_set(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let mut profile = nucleusdb::halo::profile::load();
    let previous_profile = profile.clone();
    let rename = arguments
        .get("rename")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let old_name = profile.display_name.clone();

    if let Some(name_raw) = arguments.get("display_name").and_then(|v| v.as_str()) {
        let name = name_raw.trim();
        if name.is_empty() {
            return Err("display_name must not be empty".to_string());
        }
        let changed = old_name
            .as_ref()
            .map(|prev| prev.trim() != name)
            .unwrap_or(true);
        if changed && profile.name_locked && profile.has_name() && !rename {
            return Err("profile name is locked; set rename=true to rotate it".to_string());
        }
        if changed && profile.has_name() {
            profile.name_revision = profile.name_revision.saturating_add(1);
        }
        profile.display_name = Some(name.to_string());
        profile.name_locked = true;
    }

    if let Some(avatar_type) = arguments.get("avatar_type").and_then(|v| v.as_str()) {
        profile.avatar_type = Some(avatar_type.to_string());
    }
    if let Some(avatar_data) = arguments.get("avatar_data").and_then(|v| v.as_str()) {
        if avatar_data.len() > 512 * 1024 {
            return Err("avatar_data exceeds 512KB limit".to_string());
        }
        profile.avatar_data = Some(avatar_data.to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    if profile.created_at.is_none() {
        profile.created_at = Some(now.clone());
    }
    profile.updated_at = Some(now);
    nucleusdb::halo::profile::save(&profile)?;
    if let Err(e) = nucleusdb::halo::identity_ledger::append_profile_update(
        profile.display_name.as_deref(),
        profile.avatar_type.as_deref(),
        profile.name_locked,
        profile.name_revision,
    ) {
        let _ = nucleusdb::halo::profile::save(&previous_profile);
        return Err(format!(
            "identity ledger append failed; profile update rolled back: {e}"
        ));
    }
    Ok(json!({
        "status": "ok",
        "profile": profile,
    }))
}

fn tool_identity_device_scan(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let tier = nucleusdb::puf::core::PufTier::detect();
    let components: Vec<Value> = nucleusdb::puf::core::collect_auto()
        .map(|result| {
            result
                .components
                .iter()
                .map(|c| {
                    json!({
                        "name": c.name,
                        "entropy_bits": c.entropy_bits,
                        "stable": c.stable,
                        "value_preview": String::from_utf8_lossy(&c.value[..c.value.len().min(32)]).to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "status": "ok",
        "tier": format!("{tier:?}"),
        "components": components,
    }))
}

fn tool_identity_device_save(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let browser_fp = arguments
        .get("browser_fingerprint")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let selected: Vec<String> = arguments
        .get("selected_components")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    let puf_result = nucleusdb::puf::core::collect_auto();
    let puf_fingerprint_hex = puf_result.as_ref().map(|r| {
        format!(
            "sha256:{}",
            nucleusdb::transparency::ct6962::hex_encode(&r.fingerprint)
        )
    });
    let puf_tier = puf_result
        .as_ref()
        .map(|r| format!("{:?}", r.tier).to_ascii_lowercase());
    let mut components = puf_result
        .as_ref()
        .map(|result| result.components.clone())
        .unwrap_or_default();
    if !selected.is_empty() {
        components.retain(|c| selected.contains(&c.name));
    }

    let mut entropy_bits = components.iter().map(|c| c.entropy_bits).sum::<u32>();
    if let Some(fp) = browser_fp.clone() {
        components.push(nucleusdb::puf::core::PufComponent {
            name: "browser_fingerprint".to_string(),
            value: fp.into_bytes(),
            entropy_bits: 32,
            stable: true,
        });
        entropy_bits = entropy_bits.saturating_add(32);
    }
    if components.is_empty() {
        return Err("no identity components selected".to_string());
    }

    let digest = nucleusdb::puf::core::canonical_fingerprint(&components);
    let hex = format!(
        "sha256:{}",
        nucleusdb::transparency::ct6962::hex_encode(&digest)
    );

    let mut cfg = nucleusdb::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.device = Some(nucleusdb::halo::identity::DeviceIdentity {
        enabled: true,
        browser_fingerprint: browser_fp,
        selected_components: selected,
        composite_fingerprint_hex: Some(hex.clone()),
        puf_fingerprint_hex: puf_fingerprint_hex.clone(),
        puf_tier: puf_tier.clone(),
        entropy_bits,
        last_collected: Some(chrono::Utc::now().to_rfc3339()),
    });
    nucleusdb::halo::identity::save(&cfg)?;
    if let Err(e) = nucleusdb::halo::identity_ledger::append_device_update(
        true,
        entropy_bits,
        components.len(),
        cfg.device
            .as_ref()
            .and_then(|d| d.browser_fingerprint.as_deref())
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false),
        puf_fingerprint_hex.as_deref(),
        puf_tier.as_deref(),
    ) {
        let _ = nucleusdb::halo::identity::save(&previous_cfg);
        return Err(format!(
            "identity ledger append failed; device update rolled back: {e}"
        ));
    }

    Ok(json!({
        "status": "ok",
        "fingerprint_hex": hex,
        "entropy_bits": entropy_bits,
        "puf_fingerprint_hex": puf_fingerprint_hex,
        "puf_tier": puf_tier,
    }))
}

fn tool_identity_network_probe(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let local_ip = (|| -> Option<String> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.connect("8.8.8.8:80").ok()?;
        socket.local_addr().ok().map(|addr| addr.ip().to_string())
    })();
    let mac_address = mac_address::get_mac_address()
        .ok()
        .flatten()
        .map(|mac| mac.to_string());
    Ok(json!({
        "status": "ok",
        "local_ip": local_ip,
        "mac_address": mac_address,
    }))
}

fn tool_identity_network_save(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let share_local_ip = arguments
        .get("share_local_ip")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let share_public_ip = arguments
        .get("share_public_ip")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let share_mac = arguments
        .get("share_mac")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let local_ip_hash = if share_local_ip {
        arguments
            .get("local_ip")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|ip| {
                let mut h = sha2::Sha256::new();
                use sha2::Digest;
                h.update(ip.as_bytes());
                format!(
                    "sha256:{}",
                    nucleusdb::transparency::ct6962::hex_encode(&h.finalize())
                )
            })
    } else {
        None
    };
    let public_ip_hash = if share_public_ip {
        arguments
            .get("public_ip")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .map(|ip| {
                let mut h = sha2::Sha256::new();
                use sha2::Digest;
                h.update(ip.as_bytes());
                format!(
                    "sha256:{}",
                    nucleusdb::transparency::ct6962::hex_encode(&h.finalize())
                )
            })
    } else {
        None
    };
    let mac_addresses: Vec<String> = if share_mac {
        arguments
            .get("mac_addresses")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut cfg = nucleusdb::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.network = Some(nucleusdb::halo::identity::NetworkIdentity {
        share_local_ip,
        share_public_ip,
        share_mac,
        local_ip_hash,
        public_ip_hash,
        mac_addresses,
    });
    nucleusdb::halo::identity::save(&cfg)?;
    let network = cfg.network.as_ref();
    if let Err(e) = nucleusdb::halo::identity_ledger::append_network_update(
        network.map(|n| n.share_local_ip).unwrap_or(false),
        network.map(|n| n.share_public_ip).unwrap_or(false),
        network.map(|n| n.share_mac).unwrap_or(false),
        network
            .and_then(|n| n.local_ip_hash.as_deref())
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        network
            .and_then(|n| n.public_ip_hash.as_deref())
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        network.map(|n| n.mac_addresses.len()).unwrap_or(0),
    ) {
        let _ = nucleusdb::halo::identity::save(&previous_cfg);
        return Err(format!(
            "identity ledger append failed; network update rolled back: {e}"
        ));
    }

    Ok(json!({
        "status": "ok",
        "network": cfg.network,
    }))
}

fn tool_identity_tier_set(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let tier_raw = arguments
        .get("tier")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "tier is required".to_string())?;
    let tier = mcp_parse_identity_tier(tier_raw)
        .ok_or_else(|| "tier must be one of: max-safe, less-safe, low-security".to_string())?;
    let applied_by = arguments
        .get("applied_by")
        .and_then(|v| v.as_str())
        .unwrap_or("mcp");
    let step_failures = arguments
        .get("step_failures")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let mut cfg = nucleusdb::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.security_tier = Some(tier.clone());
    nucleusdb::halo::identity::save(&cfg)?;
    if let Err(e) = nucleusdb::halo::identity_ledger::append_safety_tier_applied(
        mcp_identity_tier_label(&tier),
        applied_by,
        step_failures,
    ) {
        let _ = nucleusdb::halo::identity::save(&previous_cfg);
        return Err(format!(
            "identity ledger append failed; tier update rolled back: {e}"
        ));
    }

    Ok(json!({
        "status": "ok",
        "tier": mcp_identity_tier_label(&tier),
        "applied_by": applied_by,
        "step_failures": step_failures,
    }))
}

fn tool_identity_anonymous_set(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let enabled = arguments
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "enabled is required (true|false)".to_string())?;
    let mut cfg = nucleusdb::halo::identity::load();
    let previous_cfg = cfg.clone();
    cfg.version = Some(1);
    cfg.anonymous_mode = enabled;
    let mut cleared_device = false;
    let mut cleared_network = false;
    if enabled {
        cleared_device = cfg.device.is_some();
        cleared_network = cfg.network.is_some();
        cfg.device = None;
        cfg.network = None;
    }
    nucleusdb::halo::identity::save(&cfg)?;
    if let Err(e) = nucleusdb::halo::identity_ledger::append_anonymous_mode_update(
        enabled,
        cleared_device,
        cleared_network,
    ) {
        let _ = nucleusdb::halo::identity::save(&previous_cfg);
        return Err(format!(
            "identity ledger append failed; anonymous mode update rolled back: {e}"
        ));
    }
    Ok(json!({
        "status": "ok",
        "anonymous_mode": enabled,
        "cleared_device": cleared_device,
        "cleared_network": cleared_network,
    }))
}

fn tool_identity_social_connect(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let provider = arguments
        .get("provider")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "provider is required".to_string())?;
    let token = arguments
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "token is required".to_string())?;
    if token.trim().is_empty() {
        return Err("token must not be empty".to_string());
    }
    if !mcp_is_supported_social_provider(provider) {
        return Err(format!(
            "unsupported provider: {}. Supported: {}",
            provider,
            mcp_supported_social_providers().join(", ")
        ));
    }

    let provider_norm = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    let source = arguments
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("mcp");
    let selected = arguments
        .get("selected")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let expires_days = arguments
        .get("expires_in_days")
        .and_then(|v| v.as_u64())
        .unwrap_or(30)
        .clamp(1, 365);
    let now = now_unix_secs();
    let expires_at = Some(now.saturating_add(expires_days.saturating_mul(86_400)));

    let storage = mcp_store_social_token(&provider_norm, token.trim())?;
    nucleusdb::halo::identity_ledger::append_social_connect(
        nucleusdb::halo::identity_ledger::SocialConnectInput {
            provider: &provider_norm,
            token: token.trim(),
            expires_at,
            source,
        },
    )?;

    let mut cfg = nucleusdb::halo::identity::load();
    cfg.version = Some(1);
    let st = cfg
        .social
        .providers
        .entry(provider_norm.clone())
        .or_default();
    st.selected = selected;
    st.expires_at = expires_at;
    st.source = Some(source.to_string());
    st.last_connected_at = Some(chrono::Utc::now().to_rfc3339());
    cfg.social.last_updated = Some(chrono::Utc::now().to_rfc3339());
    nucleusdb::halo::identity::save(&cfg)?;

    Ok(json!({
        "status": "ok",
        "provider": provider_norm,
        "storage": storage,
        "selected": selected,
        "expires_at": expires_at,
    }))
}

fn tool_identity_social_revoke(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let provider = arguments
        .get("provider")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "provider is required".to_string())?;
    if !mcp_is_supported_social_provider(provider) {
        return Err(format!(
            "unsupported provider: {}. Supported: {}",
            provider,
            mcp_supported_social_providers().join(", ")
        ));
    }
    let provider_norm = nucleusdb::halo::identity_ledger::normalize_social_provider(provider);
    let reason = arguments
        .get("reason")
        .and_then(|v| v.as_str())
        .unwrap_or("operator_requested");

    mcp_clear_social_token(&provider_norm)?;
    nucleusdb::halo::identity_ledger::append_social_revoke(&provider_norm, Some(reason))?;

    let mut cfg = nucleusdb::halo::identity::load();
    if let Some(p) = cfg.social.providers.get_mut(&provider_norm) {
        p.selected = false;
        p.expires_at = None;
        p.source = Some("revoked".to_string());
    }
    cfg.social.last_updated = Some(chrono::Utc::now().to_rfc3339());
    nucleusdb::halo::identity::save(&cfg)?;

    Ok(json!({
        "status": "ok",
        "provider": provider_norm,
        "reason": reason,
    }))
}

fn tool_identity_super_secure_set(arguments: Value) -> Result<Value, String> {
    let option = arguments
        .get("option")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "option is required".to_string())?
        .trim()
        .to_ascii_lowercase();
    let enabled = arguments
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| "enabled boolean is required".to_string())?;
    if option != "passkey" && option != "security_key" && option != "totp" {
        return Err("option must be one of: passkey, security_key, totp".to_string());
    }

    let mut cfg = nucleusdb::halo::identity::load();
    let mut metadata = json!({});
    match option.as_str() {
        "passkey" => cfg.super_secure.passkey_enabled = enabled,
        "security_key" => cfg.super_secure.security_key_enabled = enabled,
        "totp" => {
            cfg.super_secure.totp_enabled = enabled;
            if let Some(label) = arguments.get("label").and_then(|v| v.as_str()) {
                cfg.super_secure.totp_label = Some(label.to_string());
                metadata = json!({"label": label});
            }
        }
        _ => {}
    }
    cfg.super_secure.last_updated = Some(chrono::Utc::now().to_rfc3339());
    nucleusdb::halo::identity::save(&cfg)?;
    nucleusdb::halo::identity_ledger::append_super_secure_update(&option, enabled, metadata)?;

    Ok(json!({
        "status": "ok",
        "option": option,
        "enabled": enabled,
        "state": cfg.super_secure,
    }))
}

fn decode_hex_32_mcp_field(input: &str, field_name: &str) -> Result<[u8; 32], String> {
    let raw = input.trim().strip_prefix("0x").unwrap_or(input.trim());
    if raw.len() != 64 {
        return Err(format!("{field_name} must be exactly 64 hex chars"));
    }
    let mut out = [0u8; 32];
    for (idx, chunk) in raw.as_bytes().chunks_exact(2).enumerate() {
        let pair =
            std::str::from_utf8(chunk).map_err(|_| format!("{field_name} must be valid hex"))?;
        out[idx] =
            u8::from_str_radix(pair, 16).map_err(|_| format!("{field_name} must be valid hex"))?;
    }
    Ok(out)
}

fn decode_hex_32_mcp(input: &str) -> Result<[u8; 32], String> {
    decode_hex_32_mcp_field(input, "grantee_puf_hex")
}

fn load_grants_for_mcp() -> nucleusdb::pod::acl::GrantStore {
    let path = config::db_path().with_extension("pod_grants.json");
    let bytes = match std::fs::read(&path) {
        Ok(v) => v,
        Err(_) => return nucleusdb::pod::acl::GrantStore::new(),
    };
    let parsed: Vec<nucleusdb::pod::acl::AccessGrant> =
        serde_json::from_slice(&bytes).unwrap_or_default();
    nucleusdb::pod::acl::GrantStore::from_grants(parsed)
}

fn tool_identity_pod_share(arguments: Value) -> Result<Value, String> {
    let profile = nucleusdb::halo::profile::load();
    let identity = nucleusdb::halo::identity::load();
    let ledger = nucleusdb::halo::identity_ledger::project_ledger_status(now_unix_secs())?;

    let mut records =
        nucleusdb::pod::identity_share::materialize_identity_records(&profile, &identity, &ledger);
    let include_ledger = arguments
        .get("include_ledger")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !include_ledger {
        records.retain(|r| !r.key.starts_with("identity/ledger/"));
    }

    let key_patterns: Vec<String> = arguments
        .get("key_patterns")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_else(nucleusdb::pod::identity_share::default_identity_patterns);
    let selected =
        nucleusdb::pod::identity_share::select_records_by_patterns(&records, &key_patterns);

    let require_grants = arguments
        .get("require_grants")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let grantee_hex = arguments
        .get("grantee_puf_hex")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let (shared, denied_keys) = if require_grants || grantee_hex.is_some() {
        let hex = grantee_hex
            .as_deref()
            .ok_or_else(|| "grantee_puf_hex is required when require_grants=true".to_string())?;
        let grantee = decode_hex_32_mcp(&hex)?;
        let grants = load_grants_for_mcp();
        nucleusdb::pod::identity_share::filter_records_by_grants(&selected, &grants, &grantee)
    } else {
        (selected, Vec::new())
    };
    let proof_envelope = nucleusdb::pod::identity_share::build_share_envelope(
        &shared,
        &ledger,
        &key_patterns,
        require_grants,
        grantee_hex.as_deref(),
    )?;
    let proof_verification = nucleusdb::pod::identity_share::verify_share_envelope(
        &proof_envelope,
        &shared,
        &ledger,
        &key_patterns,
        require_grants,
        grantee_hex.as_deref(),
    );

    Ok(json!({
        "status": "ok",
        "patterns": key_patterns,
        "include_ledger": include_ledger,
        "require_grants": require_grants,
        "record_count": shared.len(),
        "records": shared,
        "share_map": nucleusdb::pod::identity_share::records_to_json_map(&shared),
        "proof_envelope": proof_envelope,
        "proof_verification": proof_verification,
        "denied_keys": denied_keys,
    }))
}

fn tool_genesis_status(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let seed_stored = nucleusdb::halo::genesis_seed::seed_exists();
    let seed_hash = nucleusdb::halo::genesis_seed::load_seed_sha256()
        .ok()
        .flatten();
    let latest = nucleusdb::halo::identity_ledger::latest_genesis_event()?;
    if let Some(entry) = latest {
        let completed = mcp_genesis_is_completed_status(&entry.status);
        let curby_pulse_id = entry.payload.get("curby_pulse_id").and_then(|v| v.as_u64());
        let sources_count = entry
            .payload
            .get("policy")
            .and_then(|p| p.get("actual_sources"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let combined_entropy_sha256 = entry
            .payload
            .get("combined_entropy_sha256")
            .cloned()
            .unwrap_or(Value::Null);
        return Ok(json!({
            "status": "ok",
            "completed": completed,
            "genesis_status": entry.status,
            "summary": entry.payload,
            "genesis_entropy_sha256": entry.genesis_entropy_sha256,
            "curby_pulse_id": curby_pulse_id,
            "sources_count": sources_count,
            "combined_entropy_sha256": combined_entropy_sha256,
            "seed_stored": seed_stored,
            "seed_hash_sha256": seed_hash,
            "seq": entry.seq,
            "timestamp": entry.timestamp,
            "entry_hash": entry.entry_hash,
            "signed": entry.signature.is_some(),
            "signature_required_for_genesis": true,
        }));
    }
    Ok(json!({
        "status": "ok",
        "completed": false,
        "genesis_status": "missing",
        "seed_stored": seed_stored,
        "seed_hash_sha256": seed_hash,
        "signature_required_for_genesis": true,
    }))
}

fn tool_genesis_harvest(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Genesis)?;
    if !nucleusdb::halo::pq::has_wallet() {
        let _ = nucleusdb::halo::pq::keygen_pq(false)?;
    }

    if let Some(latest) = nucleusdb::halo::identity_ledger::latest_genesis_event()? {
        if mcp_genesis_is_completed_status(&latest.status) {
            return Ok(json!({
                "status": "ok",
                "success": true,
                "already_completed": true,
                "completed": true,
                "sources_count": latest
                    .payload
                    .get("policy")
                    .and_then(|p| p.get("actual_sources"))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                "curby_pulse_id": latest.payload.get("curby_pulse_id").and_then(|v| v.as_u64()),
                "combined_entropy_sha256": latest.payload.get("combined_entropy_sha256").cloned().unwrap_or(Value::Null),
                "genesis_entropy_sha256": latest.genesis_entropy_sha256,
            }));
        }
    }

    let result = nucleusdb::halo::genesis_entropy::harvest_entropy()
        .map_err(|err| format!("{}: {}", err.error_code, err.message.trim()))?;

    match nucleusdb::halo::genesis_seed::load_seed_sha256() {
        Ok(Some(existing)) if existing == result.combined_entropy_sha256 => {}
        Ok(Some(existing)) => {
            return Err(format!(
                "GENESIS_SEED_MISMATCH: existing sealed genesis seed hash does not match harvested value (existing={}, new={})",
                existing, result.combined_entropy_sha256
            ));
        }
        Ok(None) => {
            nucleusdb::halo::genesis_seed::store_seed_once(
                &result.combined_entropy,
                &result.combined_entropy_sha256,
            )?;
        }
        Err(e) => {
            return Err(format!(
                "SEED_READ_FAILURE: could not read existing sealed genesis seed: {e}"
            ));
        }
    }

    let payload = json!({
        "combined_entropy_sha256": result.combined_entropy_sha256,
        "sources": result.sources,
        "failed_sources": result.failed_sources,
        "policy": {
            "min_sources": 2,
            "actual_sources": result.sources_count,
        },
        "curby_pulse_id": result.curby_pulse_id,
        "drand_normalization": "sha512",
        "duration_ms": result.duration_ms,
    });
    let entry = nucleusdb::halo::identity_ledger::append_genesis_event("completed", payload)?;
    Ok(json!({
        "status": "ok",
        "success": true,
        "completed": true,
        "sources_count": result.sources_count,
        "curby_pulse_id": result.curby_pulse_id,
        "combined_entropy_sha256": result.combined_entropy_sha256,
        "sources": result.sources,
        "failed_sources": result.failed_sources,
        "duration_ms": result.duration_ms,
        "ledger_seq": entry.seq,
        "ledger_entry_hash": entry.entry_hash,
        "ledger_signed": entry.signature.is_some(),
        "genesis_entropy_sha256": entry.genesis_entropy_sha256,
    }))
}

fn tool_genesis_reset(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Genesis)?;
    if !mcp_genesis_reset_enabled() {
        return Err(
            "genesis reset is disabled by policy (set AGENTHALO_ENABLE_GENESIS_RESET=1 to enable)"
                .to_string(),
        );
    }
    if matches!(
        nucleusdb::halo::identity_ledger::latest_completed_genesis_hash(),
        Ok(Some(_))
    ) {
        return Err("genesis reset is blocked after a completed genesis commit".to_string());
    }
    let reason =
        mcp_normalize_genesis_reset_reason(arguments.get("reason").and_then(|v| v.as_str()));
    let payload = json!({
        "reason": reason,
        "reset_at": now_unix_secs(),
    });
    let entry = nucleusdb::halo::identity_ledger::append_genesis_event("reset", payload)?;
    Ok(json!({
        "status": "ok",
        "completed": false,
        "genesis_status": "reset",
        "ledger_seq": entry.seq,
        "ledger_entry_hash": entry.entry_hash,
    }))
}

fn wdk_manager_mutex() -> &'static Mutex<nucleusdb::halo::wdk_proxy::WdkManager> {
    static WDK_MANAGER: OnceLock<Mutex<nucleusdb::halo::wdk_proxy::WdkManager>> = OnceLock::new();
    WDK_MANAGER.get_or_init(|| Mutex::new(nucleusdb::halo::wdk_proxy::WdkManager::new()))
}

fn with_wdk_manager<T>(
    f: impl FnOnce(&mut nucleusdb::halo::wdk_proxy::WdkManager) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = wdk_manager_mutex()
        .lock()
        .map_err(|e| format!("WDK manager lock poisoned: {e}"))?;
    f(&mut guard)
}

fn mcp_wallet_ledger_event(
    kind: nucleusdb::halo::identity_ledger::IdentityLedgerKind,
    status: &str,
    payload: Value,
) -> (bool, Option<String>) {
    match nucleusdb::halo::identity_ledger::append_wallet_event(kind, status, payload) {
        Ok(_) => (true, None),
        Err(e) => (false, Some(e)),
    }
}

fn wdk_seed_word_count(seed: &str) -> usize {
    seed.split_whitespace().count()
}

fn wdk_is_valid_seed_phrase(seed: &str) -> bool {
    let normalized = seed.trim();
    if !matches!(wdk_seed_word_count(normalized), 12 | 24) {
        return false;
    }
    Mnemonic::parse_in_normalized(Language::English, normalized).is_ok()
}

fn wdk_is_supported_chain(chain: &str) -> bool {
    matches!(
        chain.trim().to_ascii_lowercase().as_str(),
        "bitcoin" | "ethereum" | "polygon" | "arbitrum"
    )
}

fn wdk_is_hex_40(input: &str) -> bool {
    if input.len() != 40 {
        return false;
    }
    input.chars().all(|c| c.is_ascii_hexdigit())
}

fn wdk_is_valid_address(chain: &str, address: &str) -> bool {
    let chain = chain.trim().to_ascii_lowercase();
    let address = address.trim();
    if address.is_empty() {
        return false;
    }
    if chain == "bitcoin" {
        let len = address.len();
        let bech32 = address.starts_with("bc1") || address.starts_with("tb1");
        let legacy = (address.starts_with('1')
            || address.starts_with('3')
            || address.starts_with('m')
            || address.starts_with('n')
            || address.starts_with('2'))
            && address.chars().all(|c| c.is_ascii_alphanumeric());
        return (bech32 || legacy) && (26..=90).contains(&len);
    }
    if matches!(chain.as_str(), "ethereum" | "polygon" | "arbitrum") {
        let Some(rest) = address.strip_prefix("0x") else {
            return false;
        };
        return wdk_is_hex_40(rest);
    }
    false
}

fn wdk_is_valid_amount(chain: &str, amount: &str) -> bool {
    let amount = amount.trim();
    if amount.is_empty() || amount.len() > 80 || !amount.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let parsed = amount.parse::<u128>().ok().unwrap_or(0);
    if parsed == 0 {
        return false;
    }
    if chain.eq_ignore_ascii_case("bitcoin") {
        parsed <= 21_000_000_u128.saturating_mul(100_000_000)
    } else {
        true
    }
}

fn validate_wdk_transfer(
    chain: &str,
    to: &str,
    amount: &str,
) -> Result<(String, String, String), String> {
    let chain = chain.trim().to_ascii_lowercase();
    let to = to.trim().to_string();
    let amount = amount.trim().to_string();
    if !wdk_is_supported_chain(&chain) {
        return Err("unsupported chain; expected bitcoin|ethereum|polygon|arbitrum".to_string());
    }
    if !wdk_is_valid_address(&chain, &to) {
        return Err(format!("invalid recipient address for chain {chain}"));
    }
    if !wdk_is_valid_amount(&chain, &amount) {
        return Err("amount must be a positive integer string within allowed range".to_string());
    }
    Ok((chain, to, amount))
}

fn tool_wallet_status(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let available = nucleusdb::halo::wdk_proxy::WdkManager::is_available();
    let wallet_exists = nucleusdb::halo::wdk_proxy::wallet_exists();
    if !available {
        return Ok(json!({
            "status": "ok",
            "available": false,
            "wallet_exists": wallet_exists,
            "sidecar_running": false,
            "sidecar": null,
        }));
    }
    with_wdk_manager(|mgr| {
        let sidecar_running = mgr.is_running();
        let sidecar = if sidecar_running {
            mgr.get("/status")
                .unwrap_or_else(|e| json!({"status":"error","message": e}))
        } else {
            json!({"running": false})
        };
        Ok(json!({
            "status": "ok",
            "available": true,
            "wallet_exists": wallet_exists,
            "sidecar_running": sidecar_running,
            "sidecar": sidecar,
        }))
    })
}

fn tool_wallet_create(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let passphrase = arguments
        .get("passphrase")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "passphrase is required".to_string())?;
    if passphrase.trim().len() < 8 {
        return Err("passphrase must be at least 8 characters".to_string());
    }
    if nucleusdb::halo::wdk_proxy::wallet_exists() {
        return Err("wallet already exists; unlock or delete it first".to_string());
    }
    let seed_phrase =
        nucleusdb::halo::genesis_seed::derive_wallet_mnemonic()?.ok_or_else(|| {
            "genesis seed not available; complete Genesis ceremony before wallet creation"
                .to_string()
        })?;
    let genesis_seed_sha256 =
        nucleusdb::halo::genesis_seed::load_seed_sha256()?.unwrap_or_default();
    if !nucleusdb::halo::wdk_proxy::WdkManager::is_available() {
        return Err(
            "WDK sidecar unavailable; install with `cd wdk-sidecar && npm install`".to_string(),
        );
    }

    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            mgr.start()?;
        }
        if let Err(e) = mgr.post("/init", &json!({"seed": seed_phrase})) {
            let _ = mgr.post("/destroy", &json!({}));
            mgr.stop();
            return Err(e);
        }
        let encrypted = nucleusdb::halo::wdk_proxy::encrypt_seed(&seed_phrase, passphrase)?;
        if let Err(e) = nucleusdb::halo::wdk_proxy::save_encrypted_seed(&encrypted) {
            let _ = mgr.post("/destroy", &json!({}));
            mgr.stop();
            return Err(e);
        }
        let (ledger_logged, ledger_error) = mcp_wallet_ledger_event(
            nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletCreated,
            "created",
            json!({
                "chains": encrypted.chains,
                "kdf": encrypted.kdf,
                "genesis_bound": true,
                "genesis_seed_sha256": genesis_seed_sha256,
            }),
        );
        let accounts = mgr.get("/accounts").unwrap_or_else(|_| json!({}));
        Ok(json!({
            "status": "ok",
            "message": "wallet created from genesis-bound entropy and encrypted",
            "genesis_bound": true,
            "genesis_seed_sha256": genesis_seed_sha256,
            "ledger_logged": ledger_logged,
            "ledger_error": ledger_error,
            "accounts": accounts.get("accounts").cloned().unwrap_or(Value::Array(Vec::new())),
        }))
    })
}

fn tool_wallet_import(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let seed = arguments
        .get("seed")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "seed is required".to_string())?;
    let passphrase = arguments
        .get("passphrase")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "passphrase is required".to_string())?;
    if !wdk_is_valid_seed_phrase(seed) {
        return Err("seed phrase must be a valid 12 or 24-word BIP-39 mnemonic".to_string());
    }
    if passphrase.trim().len() < 8 {
        return Err("passphrase must be at least 8 characters".to_string());
    }
    if nucleusdb::halo::wdk_proxy::wallet_exists() {
        return Err("wallet already exists; delete it first to import a new seed".to_string());
    }
    if !nucleusdb::halo::wdk_proxy::WdkManager::is_available() {
        return Err(
            "WDK sidecar unavailable; install with `cd wdk-sidecar && npm install`".to_string(),
        );
    }

    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            mgr.start()?;
        }
        if let Err(e) = mgr.post("/init", &json!({"seed": seed.trim()})) {
            let _ = mgr.post("/destroy", &json!({}));
            mgr.stop();
            return Err(e);
        }
        let encrypted = nucleusdb::halo::wdk_proxy::encrypt_seed(seed.trim(), passphrase)?;
        if let Err(e) = nucleusdb::halo::wdk_proxy::save_encrypted_seed(&encrypted) {
            let _ = mgr.post("/destroy", &json!({}));
            mgr.stop();
            return Err(e);
        }
        let (ledger_logged, ledger_error) = mcp_wallet_ledger_event(
            nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletImported,
            "imported",
            json!({
                "chains": encrypted.chains,
                "kdf": encrypted.kdf,
            }),
        );
        let accounts = mgr.get("/accounts").unwrap_or_else(|_| json!({}));
        Ok(json!({
            "status": "ok",
            "message": "wallet imported and encrypted",
            "ledger_logged": ledger_logged,
            "ledger_error": ledger_error,
            "accounts": accounts.get("accounts").cloned().unwrap_or(Value::Array(Vec::new())),
        }))
    })
}

fn tool_wallet_unlock(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let passphrase = arguments
        .get("passphrase")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "passphrase is required".to_string())?;
    let encrypted = nucleusdb::halo::wdk_proxy::load_encrypted_seed()
        .ok_or_else(|| "no WDK wallet found".to_string())?;
    let seed = nucleusdb::halo::wdk_proxy::decrypt_seed(&encrypted, passphrase)?;
    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            mgr.start()?;
        }
        let sidecar = mgr.post("/init", &json!({"seed": seed}))?;
        let (ledger_logged, ledger_error) = mcp_wallet_ledger_event(
            nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletUnlocked,
            "unlocked",
            json!({
                "sidecar_initialized": sidecar.get("initialized").and_then(|v| v.as_bool()).unwrap_or(false),
            }),
        );
        Ok(json!({
            "status": "ok",
            "sidecar": sidecar,
            "ledger_logged": ledger_logged,
            "ledger_error": ledger_error,
        }))
    })
}

fn tool_wallet_accounts(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            return Err("wallet is locked; unlock it first".to_string());
        }
        let out = mgr.get("/accounts")?;
        Ok(json!({
            "status": "ok",
            "accounts": out.get("accounts").cloned().unwrap_or(Value::Array(Vec::new())),
            "raw": out,
        }))
    })
}

fn tool_wallet_balances(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            return Err("wallet is locked; unlock it first".to_string());
        }
        let out = mgr.get("/balances")?;
        Ok(json!({
            "status": "ok",
            "balances": out.get("balances").cloned().unwrap_or(Value::Array(Vec::new())),
            "raw": out,
        }))
    })
}

fn tool_wallet_quote(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let chain = arguments
        .get("chain")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "chain is required".to_string())?;
    let to = arguments
        .get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "to is required".to_string())?;
    let amount = arguments
        .get("amount")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "amount is required".to_string())?;
    let (chain, to, amount) = validate_wdk_transfer(chain, to, amount)?;
    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            return Err("wallet is locked; unlock it first".to_string());
        }
        let out = mgr.post(
            "/quote",
            &json!({
                "chain": chain,
                "to": to,
                "amount": amount,
            }),
        )?;
        Ok(json!({
            "status": "ok",
            "quote": out,
        }))
    })
}

fn tool_wallet_send(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let chain = arguments
        .get("chain")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "chain is required".to_string())?;
    let to = arguments
        .get("to")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "to is required".to_string())?;
    let amount = arguments
        .get("amount")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "amount is required".to_string())?;
    let (chain, to, amount) = validate_wdk_transfer(chain, to, amount)?;
    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            return Err("wallet is locked; unlock it first".to_string());
        }
        let out = mgr.post(
            "/send",
            &json!({
                "chain": chain,
                "to": to,
                "amount": amount,
            }),
        )?;
        Ok(json!({
            "status": "ok",
            "transfer": out,
        }))
    })
}

fn tool_wallet_fees(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    with_wdk_manager(|mgr| {
        if !mgr.is_running() {
            return Err("wallet is locked; unlock it first".to_string());
        }
        let out = mgr.get("/fees")?;
        Ok(json!({
            "status": "ok",
            "fees": out,
        }))
    })
}

fn tool_wallet_lock(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    with_wdk_manager(|mgr| {
        if mgr.is_running() {
            let _ = mgr.post("/destroy", &json!({}));
        }
        mgr.stop();
        let (ledger_logged, ledger_error) = mcp_wallet_ledger_event(
            nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletLocked,
            "locked",
            json!({}),
        );
        Ok(json!({
            "status": "ok",
            "message": "wallet locked",
            "ledger_logged": ledger_logged,
            "ledger_error": ledger_error,
        }))
    })
}

fn tool_wallet_delete(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Wallet)?;
    let confirm = arguments
        .get("confirm")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "confirm is required and must be DELETE".to_string())?;
    if confirm.trim() != "DELETE" {
        return Err("confirm must be exactly DELETE".to_string());
    }
    with_wdk_manager(|mgr| {
        if mgr.is_running() {
            let _ = mgr.post("/destroy", &json!({}));
        }
        mgr.stop();
        let seed_path = nucleusdb::halo::wdk_proxy::encrypted_seed_path();
        if seed_path.exists() {
            std::fs::remove_file(&seed_path).map_err(|e| {
                format!(
                    "failed to delete encrypted seed at {}: {e}",
                    seed_path.display()
                )
            })?;
        }
        let (ledger_logged, ledger_error) = mcp_wallet_ledger_event(
            nucleusdb::halo::identity_ledger::IdentityLedgerKind::WalletDeleted,
            "deleted",
            json!({}),
        );
        Ok(json!({
            "status": "ok",
            "message": "wallet permanently deleted",
            "ledger_logged": ledger_logged,
            "ledger_error": ledger_error,
        }))
    })
}

fn parse_access_mode_mcp(input: &str) -> Result<AccessMode, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "read" => Ok(AccessMode::Read),
        "write" => Ok(AccessMode::Write),
        "append" => Ok(AccessMode::Append),
        "control" => Ok(AccessMode::Control),
        other => Err(format!(
            "unknown access mode: {other} (expected read|write|append|control)"
        )),
    }
}

fn parse_access_modes_mcp(arguments: &Value) -> Result<Vec<AccessMode>, String> {
    let Some(items) = arguments.get("modes").and_then(|v| v.as_array()) else {
        return Ok(vec![AccessMode::Read]);
    };
    let modes = items
        .iter()
        .filter_map(|v| v.as_str())
        .map(parse_access_mode_mcp)
        .collect::<Result<Vec<_>, _>>()?;
    if modes.is_empty() {
        return Err("modes must include at least one value".to_string());
    }
    Ok(modes)
}

fn requested_permissions_from_action(
    action: &str,
) -> Result<nucleusdb::pod::acl::GrantPermissions, String> {
    match action.trim().to_ascii_lowercase().as_str() {
        "read" => Ok(nucleusdb::pod::acl::GrantPermissions {
            read: true,
            write: false,
            append: false,
            control: false,
        }),
        "write" => Ok(nucleusdb::pod::acl::GrantPermissions {
            read: false,
            write: true,
            append: false,
            control: false,
        }),
        "append" => Ok(nucleusdb::pod::acl::GrantPermissions {
            read: false,
            write: false,
            append: true,
            control: false,
        }),
        "control" => Ok(nucleusdb::pod::acl::GrantPermissions {
            read: false,
            write: false,
            append: false,
            control: true,
        }),
        other => Err(format!(
            "unknown action: {other} (expected read|write|append|control)"
        )),
    }
}

fn cached_credential_keypair() -> Result<&'static zk_credential::CredentialKeypair, String> {
    static KEYS: OnceLock<zk_credential::CredentialKeypair> = OnceLock::new();
    if let Some(keys) = KEYS.get() {
        return Ok(keys);
    }
    let keys = zk_credential::setup_credential_circuit()?;
    let _ = KEYS.set(keys);
    KEYS.get()
        .ok_or_else(|| "failed to initialize cached credential keypair".to_string())
}

fn tool_access_grant(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let grantee_did = arguments
        .get("grantee_did")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "grantee_did is required".to_string())?;
    let pattern = arguments
        .get("pattern")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "pattern is required".to_string())?;
    let ttl_seconds = arguments
        .get("ttl_seconds")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600);
    let delegatable = arguments
        .get("delegatable")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let modes = parse_access_modes_mcp(&arguments)?;

    let seed = nucleusdb::halo::genesis_seed::load_seed_bytes()?
        .ok_or_else(|| "genesis seed is required before issuing capability tokens".to_string())?;
    let identity = nucleusdb::halo::did::did_from_genesis_seed(&seed)?;
    let now = now_unix_secs();
    let token = capability::create_capability(
        &identity,
        grantee_did,
        capability::AgentClass::Specific {
            did_uri: grantee_did.to_string(),
        },
        &[pattern.to_string()],
        &modes,
        now,
        now.saturating_add(ttl_seconds),
        delegatable,
    )?;

    let path = config::capability_store_path();
    let mut store = CapabilityStore::load_or_default(&path)?;
    store.create(token.clone());
    store.save(&path)?;

    Ok(json!({
        "status": "ok",
        "token_id_hex": format!("0x{}", nucleusdb::halo::util::hex_encode(&token.token_id)),
        "grantor_did": token.grantor_did,
        "grantee_did": token.grantee_did,
        "modes": token.modes,
        "pattern": pattern,
        "expires_at": token.expires_at,
        "delegatable": token.delegatable,
    }))
}

fn tool_access_revoke(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let token_id_hex = arguments
        .get("token_id_hex")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "token_id_hex is required".to_string())?;
    let token_id = decode_hex_32_mcp_field(token_id_hex, "token_id_hex")?;
    let path = config::capability_store_path();
    let mut store = CapabilityStore::load_or_default(&path)?;
    let revoked = store.revoke(&token_id);
    if revoked {
        store.save(&path)?;
    }
    Ok(json!({
        "status": "ok",
        "revoked": revoked,
        "token_id_hex": token_id_hex,
    }))
}

fn tool_access_list(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let active_only = arguments
        .get("active_only")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let grantee_did = arguments
        .get("grantee_did")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let now = now_unix_secs();
    let store = CapabilityStore::load_or_default(&config::capability_store_path())?;
    let mut tokens = if active_only {
        store
            .list_active(now)
            .into_iter()
            .cloned()
            .collect::<Vec<_>>()
    } else {
        store.list_all().to_vec()
    };
    if let Some(grantee) = grantee_did.as_ref() {
        tokens.retain(|t| &t.grantee_did == grantee);
    }
    Ok(json!({
        "status": "ok",
        "count": tokens.len(),
        "active_only": active_only,
        "grantee_did": grantee_did,
        "tokens": tokens,
    }))
}

fn tool_access_verify(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let token_value = arguments
        .get("token")
        .cloned()
        .ok_or_else(|| "token object is required".to_string())?;
    let token: capability::CapabilityToken =
        serde_json::from_value(token_value).map_err(|e| format!("parse capability token: {e}"))?;
    let now = now_unix_secs();
    let result = capability::verify_capability(&token, now);
    Ok(json!({
        "status": if result.is_ok() { "ok" } else { "error" },
        "verified": result.is_ok(),
        "error": result.err(),
        "token_id_hex": format!("0x{}", nucleusdb::halo::util::hex_encode(&token.token_id)),
    }))
}

fn tool_access_evaluate(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let agent_did = arguments
        .get("agent_did")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "agent_did is required".to_string())?;
    let resource_key = arguments
        .get("resource_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "resource_key is required".to_string())?;
    let mode = parse_access_mode_mcp(
        arguments
            .get("mode")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "mode is required".to_string())?,
    )?;
    let agent_tier = arguments
        .get("agent_tier")
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);
    let agent_puf = arguments
        .get("agent_puf_hex")
        .and_then(|v| v.as_str())
        .map(|hex| decode_hex_32_mcp_field(hex, "agent_puf_hex"))
        .transpose()?;
    let store = PolicyStore::load_or_default(&config::access_policy_store_path())?;
    let decision = store.evaluate(AccessContext {
        agent_did: Some(agent_did),
        agent_tier,
        agent_puf: agent_puf.as_ref(),
        resource_key,
        mode,
        now: now_unix_secs(),
    });
    Ok(json!({
        "status": "ok",
        "decision": decision,
    }))
}

fn tool_proof_gate_status(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let cfg = proof_gate::load_gate_config()?;
    Ok(json!({
        "status": "ok",
        "enabled": cfg.enabled,
        "certificate_dir": cfg.certificate_dir,
        "tool_count": cfg.requirements.len(),
        "requirements_total": cfg.requirements.values().map(|v| v.len()).sum::<usize>(),
    }))
}

fn tool_proof_gate_verify(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let path = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "path is required".to_string())?;
    let out = proof_gate::verify_certificate(Path::new(path))?;
    serde_json::to_value(out).map_err(|e| format!("serialize verification result: {e}"))
}

fn tool_proof_gate_submit(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Sign)?;
    let path = arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "path is required".to_string())?;
    let stored = proof_gate::submit_certificate(Path::new(path))?;
    Ok(json!({
        "status": "ok",
        "stored_at": stored,
    }))
}

fn tool_zk_prove_credential(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Sign)?;
    let grant_value = arguments
        .get("grant")
        .cloned()
        .ok_or_else(|| "grant is required".to_string())?;
    let grant: nucleusdb::pod::acl::AccessGrant =
        serde_json::from_value(grant_value).map_err(|e| format!("parse grant: {e}"))?;
    let grantee_did = arguments
        .get("grantee_did")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "grantee_did is required".to_string())?;
    let action = arguments
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "action is required".to_string())?;
    let requested = requested_permissions_from_action(action)?;
    let now = arguments
        .get("current_time")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(now_unix_secs);

    let (pk, _vk) = cached_credential_keypair()?;
    let bundle = zk_credential::prove_credential(&pk, &grant, grantee_did, requested, now)?;
    Ok(json!({
        "status": "ok",
        "proof_bundle": bundle,
    }))
}

fn tool_zk_verify_credential(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let bundle_value = arguments
        .get("proof_bundle")
        .cloned()
        .ok_or_else(|| "proof_bundle is required".to_string())?;
    let bundle: zk_credential::CredentialProofBundle =
        serde_json::from_value(bundle_value).map_err(|e| format!("parse proof_bundle: {e}"))?;
    let (_pk, vk) = cached_credential_keypair()?;
    let verified = zk_credential::verify_credential_proof(&vk, &bundle)?;
    Ok(json!({
        "status": "ok",
        "verified": verified,
    }))
}

fn tool_zk_prove_anonymous_membership(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Sign)?;
    let grant_value = arguments
        .get("grant")
        .cloned()
        .ok_or_else(|| "grant is required".to_string())?;
    let grant: nucleusdb::pod::acl::AccessGrant =
        serde_json::from_value(grant_value).map_err(|e| format!("parse grant: {e}"))?;
    let grantee_did = arguments
        .get("grantee_did")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "grantee_did is required".to_string())?;
    let action = arguments
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "action is required".to_string())?;
    let requested = requested_permissions_from_action(action)?;
    let witness_value = arguments
        .get("witness")
        .cloned()
        .ok_or_else(|| "witness is required".to_string())?;
    let witness: zk_credential::AnonymousMembershipWitness =
        serde_json::from_value(witness_value).map_err(|e| format!("parse witness: {e}"))?;
    let now = arguments
        .get("current_time")
        .and_then(|v| v.as_u64())
        .unwrap_or_else(now_unix_secs);

    let (pk, _vk) = cached_credential_keypair()?;
    let bundle = zk_credential::prove_anonymous_membership(
        &pk,
        &grant,
        grantee_did,
        requested,
        now,
        &witness,
    )?;
    Ok(json!({
        "status": "ok",
        "proof_bundle": bundle,
    }))
}

fn tool_zk_verify_anonymous_membership(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let bundle_value = arguments
        .get("proof_bundle")
        .cloned()
        .ok_or_else(|| "proof_bundle is required".to_string())?;
    let bundle: zk_credential::AnonymousCredentialProofBundle =
        serde_json::from_value(bundle_value).map_err(|e| format!("parse proof_bundle: {e}"))?;
    let (_pk, vk) = cached_credential_keypair()?;
    let verified = zk_credential::verify_anonymous_membership_proof(&vk, &bundle)?;
    Ok(json!({
        "status": "ok",
        "verified": verified,
    }))
}

fn tool_zk_compute_prove(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Sign)?;
    let req_value = arguments
        .get("request")
        .cloned()
        .ok_or_else(|| "request is required".to_string())?;
    let request: zk_compute::ComputeRequest =
        serde_json::from_value(req_value).map_err(|e| format!("parse request: {e}"))?;
    let receipt = zk_compute::prove_computation(&request)?;
    Ok(json!({
        "status": "ok",
        "receipt": receipt,
    }))
}

fn tool_zk_compute_verify(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let receipt_value = arguments
        .get("receipt")
        .cloned()
        .ok_or_else(|| "receipt is required".to_string())?;
    let receipt: zk_compute::ComputeReceipt =
        serde_json::from_value(receipt_value).map_err(|e| format!("parse receipt: {e}"))?;
    let verified = zk_compute::verify_computation(&receipt)?;
    Ok(json!({
        "status": "ok",
        "verified": verified,
    }))
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

fn governance_votes_path() -> std::path::PathBuf {
    config::halo_dir().join("governance_votes.jsonl")
}

fn append_jsonl(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create directory {}: {e}", parent.display()))?;
    }
    let line = serde_json::to_string(value).map_err(|e| format!("serialize json line: {e}"))?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    file.write_all(line.as_bytes())
        .and_then(|_| file.write_all(b"\n"))
        .map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(())
}

fn tally_votes(proposal_id: &str) -> Result<Value, String> {
    let path = governance_votes_path();
    if !path.exists() {
        return Ok(json!({"yes": 0, "no": 0, "abstain": 0, "total": 0}));
    }
    let raw = fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut yes = 0u64;
    let mut no = 0u64;
    let mut abstain = 0u64;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let parsed: Value =
            serde_json::from_str(line).map_err(|e| format!("parse governance vote line: {e}"))?;
        if parsed
            .get("proposal_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            != proposal_id
        {
            continue;
        }
        match parsed.get("choice").and_then(|v| v.as_str()).unwrap_or("") {
            "yes" => yes += 1,
            "no" => no += 1,
            "abstain" => abstain += 1,
            _ => {}
        }
    }
    Ok(json!({
        "yes": yes,
        "no": no,
        "abstain": abstain,
        "total": yes + no + abstain
    }))
}

fn sanitize_target_for_path(target: &str) -> String {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return "default".to_string();
    }
    trimmed
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn parse_tx_hash(raw: &str) -> Option<String> {
    raw.split(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | '(' | ')' | '"' | '\'' | ';'))
        .find(|tok| {
            tok.len() == 66
                && tok.starts_with("0x")
                && tok[2..].chars().all(|c| c.is_ascii_hexdigit())
        })
        .map(|tok| tok.to_string())
}

fn execute_onchain_workflow_call(
    function_signature: &str,
    function_args: Vec<String>,
    payload: &Value,
    rpc_url_arg: Option<&str>,
    contract_arg: Option<&str>,
    private_key_env_arg: Option<&str>,
    require_contract: bool,
    digest_domain: &str,
) -> Result<Option<String>, String> {
    let cfg = load_onchain_config_or_default();
    let rpc_url = rpc_url_arg
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .or_else(|| {
            std::env::var("AGENTHALO_PROTOCOL_RPC_URL")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| cfg.rpc_url.clone());
    let contract = contract_arg
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .or_else(|| {
            std::env::var("AGENTHALO_PROTOCOL_CONTRACT")
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_else(|| cfg.contract_address.clone());

    if contract.trim().is_empty() {
        if require_contract {
            return Err(
                "missing contract address: pass contract_address or set AGENTHALO_PROTOCOL_CONTRACT"
                    .to_string(),
            );
        }
        return Ok(None);
    }

    if onchain_simulation_enabled() {
        let mut digest_payload = serde_json::to_vec(payload)
            .map_err(|e| format!("serialize payload for digest: {e}"))?;
        digest_payload.extend_from_slice(function_signature.as_bytes());
        digest_payload.extend_from_slice(contract.as_bytes());
        digest_payload.extend_from_slice(rpc_url.as_bytes());
        let digest = digest_json(
            digest_domain,
            &json!({
                "payload_hex": hex::encode(digest_payload)
            }),
        )?;
        return Ok(Some(format!("0x{digest}")));
    }

    let private_key_env = private_key_env_arg
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_else(|| cfg.private_key_env.clone());
    let private_key = std::env::var(&private_key_env).map_err(|_| {
        format!("missing private key env var `{private_key_env}` for on-chain submission")
    })?;

    let mut args = vec![
        "send".to_string(),
        "--async".to_string(),
        "--rpc-url".to_string(),
        rpc_url,
        "--private-key".to_string(),
        private_key,
        contract,
        function_signature.to_string(),
    ];
    args.extend(function_args);
    let mut cmd = Command::new("cast");
    cmd.args(&args);
    nym::apply_proxy_env_for_cast(&mut cmd, &args)?;
    let out = cmd.output().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "`cast` command not found".to_string()
        } else {
            format!("cast execution failed: {e}")
        }
    })?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !out.status.success() {
        return Err(format!(
            "cast failed status={} stdout=`{}` stderr=`{}`",
            out.status, stdout, stderr
        ));
    }
    let merged = if stdout.is_empty() {
        stderr
    } else if stderr.is_empty() {
        stdout
    } else {
        format!("{stdout}\n{stderr}")
    };
    let tx_hash = parse_tx_hash(&merged)
        .ok_or_else(|| format!("failed to parse transaction hash from cast output: {merged}"))?;
    Ok(Some(tx_hash))
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
    let submit_onchain = arguments
        .get("submit_onchain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let op = "vote";
    let vote = json!({
        "vote_id": uuid::Uuid::new_v4().to_string(),
        "proposal_id": proposal_id,
        "choice": choice,
        "reason": reason,
        "timestamp": now_unix_secs()
    });
    append_jsonl(&governance_votes_path(), &vote)?;
    let tally = tally_votes(
        vote.get("proposal_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
    )?;
    let tx_hash = if submit_onchain {
        execute_onchain_workflow_call(
            arguments
                .get("function_signature")
                .and_then(|v| v.as_str())
                .unwrap_or("castVote(string,string,string)"),
            vec![
                vote["proposal_id"].as_str().unwrap_or_default().to_string(),
                vote["choice"].as_str().unwrap_or_default().to_string(),
                vote["reason"].as_str().unwrap_or_default().to_string(),
            ],
            &vote,
            arguments.get("rpc_url").and_then(|v| v.as_str()),
            arguments.get("contract_address").and_then(|v| v.as_str()),
            arguments.get("private_key_env").and_then(|v| v.as_str()),
            true,
            "agenthalo.vote.onchain.tx.v1",
        )?
    } else {
        None
    };
    let digest = digest_json("agenthalo.vote.v1", &vote)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": if submit_onchain { "submitted" } else { "stored" },
        "note": if submit_onchain {
            "vote stored and submitted to chain"
        } else {
            "vote stored in local governance ledger"
        },
        "result_digest": digest,
        "vote": vote,
        "tally": tally,
        "tx_hash": tx_hash
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
    let sessions_count = list_sessions(&db_path)?.len();
    let target_endpoint = if target.starts_with("http://") || target.starts_with("https://") {
        Some(target.clone())
    } else if target == "cloudflare" {
        std::env::var("AGENTHALO_SYNC_ENDPOINT")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    } else {
        None
    };
    let sync = json!({
        "sync_id": uuid::Uuid::new_v4().to_string(),
        "target": target,
        "sessions_considered": sessions_count,
        "timestamp": now_unix_secs(),
        "mode": "delta-sync"
    });
    let sync_dir = config::halo_dir()
        .join("sync")
        .join(sanitize_target_for_path(
            sync["target"].as_str().unwrap_or("default"),
        ));
    fs::create_dir_all(&sync_dir)
        .map_err(|e| format!("create sync dir {}: {e}", sync_dir.display()))?;
    let artifact_path = sync_dir.join(format!(
        "{}.json",
        sync["sync_id"].as_str().unwrap_or("sync")
    ));
    let sync_body =
        serde_json::to_vec_pretty(&sync).map_err(|e| format!("serialize sync artifact: {e}"))?;
    fs::write(&artifact_path, &sync_body)
        .map_err(|e| format!("write sync artifact {}: {e}", artifact_path.display()))?;
    let mut remote_status = None;
    if let Some(endpoint) = target_endpoint {
        let resp = http_client::post(&endpoint)?
            .content_type("application/json")
            .send_json(sync.clone())
            .map_err(|e| format!("sync push failed: {e}"))?;
        let status = resp.status();
        let status_ok = status.is_success();
        remote_status = Some(json!({
            "endpoint": endpoint,
            "status_code": status.as_u16(),
            "ok": status_ok
        }));
        if !status_ok {
            return Err(format!("sync endpoint returned HTTP {}", status.as_u16()));
        }
    }
    let digest = digest_json("agenthalo.sync.v1", &sync)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": "completed",
        "note": "sync artifact created and transport executed",
        "result_digest": digest,
        "sync": sync,
        "artifact_path": artifact_path.display().to_string(),
        "remote": remote_status
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
    let submit_onchain = arguments
        .get("submit_onchain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let op = "privacy_pool_create";
    let pool = json!({
        "pool_id": format!("pool-{}", uuid::Uuid::new_v4()),
        "chain": chain,
        "asset": asset,
        "denomination": denomination,
        "timestamp": now_unix_secs(),
        "status": "created"
    });
    let tx_hash = if submit_onchain {
        execute_onchain_workflow_call(
            arguments
                .get("function_signature")
                .and_then(|v| v.as_str())
                .unwrap_or("createPool(string,string,uint256)"),
            vec![
                pool["chain"].as_str().unwrap_or_default().to_string(),
                pool["asset"].as_str().unwrap_or_default().to_string(),
                pool["denomination"]
                    .as_u64()
                    .unwrap_or_default()
                    .to_string(),
            ],
            &pool,
            arguments.get("rpc_url").and_then(|v| v.as_str()),
            arguments.get("contract_address").and_then(|v| v.as_str()),
            arguments.get("private_key_env").and_then(|v| v.as_str()),
            false,
            "agenthalo.privacy_pool.create.tx.v1",
        )?
    } else {
        None
    };
    let digest = digest_json("agenthalo.privacy_pool.create.v1", &pool)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": if submit_onchain { "submitted" } else { "stored" },
        "note": if submit_onchain {
            "privacy pool created and submitted on-chain"
        } else {
            "privacy pool created in local workflow ledger"
        },
        "result_digest": digest,
        "pool": pool,
        "tx_hash": tx_hash
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
    let submit_onchain = arguments
        .get("submit_onchain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let op = "privacy_pool_withdraw";
    let withdrawal = json!({
        "withdrawal_id": format!("wd-{}", uuid::Uuid::new_v4()),
        "pool_id": pool_id,
        "recipient": recipient,
        "amount": amount,
        "timestamp": now_unix_secs(),
        "status": "submitted"
    });
    let tx_hash = if submit_onchain {
        execute_onchain_workflow_call(
            arguments
                .get("function_signature")
                .and_then(|v| v.as_str())
                .unwrap_or("withdrawFromPool(string,address,uint256)"),
            vec![
                withdrawal["pool_id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                withdrawal["recipient"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                withdrawal["amount"]
                    .as_u64()
                    .unwrap_or_default()
                    .to_string(),
            ],
            &withdrawal,
            arguments.get("rpc_url").and_then(|v| v.as_str()),
            arguments.get("contract_address").and_then(|v| v.as_str()),
            arguments.get("private_key_env").and_then(|v| v.as_str()),
            false,
            "agenthalo.privacy_pool.withdraw.tx.v1",
        )?
    } else {
        None
    };
    let digest = digest_json("agenthalo.privacy_pool.withdraw.v1", &withdrawal)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": if submit_onchain { "submitted" } else { "stored" },
        "note": if submit_onchain {
            "privacy pool withdrawal submitted on-chain"
        } else {
            "privacy pool withdrawal stored in local workflow ledger"
        },
        "result_digest": digest,
        "withdrawal": withdrawal,
        "tx_hash": tx_hash
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
    let submit_onchain = arguments
        .get("submit_onchain")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

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
    let tx_hash = if submit_onchain {
        execute_onchain_workflow_call(
            arguments
                .get("function_signature")
                .and_then(|v| v.as_str())
                .unwrap_or("bridgeTransfer(string,string,string,uint256,address)"),
            vec![
                transfer["from_chain"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                transfer["to_chain"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                transfer["asset"].as_str().unwrap_or_default().to_string(),
                transfer["amount"].as_u64().unwrap_or_default().to_string(),
                transfer["recipient"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            ],
            &transfer,
            arguments.get("rpc_url").and_then(|v| v.as_str()),
            arguments.get("contract_address").and_then(|v| v.as_str()),
            arguments.get("private_key_env").and_then(|v| v.as_str()),
            false,
            "agenthalo.pq_bridge.transfer.tx.v1",
        )?
    } else {
        None
    };
    let digest = digest_json("agenthalo.pq_bridge.transfer.v1", &transfer)?;
    record_paid_operation_for_halo(op, 0, None, Some(digest.clone()), true, None)?;
    Ok(json!({
        "status": if submit_onchain { "submitted" } else { "stored" },
        "note": if submit_onchain {
            "PQ bridge transfer submitted on-chain"
        } else {
            "PQ bridge transfer stored in local workflow ledger"
        },
        "result_digest": digest,
        "transfer": transfer,
        "tx_hash": tx_hash
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
        "tool_proxy_endpoint": agentpmt::resolved_mcp_endpoint(&pmt_cfg),
        "tool_proxy_auth_configured": agentpmt::has_bearer_token(),
        "session_count": session_count,
        "total_cost_usd": total_cost,
        "total_tokens": total_tokens,
        "latest_session": latest,
        "db_path": db_path.to_string_lossy(),
        "version": "0.3.0",
    }))
}

fn tool_nym_status(_arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    serde_json::to_value(nym::status()).map_err(|e| format!("serialize nym status: {e}"))
}

fn tool_privacy_classify(arguments: Value) -> Result<Value, String> {
    mcp_require_scope(CryptoScope::Identity)?;
    let url = arguments
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "url is required".to_string())?;
    let level = privacy_controller::classify_url(url);
    let via_mixnet = nym::should_route_via_mixnet(url);
    let route = nym::ensure_route_allowed(url).map(|v| {
        v.map(|proxy| json!({"transport": "socks5", "proxy": proxy}))
            .unwrap_or_else(|| json!({"transport": "direct"}))
    });
    match route {
        Ok(route_info) => Ok(json!({
            "status": "ok",
            "url": url,
            "privacy_level": level,
            "via_mixnet": via_mixnet,
            "route": route_info,
            "fail_closed": nym::is_fail_closed(),
        })),
        Err(err) => Ok(json!({
            "status": "blocked",
            "url": url,
            "privacy_level": level,
            "via_mixnet": via_mixnet,
            "route": json!({"transport": "blocked"}),
            "fail_closed": nym::is_fail_closed(),
            "error": err,
        })),
    }
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
    let proxy_cfg = nucleusdb::halo::pricing::load_proxy_config();
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
                "endpoint": agentpmt::resolved_mcp_endpoint(&pmt_cfg),
                "auth_configured": agentpmt::has_bearer_token(),
                "budget_tag": pmt_cfg.budget_tag,
                "note": "All third-party tools are accessed exclusively through AgentPMT. Tools, workflows, skills, and agent configurations are only available via AgentPMT MCP.",
            },
            "metered_proxy": {
                "enabled": proxy_cfg.enabled,
                "markup_pct": proxy_cfg.markup_pct,
                "rate_limit_rpm": proxy_cfg.rate_limit_rpm,
                "daily_token_limit": proxy_cfg.daily_token_limit,
                "note": "LLM inference via OpenRouter. All models accessed through operator's account with markup.",
            },
            "metered_storage": {
                "available": true,
                "provider": "pinata",
                "note": "IPFS storage via Pinata. All storage accessed through operator's account with markup.",
            },
        },
        "monetization": {
            "funding_channels": ["agentpmt_tokens", "x402_direct"],
            "note": "All funding must come through AgentPMT.com token purchase or x402direct USDC payment. No other payment method accepted.",
            "services": {
                "llm_inference": "OpenRouter proxy (all models, markup applied)",
                "ipfs_storage": "Pinata proxy (pin/unpin, markup applied)",
                "tools": "AgentPMT MCP (third-party tool access)",
                "workflows": "AgentPMT MCP (workflow execution)",
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
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn make_temp_home(prefix: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        home
    }

    #[test]
    fn unknown_tool_sets_error_flag() {
        let out = tool_call_response("does_not_exist", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(true));
        assert!(out.get("structuredContent").is_some());
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
        assert!(out.get("structuredContent").is_some());

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn identity_tools_social_roundtrip() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_identity_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let connect = tool_identity_social_connect(json!({
            "provider": "google",
            "token": "tok-test-123",
            "expires_in_days": 30,
            "selected": true,
            "source": "mcp_test"
        }))
        .expect("social connect");
        assert_eq!(connect["status"], "ok");
        assert_eq!(connect["provider"], "google");

        let status = tool_identity_status(json!({})).expect("identity status");
        assert_eq!(status["status"], "ok");
        assert_eq!(status["ledger"]["chain_valid"], true);

        let revoke = tool_identity_social_revoke(json!({
            "provider": "google",
            "reason": "test_revoke"
        }))
        .expect("social revoke");
        assert_eq!(revoke["status"], "ok");

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn identity_tool_tier_set_roundtrip() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_identity_tier_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let set = tool_identity_tier_set(json!({
            "tier": "max-safe",
            "applied_by": "mcp_test",
            "step_failures": 0
        }))
        .expect("tier set");
        assert_eq!(set["status"], "ok");
        assert_eq!(set["tier"], "max-safe");

        let status = tool_identity_status(json!({})).expect("identity status");
        assert_eq!(status["status"], "ok");
        assert_eq!(status["identity"]["security_tier"], "max-safe");

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn profile_set_roundtrip() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_profile_roundtrip_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let out = tool_profile_set(json!({
            "display_name": "MCP Profile",
            "avatar_type": "initials",
            "rename": false
        }))
        .expect("profile set");
        assert_eq!(out["status"], "ok");
        assert_eq!(out["profile"]["display_name"], "MCP Profile");

        let got = tool_profile_get(json!({})).expect("profile get");
        assert_eq!(got["status"], "ok");
        assert_eq!(got["profile"]["display_name"], "MCP Profile");

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn identity_device_and_network_probe_tools_return_ok() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_identity_probe_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let device = tool_identity_device_scan(json!({})).expect("device scan");
        assert_eq!(device["status"], "ok");
        assert!(device.get("components").is_some());

        let network = tool_identity_network_probe(json!({})).expect("network probe");
        assert_eq!(network["status"], "ok");
        assert!(network.get("local_ip").is_some());

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn identity_anonymous_set_roundtrip() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_identity_anon_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let enabled = tool_identity_anonymous_set(json!({"enabled": true})).expect("anon enable");
        assert_eq!(enabled["status"], "ok");
        assert_eq!(enabled["anonymous_mode"], true);

        let status = tool_identity_status(json!({})).expect("identity status");
        assert_eq!(status["identity"]["anonymous_mode"], true);

        let disabled =
            tool_identity_anonymous_set(json!({"enabled": false})).expect("anon disable");
        assert_eq!(disabled["anonymous_mode"], false);

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn genesis_status_tool_returns_non_error_payload() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_genesis_status_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let out = tool_call_response("genesis_status", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(false));
        let payload = out
            .get("structuredContent")
            .and_then(|v| v.as_object())
            .expect("structuredContent object");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert!(payload.contains_key("completed"));

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn wallet_status_tool_returns_non_error_payload() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_wallet_status_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let out = tool_call_response("wallet_status", json!({}));
        assert_eq!(out.get("isError").and_then(|v| v.as_bool()), Some(false));
        let payload = out
            .get("structuredContent")
            .and_then(|v| v.as_object())
            .expect("structuredContent object");
        assert_eq!(payload.get("status").and_then(|v| v.as_str()), Some("ok"));
        assert!(payload.contains_key("available"));
        assert!(payload.contains_key("wallet_exists"));

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn wallet_import_rejects_invalid_mnemonic() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_wallet_import_invalid_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let err = tool_wallet_import(json!({
            "seed": "apple banana cherry dog elephant fish grape house igloo jelly kite lemon",
            "passphrase": "passphrase123"
        }))
        .expect_err("invalid mnemonic should fail");
        assert!(err.to_ascii_lowercase().contains("bip-39"));

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn wallet_create_rejects_short_passphrase() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_wallet_create_short_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let err = tool_wallet_create(json!({
            "passphrase": "short"
        }))
        .expect_err("short passphrase should fail");
        assert!(err.to_ascii_lowercase().contains("at least 8"));

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn crypto_tools_password_roundtrip() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_crypto_roundtrip_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let created = tool_crypto_create_password(json!({
            "password": "StrongPass123!",
            "confirm": "StrongPass123!"
        }))
        .expect("create password");
        assert_eq!(created["status"], "ok");
        assert_eq!(created["migration_status"], "v2_unlocked");

        let locked = tool_crypto_lock(json!({})).expect("lock");
        assert_eq!(locked["locked"], true);

        let status_locked = tool_crypto_status(json!({})).expect("status locked");
        assert_eq!(status_locked["unlocked"], false);
        assert_eq!(status_locked["password_configured"], true);

        let unlocked = tool_crypto_unlock(json!({
            "password": "StrongPass123!"
        }))
        .expect("unlock");
        assert_eq!(unlocked["status"], "ok");
        assert_eq!(unlocked["mode"], "password");

        let status_unlocked = tool_crypto_status(json!({})).expect("status unlocked");
        assert_eq!(status_unlocked["unlocked"], true);

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn crypto_unlock_rejects_wrong_password_mcp() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_crypto_wrong_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let _ = tool_crypto_create_password(json!({
            "password": "StrongPass123!",
            "confirm": "StrongPass123!"
        }))
        .expect("create password");
        let _ = tool_crypto_lock(json!({})).expect("lock");
        let err = tool_crypto_unlock(json!({"password": "WrongPass123!"}))
            .expect_err("wrong password should fail");
        assert!(err.to_ascii_lowercase().contains("invalid password"));

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn agents_tools_roundtrip_mcp() {
        let _guard = env_lock().lock().expect("lock env");
        let home = std::env::temp_dir().join(format!(
            "agenthalo_mcp_agents_roundtrip_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).expect("create temp home");
        std::env::set_var("AGENTHALO_HOME", &home);

        let _ = tool_crypto_create_password(json!({
            "password": "StrongPass123!",
            "confirm": "StrongPass123!"
        }))
        .expect("create password");

        let created = tool_agents_authorize(json!({
            "label": "MCP Test Agent",
            "scopes": ["sign", "identity"]
        }))
        .expect("authorize");
        assert_eq!(created["status"], "ok");
        let agent_id = created["agent_id"].as_str().expect("agent id").to_string();

        let listed = tool_agents_list(json!({})).expect("list");
        assert_eq!(listed["status"], "ok");
        let listed_arr = listed["agents"].as_array().cloned().unwrap_or_default();
        assert!(
            listed_arr
                .iter()
                .any(|a| a["agent_id"].as_str() == Some(agent_id.as_str())),
            "created agent not listed: {listed}"
        );

        let revoked = tool_agents_revoke(json!({"agent_id": agent_id})).expect("revoke");
        assert_eq!(revoked["status"], "ok");
        assert_eq!(revoked["revoked"], true);

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

    #[test]
    fn resolve_mcp_secret_requires_explicit_secret_or_dev_opt_in() {
        let _guard = env_lock().lock().expect("lock env");
        std::env::remove_var("AGENTHALO_MCP_SECRET");
        std::env::remove_var("AGENTHALO_ALLOW_DEV_SECRET");

        let err = resolve_mcp_secret().expect_err("secret should be required by default");
        assert!(
            err.contains("AGENTHALO_MCP_SECRET is required"),
            "unexpected error: {err}"
        );

        std::env::set_var("AGENTHALO_ALLOW_DEV_SECRET", "1");
        let dev = resolve_mcp_secret().expect("dev opt-in should permit fallback");
        assert_eq!(dev, "agenthalo-dev-secret");
        std::env::remove_var("AGENTHALO_ALLOW_DEV_SECRET");

        std::env::set_var("AGENTHALO_MCP_SECRET", "real-secret");
        let explicit = resolve_mcp_secret().expect("explicit secret should work");
        assert_eq!(explicit, "real-secret");
        std::env::remove_var("AGENTHALO_MCP_SECRET");
    }

    #[test]
    fn sanitize_target_for_path_handles_empty_replacements_and_allowed_chars() {
        assert_eq!(sanitize_target_for_path(""), "default");
        assert_eq!(sanitize_target_for_path("  "), "default");
        assert_eq!(
            sanitize_target_for_path("cloudflare/main prod"),
            "cloudflare_main_prod"
        );
        assert_eq!(sanitize_target_for_path("Alpha-9_.beta"), "Alpha-9_.beta");
    }

    #[test]
    fn parse_tx_hash_extracts_valid_hash_and_rejects_garbage() {
        let raw =
            "submitted tx: 0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef (ok)";
        let hash = parse_tx_hash(raw).expect("hash");
        assert_eq!(hash.len(), 66);
        assert!(hash.starts_with("0x"));
        assert!(parse_tx_hash("no transaction hash here").is_none());
    }

    #[test]
    fn append_jsonl_and_tally_votes_counts_expected_choices() {
        let _guard = env_lock().lock().expect("lock env");
        let home = make_temp_home("agenthalo_mcp_vote_tally");
        std::env::set_var("AGENTHALO_HOME", &home);

        let path = governance_votes_path();
        append_jsonl(
            &path,
            &json!({"proposal_id":"prop-1","choice":"yes","timestamp":now_unix_secs()}),
        )
        .expect("append yes");
        append_jsonl(
            &path,
            &json!({"proposal_id":"prop-1","choice":"no","timestamp":now_unix_secs()}),
        )
        .expect("append no");

        let tally = tally_votes("prop-1").expect("tally");
        assert_eq!(tally["yes"], 1);
        assert_eq!(tally["no"], 1);
        assert_eq!(tally["abstain"], 0);
        assert_eq!(tally["total"], 2);

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tool_vote_local_path_returns_stored_and_null_tx_hash() {
        let _guard = env_lock().lock().expect("lock env");
        let home = make_temp_home("agenthalo_mcp_vote_local");
        std::env::set_var("AGENTHALO_HOME", &home);
        std::env::remove_var("AGENTHALO_ONCHAIN_SIMULATION");

        let out = tool_vote(json!({
            "proposal_id": "proposal-local",
            "choice": "yes",
            "reason": "local path",
            "submit_onchain": false
        }))
        .expect("vote local");
        assert_eq!(out["status"], "stored");
        assert!(out["tally"].is_object());
        assert!(out["tx_hash"].is_null());

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tool_vote_onchain_simulation_returns_submitted_tx_hash() {
        let _guard = env_lock().lock().expect("lock env");
        let home = make_temp_home("agenthalo_mcp_vote_simulation");
        std::env::set_var("AGENTHALO_HOME", &home);
        std::env::set_var("AGENTHALO_ONCHAIN_SIMULATION", "1");

        let out = tool_vote(json!({
            "proposal_id": "proposal-sim",
            "choice": "no",
            "submit_onchain": true,
            "contract_address": "0xabc"
        }))
        .expect("vote simulation");
        let tx_hash = out["tx_hash"].as_str().expect("tx hash string");
        assert_eq!(out["status"], "submitted");
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);

        std::env::remove_var("AGENTHALO_ONCHAIN_SIMULATION");
        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tool_sync_local_path_creates_artifact() {
        let _guard = env_lock().lock().expect("lock env");
        let home = make_temp_home("agenthalo_mcp_sync_local");
        std::env::set_var("AGENTHALO_HOME", &home);

        let out = tool_sync(json!({"target":"local-test"})).expect("sync");
        assert_eq!(out["status"], "completed");
        let artifact_path = out["artifact_path"].as_str().expect("artifact path");
        assert!(std::path::Path::new(artifact_path).exists());

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tool_privacy_pool_create_local_only_returns_null_tx_hash() {
        let _guard = env_lock().lock().expect("lock env");
        let home = make_temp_home("agenthalo_mcp_pool_local");
        std::env::set_var("AGENTHALO_HOME", &home);
        addons::set_enabled("agentpmt-workflows", true).expect("enable workflow add-on");

        let out = tool_privacy_pool_create(json!({
            "chain": "base-sepolia",
            "asset": "USDC",
            "denomination": 1000
        }))
        .expect("pool local");
        assert_eq!(out["status"], "stored");
        assert!(out["tx_hash"].is_null());

        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tool_privacy_pool_create_simulation_returns_tx_hash() {
        let _guard = env_lock().lock().expect("lock env");
        let home = make_temp_home("agenthalo_mcp_pool_simulation");
        std::env::set_var("AGENTHALO_HOME", &home);
        std::env::set_var("AGENTHALO_ONCHAIN_SIMULATION", "1");
        addons::set_enabled("agentpmt-workflows", true).expect("enable workflow add-on");

        let out = tool_privacy_pool_create(json!({
            "chain": "base-sepolia",
            "asset": "USDC",
            "denomination": 1000,
            "submit_onchain": true,
            "contract_address": "0xabc"
        }))
        .expect("pool simulation");
        let tx_hash = out["tx_hash"].as_str().expect("tx hash string");
        assert_eq!(out["status"], "submitted");
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);

        std::env::remove_var("AGENTHALO_ONCHAIN_SIMULATION");
        std::env::remove_var("AGENTHALO_HOME");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn execute_onchain_workflow_call_simulation_is_deterministic() {
        let _guard = env_lock().lock().expect("lock env");
        std::env::set_var("AGENTHALO_ONCHAIN_SIMULATION", "1");
        let payload = json!({
            "id": "deterministic-test",
            "amount": 42
        });
        let tx_a = execute_onchain_workflow_call(
            "bridgeTransfer(string,string,string,uint256,address)",
            vec![
                "base-sepolia".to_string(),
                "base-mainnet".to_string(),
                "USDC".to_string(),
                "42".to_string(),
                "0x1111111111111111111111111111111111111111".to_string(),
            ],
            &payload,
            Some("https://rpc.example"),
            Some("0xabc"),
            None,
            false,
            "agenthalo.det.test.v1",
        )
        .expect("tx_a");
        let tx_b = execute_onchain_workflow_call(
            "bridgeTransfer(string,string,string,uint256,address)",
            vec![
                "base-sepolia".to_string(),
                "base-mainnet".to_string(),
                "USDC".to_string(),
                "42".to_string(),
                "0x1111111111111111111111111111111111111111".to_string(),
            ],
            &payload,
            Some("https://rpc.example"),
            Some("0xabc"),
            None,
            false,
            "agenthalo.det.test.v1",
        )
        .expect("tx_b");
        assert_eq!(tx_a, tx_b);
        let tx_hash = tx_a.expect("expected hash");
        assert!(tx_hash.starts_with("0x"));
        assert_eq!(tx_hash.len(), 66);
        std::env::remove_var("AGENTHALO_ONCHAIN_SIMULATION");
    }
}
