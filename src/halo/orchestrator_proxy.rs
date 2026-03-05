use serde_json::{json, Value};
use std::time::Duration;

use super::http_client;

pub const PROXY_ENABLE_VARS: [&str; 2] = [
    "AGENTHALO_ORCHESTRATOR_PROXY_VIA_MCP",
    "NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO",
];
pub const PROXY_ENDPOINT_VARS: [&str; 2] = [
    "AGENTHALO_ORCHESTRATOR_MCP_ENDPOINT",
    "NUCLEUSDB_ORCHESTRATOR_PROXY_ENDPOINT",
];

pub fn truthy_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| truthy_value(&v))
        .unwrap_or(false)
}

pub fn orchestrator_proxy_enabled() -> bool {
    for var_name in PROXY_ENABLE_VARS {
        if let Ok(value) = std::env::var(var_name) {
            return truthy_value(&value);
        }
    }
    std::env::var("AGENTHALO_MCP_SECRET")
        .ok()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || truthy_env("AGENTHALO_ALLOW_DEV_SECRET")
}

pub fn orchestrator_proxy_endpoint() -> String {
    for var_name in PROXY_ENDPOINT_VARS {
        if let Ok(value) = std::env::var(var_name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    let host = std::env::var("AGENTHALO_MCP_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("AGENTHALO_MCP_PORT").unwrap_or_else(|_| "8390".to_string());
    format!("http://{host}:{port}/mcp")
}

pub fn orchestrator_proxy_secret() -> Option<String> {
    std::env::var("AGENTHALO_MCP_SECRET")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            if truthy_env("AGENTHALO_ALLOW_DEV_SECRET") {
                Some("agenthalo-dev-secret".to_string())
            } else {
                None
            }
        })
}

pub async fn call_orchestrator_tool(tool_name: &str, arguments: Value) -> Result<Value, String> {
    let endpoint = orchestrator_proxy_endpoint();
    let name = tool_name.to_string();
    let secret = orchestrator_proxy_secret();
    tokio::task::spawn_blocking(move || -> Result<Value, String> {
        let mut req = http_client::post_with_timeout(&endpoint, Duration::from_secs(20))
            .map_err(|e| format!("build orchestrator MCP request: {e}"))?
            .content_type("application/json")
            .header("Accept", "application/json");
        if let Some(secret) = secret.as_deref() {
            req = req.header("Authorization", &format!("Bearer {secret}"));
        }

        let body = req
            .send_json(json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": name,
                    "arguments": arguments,
                }
            }))
            .map_err(|e| format!("orchestrator MCP call failed: {e}"))?
            .into_body()
            .read_json::<Value>()
            .map_err(|e| format!("parse orchestrator MCP response: {e}"))?;

        if let Some(err) = body.get("error") {
            return Err(format!("orchestrator MCP rpc error: {err}"));
        }
        let result = body
            .get("result")
            .ok_or_else(|| "orchestrator MCP response missing result".to_string())?;
        let structured = result
            .get("structuredContent")
            .cloned()
            .or_else(|| {
                result
                    .get("content")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .and_then(|v| v.get("text"))
                    .and_then(|v| v.as_str())
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
            })
            .ok_or_else(|| "orchestrator MCP response missing content".to_string())?;
        if result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(structured
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("orchestrator tool call failed")
                .to_string());
        }
        Ok(structured)
    })
    .await
    .map_err(|e| format!("orchestrator MCP join error: {e}"))?
}

fn truthy_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::env_lock;

    struct EnvVarGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var(key).ok();
            match value {
                Some(v) => {
                    // SAFETY: serialized by env_lock in every test here.
                    unsafe { std::env::set_var(key, v) };
                }
                None => {
                    // SAFETY: serialized by env_lock in every test here.
                    unsafe { std::env::remove_var(key) };
                }
            }
            Self { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(v) = self.prev.take() {
                // SAFETY: serialized by env_lock in every test here.
                unsafe { std::env::set_var(self.key, v) };
            } else {
                // SAFETY: serialized by env_lock in every test here.
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    #[test]
    fn proxy_enabled_accepts_both_aliases() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _legacy = EnvVarGuard::set("NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO", Some("1"));
        let _dash = EnvVarGuard::set("AGENTHALO_ORCHESTRATOR_PROXY_VIA_MCP", None);
        let _secret = EnvVarGuard::set("AGENTHALO_MCP_SECRET", None);
        let _dev = EnvVarGuard::set("AGENTHALO_ALLOW_DEV_SECRET", None);
        assert!(orchestrator_proxy_enabled());
    }

    #[test]
    fn proxy_endpoint_honors_aliases() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _dash = EnvVarGuard::set("AGENTHALO_ORCHESTRATOR_MCP_ENDPOINT", None);
        let _legacy = EnvVarGuard::set(
            "NUCLEUSDB_ORCHESTRATOR_PROXY_ENDPOINT",
            Some("http://127.0.0.1:9999/mcp"),
        );
        assert_eq!(
            orchestrator_proxy_endpoint(),
            "http://127.0.0.1:9999/mcp".to_string()
        );
    }
}
