//! POD discovery endpoint — `.well-known/nucleus-pod`.
//!
//! Advertises the POD's capabilities so remote agents can determine
//! what backends, key prefixes, and verification methods are available.

use crate::protocol::VcBackend;
use serde::{Deserialize, Serialize};

/// Capability document served at `GET /pod/.well-known/nucleus-pod`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PodCapabilities {
    /// Schema version.
    pub version: u32,
    /// Protocol identifier.
    pub protocol: String,
    /// Owner PUF fingerprint (hex-encoded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_puf_hex: Option<String>,
    /// Supported VC backends.
    pub backends: Vec<String>,
    /// Notification endpoint (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_endpoint: Option<String>,
    /// Verification endpoint.
    pub verify_endpoint: String,
    /// Envelope fetch endpoint template.
    pub envelope_endpoint_template: String,
    /// Grants endpoint.
    pub grants_endpoint: String,
    /// MCP endpoint on the mesh network (if mesh-enabled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_endpoint: Option<String>,
    /// Agent DID URI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_did: Option<String>,
    /// Agent identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// Mesh network name this agent is connected to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_network: Option<String>,
    /// Peer agent IDs this agent is aware of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub known_peers: Vec<String>,
    /// Whether the agent exposes the local chunk store surface.
    #[serde(default)]
    pub chunk_store_available: bool,
    /// Whether Bitswap request-response is enabled on the halo mesh.
    #[serde(default)]
    pub bitswap_enabled: bool,
}

impl PodCapabilities {
    /// Build capabilities for a given base URL and backend list.
    pub fn build(base_url: &str, backends: &[VcBackend], owner_puf_hex: Option<String>) -> Self {
        let backend_names: Vec<String> = backends
            .iter()
            .map(|b| match b {
                VcBackend::Ipa => "ipa".to_string(),
                VcBackend::Kzg => "kzg".to_string(),
                VcBackend::BinaryMerkle => "binary_merkle".to_string(),
            })
            .collect();

        Self {
            version: 1,
            protocol: "nucleus-pod/v1".to_string(),
            owner_puf_hex,
            backends: backend_names,
            notify_endpoint: None,
            verify_endpoint: format!("{base_url}/pod/verify"),
            envelope_endpoint_template: format!("{base_url}/pod/{{tenant_id}}/{{key}}"),
            grants_endpoint: format!("{base_url}/pod/{{tenant_id}}/grants"),
            mcp_endpoint: None,
            agent_did: None,
            agent_id: None,
            mesh_network: None,
            known_peers: vec![],
            chunk_store_available: true,
            bitswap_enabled: crate::swarm::config::SwarmConfig::from_env().bitswap_enabled,
        }
    }

    /// Populate mesh-related fields from environment variables.
    pub fn with_mesh_from_env(mut self) -> Self {
        if let Ok(port) = std::env::var("NUCLEUSDB_MESH_PORT") {
            if let Ok(agent_id) = std::env::var("NUCLEUSDB_MESH_AGENT_ID") {
                let hostname = std::process::Command::new("hostname")
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_else(|| agent_id.clone());
                self.mcp_endpoint = Some(format!("http://{hostname}:{port}/mcp"));
                self.agent_id = Some(agent_id);
                self.mesh_network = Some("halo-mesh".to_string());
            }
        }
        if let Ok(did) = std::env::var("NUCLEUSDB_MESH_DID") {
            self.agent_did = Some(did);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_serde_roundtrip() {
        let caps = PodCapabilities::build(
            "http://localhost:3000",
            &[VcBackend::BinaryMerkle, VcBackend::Ipa],
            Some("aa".repeat(32)),
        );

        let json = serde_json::to_string_pretty(&caps).expect("serialize");
        let deserialized: PodCapabilities = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.protocol, "nucleus-pod/v1");
        assert_eq!(deserialized.backends.len(), 2);
        assert!(deserialized.owner_puf_hex.is_some());
    }
}
