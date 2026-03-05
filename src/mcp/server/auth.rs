//! Dual authentication for the NucleusDB remote MCP server.
//!
//! Two auth paths, evaluated in order:
//!
//! 1. **CAB-as-bearer-token**: `Authorization: Bearer cab:<base64(json)>` where the JSON
//!    payload contains `{ agent_address, contract_address, rpc_url, signature }`.
//!    Verified against the on-chain TrustVerifier contract.
//!
//! 2. **OAuth 2.1 JWT**: `Authorization: Bearer <jwt>` — standard JWT validated against
//!    a configured shared secret (HS256).
//!
//! Per-tool scope enforcement gates each MCP tool call to a required scope.

use axum::{
    body::Body,
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::Engine;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

const INTERNAL_AUTH_HEADER: &str = "x-nucleusdb-internal-auth";

/// Scopes that gate access to tool categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolScope {
    /// Read-only DB operations: help, status, query, verify, export, history.
    Read,
    /// Trust verification (read-only on-chain queries).
    TrustVerify,
    /// Write DB operations: execute_sql, create_database, open_database, checkpoint, channels.
    Write,
    /// Trust attestation (on-chain submit): agent_register, agent_reattest, register_chain, etc.
    TrustAttest,
    /// Container launch.
    Container,
}

impl ToolScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::TrustVerify => "trust:verify",
            Self::Write => "write",
            Self::TrustAttest => "trust:attest",
            Self::Container => "container",
        }
    }

    pub fn parse_scope(s: &str) -> Option<Self> {
        match s {
            "read" => Some(Self::Read),
            "trust:verify" => Some(Self::TrustVerify),
            "write" => Some(Self::Write),
            "trust:attest" => Some(Self::TrustAttest),
            "container" => Some(Self::Container),
            _ => None,
        }
    }

    /// Return the required scope for a given MCP tool name.
    pub fn for_tool(tool_name: &str) -> Self {
        match tool_name {
            // Read-only DB
            "nucleusdb_help"
            | "nucleusdb_status"
            | "nucleusdb_query"
            | "nucleusdb_query_range"
            | "nucleusdb_verify"
            | "nucleusdb_export"
            | "nucleusdb_history"
            | "abraxas_query_records"
            | "abraxas_record_status"
            | "abraxas_merge_status"
            | "abraxas_workspace_diff"
            | "access_list"
            | "access_verify"
            | "access_evaluate"
            | "proof_gate_status"
            | "proof_gate_verify"
            | "proof_gate_requirements" => Self::Read,
            // Trust verification (read-only chain queries)
            "nucleusdb_verify_agent"
            | "verify_agent_multichain"
            | "register_chain"
            | "zk_verify_credential"
            | "zk_verify_anonymous_membership"
            | "zk_compute_verify" => Self::TrustVerify,
            // Write DB operations
            "nucleusdb_execute_sql"
            | "nucleusdb_create_database"
            | "nucleusdb_open_database"
            | "nucleusdb_checkpoint"
            | "abraxas_submit_record"
            | "abraxas_resolve_conflict"
            | "abraxas_export_git"
            | "abraxas_workspace_init"
            | "abraxas_workspace_submit"
            | "access_grant"
            | "access_revoke"
            | "proof_gate_submit" => Self::Write,
            // Trust attestation (on-chain submit)
            "nucleusdb_agent_register"
            | "submit_composite_attestation"
            | "zk_prove_credential"
            | "zk_prove_anonymous_membership"
            | "zk_compute_prove" => Self::TrustAttest,
            // Container lifecycle
            "nucleusdb_container_launch"
            | "nucleusdb_container_list"
            | "nucleusdb_container_status"
            | "nucleusdb_container_stop"
            | "nucleusdb_container_logs" => Self::Container,
            // Mesh network (read-only discovery)
            "mesh_peers" | "mesh_ping" => Self::Read,
            // Mesh network (write: remote calls and envelope exchange)
            "mesh_call" | "mesh_exchange_envelope" => Self::Write,
            // Mesh network (grant: capability delegation)
            "mesh_grant" => Self::TrustAttest,
            // Orchestrator agent lifecycle
            "orchestrator_list" | "orchestrator_get_result" => Self::Read,
            "orchestrator_launch"
            | "orchestrator_send_task"
            | "orchestrator_pipe"
            | "orchestrator_stop" => Self::Container,
            // Unknown tools default to most restrictive
            _ => Self::TrustAttest,
        }
    }
}

impl std::str::FromStr for ToolScope {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse_scope(s).ok_or(())
    }
}

/// Authenticated identity extracted from a valid token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthIdentity {
    /// Agent wallet address (CAB) or OAuth subject.
    pub subject: String,
    /// Authentication method used.
    pub method: AuthMethod,
    /// Granted scopes.
    pub scopes: HashSet<String>,
    /// PUF tier from on-chain attestation (CAB only).
    pub puf_tier: Option<u8>,
    /// Whether the agent is verified on-chain (CAB only).
    pub verified_onchain: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuthMethod {
    Cab,
    Oauth,
}

impl AuthIdentity {
    /// Check whether this identity has the required scope.
    pub fn has_scope(&self, scope: ToolScope) -> bool {
        self.scopes.contains(scope.as_str())
    }
}

/// CAB bearer token payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CabToken {
    /// Agent wallet address (0x-prefixed).
    pub agent_address: String,
    /// TrustVerifier contract address.
    pub contract_address: String,
    /// EVM RPC URL — must match a trusted RPC in the server's allowlist.
    pub rpc_url: String,
    /// Hex-encoded signature for replay prevention. Required when auth is enabled.
    pub signature: Option<String>,
    /// Unique nonce (e.g. UUID hex) to prevent replay attacks.
    pub nonce: Option<String>,
    /// Unix timestamp (seconds). Tokens older than `cab_max_age_secs` are rejected.
    pub timestamp: Option<u64>,
}

/// OAuth JWT claims.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClaims {
    pub sub: String,
    pub scope: Option<String>,
    pub exp: Option<u64>,
    pub iat: Option<u64>,
}

/// Server-level auth configuration.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// When true, auth is required. When false, all requests pass (dev mode).
    pub enabled: bool,
    /// Secret for JWT validation (HS256). If empty, JWT auth is disabled.
    pub jwt_secret: String,
    /// Default scopes granted to CAB-authenticated agents by tier.
    /// Tier 1-2 (consumer/server): read + trust:verify.
    /// Tier 3 (server_tpm): read + trust:verify + write.
    /// Tier 4 (dgx): all scopes.
    pub cab_tier_scopes: Vec<(u8, Vec<ToolScope>)>,
    /// Trusted RPC URLs for CAB token verification. If non-empty, only these
    /// RPC URLs are accepted — prevents attacker-controlled oracle attacks.
    pub trusted_rpc_urls: Vec<String>,
    /// Maximum age (in seconds) for CAB token timestamps. Default: 300 (5 min).
    pub cab_max_age_secs: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            jwt_secret: String::new(),
            cab_tier_scopes: vec![
                (1, vec![ToolScope::Read, ToolScope::TrustVerify]),
                (2, vec![ToolScope::Read, ToolScope::TrustVerify]),
                (
                    3,
                    vec![ToolScope::Read, ToolScope::TrustVerify, ToolScope::Write],
                ),
                (
                    4,
                    vec![
                        ToolScope::Read,
                        ToolScope::TrustVerify,
                        ToolScope::Write,
                        ToolScope::TrustAttest,
                        ToolScope::Container,
                    ],
                ),
            ],
            trusted_rpc_urls: Vec::new(),
            cab_max_age_secs: 300,
        }
    }
}

impl AuthConfig {
    /// Resolve scopes for a given CAB tier.
    pub fn scopes_for_tier(&self, tier: u8) -> HashSet<String> {
        self.cab_tier_scopes
            .iter()
            .find(|(t, _)| *t == tier)
            .map(|(_, scopes)| scopes.iter().map(|s| s.as_str().to_string()).collect())
            .unwrap_or_else(|| {
                // Default: read-only for unknown tiers.
                [ToolScope::Read.as_str().to_string()].into_iter().collect()
            })
    }
}

/// Extract and validate authentication from request headers.
///
/// Returns `Ok(Some(identity))` on valid auth, `Ok(None)` when auth is disabled,
/// `Err(status)` on invalid credentials.
pub fn authenticate(
    headers: &HeaderMap,
    config: &AuthConfig,
) -> Result<Option<AuthIdentity>, (StatusCode, String)> {
    if !config.enabled {
        return Ok(None);
    }

    let auth_header = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "missing Authorization header".to_string(),
        ))?;

    let token = auth_header
        .strip_prefix("Bearer ")
        .or_else(|| auth_header.strip_prefix("bearer "))
        .ok_or((
            StatusCode::UNAUTHORIZED,
            "expected Bearer token".to_string(),
        ))?
        .trim();

    // Try CAB token first.
    if let Some(cab_token) = token.strip_prefix("cab:") {
        return authenticate_cab(cab_token, config);
    }

    // Try OAuth JWT.
    if !config.jwt_secret.is_empty() {
        return authenticate_jwt(token, config);
    }

    Err((
        StatusCode::UNAUTHORIZED,
        "no valid auth method available".to_string(),
    ))
}

/// Validate a CAB bearer token by verifying the agent on-chain.
fn authenticate_cab(
    encoded: &str,
    config: &AuthConfig,
) -> Result<Option<AuthIdentity>, (StatusCode, String)> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("invalid cab token encoding: {e}"),
            )
        })?;

    let cab: CabToken = serde_json::from_slice(&decoded).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid cab token payload: {e}"),
        )
    })?;

    if cab.agent_address.trim().is_empty()
        || cab.contract_address.trim().is_empty()
        || cab.rpc_url.trim().is_empty()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "cab token requires agent_address, contract_address, rpc_url".to_string(),
        ));
    }

    // RPC URL allowlist — reject attacker-controlled oracles.
    if !config.trusted_rpc_urls.is_empty() {
        let rpc_trimmed = cab.rpc_url.trim();
        if !config.trusted_rpc_urls.iter().any(|u| u == rpc_trimmed) {
            return Err((
                StatusCode::FORBIDDEN,
                format!(
                    "rpc_url '{}' is not in the trusted RPC allowlist",
                    rpc_trimmed
                ),
            ));
        }
    }

    // Replay protection — require timestamp + nonce when auth is enforced.
    if let Some(ts) = cab.timestamp {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if ts > now + 60 {
            return Err((
                StatusCode::BAD_REQUEST,
                "cab token timestamp is in the future".to_string(),
            ));
        }
        if now.saturating_sub(ts) > config.cab_max_age_secs {
            return Err((
                StatusCode::UNAUTHORIZED,
                format!(
                    "cab token expired (age {}s exceeds max {}s)",
                    now.saturating_sub(ts),
                    config.cab_max_age_secs
                ),
            ));
        }
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "cab token requires 'timestamp' field for replay protection".to_string(),
        ));
    }

    if cab.nonce.as_ref().is_none_or(|n| n.trim().is_empty()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "cab token requires non-empty 'nonce' field for replay protection".to_string(),
        ));
    }

    if cab.signature.as_ref().is_none_or(|s| s.trim().is_empty()) {
        return Err((
            StatusCode::BAD_REQUEST,
            "cab token requires non-empty 'signature' field".to_string(),
        ));
    }

    // Verify on-chain via cast.
    let status = crate::trust::onchain::verify_agent_onchain(
        cab.rpc_url.trim(),
        cab.contract_address.trim(),
        cab.agent_address.trim(),
    )
    .map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            format!("on-chain verification failed: {e}"),
        )
    })?;

    if !status.verified {
        return Err((
            StatusCode::FORBIDDEN,
            format!("agent {} is not verified on-chain", cab.agent_address),
        ));
    }

    let tier = status.tier.unwrap_or(1);
    let scopes = config.scopes_for_tier(tier);

    Ok(Some(AuthIdentity {
        subject: cab.agent_address.trim().to_string(),
        method: AuthMethod::Cab,
        scopes,
        puf_tier: Some(tier),
        verified_onchain: Some(true),
    }))
}

/// Validate an OAuth JWT bearer token.
fn authenticate_jwt(
    token: &str,
    config: &AuthConfig,
) -> Result<Option<AuthIdentity>, (StatusCode, String)> {
    let key = DecodingKey::from_secret(config.jwt_secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.required_spec_claims = ["sub"].iter().map(|s| s.to_string()).collect();

    let token_data = decode::<OAuthClaims>(token, &key, &validation)
        .map_err(|e| (StatusCode::UNAUTHORIZED, format!("invalid JWT: {e}")))?;

    let claims = token_data.claims;
    let scopes: HashSet<String> = claims
        .scope
        .unwrap_or_default()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    // If no scopes in token, grant read-only by default.
    let scopes = if scopes.is_empty() {
        [ToolScope::Read.as_str().to_string()].into_iter().collect()
    } else {
        scopes
    };

    Ok(Some(AuthIdentity {
        subject: claims.sub,
        method: AuthMethod::Oauth,
        scopes,
        puf_tier: None,
        verified_onchain: None,
    }))
}

fn validate_tool_scope_from_jsonrpc(
    identity: &AuthIdentity,
    json: &serde_json::Value,
) -> Result<(), (StatusCode, String)> {
    let Some(obj) = json.as_object() else {
        return Ok(());
    };
    let Some(method) = obj.get("method").and_then(|m| m.as_str()) else {
        return Ok(());
    };
    if method != "tools/call" {
        return Ok(());
    }
    let tool_name = obj
        .get("params")
        .and_then(|p| p.as_object())
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .ok_or((
            StatusCode::BAD_REQUEST,
            "tools/call requires params.name".to_string(),
        ))?;
    let required = ToolScope::for_tool(tool_name);
    if identity.has_scope(required) {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        format!(
            "subject '{}' lacks scope '{}' required for tool '{}'",
            identity.subject,
            required.as_str(),
            tool_name
        ),
    ))
}

fn validate_tool_scope_from_request_body(
    identity: &AuthIdentity,
    body: &[u8],
) -> Result<(), (StatusCode, String)> {
    if body.is_empty() {
        return Ok(());
    }
    let parsed: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => {
            // Let rmcp parse/validate malformed JSON-RPC.
            return Ok(());
        }
    };
    if let Some(items) = parsed.as_array() {
        for item in items {
            validate_tool_scope_from_jsonrpc(identity, item)?;
        }
        return Ok(());
    }
    validate_tool_scope_from_jsonrpc(identity, &parsed)
}

/// Axum middleware layer that extracts auth identity and stores it in request extensions.
pub async fn auth_middleware(
    axum::extract::State(config): axum::extract::State<Arc<AuthConfig>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    // DIDComm envelopes are authenticated at the message layer (dual signatures + DID binding).
    // Keep transport auth disabled for this route so mesh peers can deliver envelopes without
    // requiring an MCP bearer token.
    if matches!(
        request.uri().path(),
        "/didcomm" | "/.well-known/nucleus-pod"
    ) {
        return next.run(request).await;
    }

    // Internal loopback dispatch from /didcomm -> /mcp uses a process-local shared secret.
    if has_internal_auth_bypass(request.headers()) {
        return next.run(request).await;
    }

    match authenticate(request.headers(), &config) {
        Ok(identity) => {
            if let Some(id) = identity {
                let (parts, body) = request.into_parts();
                let body_bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        let body = serde_json::json!({
                            "error": format!("failed to read request body: {e}")
                        });
                        return (StatusCode::BAD_REQUEST, axum::Json(body)).into_response();
                    }
                };
                if let Err((status, message)) =
                    validate_tool_scope_from_request_body(&id, &body_bytes)
                {
                    let body = serde_json::json!({
                        "error": message,
                        "hint": "Use a token with the required tool scope"
                    });
                    return (status, axum::Json(body)).into_response();
                }
                let mut request = Request::from_parts(parts, Body::from(body_bytes));
                request.extensions_mut().insert(id);
                return next.run(request).await;
            }
            next.run(request).await
        }
        Err((status, message)) => {
            let body = serde_json::json!({
                "error": message,
                "hint": "Use Authorization: Bearer cab:<base64(json)> for CAB auth or Bearer <jwt> for OAuth"
            });
            (status, axum::Json(body)).into_response()
        }
    }
}

fn has_internal_auth_bypass(headers: &HeaderMap) -> bool {
    let expected = match std::env::var("NUCLEUSDB_INTERNAL_AUTH_KEY") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return false,
    };
    let Some(provided) = headers.get(INTERNAL_AUTH_HEADER) else {
        return false;
    };
    provided
        .to_str()
        .map(|value| value == expected)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;

    #[test]
    fn tool_scope_mapping_covers_all_tools() {
        let db_tools = [
            "nucleusdb_help",
            "nucleusdb_status",
            "nucleusdb_query",
            "nucleusdb_query_range",
            "nucleusdb_verify",
            "nucleusdb_export",
            "nucleusdb_history",
            "abraxas_query_records",
            "abraxas_record_status",
            "abraxas_merge_status",
            "abraxas_workspace_diff",
            "access_list",
            "access_verify",
            "access_evaluate",
            "proof_gate_status",
            "proof_gate_verify",
            "proof_gate_requirements",
        ];
        for t in &db_tools {
            assert_eq!(ToolScope::for_tool(t), ToolScope::Read, "tool {t}");
        }

        let trust_read = [
            "nucleusdb_verify_agent",
            "verify_agent_multichain",
            "register_chain",
            "zk_verify_credential",
            "zk_verify_anonymous_membership",
            "zk_compute_verify",
        ];
        for t in &trust_read {
            assert_eq!(ToolScope::for_tool(t), ToolScope::TrustVerify, "tool {t}");
        }

        let write_tools = [
            "nucleusdb_execute_sql",
            "nucleusdb_create_database",
            "nucleusdb_open_database",
            "nucleusdb_checkpoint",
            "abraxas_submit_record",
            "abraxas_resolve_conflict",
            "abraxas_export_git",
            "abraxas_workspace_init",
            "abraxas_workspace_submit",
            "access_grant",
            "access_revoke",
            "proof_gate_submit",
        ];
        for t in &write_tools {
            assert_eq!(ToolScope::for_tool(t), ToolScope::Write, "tool {t}");
        }

        let attest_tools = [
            "nucleusdb_agent_register",
            "submit_composite_attestation",
            "zk_prove_credential",
            "zk_prove_anonymous_membership",
            "zk_compute_prove",
        ];
        for t in &attest_tools {
            assert_eq!(ToolScope::for_tool(t), ToolScope::TrustAttest, "tool {t}");
        }

        let container_tools = [
            "nucleusdb_container_launch",
            "nucleusdb_container_list",
            "nucleusdb_container_status",
            "nucleusdb_container_stop",
            "nucleusdb_container_logs",
        ];
        for t in &container_tools {
            assert_eq!(ToolScope::for_tool(t), ToolScope::Container, "tool {t}");
        }
        // Unknown tools → most restrictive
        assert_eq!(ToolScope::for_tool("unknown_tool"), ToolScope::TrustAttest);
    }

    #[test]
    fn default_config_tier_scopes() {
        let config = AuthConfig::default();
        let t1 = config.scopes_for_tier(1);
        assert!(t1.contains("read"));
        assert!(t1.contains("trust:verify"));
        assert!(!t1.contains("write"));

        let t4 = config.scopes_for_tier(4);
        assert!(t4.contains("read"));
        assert!(t4.contains("trust:verify"));
        assert!(t4.contains("write"));
        assert!(t4.contains("trust:attest"));
        assert!(t4.contains("container"));
    }

    #[test]
    fn disabled_auth_passes_everything() {
        let config = AuthConfig {
            enabled: false,
            ..Default::default()
        };
        let headers = HeaderMap::new();
        let result = authenticate(&headers, &config);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn default_auth_is_enabled() {
        let config = AuthConfig::default();
        assert!(config.enabled, "auth should be enabled by default");
    }

    #[test]
    fn cab_token_roundtrip() {
        let cab = CabToken {
            agent_address: "0x1234".to_string(),
            contract_address: "0x5678".to_string(),
            rpc_url: "https://rpc.example.com".to_string(),
            signature: Some("0xabcd".to_string()),
            nonce: Some("deadbeef".to_string()),
            timestamp: Some(1700000000),
        };
        let json = serde_json::to_vec(&cab).unwrap();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&json);
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .unwrap();
        let parsed: CabToken = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(parsed.agent_address, "0x1234");
        assert_eq!(parsed.nonce, Some("deadbeef".to_string()));
        assert_eq!(parsed.timestamp, Some(1700000000));
    }

    #[test]
    fn jwt_auth_validates_token() {
        use jsonwebtoken::{encode, EncodingKey, Header};

        let secret = "test-secret-key-for-nucleusdb";
        let config = AuthConfig {
            enabled: true,
            jwt_secret: secret.to_string(),
            ..Default::default()
        };

        let claims = OAuthClaims {
            sub: "agent-001".to_string(),
            scope: Some("read trust:verify write".to_string()),
            exp: Some(u64::MAX),
            iat: Some(0),
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {token}").parse().unwrap());

        let result = authenticate(&headers, &config).unwrap().unwrap();
        assert_eq!(result.subject, "agent-001");
        assert_eq!(result.method, AuthMethod::Oauth);
        assert!(result.has_scope(ToolScope::Read));
        assert!(result.has_scope(ToolScope::TrustVerify));
        assert!(result.has_scope(ToolScope::Write));
        assert!(!result.has_scope(ToolScope::TrustAttest));
    }

    #[test]
    fn internal_auth_bypass_requires_exact_match() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let mut headers = HeaderMap::new();
        std::env::set_var("NUCLEUSDB_INTERNAL_AUTH_KEY", "test-internal-key");
        headers.insert(INTERNAL_AUTH_HEADER, "test-internal-key".parse().unwrap());
        assert!(has_internal_auth_bypass(&headers));
        headers.insert(INTERNAL_AUTH_HEADER, "wrong-key".parse().unwrap());
        assert!(!has_internal_auth_bypass(&headers));
        std::env::remove_var("NUCLEUSDB_INTERNAL_AUTH_KEY");
        assert!(!has_internal_auth_bypass(&headers));
    }

    fn mk_identity_with_scopes(scopes: &[&str]) -> AuthIdentity {
        AuthIdentity {
            subject: "qa-agent".to_string(),
            method: AuthMethod::Oauth,
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
            puf_tier: None,
            verified_onchain: None,
        }
    }

    #[test]
    fn scope_validation_allows_read_and_blocks_write() {
        let id = mk_identity_with_scopes(&["read"]);
        let read_call = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "nucleusdb_status",
                "arguments": {}
            }
        });
        let write_call = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "nucleusdb_execute_sql",
                "arguments": { "sql": "COMMIT;" }
            }
        });
        let read_bytes = serde_json::to_vec(&read_call).unwrap();
        let write_bytes = serde_json::to_vec(&write_call).unwrap();

        assert!(validate_tool_scope_from_request_body(&id, &read_bytes).is_ok());
        let denied = validate_tool_scope_from_request_body(&id, &write_bytes).unwrap_err();
        assert_eq!(denied.0, StatusCode::FORBIDDEN);
        assert!(denied.1.contains("nucleusdb_execute_sql"));
    }

    #[test]
    fn scope_validation_blocks_unknown_tools_by_default() {
        let id = mk_identity_with_scopes(&["read", "write", "trust:verify"]);
        let unknown_call = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": {
                "name": "mystery_tool",
                "arguments": {}
            }
        });
        let unknown_bytes = serde_json::to_vec(&unknown_call).unwrap();
        let denied = validate_tool_scope_from_request_body(&id, &unknown_bytes).unwrap_err();
        assert_eq!(denied.0, StatusCode::FORBIDDEN);
        assert!(denied.1.contains("trust:attest"));
    }
}
