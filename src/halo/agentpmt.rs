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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentPmtConfig {
    /// Whether tool proxy is enabled (agents can use AgentPMT tools).
    pub enabled: bool,
    /// Optional label for budget tracking (e.g. "project-alpha").
    #[serde(default)]
    pub budget_tag: Option<String>,
    /// Optional MCP endpoint override (for local testing).
    #[serde(default)]
    pub mcp_endpoint: Option<String>,
    /// Updated-at epoch seconds.
    #[serde(default)]
    pub updated_at: u64,
}

impl Default for AgentPmtConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            budget_tag: None,
            mcp_endpoint: None,
            updated_at: 0,
        }
    }
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
}

/// The full cached catalog.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ToolCatalog {
    pub tools: Vec<ProxiedTool>,
    /// ISO-8601 timestamp of last refresh.
    #[serde(default)]
    pub refreshed_at: Option<String>,
}

impl ToolCatalog {
    /// Return MCP-formatted tool list entries, prefixed with `agentpmt/`.
    pub fn as_mcp_tools(&self) -> Vec<serde_json::Value> {
        self.tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": format!("agentpmt/{}", t.name),
                    "description": format!("[AgentPMT] {}", t.description)
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
            },
            ProxiedTool {
                name: "gmail_search".to_string(),
                description: "Search Gmail inbox".to_string(),
                category: Some("email".to_string()),
            },
            ProxiedTool {
                name: "calendar_create_event".to_string(),
                description: "Create a Google Calendar event".to_string(),
                category: Some("productivity".to_string()),
            },
            ProxiedTool {
                name: "stripe_charge".to_string(),
                description: "Create a Stripe payment charge".to_string(),
                category: Some("payments".to_string()),
            },
            ProxiedTool {
                name: "etherscan_tx_lookup".to_string(),
                description: "Look up on-chain transaction details".to_string(),
                category: Some("blockchain".to_string()),
            },
            ProxiedTool {
                name: "slack_post_message".to_string(),
                description: "Post a message to Slack".to_string(),
                category: Some("messaging".to_string()),
            },
        ],
        refreshed_at: Some(chrono::Utc::now().to_rfc3339()),
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

pub fn refresh_tool_catalog() -> Result<ToolCatalog, String> {
    let catalog = default_tool_catalog();
    save_tool_catalog(&catalog)?;
    Ok(catalog)
}

/// Get the merged tool list: if tool proxy is enabled and a catalog
/// exists, return the proxied tools.  Otherwise return empty.
pub fn proxied_tools_for_listing() -> Vec<serde_json::Value> {
    if !is_tool_proxy_enabled() {
        return vec![];
    }
    load_tool_catalog().as_mcp_tools()
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
                },
                ProxiedTool {
                    name: "stripe_charge".to_string(),
                    description: "Create a Stripe charge".to_string(),
                    category: Some("payments".to_string()),
                },
            ],
            refreshed_at: Some("2026-02-24T12:00:00Z".to_string()),
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
            }],
            refreshed_at: None,
        };
        let mcp = catalog.as_mcp_tools();
        assert_eq!(mcp.len(), 1);
        assert_eq!(mcp[0]["name"], "agentpmt/gmail_send");
        assert!(mcp[0]["description"]
            .as_str()
            .unwrap()
            .starts_with("[AgentPMT]"));
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
        assert!(catalog.refreshed_at.is_some());
    }
}
