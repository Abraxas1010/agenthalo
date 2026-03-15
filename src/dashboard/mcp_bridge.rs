use crate::halo::http_client;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

const MCP_BOOT_TIMEOUT: Duration = Duration::from_secs(5);
// Container bring-up and remote MCP forwarding can legitimately take tens of
// seconds, so the dashboard bridge must not impose a short global timeout.
const MCP_HTTP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct McpToolInfo {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub category: String,
    pub domain: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub read_only_hint: Option<bool>,
    pub destructive_hint: Option<bool>,
    pub idempotent_hint: Option<bool>,
    pub open_world_hint: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpCategoryInfo {
    pub category: String,
    pub domain: String,
    pub tool_count: usize,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct McpInvokeResult {
    pub tool: String,
    pub is_error: bool,
    pub structured_content: Option<Value>,
    pub content: Vec<String>,
}

pub async fn tool_catalog() -> Result<Vec<McpToolInfo>, String> {
    tokio::task::spawn_blocking(tool_catalog_blocking)
        .await
        .map_err(|e| format!("tool catalog task join: {e}"))?
}

pub async fn tool_detail(name: &str) -> Result<Option<McpToolInfo>, String> {
    let catalog = tool_catalog().await?;
    Ok(catalog.into_iter().find(|tool| tool.name == name))
}

pub async fn category_summary() -> Result<Vec<McpCategoryInfo>, String> {
    let catalog = tool_catalog().await?;
    let mut by_category: BTreeMap<String, McpCategoryInfo> = BTreeMap::new();
    for tool in catalog {
        let entry = by_category
            .entry(tool.category.clone())
            .or_insert_with(|| McpCategoryInfo {
                category: tool.category.clone(),
                domain: tool.domain.clone(),
                tool_count: 0,
                tools: Vec::new(),
            });
        entry.tool_count += 1;
        entry.tools.push(tool.name);
    }
    Ok(by_category.into_values().collect())
}

pub async fn invoke_tool(tool: &str, params: Value) -> Result<McpInvokeResult, String> {
    let tool = tool.to_string();
    tokio::task::spawn_blocking(move || invoke_tool_blocking(&tool, params))
        .await
        .map_err(|e| format!("MCP invoke task join: {e}"))?
}

pub fn running_session_endpoint() -> Result<(String, String), String> {
    let slot = bridge_session_slot();
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    ensure_live_session(&mut guard)?;
    let session = guard
        .as_ref()
        .ok_or_else(|| "MCP bridge session unexpectedly missing".to_string())?;
    Ok((session.endpoint.clone(), session.secret.clone()))
}

fn tool_catalog_blocking() -> Result<Vec<McpToolInfo>, String> {
    let result = with_bridge_session(|session| session.call("tools/list", json!({})))?;
    let tools = result
        .get("tools")
        .and_then(|v| v.as_array())
        .ok_or_else(|| format!("tools/list response missing tools array: {result}"))?;
    let mut items = tools
        .iter()
        .cloned()
        .map(normalize_tool_value)
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        a.category
            .cmp(&b.category)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(items)
}

fn invoke_tool_blocking(tool: &str, params: Value) -> Result<McpInvokeResult, String> {
    let arguments = match params {
        Value::Object(map) => Value::Object(map),
        Value::Null => json!({}),
        other => {
            return Err(format!(
                "tool arguments must be a JSON object, got {}",
                other_type_name(&other)
            ));
        }
    };

    let request = json!({
        "name": tool,
        "arguments": arguments
    });
    let result = with_bridge_session(|session| session.call("tools/call", request.clone()))?;

    Ok(McpInvokeResult {
        tool: tool.to_string(),
        is_error: result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        structured_content: result.get("structuredContent").cloned(),
        content: result
            .get("content")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(|v| v.as_str()))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
    })
}

fn bridge_session_slot() -> &'static Mutex<Option<BridgeSession>> {
    static SLOT: OnceLock<Mutex<Option<BridgeSession>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn with_bridge_session<T, F>(op: F) -> Result<T, String>
where
    F: FnOnce(&BridgeSession) -> Result<T, String>,
{
    let slot = bridge_session_slot();
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    ensure_live_session(&mut guard)?;

    let result = {
        let session = guard
            .as_ref()
            .ok_or_else(|| "MCP bridge session unexpectedly missing".to_string())?;
        op(session)
    };

    if result.is_err() {
        // Do not auto-retry the current tool call after transport failure:
        // some MCP tools are not read-only and duplicate execution would be unsafe.
        let should_reset = guard
            .as_mut()
            .map(|session| !session.is_healthy())
            .unwrap_or(true);
        if should_reset {
            *guard = None;
        }
    }

    result
}

fn ensure_live_session(slot: &mut Option<BridgeSession>) -> Result<(), String> {
    let restart = slot
        .as_mut()
        .map(|session| !session.is_healthy())
        .unwrap_or(true);
    if restart {
        *slot = None;
        *slot = Some(BridgeSession::start()?);
    }
    Ok(())
}

struct BridgeSession {
    child: Option<Child>,
    endpoint: String,
    secret: String,
}

impl BridgeSession {
    fn start() -> Result<Self, String> {
        if let Some((endpoint, secret)) = configured_bridge_target() {
            let session = Self {
                child: None,
                endpoint,
                secret,
            };
            if session.wait_until_ready().is_ok() {
                let _ = session.call("initialize", json!({}))?;
                return Ok(session);
            }
        }

        let bin = resolve_agenthalo_mcp_server_bin()?;
        let port = reserve_local_port()?;
        let endpoint = format!("http://127.0.0.1:{port}");
        let secret = random_bridge_secret()?;
        let child = Command::new(&bin)
            .env("AGENTHALO_MCP_SECRET", &secret)
            .env("AGENTHALO_MCP_HOST", "127.0.0.1")
            .env("AGENTHALO_MCP_PORT", port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("spawn `{}`: {e}", bin.display()))?;
        let session = Self {
            child: Some(child),
            endpoint,
            secret,
        };
        session.wait_until_ready()?;
        let _ = session.call("initialize", json!({}))?;
        Ok(session)
    }

    fn wait_until_ready(&self) -> Result<(), String> {
        let deadline = std::time::Instant::now() + MCP_BOOT_TIMEOUT;
        while std::time::Instant::now() < deadline {
            if self.health_check() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!(
            "timeout waiting for MCP server health at {}/health",
            self.endpoint
        ))
    }

    fn health_check(&self) -> bool {
        if let Ok(req) = http_client::get_with_timeout(
            &format!("{}/health", self.endpoint),
            Duration::from_millis(250),
        ) {
            if let Ok(resp) = req.call() {
                return resp.status().as_u16() == 200;
            }
        }
        false
    }

    fn is_healthy(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => match child.try_wait() {
                Ok(Some(_)) | Err(_) => false,
                Ok(None) => self.health_check(),
            },
            None => self.health_check(),
        }
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let req_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params
        });
        let raw = serde_json::to_string(&req_body)
            .map_err(|e| format!("serialize MCP request `{method}`: {e}"))?;
        let resp =
            http_client::post_with_timeout(&format!("{}/mcp", self.endpoint), MCP_HTTP_TIMEOUT)?
                .header("Authorization", &format!("Bearer {}", self.secret))
                .content_type("application/json")
                .send(raw)
                .map_err(|e| format!("HTTP MCP request `{method}` failed: {e}"))?;
        let body: Value = resp
            .into_body()
            .read_json()
            .map_err(|e| format!("parse MCP response `{method}`: {e}"))?;
        if let Some(error) = body.get("error") {
            return Err(format!("MCP `{method}` error: {error}"));
        }
        body.get("result")
            .cloned()
            .ok_or_else(|| format!("MCP `{method}` response missing result: {body}"))
    }
}

impl Drop for BridgeSession {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn configured_bridge_target() -> Option<(String, String)> {
    let secret = crate::halo::orchestrator_proxy::orchestrator_proxy_secret()?;
    let endpoint = crate::halo::orchestrator_proxy::orchestrator_proxy_endpoint();
    let base = endpoint
        .strip_suffix("/mcp")
        .unwrap_or(endpoint.as_str())
        .trim_end_matches('/')
        .to_string();
    if base.is_empty() {
        None
    } else {
        Some((base, secret))
    }
}

fn random_bridge_secret() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("generate bridge secret: {e}"))?;
    Ok(crate::halo::util::hex_encode(&bytes))
}

fn reserve_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind local port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local addr for reserved port: {e}"))?
        .port();
    drop(listener);
    Ok(port)
}

fn resolve_agenthalo_mcp_server_bin() -> Result<PathBuf, String> {
    let env_candidates = [
        "AGENTHALO_MCP_SERVER_BIN",
        "CARGO_BIN_EXE_agenthalo-mcp-server",
        "CARGO_BIN_EXE_agenthalo_mcp_server",
    ];
    for key in env_candidates {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                let path = PathBuf::from(trimmed);
                if path.exists() {
                    return Ok(path);
                }
            }
        }
    }

    if let Ok(path) = which("agenthalo-mcp-server") {
        return Ok(path);
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in [
        "target/debug/agenthalo-mcp-server",
        "target/debug/agenthalo_mcp_server",
        "target/release/agenthalo-mcp-server",
    ] {
        let path = manifest_dir.join(rel);
        if path.exists() {
            return Ok(path);
        }
    }

    Err("could not locate `agenthalo-mcp-server`; set AGENTHALO_MCP_SERVER_BIN".to_string())
}

fn which(binary: &str) -> Result<PathBuf, String> {
    let path_env = std::env::var_os("PATH").ok_or_else(|| "PATH is unset".to_string())?;
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(binary);
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    Err(format!("`{binary}` not found on PATH"))
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn normalize_tool_value(tool: Value) -> McpToolInfo {
    let name = tool
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let (category, domain) = classify_tool(&name);
    let annotations = tool.get("annotations").cloned().unwrap_or(Value::Null);
    McpToolInfo {
        name,
        title: tool
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        description: tool
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        category,
        domain,
        input_schema: tool
            .get("inputSchema")
            .cloned()
            .unwrap_or_else(|| json!({})),
        output_schema: tool.get("outputSchema").cloned(),
        read_only_hint: annotations.get("readOnlyHint").and_then(|v| v.as_bool()),
        destructive_hint: annotations.get("destructiveHint").and_then(|v| v.as_bool()),
        idempotent_hint: annotations.get("idempotentHint").and_then(|v| v.as_bool()),
        open_world_hint: annotations.get("openWorldHint").and_then(|v| v.as_bool()),
    }
}

fn classify_tool(name: &str) -> (String, String) {
    let category = match name {
        "register_chain" | "verify_agent_multichain" => "onchain".to_string(),
        _ => {
            if let Some((prefix, _)) = name.split_once('/') {
                prefix.to_string()
            } else if let Some((prefix, _)) = name.split_once('_') {
                prefix.to_string()
            } else {
                "misc".to_string()
            }
        }
    };
    let domain = match category.as_str() {
        "nucleusdb" => "NucleusDB".to_string(),
        "p2pclaw" => "P2PCLAW".to_string(),
        "orchestrator" => "Orchestrator".to_string(),
        "agentpmt" => "AgentPMT".to_string(),
        "mesh" => "Mesh".to_string(),
        "identity" => "Identity".to_string(),
        "wallet" => "Wallet".to_string(),
        "proof" => "Verification".to_string(),
        "halo" => "HALO".to_string(),
        "onchain" => "Onchain".to_string(),
        other => other.to_string(),
    };
    (category, domain)
}

fn other_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_tool, configured_bridge_target};
    use crate::test_support::{lock_env, EnvVarGuard};

    #[test]
    fn classify_tool_applies_onchain_overrides() {
        assert_eq!(
            classify_tool("register_chain"),
            ("onchain".to_string(), "Onchain".to_string())
        );
        assert_eq!(
            classify_tool("verify_agent_multichain"),
            ("onchain".to_string(), "Onchain".to_string())
        );
    }

    #[test]
    fn classify_tool_uses_prefixes_for_regular_names() {
        assert_eq!(
            classify_tool("nucleusdb_container_launch"),
            ("nucleusdb".to_string(), "NucleusDB".to_string())
        );
        assert_eq!(
            classify_tool("agentpmt/search"),
            ("agentpmt".to_string(), "AgentPMT".to_string())
        );
    }

    #[test]
    fn configured_bridge_target_prefers_existing_mcp_endpoint() {
        let _guard = lock_env();
        let _secret = EnvVarGuard::set("AGENTHALO_MCP_SECRET", Some("bridge-secret"));
        let _endpoint = EnvVarGuard::set(
            "AGENTHALO_ORCHESTRATOR_MCP_ENDPOINT",
            Some("http://127.0.0.1:8390/mcp"),
        );
        let (endpoint, secret) = configured_bridge_target().expect("configured target");
        assert_eq!(endpoint, "http://127.0.0.1:8390");
        assert_eq!(secret, "bridge-secret");
    }
}
