//! Content-addressable blob store for NucleusDB.
//!
//! Blob types (Text, Json, Bytes, Vector) store a content-hash in the state
//! vector and the actual payload here.  The blob store is keyed by the
//! user-facing key name (not content hash) so that each key→blob mapping is
//! unique and retrievable.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// In-memory content-addressable blob store.
///
/// Keyed by the user-facing key name.  Persisted as part of the NucleusDB
/// snapshot.  Content-addressing is enforced at the typed_value layer: the
/// u64 cell in the state vector = SHA-256(key | "|" | blob_data)[0..8].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobStore {
    blobs: BTreeMap<String, Vec<u8>>,
}

impl BlobStore {
    pub fn new() -> Self {
        Self {
            blobs: BTreeMap::new(),
        }
    }

    /// Store a blob for the given key.  Overwrites any existing blob.
    pub fn put(&mut self, key: &str, data: Vec<u8>) {
        self.blobs.insert(key.to_string(), data);
    }

    /// Retrieve a blob by key.
    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.blobs.get(key).map(|v| v.as_slice())
    }

    /// Remove a blob by key.
    pub fn remove(&mut self, key: &str) -> Option<Vec<u8>> {
        self.blobs.remove(key)
    }

    /// Check if a key has a blob.
    pub fn contains(&self, key: &str) -> bool {
        self.blobs.contains_key(key)
    }

    /// Number of stored blobs.
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }

    /// Total bytes across all blobs.
    pub fn total_bytes(&self) -> usize {
        self.blobs.values().map(|v| v.len()).sum()
    }

    /// Iterate all (key, blob) pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.blobs.iter().map(|(k, v)| (k.as_str(), v.as_slice()))
    }
}

impl Default for BlobStore {
    fn default() -> Self {
        Self::new()
    }
}
