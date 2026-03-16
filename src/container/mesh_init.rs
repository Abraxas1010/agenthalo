//! Native mesh self-registration.
//!
//! Called by the MCP server binary at startup when `NUCLEUSDB_MESH_PORT` is set.
//! Registers this process in the shared peer registry so other local AgentHALO
//! subprocesses can discover it. Deregisters at shutdown.

use crate::container::mesh::{mesh_registry_path, PeerInfo, PeerRegistry, DEFAULT_MCP_PORT};

/// Check whether this process is running inside a mesh-enabled agent session.
pub fn mesh_enabled() -> bool {
    std::env::var("NUCLEUSDB_MESH_AGENT_ID")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// Register this process in the mesh peer registry.
/// Called at MCP server startup.
pub fn register_self_in_mesh() -> Result<(), String> {
    let mesh_port: u16 = std::env::var("NUCLEUSDB_MESH_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_MCP_PORT);
    let agent_id = std::env::var("NUCLEUSDB_MESH_AGENT_ID")
        .map_err(|_| "NUCLEUSDB_MESH_AGENT_ID is required when mesh is enabled".to_string())?;
    if agent_id.trim().is_empty() {
        return Err("NUCLEUSDB_MESH_AGENT_ID cannot be empty when mesh is enabled".to_string());
    }
    let did_uri = std::env::var("NUCLEUSDB_MESH_DID").ok();

    let container_name = resolve_hostname().unwrap_or_else(|| agent_id.clone());
    let endpoint_host = resolve_endpoint_host();

    let mcp_endpoint = format!("http://{endpoint_host}:{mesh_port}/mcp");
    let discovery_endpoint = format!("http://{endpoint_host}:{mesh_port}/.well-known/nucleus-pod");

    let now = crate::pod::now_unix();
    let peer = PeerInfo {
        agent_id: agent_id.clone(),
        container_name,
        did_uri,
        mcp_endpoint,
        discovery_endpoint,
        registered_at: now,
        last_seen: now,
    };

    let path = mesh_registry_path();
    let _lock = crate::container::mesh::PeerRegistryLock::acquire(path.as_path())?;
    let mut registry = PeerRegistry::load(path.as_path()).unwrap_or_default();
    registry.register(peer);
    registry.save(path.as_path())?;

    eprintln!("[mesh] registered as agent '{agent_id}' on port {mesh_port}");
    Ok(())
}

fn resolve_endpoint_host() -> String {
    std::env::var("AGENTHALO_MCP_HOST")
        .ok()
        .or_else(|| std::env::var("NUCLEUSDB_MCP_HOST").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(resolve_local_ip)
        .unwrap_or_else(|| "127.0.0.1".to_string())
}

/// Deregister this process from the mesh peer registry.
/// Called at MCP server shutdown.
pub fn deregister_self_from_mesh() {
    let agent_id = std::env::var("NUCLEUSDB_MESH_AGENT_ID").unwrap_or_default();
    if agent_id.is_empty() {
        return;
    }
    let path = mesh_registry_path();
    let _lock = match crate::container::mesh::PeerRegistryLock::acquire(path.as_path()) {
        Ok(lock) => lock,
        Err(_) => return,
    };
    if let Ok(mut registry) = PeerRegistry::load(path.as_path()) {
        registry.deregister(&agent_id);
        let _ = registry.save(path.as_path());
        eprintln!("[mesh] deregistered agent '{agent_id}'");
    }
}

/// Resolve host name using the local system hostname command.
fn resolve_hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
}

fn resolve_local_ip() -> Option<String> {
    std::process::Command::new("hostname")
        .arg("-i")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .find(|value| value.parse::<std::net::IpAddr>().is_ok())
                .map(str::to_string)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn mesh_enabled_detects_env() {
        let _guard = test_support::lock_env();
        let had = std::env::var("NUCLEUSDB_MESH_AGENT_ID").ok();
        std::env::remove_var("NUCLEUSDB_MESH_AGENT_ID");
        assert!(!mesh_enabled());
        std::env::set_var("NUCLEUSDB_MESH_AGENT_ID", "agent-test");
        assert!(mesh_enabled());
        if let Some(v) = had {
            std::env::set_var("NUCLEUSDB_MESH_AGENT_ID", v);
        } else {
            std::env::remove_var("NUCLEUSDB_MESH_AGENT_ID");
        }
    }

    #[test]
    fn resolve_hostname_returns_nonempty() {
        // On any unix system, `hostname` should return something
        if let Some(h) = resolve_hostname() {
            assert!(!h.is_empty());
        }
    }

    #[test]
    fn resolve_local_ip_is_optional_but_valid_when_present() {
        if let Some(ip) = resolve_local_ip() {
            assert!(ip.parse::<std::net::IpAddr>().is_ok());
        }
    }

    #[test]
    fn resolve_endpoint_host_prefers_explicit_bind_host() {
        let _guard = test_support::lock_env();
        let prev = std::env::var("AGENTHALO_MCP_HOST").ok();
        std::env::set_var("AGENTHALO_MCP_HOST", "127.0.0.1");
        assert_eq!(resolve_endpoint_host(), "127.0.0.1".to_string());
        if let Some(v) = prev {
            std::env::set_var("AGENTHALO_MCP_HOST", v);
        } else {
            std::env::remove_var("AGENTHALO_MCP_HOST");
        }
    }

    #[test]
    fn register_and_deregister_use_registry_override() {
        let _guard = test_support::lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("mesh-peers.json");

        let prev_registry = std::env::var("NUCLEUSDB_MESH_REGISTRY").ok();
        let prev_agent = std::env::var("NUCLEUSDB_MESH_AGENT_ID").ok();
        let prev_port = std::env::var("NUCLEUSDB_MESH_PORT").ok();
        let prev_did = std::env::var("NUCLEUSDB_MESH_DID").ok();

        std::env::set_var("NUCLEUSDB_MESH_REGISTRY", path.display().to_string());
        std::env::set_var("NUCLEUSDB_MESH_AGENT_ID", "agent-test");
        std::env::set_var("NUCLEUSDB_MESH_PORT", "3000");
        std::env::set_var("NUCLEUSDB_MESH_DID", "did:key:z6MkTest");

        register_self_in_mesh().expect("register");
        let reg = PeerRegistry::load(path.as_path()).expect("load registry");
        let peer = reg.find("agent-test").expect("registered peer");
        assert!(peer
            .discovery_endpoint
            .ends_with("/.well-known/nucleus-pod"));

        deregister_self_from_mesh();
        let reg_after = PeerRegistry::load(path.as_path()).expect("load registry");
        assert!(reg_after.find("agent-test").is_none());

        if let Some(v) = prev_registry {
            std::env::set_var("NUCLEUSDB_MESH_REGISTRY", v);
        } else {
            std::env::remove_var("NUCLEUSDB_MESH_REGISTRY");
        }
        if let Some(v) = prev_agent {
            std::env::set_var("NUCLEUSDB_MESH_AGENT_ID", v);
        } else {
            std::env::remove_var("NUCLEUSDB_MESH_AGENT_ID");
        }
        if let Some(v) = prev_port {
            std::env::set_var("NUCLEUSDB_MESH_PORT", v);
        } else {
            std::env::remove_var("NUCLEUSDB_MESH_PORT");
        }
        if let Some(v) = prev_did {
            std::env::set_var("NUCLEUSDB_MESH_DID", v);
        } else {
            std::env::remove_var("NUCLEUSDB_MESH_DID");
        }
    }
}
