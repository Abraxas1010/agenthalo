use crate::halo::util::hex_encode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const CIRCUIT_METADATA_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CircuitPolicy {
    #[default]
    DevDeterministic,
    ProductionRequired,
}

impl CircuitPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DevDeterministic => "dev",
            Self::ProductionRequired => "production",
        }
    }

    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" | "deterministic" => Ok(Self::DevDeterministic),
            "production" | "prod" => Ok(Self::ProductionRequired),
            other => Err(format!(
                "invalid circuit policy `{other}` (expected `dev` or `production`)"
            )),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
pub struct CircuitArtifactMetadata {
    pub schema_version: u32,
    pub setup_mode: CircuitPolicy,
    pub created_at: u64,
    pub max_events: usize,
    pub public_input_schema_version: u32,
    pub pk_sha256: String,
    pub vk_sha256: String,
}

pub fn key_hash_hex(raw: &[u8]) -> String {
    let digest = Sha256::digest(raw);
    hex_encode(&digest)
}

pub fn save_metadata(path: &Path, metadata: &CircuitArtifactMetadata) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create metadata dir {}: {e}", parent.display()))?;
    }
    let raw = serde_json::to_vec_pretty(metadata)
        .map_err(|e| format!("serialize circuit metadata: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("write circuit metadata {}: {e}", path.display()))
}

pub fn load_metadata(path: &Path) -> Result<CircuitArtifactMetadata, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read circuit metadata {}: {e}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|e| format!("parse circuit metadata {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_circuit_meta_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("metadata.json");
        let meta = CircuitArtifactMetadata {
            schema_version: CIRCUIT_METADATA_SCHEMA_VERSION,
            setup_mode: CircuitPolicy::DevDeterministic,
            created_at: 1,
            max_events: 256,
            public_input_schema_version: 1,
            pk_sha256: "11".repeat(32),
            vk_sha256: "22".repeat(32),
        };
        save_metadata(&path, &meta).expect("save metadata");
        let got = load_metadata(&path).expect("load metadata");
        assert_eq!(got, meta);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
