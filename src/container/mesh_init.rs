//! Mesh network self-registration.
//!
//! Called by the MCP server binary at startup when `NUCLEUSDB_MESH_PORT` is set.
//! Registers this container in the shared peer registry so other containers
//! can discover it. Deregisters at shutdown.

use crate::container::mesh::{mesh_registry_path, PeerInfo, PeerRegistry, DEFAULT_MCP_PORT};

/// Check whether this process is running inside a mesh-enabled container.
pub fn mesh_enabled() -> bool {
    std::env::var("NUCLEUSDB_MESH_AGENT_ID")
        .ok()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false)
}

/// Register this container in the mesh peer registry.
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

    // Container hostname — set by --hostname in launch_container()
    let container_name = resolve_hostname().unwrap_or_else(|| agent_id.clone());

    let mcp_endpoint = format!("http://{container_name}:{mesh_port}/mcp");
    let discovery_endpoint =
        format!("http://{container_name}:{mesh_port}/pod/.well-known/nucleus-pod");

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
    let mut registry = PeerRegistry::load(path.as_path()).unwrap_or_default();
    registry.register(peer);
    registry.save(path.as_path())?;

    eprintln!("[mesh] registered as agent '{agent_id}' on port {mesh_port}");
    Ok(())
}

/// Deregister this container from the mesh peer registry.
/// Called at MCP server shutdown.
pub fn deregister_self_from_mesh() {
    let agent_id = std::env::var("NUCLEUSDB_MESH_AGENT_ID").unwrap_or_default();
    if agent_id.is_empty() {
        return;
    }
    let path = mesh_registry_path();
    if let Ok(mut registry) = PeerRegistry::load(path.as_path()) {
        registry.deregister(&agent_id);
        let _ = registry.save(path.as_path());
        eprintln!("[mesh] deregistered agent '{agent_id}'");
    }
}

/// Resolve container hostname. Uses the `hostname` command as a portable
/// fallback (works inside Docker without extra crate dependencies).
fn resolve_hostname() -> Option<String> {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|h| !h.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn mesh_enabled_detects_env() {
        let _guard = env_lock().lock().expect("lock env");
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
    fn register_and_deregister_use_registry_override() {
        let _guard = env_lock().lock().expect("lock env");
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
        assert!(reg.find("agent-test").is_some());

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
