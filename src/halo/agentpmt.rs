//! AgentPMT integration — tool proxy configuration and catalog.
//!
//! AgentPMT is an MCP-native tool infrastructure platform providing
//! budget-controlled access to 100+ third-party tools (Gmail, Stripe,
//! Google Workspace, blockchain scanners, etc.).  AgentHALO does NOT
//! sell products through AgentPMT.  Instead, wrapped agents can use
//! AgentPMT's tools, and AgentHALO records those calls in its trace
//! for observability.
//!
//! AgentHALO's own features (attest, audit, trust, sign) are gated
//! by the CAB license system, not by AgentPMT credits.
//!
//! ## Unified tool surface
//!
//! From the agent's perspective, all tools appear in a single MCP
//! `tools/list`.  Native AgentHALO tools appear as-is (`attest`,
//! `audit_contract`, etc.).  AgentPMT tools appear with an `agentpmt/`
//! prefix (e.g., `agentpmt/gmail_send`, `agentpmt/stripe_charge`).
//! This separation keeps the namespaces clean and lets AgentPMT
//! evolve independently.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the AgentPMT tool proxy integration.
///
/// When enabled, wrapped agents can discover and call AgentPMT tools.
/// Budget controls and credentials live on the AgentPMT side —
/// configured by the human via the AgentPMT dashboard.  AgentHALO
/// simply records tool calls in the trace for cost tracking.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentPmtConfig {
    /// Whether tool proxy is enabled (agents can use AgentPMT tools).
    pub enabled: bool,
    /// Optional label for budget tracking (e.g. "project-alpha").
    #[serde(default)]
    pub budget_tag: Option<String>,
    /// Optional MCP endpoint override (for local testing).
    #[serde(default)]
    pub mcp_endpoint: Option<String>,
    /// Optional bearer token fallback when env/vault is unavailable.
    #[serde(default)]
    pub auth_token: Option<String>,
    /// Updated-at epoch seconds.
    #[serde(default)]
    pub updated_at: u64,
}

// ---------------------------------------------------------------------------
// Tool catalog — cached snapshot of AgentPMT's available tools.
// Stored as JSON so it can be refreshed independently.
// ---------------------------------------------------------------------------

/// A single tool entry in the AgentPMT catalog.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProxiedTool {
    /// Tool name as known to AgentPMT (e.g. "gmail_send").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Category tag (e.g. "email", "payments", "blockchain").
    #[serde(default)]
    pub category: Option<String>,
    /// Optional MCP input schema for the tool.
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Option<Value>,
}

/// The full cached catalog.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToolCatalog {
    pub tools: Vec<ProxiedTool>,
    /// ISO-8601 timestamp of last refresh.
    #[serde(default)]
    pub refreshed_at: Option<String>,
    /// Number of marketplace tools discovered via AgentPMT-Tool-Search-and-Execution.
    /// The `tools` vec holds MCP interface tools (meta-tools); this count reflects
    /// the actual vendor products available through the marketplace.
    #[serde(default)]
    pub marketplace_tool_count: usize,
}

impl ToolCatalog {
    fn default_input_schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": true
        })
    }

    /// Return MCP-formatted tool list entries, prefixed with `agentpmt/`.
    pub fn as_mcp_tools(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| {
                json!({
                    "name": format!("agentpmt/{}", t.name),
                    "description": format!("[AgentPMT] {}", t.description),
                    "inputSchema": t
                        .input_schema
                        .clone()
                        .unwrap_or_else(Self::default_input_schema),
                })
            })
            .collect()
    }

    /// Check if a tool name (without prefix) exists in the catalog.
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name == name)
    }
}

pub fn default_tool_catalog() -> ToolCatalog {
    ToolCatalog {
        tools: vec![
            ProxiedTool {
                name: "gmail_send".to_string(),
                description: "Send email via Gmail".to_string(),
                category: Some("email".to_string()),
                input_schema: None,
            },
            ProxiedTool {
                name: "gmail_search".to_string(),
                description: "Search Gmail inbox".to_string(),
                category: Some("email".to_string()),
                input_schema: None,
            },
            ProxiedTool {
                name: "calendar_create_event".to_string(),
                description: "Create a Google Calendar event".to_string(),
                category: Some("productivity".to_string()),
                input_schema: None,
            },
            ProxiedTool {
                name: "stripe_charge".to_string(),
                description: "Create a Stripe payment charge".to_string(),
                category: Some("payments".to_string()),
                input_schema: None,
            },
            ProxiedTool {
                name: "etherscan_tx_lookup".to_string(),
                description: "Look up on-chain transaction details".to_string(),
                category: Some("blockchain".to_string()),
                input_schema: None,
            },
            ProxiedTool {
                name: "slack_post_message".to_string(),
                description: "Post a message to Slack".to_string(),
                category: Some("messaging".to_string()),
                input_schema: None,
            },
            // x402_pay and x402_verify are now native AgentHALO tools
            // (x402_pay, x402_check, x402_balance). Do not duplicate here.
        ],
        refreshed_at: Some(chrono::Utc::now().to_rfc3339()),
        marketplace_tool_count: 0,
    }
}

/// Record of a tool call routed through AgentPMT, for trace observability.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub tool_name: String,
    pub budget_tag: Option<String>,
    pub timestamp: u64,
    pub success: bool,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// File paths
// ---------------------------------------------------------------------------

pub fn agentpmt_config_path() -> PathBuf {
    crate::halo::config::halo_dir().join("agentpmt.json")
}

pub fn tool_catalog_path() -> PathBuf {
    crate::halo::config::halo_dir().join("agentpmt_tools.json")
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

fn normalized_opt(value: Option<&str>) -> Option<String> {
    value.and_then(|v| {
        let s = v.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    })
}

/// Resolve the AgentPMT MCP endpoint.
///
/// Order:
/// 1. `agentpmt.json:mcp_endpoint`
/// 2. `AGENTPMT_MCP_ENDPOINT`
/// 3. built-in testnet endpoint
pub fn resolved_mcp_endpoint(cfg: &AgentPmtConfig) -> String {
    normalized_opt(cfg.mcp_endpoint.as_deref())
        .or_else(|| normalized_opt(std::env::var("AGENTPMT_MCP_ENDPOINT").ok().as_deref()))
        .unwrap_or_else(|| "https://testnet.api.agentpmt.com/mcp".to_string())
}

fn token_from_vault() -> Option<String> {
    let wallet_path = crate::halo::config::pq_wallet_path();
    let vault_path = crate::halo::config::vault_path();
    if !wallet_path.exists() || !vault_path.exists() {
        return None;
    }
    let vault = crate::halo::vault::Vault::open(&wallet_path, &vault_path).ok()?;
    normalized_opt(vault.get_key("agentpmt").ok().as_deref())
}

/// Resolve AgentPMT bearer token from env/config/vault.
///
/// Order:
/// 1. `AGENTPMT_BEARER_TOKEN`
/// 2. `AGENTPMT_API_KEY`
/// 3. `agentpmt.json:auth_token`
/// 4. Vault key `agentpmt`
pub fn resolved_bearer_token(cfg: &AgentPmtConfig) -> Option<String> {
    normalized_opt(std::env::var("AGENTPMT_BEARER_TOKEN").ok().as_deref())
        .or_else(|| normalized_opt(std::env::var("AGENTPMT_API_KEY").ok().as_deref()))
        .or_else(|| normalized_opt(cfg.auth_token.as_deref()))
        .or_else(token_from_vault)
}

pub fn has_bearer_token() -> bool {
    let cfg = load_or_default();
    resolved_bearer_token(&cfg).is_some()
}

// ---------------------------------------------------------------------------
// Config persistence
// ---------------------------------------------------------------------------

pub fn load_config(path: &Path) -> Result<AgentPmtConfig, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read agentpmt config {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse agentpmt config {}: {e}", path.display()))
}

pub fn load_or_default() -> AgentPmtConfig {
    let path = agentpmt_config_path();
    load_config(&path).unwrap_or_default()
}

pub fn save_config(path: &Path, config: &AgentPmtConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create agentpmt config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(config)
        .map_err(|e| format!("serialize agentpmt config: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("write agentpmt config {}: {e}", path.display()))
}

pub fn is_tool_proxy_enabled() -> bool {
    load_or_default().enabled
}

fn sanitize_agentpmt_error(err: &ureq::Error) -> String {
    let msg = err.to_string();
    if msg.contains("Bearer ")
        || msg.contains("AGENTPMT_BEARER_TOKEN")
        || msg.contains("AGENTPMT_API_KEY")
    {
        "AgentPMT MCP request failed (credentials redacted)".to_string()
    } else {
        format!("AgentPMT MCP request failed: {msg}")
    }
}

fn mcp_call(method: &str, params: Value) -> Result<Value, String> {
    let cfg = load_or_default();
    let endpoint = resolved_mcp_endpoint(&cfg);
    let token = resolved_bearer_token(&cfg);

    let mut req = ureq::post(&endpoint)
        .header("Accept", "application/json")
        .content_type("application/json");
    if let Some(token) = token {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }

    let resp = req
        .send_json(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        }))
        .map_err(|e| sanitize_agentpmt_error(&e))?;

    let body: Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse AgentPMT MCP response: {e}"))?;

    if let Some(err) = body.get("error") {
        let code = err.get("code").and_then(|v| v.as_i64()).unwrap_or(-32000);
        let message = err
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown AgentPMT MCP error");
        return Err(format!("AgentPMT MCP error {code}: {message}"));
    }

    body.get("result")
        .cloned()
        .ok_or_else(|| "invalid AgentPMT MCP response: missing result".to_string())
}

// ---------------------------------------------------------------------------
// Tool catalog persistence
// ---------------------------------------------------------------------------

pub fn load_tool_catalog() -> ToolCatalog {
    let path = tool_catalog_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => ToolCatalog::default(),
    }
}

pub fn save_tool_catalog(catalog: &ToolCatalog) -> Result<(), String> {
    let path = tool_catalog_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create agentpmt tools dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(catalog)
        .map_err(|e| format!("serialize tool catalog: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write tool catalog {}: {e}", path.display()))
}

fn parse_remote_catalog(result: &Value) -> Result<ToolCatalog, String> {
    let tools = result
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "invalid AgentPMT tools/list result: missing tools array".to_string())?;

    let mut seen = HashSet::new();
    let mut parsed = Vec::new();
    for tool in tools {
        let raw_name = tool
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "invalid AgentPMT tool entry: missing name".to_string())?;
        let name = raw_name.strip_prefix("agentpmt/").unwrap_or(raw_name);
        if name.is_empty() || !seen.insert(name.to_string()) {
            continue;
        }
        let description = tool
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("AgentPMT tool");
        let category = tool
            .get("category")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                tool.get("annotations")
                    .and_then(|v| v.get("category"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            });
        let input_schema = tool
            .get("inputSchema")
            .cloned()
            .filter(|v| v.is_object())
            .or_else(|| {
                tool.get("parameters").cloned().map(|params| {
                    json!({
                        "type": "object",
                        "properties": params,
                        "additionalProperties": true,
                    })
                })
            });

        parsed.push(ProxiedTool {
            name: name.to_string(),
            description: description.to_string(),
            category,
            input_schema,
        });
    }

    if parsed.is_empty() {
        return Err("AgentPMT tools/list returned no tools".to_string());
    }

    Ok(ToolCatalog {
        tools: parsed,
        refreshed_at: Some(chrono::Utc::now().to_rfc3339()),
        marketplace_tool_count: 0, // populated by refresh_tool_catalog after discovery
    })
}

/// Discover actual marketplace tool count via `AgentPMT-Tool-Search-and-Execution`.
///
/// The MCP `tools/list` returns meta-tools (search, workflows, etc.), not the
/// real vendor products.  This function calls the search meta-tool with
/// `action: "get_tools"` to get the total count of available marketplace tools.
fn extract_mcp_text(result: &Value) -> Option<Value> {
    let text = result
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())?;
    serde_json::from_str(text).ok()
}

fn discover_marketplace_tool_count() -> usize {
    // First try page_size=1 — if the response has a total/pagination field, one request suffices.
    let result = mcp_call(
        "tools/call",
        json!({
            "name": "AgentPMT-Tool-Search-and-Execution",
            "arguments": { "action": "get_tools", "page": 1, "page_size": 100 }
        }),
    );
    let Ok(result) = result else {
        return 0;
    };
    let Some(parsed) = extract_mcp_text(&result) else {
        return 0;
    };

    // AgentPMT response: { pagination: { total_count, total_pages, ... }, tools: [...] }
    if let Some(total) = parsed
        .get("pagination")
        .and_then(|p| {
            p.get("total_count")
                .or_else(|| p.get("totalCount"))
                .or_else(|| p.get("total"))
        })
        .or_else(|| parsed.get("total"))
        .or_else(|| parsed.get("totalCount"))
        .or_else(|| parsed.get("total_count"))
        .and_then(|v| v.as_u64())
    {
        return total as usize;
    }

    // Fallback: count items in the response array
    parsed
        .get("tools")
        .or_else(|| parsed.get("products"))
        .or_else(|| parsed.get("results"))
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

/// Refresh the tool catalog.
///
/// Uses AgentPMT MCP `tools/list` over HTTP JSON-RPC.
/// If `AGENTHALO_AGENTPMT_STUB=1` is set, uses built-in defaults.
/// Also discovers the marketplace tool count via the search meta-tool.
pub fn refresh_tool_catalog() -> Result<ToolCatalog, String> {
    let mut catalog = if is_truthy_env("AGENTHALO_AGENTPMT_STUB") {
        default_tool_catalog()
    } else {
        let result = mcp_call("tools/list", json!({}))?;
        parse_remote_catalog(&result)?
    };
    catalog.marketplace_tool_count = discover_marketplace_tool_count();
    save_tool_catalog(&catalog)?;
    Ok(catalog)
}

/// Get the merged tool list: if tool proxy is enabled and a catalog
/// exists, return the proxied tools.  Otherwise return empty.
pub fn proxied_tools_for_listing() -> Vec<Value> {
    if !is_tool_proxy_enabled() {
        return vec![];
    }
    let cached = load_tool_catalog();
    if !cached.tools.is_empty() {
        return cached.as_mcp_tools();
    }
    match refresh_tool_catalog() {
        Ok(catalog) => catalog.as_mcp_tools(),
        Err(_) => vec![],
    }
}

/// Check whether a tool call should be proxied to AgentPMT.
/// Returns the unprefixed tool name if yes.
pub fn is_proxied_tool(name: &str) -> Option<String> {
    let suffix = name.strip_prefix("agentpmt/")?;
    if suffix.trim().is_empty() {
        return None;
    }
    Some(suffix.to_string())
}

/// Forward a proxied AgentPMT tool call through MCP `tools/call`.
pub fn call_tool(tool_name: &str, arguments: Value) -> Result<Value, String> {
    if is_truthy_env("AGENTHALO_AGENTPMT_STUB") {
        return Ok(json!({
            "content": [
                {
                    "type": "text",
                    "text": json!({
                        "status": "ok",
                        "stub": true,
                        "tool": tool_name,
                        "arguments": arguments
                    }).to_string()
                }
            ],
            "isError": false
        }));
    }

    let primary = mcp_call(
        "tools/call",
        json!({
            "name": tool_name,
            "arguments": arguments
        }),
    );
    match primary {
        Ok(v) => Ok(v),
        Err(e) => {
            // Compatibility fallback: some deployments may expect namespaced tool IDs.
            if tool_name.starts_with("agentpmt/")
                || !(e.contains("-32601")
                    || e.contains("-32602")
                    || e.to_ascii_lowercase().contains("unknown"))
            {
                return Err(e);
            }
            mcp_call(
                "tools/call",
                json!({
                    "name": format!("agentpmt/{tool_name}"),
                    "arguments": arguments
                }),
            )
        }
    }
}

pub fn extract_tool_call_error(proxy_result: &Value) -> Option<String> {
    if !proxy_result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let text = proxy_result
        .get("content")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("text"))
        .and_then(|v| v.as_str())
        .unwrap_or("AgentPMT tool call failed");
    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
        if let Some(msg) = parsed.get("message").and_then(|v| v.as_str()) {
            return Some(msg.to_string());
        }
    }
    Some(text.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_test_agentpmt_{}_{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("agentpmt.json");

        let config = AgentPmtConfig {
            enabled: true,
            budget_tag: Some("test-project".to_string()),
            mcp_endpoint: None,
            auth_token: None,
            updated_at: now_secs(),
        };
        save_config(&path, &config).expect("save config");
        let loaded = load_config(&path).expect("load config");
        assert!(loaded.enabled);
        assert_eq!(loaded.budget_tag.as_deref(), Some("test-project"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_is_disabled() {
        let cfg = AgentPmtConfig::default();
        assert!(!cfg.enabled);
        assert!(cfg.budget_tag.is_none());
    }

    #[test]
    fn legacy_config_deserializes_with_defaults() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_test_agentpmt_legacy_{}_{}",
            std::process::id(),
            now_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("agentpmt.json");
        std::fs::write(&path, r#"{"enabled":true}"#).expect("write legacy config");

        let loaded = load_config(&path).expect("load legacy config");
        assert!(loaded.enabled);
        assert!(loaded.budget_tag.is_none());
        assert!(loaded.mcp_endpoint.is_none());
        assert!(loaded.auth_token.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tool_catalog_roundtrip() {
        let catalog = ToolCatalog {
            tools: vec![
                ProxiedTool {
                    name: "gmail_send".to_string(),
                    description: "Send an email via Gmail".to_string(),
                    category: Some("email".to_string()),
                    input_schema: None,
                },
                ProxiedTool {
                    name: "stripe_charge".to_string(),
                    description: "Create a Stripe charge".to_string(),
                    category: Some("payments".to_string()),
                    input_schema: None,
                },
            ],
            refreshed_at: Some("2026-02-24T12:00:00Z".to_string()),
            marketplace_tool_count: 42,
        };

        let json = serde_json::to_string_pretty(&catalog).expect("serialize");
        let loaded: ToolCatalog = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(loaded.tools.len(), 2);
        assert_eq!(loaded.tools[0].name, "gmail_send");
        assert!(loaded.has_tool("gmail_send"));
        assert!(!loaded.has_tool("unknown_tool"));
    }

    #[test]
    fn mcp_tools_have_prefix() {
        let catalog = ToolCatalog {
            tools: vec![ProxiedTool {
                name: "gmail_send".to_string(),
                description: "Send email".to_string(),
                category: None,
                input_schema: None,
            }],
            refreshed_at: None,
            marketplace_tool_count: 0,
        };
        let mcp = catalog.as_mcp_tools();
        assert_eq!(mcp.len(), 1);
        assert_eq!(mcp[0]["name"], "agentpmt/gmail_send");
        assert!(mcp[0]["description"]
            .as_str()
            .unwrap()
            .starts_with("[AgentPMT]"));
        assert_eq!(mcp[0]["inputSchema"]["type"], "object");
    }

    #[test]
    fn is_proxied_tool_works() {
        assert_eq!(
            is_proxied_tool("agentpmt/gmail_send"),
            Some("gmail_send".to_string())
        );
        assert_eq!(is_proxied_tool("attest"), None);
        assert_eq!(is_proxied_tool("agentpmt/"), None);
    }

    #[test]
    fn default_catalog_has_expected_tools() {
        let catalog = default_tool_catalog();
        assert!(catalog.has_tool("gmail_send"));
        assert!(catalog.has_tool("stripe_charge"));
        // x402_pay and x402_verify are now native AgentHALO tools, not in AgentPMT catalog.
        assert!(!catalog.has_tool("x402_pay"));
        assert!(!catalog.has_tool("x402_verify"));
        assert!(catalog.refreshed_at.is_some());
    }

    #[test]
    fn resolved_endpoint_prefers_config_then_env_then_default() {
        let mut cfg = AgentPmtConfig::default();
        std::env::remove_var("AGENTPMT_MCP_ENDPOINT");
        assert_eq!(
            resolved_mcp_endpoint(&cfg),
            "https://testnet.api.agentpmt.com/mcp"
        );
        std::env::set_var("AGENTPMT_MCP_ENDPOINT", "https://env.example/mcp");
        assert_eq!(resolved_mcp_endpoint(&cfg), "https://env.example/mcp");
        cfg.mcp_endpoint = Some("https://cfg.example/mcp".to_string());
        assert_eq!(resolved_mcp_endpoint(&cfg), "https://cfg.example/mcp");
        std::env::remove_var("AGENTPMT_MCP_ENDPOINT");
    }

    #[test]
    fn parse_remote_catalog_strips_agentpmt_prefix() {
        let result = json!({
            "tools": [
                {
                    "name": "agentpmt/gmail_send",
                    "description": "Send email",
                    "category": "email",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"to": {"type": "string"}}
                    }
                },
                {"name": "stripe_charge", "description": "Charge customer", "annotations": {"category": "payments"}}
            ]
        });
        let catalog = parse_remote_catalog(&result).expect("parse catalog");
        assert_eq!(catalog.tools.len(), 2);
        assert!(catalog.has_tool("gmail_send"));
        assert!(catalog.has_tool("stripe_charge"));
        assert_eq!(catalog.tools[1].category.as_deref(), Some("payments"));
        assert_eq!(
            catalog.tools[0]
                .input_schema
                .as_ref()
                .and_then(|v| v.get("type")),
            Some(&json!("object"))
        );
    }

    #[test]
    fn extract_tool_call_error_parses_json_message() {
        let msg = extract_tool_call_error(&json!({
            "isError": true,
            "content": [{"type":"text","text":"{\"status\":\"error\",\"message\":\"denied\"}"}]
        }))
        .expect("error message");
        assert_eq!(msg, "denied");
    }

    #[test]
    fn call_tool_stub_mode_returns_success_payload() {
        std::env::set_var("AGENTHALO_AGENTPMT_STUB", "1");
        let result = call_tool("gmail_send", json!({"to":"a@example.com"})).expect("stub call");
        assert_eq!(result["isError"], false);
        std::env::remove_var("AGENTHALO_AGENTPMT_STUB");
    }
}
