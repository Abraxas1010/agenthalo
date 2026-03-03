//! Container mesh network for inter-agent MCP communication.
//!
//! Creates a shared Docker bridge network (`halo-mesh`) so containers
//! can reach each other's MCP HTTP endpoints. Peers are discovered via
//! Docker DNS (container-name:port) and registered in a shared peer list.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub const MESH_NETWORK_NAME: &str = "halo-mesh";
pub const DEFAULT_MCP_PORT: u16 = 3000;
pub const MESH_REGISTRY_PATH: &str = "/data/mesh/peers.json";

/// Resolve the peer registry path.
///
/// Supports `NUCLEUSDB_MESH_REGISTRY` override for tests and non-container
/// deployments; defaults to `/data/mesh/peers.json` in container mode.
pub fn mesh_registry_path() -> std::path::PathBuf {
    std::env::var("NUCLEUSDB_MESH_REGISTRY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(MESH_REGISTRY_PATH))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub agent_id: String,
    pub container_name: String,
    pub did_uri: Option<String>,
    pub mcp_endpoint: String,
    pub discovery_endpoint: String,
    pub registered_at: u64,
    pub last_seen: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeerRegistry {
    pub peers: BTreeMap<String, PeerInfo>,
}

/// Ensure the shared Docker bridge network exists.
pub fn ensure_mesh_network() -> Result<(), String> {
    let inspect = Command::new("docker")
        .args(["network", "inspect", MESH_NETWORK_NAME])
        .output()
        .map_err(|e| format!("docker network inspect failed: {e}"))?;
    if inspect.status.success() {
        return Ok(());
    }
    let create = Command::new("docker")
        .args([
            "network",
            "create",
            "--driver",
            "bridge",
            "--label",
            "nucleusdb.mesh=true",
            MESH_NETWORK_NAME,
        ])
        .output()
        .map_err(|e| format!("docker network create failed: {e}"))?;
    if !create.status.success() {
        return Err(format!(
            "failed to create mesh network: {}",
            String::from_utf8_lossy(&create.stderr)
        ));
    }
    Ok(())
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load registry from a shared JSON file.
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let data = std::fs::read(path)
            .map_err(|e| format!("read peer registry {}: {e}", path.display()))?;
        serde_json::from_slice(&data)
            .map_err(|e| format!("parse peer registry {}: {e}", path.display()))
    }

    /// Save registry atomically (write-tmp-rename).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create registry dir {}: {e}", parent.display()))?;
        }
        let data =
            serde_json::to_vec_pretty(self).map_err(|e| format!("serialize peer registry: {e}"))?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &data)
            .map_err(|e| format!("write temp registry {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| {
            format!(
                "rename registry {} -> {}: {e}",
                tmp.display(),
                path.display()
            )
        })?;
        Ok(())
    }

    /// Register a peer. Updates existing entry if agent_id matches.
    pub fn register(&mut self, peer: PeerInfo) {
        self.peers.insert(peer.agent_id.clone(), peer);
    }

    /// Remove a peer by agent_id.
    pub fn deregister(&mut self, agent_id: &str) -> bool {
        self.peers.remove(agent_id).is_some()
    }

    /// List all peers except self.
    pub fn peers_except(&self, my_agent_id: &str) -> Vec<&PeerInfo> {
        self.peers
            .values()
            .filter(|p| p.agent_id != my_agent_id)
            .collect()
    }

    /// Find peer by agent_id.
    pub fn find(&self, agent_id: &str) -> Option<&PeerInfo> {
        self.peers.get(agent_id)
    }

    /// Find peer by DID URI.
    pub fn find_by_did(&self, did_uri: &str) -> Option<&PeerInfo> {
        self.peers
            .values()
            .find(|p| p.did_uri.as_deref() == Some(did_uri))
    }

    /// Prune peers not seen since `cutoff` (unix seconds).
    pub fn prune_stale(&mut self, cutoff: u64) -> usize {
        let before = self.peers.len();
        self.peers.retain(|_, p| p.last_seen >= cutoff);
        before - self.peers.len()
    }
}

/// Discover a peer by fetching its POD capabilities endpoint.
pub fn discover_peer(endpoint: &str) -> Result<PeerInfo, String> {
    use crate::halo::http_client;
    let url = format!("{endpoint}/.well-known/nucleus-pod");
    let resp = http_client::get_with_timeout(&url, std::time::Duration::from_secs(5))?
        .call()
        .map_err(|e| format!("discover peer at {url}: {e}"))?;
    let body: serde_json::Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse discovery response from {url}: {e}"))?;
    let agent_id = body
        .get("agent_id")
        .and_then(|v: &serde_json::Value| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let did_uri = body
        .get("agent_did")
        .and_then(|v: &serde_json::Value| v.as_str())
        .map(|s: &str| s.to_string());
    let mcp_endpoint = body
        .get("mcp_endpoint")
        .and_then(|v: &serde_json::Value| v.as_str())
        .unwrap_or(endpoint)
        .to_string();
    let now = crate::pod::now_unix();
    Ok(PeerInfo {
        agent_id,
        container_name: String::new(),
        did_uri,
        mcp_endpoint,
        discovery_endpoint: url,
        registered_at: now,
        last_seen: now,
    })
}

/// Ping a peer's health endpoint. Returns true if 200 OK.
pub fn ping_peer(peer: &PeerInfo) -> Result<bool, String> {
    use crate::halo::http_client;
    let url = peer.mcp_endpoint.trim_end_matches("/mcp").to_string() + "/health";
    let result = http_client::get_with_timeout(&url, std::time::Duration::from_secs(5))
        .and_then(|req| req.call().map_err(|e| format!("{e}")));
    match result {
        Ok(resp) => Ok(resp.status() == 200),
        Err(_) => Ok(false),
    }
}

/// Measure latency to a peer in milliseconds. Returns (reachable, latency_ms).
pub fn ping_peer_with_latency(peer: &PeerInfo) -> (bool, u64) {
    use crate::halo::http_client;
    let url = peer.mcp_endpoint.trim_end_matches("/mcp").to_string() + "/health";
    let start = std::time::Instant::now();
    let result = http_client::get_with_timeout(&url, std::time::Duration::from_secs(5))
        .and_then(|req| req.call().map_err(|e| format!("{e}")));
    let elapsed = start.elapsed().as_millis() as u64;
    match result {
        Ok(resp) if resp.status() == 200 => (true, elapsed),
        _ => (false, elapsed),
    }
}

/// Call a remote peer's MCP tool via HTTP JSON-RPC.
pub fn call_remote_tool(
    peer: &PeerInfo,
    tool_name: &str,
    arguments: serde_json::Value,
    auth_token: Option<&str>,
) -> Result<serde_json::Value, String> {
    use crate::halo::http_client;
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": tool_name,
            "arguments": arguments
        }
    });
    let mut req =
        http_client::post_with_timeout(&peer.mcp_endpoint, std::time::Duration::from_secs(30))?;
    if let Some(token) = auth_token {
        req = req.header("Authorization", &format!("Bearer {token}"));
    }
    let resp = req
        .send_json(&payload)
        .map_err(|e| format!("mesh_call to {} tool {tool_name}: {e}", peer.agent_id))?;
    let body: serde_json::Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse mesh_call response: {e}"))?;
    if let Some(err) = body.get("error") {
        return Err(format!(
            "remote tool error: {}",
            serde_json::to_string(err).unwrap_or_default()
        ));
    }
    Ok(body
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}

/// Send a ProofEnvelope to a remote peer for verification.
pub fn exchange_envelope(
    peer: &PeerInfo,
    envelope_json: &serde_json::Value,
    auth_token: Option<&str>,
) -> Result<serde_json::Value, String> {
    call_remote_tool(
        peer,
        "nucleusdb_verify",
        serde_json::json!({ "envelope": envelope_json }),
        auth_token,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn peer_registry_register_and_find() {
        let mut reg = PeerRegistry::new();
        let peer = PeerInfo {
            agent_id: "agent-alice".to_string(),
            container_name: "alice".to_string(),
            did_uri: Some("did:key:z6MkAlice".to_string()),
            mcp_endpoint: "http://alice:8420/mcp".to_string(),
            discovery_endpoint: "http://alice:8420/pod/.well-known/nucleus-pod".to_string(),
            registered_at: 1000,
            last_seen: 1000,
        };
        reg.register(peer.clone());
        assert!(reg.find("agent-alice").is_some());
        assert!(reg.find("agent-bob").is_none());
        assert!(reg.find_by_did("did:key:z6MkAlice").is_some());
    }

    #[test]
    fn peer_registry_deregister() {
        let mut reg = PeerRegistry::new();
        let peer = PeerInfo {
            agent_id: "agent-bob".to_string(),
            container_name: "bob".to_string(),
            did_uri: None,
            mcp_endpoint: "http://bob:8420/mcp".to_string(),
            discovery_endpoint: "http://bob:8420/pod/.well-known/nucleus-pod".to_string(),
            registered_at: 2000,
            last_seen: 2000,
        };
        reg.register(peer);
        assert!(reg.deregister("agent-bob"));
        assert!(!reg.deregister("agent-bob"));
        assert!(reg.find("agent-bob").is_none());
    }

    #[test]
    fn peer_registry_peers_except() {
        let mut reg = PeerRegistry::new();
        for name in &["alice", "bob", "carol"] {
            reg.register(PeerInfo {
                agent_id: format!("agent-{name}"),
                container_name: name.to_string(),
                did_uri: None,
                mcp_endpoint: format!("http://{name}:8420/mcp"),
                discovery_endpoint: format!("http://{name}:8420/pod/.well-known/nucleus-pod"),
                registered_at: 3000,
                last_seen: 3000,
            });
        }
        let peers = reg.peers_except("agent-alice");
        assert_eq!(peers.len(), 2);
        assert!(peers.iter().all(|p| p.agent_id != "agent-alice"));
    }

    #[test]
    fn peer_registry_prune_stale() {
        let mut reg = PeerRegistry::new();
        reg.register(PeerInfo {
            agent_id: "old".to_string(),
            container_name: "old".to_string(),
            did_uri: None,
            mcp_endpoint: String::new(),
            discovery_endpoint: String::new(),
            registered_at: 100,
            last_seen: 100,
        });
        reg.register(PeerInfo {
            agent_id: "new".to_string(),
            container_name: "new".to_string(),
            did_uri: None,
            mcp_endpoint: String::new(),
            discovery_endpoint: String::new(),
            registered_at: 5000,
            last_seen: 5000,
        });
        let pruned = reg.prune_stale(1000);
        assert_eq!(pruned, 1);
        assert!(reg.find("old").is_none());
        assert!(reg.find("new").is_some());
    }

    #[test]
    fn peer_registry_save_load_roundtrip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("peers.json");
        let mut reg = PeerRegistry::new();
        reg.register(PeerInfo {
            agent_id: "agent-x".to_string(),
            container_name: "x".to_string(),
            did_uri: Some("did:key:z6MkX".to_string()),
            mcp_endpoint: "http://x:8420/mcp".to_string(),
            discovery_endpoint: "http://x:8420/pod/.well-known/nucleus-pod".to_string(),
            registered_at: 9000,
            last_seen: 9000,
        });
        reg.save(&path).expect("save");
        let loaded = PeerRegistry::load(&path).expect("load");
        assert_eq!(loaded.peers.len(), 1);
        assert_eq!(
            loaded.find("agent-x").unwrap().did_uri.as_deref(),
            Some("did:key:z6MkX")
        );
    }

    #[test]
    fn peer_registry_load_missing_returns_empty() {
        let path = PathBuf::from("/tmp/nonexistent_mesh_registry_test.json");
        let reg = PeerRegistry::load(&path).expect("load missing");
        assert!(reg.peers.is_empty());
    }

    #[test]
    fn mesh_registry_path_respects_env_override() {
        let _guard = env_lock().lock().expect("lock env");
        let prev = std::env::var("NUCLEUSDB_MESH_REGISTRY").ok();
        std::env::set_var(
            "NUCLEUSDB_MESH_REGISTRY",
            "/tmp/nucleusdb-test-mesh-registry.json",
        );
        assert_eq!(
            mesh_registry_path(),
            PathBuf::from("/tmp/nucleusdb-test-mesh-registry.json")
        );
        if let Some(v) = prev {
            std::env::set_var("NUCLEUSDB_MESH_REGISTRY", v);
        } else {
            std::env::remove_var("NUCLEUSDB_MESH_REGISTRY");
        }
        assert_eq!(mesh_registry_path(), PathBuf::from(MESH_REGISTRY_PATH));
    }
}
