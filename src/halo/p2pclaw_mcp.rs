//! P2PCLAW MCP Sidecar Manager
//!
//! Manages a Node.js child process (`vendor/p2pclaw-mcp/index.mjs`) that
//! exposes P2PCLAW network tools via a localhost REST API. All upstream
//! gateway traffic flows through this sidecar, giving AgentHALO full
//! tracing, auth, and content gating control.
//!
//! Follows the same lifecycle pattern as [`super::wdk_proxy::WdkManager`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const DEFAULT_PORT: u16 = 7421;
const SIDECAR_DIR: &str = "vendor/p2pclaw-mcp";
const SIDECAR_DIR_ENV: &str = "P2PCLAW_MCP_DIR";
const PORT_ENV: &str = "P2PCLAW_MCP_PORT";
const AUTH_TOKEN_ENV: &str = "P2PCLAW_MCP_AUTH_TOKEN";
const AUTH_HEADER: &str = "x-agenthalo-p2pclaw-token";

/// Runtime state for the P2PCLAW MCP sidecar.
pub struct P2PClawMcpManager {
    child: Option<Child>,
    port: u16,
    auth_token: String,
    gateway_url: String,
    agent_id: String,
    agent_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SidecarStatus {
    pub status: String,
    pub version: String,
    pub gateway: String,
    pub agent_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: Vec<ToolResultContent>,
    #[serde(default, rename = "isError")]
    pub is_error: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolResultContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

impl P2PClawMcpManager {
    pub fn new(gateway_url: &str, agent_id: &str, agent_name: &str) -> Self {
        let port = std::env::var(PORT_ENV)
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let auth_token = std::env::var(AUTH_TOKEN_ENV)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                let mut token = [0u8; 32];
                getrandom::getrandom(&mut token)
                    .expect("OS entropy source unavailable for P2PCLAW MCP auth token");
                hex::encode(token)
            });
        Self {
            child: None,
            port,
            auth_token,
            gateway_url: gateway_url.to_string(),
            agent_id: agent_id.to_string(),
            agent_name: agent_name.to_string(),
        }
    }

    /// Check if Node.js and the sidecar directory are available.
    pub fn is_available() -> bool {
        let node_ok = Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !node_ok {
            return false;
        }
        let dir = Self::sidecar_dir();
        dir.join("index.mjs").exists() && dir.join("node_modules").exists()
    }

    fn sidecar_dir() -> PathBuf {
        if let Ok(raw) = std::env::var(SIDECAR_DIR_ENV) {
            let candidate = PathBuf::from(raw.trim());
            if candidate.join("index.mjs").exists() {
                return candidate;
            }
        }
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()));
        if let Some(dir) = exe_dir {
            let candidate = dir.join("..").join(SIDECAR_DIR);
            if candidate.join("index.mjs").exists() {
                return candidate;
            }
            let candidate = dir.join(SIDECAR_DIR);
            if candidate.join("index.mjs").exists() {
                return candidate;
            }
        }
        PathBuf::from(SIDECAR_DIR)
    }

    fn api_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    pub(crate) fn sync_config(&mut self, gateway_url: &str, agent_id: &str, agent_name: &str) {
        let config_changed = self.gateway_url != gateway_url
            || self.agent_id != agent_id
            || self.agent_name != agent_name;
        if !config_changed {
            return;
        }

        if self.child.is_some() {
            self.stop();
        }
        self.gateway_url = gateway_url.to_string();
        self.agent_id = agent_id.to_string();
        self.agent_name = agent_name.to_string();
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn auth_token(&self) -> &str {
        &self.auth_token
    }

    /// Start the sidecar process. Blocks until ready or timeout (10s).
    pub fn start(&mut self) -> Result<(), String> {
        if self.is_running() {
            return Ok(());
        }
        if self.child.is_none() && std::net::TcpStream::connect(("127.0.0.1", self.port)).is_ok() {
            return Err(format!(
                "P2PCLAW MCP port {} already bound — set {} to match or choose another port",
                self.port, AUTH_TOKEN_ENV
            ));
        }
        if self.child.is_some() {
            self.stop();
        }
        let sidecar_dir = Self::sidecar_dir();
        if !sidecar_dir.join("index.mjs").exists() {
            return Err(format!(
                "P2PCLAW MCP sidecar missing at {} (run: cd vendor/p2pclaw-mcp && npm install)",
                sidecar_dir.display()
            ));
        }
        let child = Command::new("node")
            .arg("index.mjs")
            .current_dir(&sidecar_dir)
            .env("P2PCLAW_MCP_PORT", self.port.to_string())
            .env("P2PCLAW_AUTH_TOKEN", &self.auth_token)
            .env("P2PCLAW_GATEWAY_URL", &self.gateway_url)
            .env("P2PCLAW_AGENT_ID", &self.agent_id)
            .env("P2PCLAW_AGENT_NAME", &self.agent_name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("start P2PCLAW MCP sidecar: {e}"))?;
        self.child = Some(child);

        let started = Instant::now();
        let timeout = Duration::from_secs(10);
        while started.elapsed() < timeout {
            if self.is_running() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        self.stop();
        Err("P2PCLAW MCP sidecar did not become ready within 10s".to_string())
    }

    /// Stop the sidecar process.
    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Check if the sidecar is responding.
    pub fn is_running(&self) -> bool {
        ureq::get(&self.api_url("/status"))
            .header(AUTH_HEADER, &self.auth_token)
            .call()
            .is_ok()
    }

    /// Get sidecar status.
    pub fn status(&self) -> Result<SidecarStatus, String> {
        let mut resp = ureq::get(&self.api_url("/status"))
            .header(AUTH_HEADER, &self.auth_token)
            .call()
            .map_err(|e| format!("P2PCLAW MCP /status: {e}"))?;
        resp.body_mut()
            .read_json::<SidecarStatus>()
            .map_err(|e| format!("parse /status: {e}"))
    }

    /// List available tools.
    pub fn list_tools(&self) -> Result<Vec<ToolDef>, String> {
        let mut resp = ureq::get(&self.api_url("/tools"))
            .header(AUTH_HEADER, &self.auth_token)
            .call()
            .map_err(|e| format!("P2PCLAW MCP /tools: {e}"))?;
        let raw: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("parse /tools: {e}"))?;
        let tools = raw
            .get("tools")
            .cloned()
            .unwrap_or(Value::Array(Vec::new()));
        serde_json::from_value(tools).map_err(|e| format!("deserialize tools: {e}"))
    }

    /// Call a tool by name with arguments.
    pub fn call_tool(&self, name: &str, arguments: &Value) -> Result<ToolResult, String> {
        let body = serde_json::json!({
            "name": name,
            "arguments": arguments,
        });
        let mut resp = ureq::post(&self.api_url("/call"))
            .header(AUTH_HEADER, &self.auth_token)
            .send_json(&body)
            .map_err(|e| format!("P2PCLAW MCP /call {name}: {e}"))?;
        resp.body_mut()
            .read_json::<ToolResult>()
            .map_err(|e| format!("parse /call {name}: {e}"))
    }

    /// Proxy a raw request to the P2PCLAW gateway via the sidecar.
    pub fn gateway_proxy(
        &self,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, String> {
        let url = self.api_url(&format!("/gateway/{}", path.trim_start_matches('/')));
        let result = match method.to_uppercase().as_str() {
            "GET" => {
                let mut resp = ureq::get(&url)
                    .header(AUTH_HEADER, &self.auth_token)
                    .call()
                    .map_err(|e| format!("gateway GET {path}: {e}"))?;
                resp.body_mut()
                    .read_json::<Value>()
                    .map_err(|e| format!("parse gateway GET {path}: {e}"))?
            }
            _ => {
                let payload = body.cloned().unwrap_or(Value::Object(Default::default()));
                let mut resp = ureq::post(&url)
                    .header(AUTH_HEADER, &self.auth_token)
                    .send_json(&payload)
                    .map_err(|e| format!("gateway POST {path}: {e}"))?;
                resp.body_mut()
                    .read_json::<Value>()
                    .map_err(|e| format!("parse gateway POST {path}: {e}"))?
            }
        };
        Ok(result)
    }
}

impl Drop for P2PClawMcpManager {
    fn drop(&mut self) {
        self.stop();
    }
}
