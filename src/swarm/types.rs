use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

fn is_valid_hex_32(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ChunkId(String);

impl ChunkId {
    pub fn new(hex: impl Into<String>) -> Result<Self, String> {
        let value = hex.into().to_ascii_lowercase();
        if !is_valid_hex_32(&value) {
            return Err("chunk id must be 64 hex chars".to_string());
        }
        Ok(Self(value))
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        Self(blake3::hash(data).to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ChunkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ChunkId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct ManifestId(String);

impl ManifestId {
    pub fn new(hex: impl Into<String>) -> Result<Self, String> {
        let value = hex.into().to_ascii_lowercase();
        if !is_valid_hex_32(&value) {
            return Err("manifest id must be 64 hex chars".to_string());
        }
        Ok(Self(value))
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        Self(blake3::hash(data).to_hex().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ManifestId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ManifestId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Binary,
    Text,
    Model,
    Dataset,
    ContainerLayer,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Chunk {
    pub id: ChunkId,
    pub index: u32,
    pub total_chunks: u32,
    pub size: usize,
    pub data: Vec<u8>,
}

impl Chunk {
    pub fn new(index: u32, total_chunks: u32, data: Vec<u8>) -> Self {
        let id = ChunkId::from_bytes(&data);
        let size = data.len();
        Self {
            id,
            index,
            total_chunks,
            size,
            data,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_id_display_roundtrip() {
        let id = ChunkId::from_bytes(b"hello");
        let parsed: ChunkId = id.to_string().parse().expect("parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn manifest_id_display_roundtrip() {
        let id = ManifestId::from_bytes(b"manifest");
        let parsed: ManifestId = id.to_string().parse().expect("parse");
        assert_eq!(parsed, id);
    }

    #[test]
    fn ids_reject_invalid_hex() {
        assert!(ChunkId::new("xyz").is_err());
        assert!(ManifestId::new("1234").is_err());
    }

    #[test]
    fn serde_roundtrip_chunk() {
        let chunk = Chunk::new(0, 1, b"hello".to_vec());
        let encoded = serde_json::to_string(&chunk).expect("serialize");
        let decoded: Chunk = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, chunk);
    }
}
