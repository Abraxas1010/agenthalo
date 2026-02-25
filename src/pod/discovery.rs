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
        }
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
