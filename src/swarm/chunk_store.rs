use crate::protocol::NucleusDb;
use crate::state::Delta;
use crate::swarm::manifest::{persist_manifest_with_proof, Manifest};
use crate::swarm::types::{Chunk, ChunkId, ManifestId};
use crate::typed_value::TypedValue;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const CHUNK_KEY_PREFIX: &str = "swarm/chunk/";
const MANIFEST_KEY_PREFIX: &str = "swarm/manifest/";
const MANIFEST_PROOF_KEY_PREFIX: &str = "swarm/manifest-proof/";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChunkStore {
    chunks: BTreeMap<ChunkId, Chunk>,
    manifests: BTreeMap<ManifestId, Manifest>,
    active_transfers: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChunkStoreStats {
    pub total_chunks: usize,
    pub total_bytes: usize,
    pub manifest_count: usize,
    pub active_transfers: usize,
}

impl ChunkStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn load_from_db(db: &NucleusDb) -> Self {
        let mut store = Self::new();
        let mut proofs = BTreeMap::new();
        for (key, _) in db.keymap.all_keys() {
            if let Some(id) = key.strip_prefix(CHUNK_KEY_PREFIX) {
                if let Some(TypedValue::Bytes(raw)) = db.get_typed(key) {
                    if let Ok(chunk) = serde_json::from_slice::<Chunk>(&raw) {
                        if let Ok(chunk_id) = id.parse::<ChunkId>() {
                            store.chunks.insert(chunk_id, chunk);
                        }
                    }
                }
            } else if let Some(id) = key.strip_prefix(MANIFEST_KEY_PREFIX) {
                if let Some(TypedValue::Json(value)) = db.get_typed(key) {
                    if let Ok(manifest) = serde_json::from_value::<Manifest>(value) {
                        if let Ok(manifest_id) = id.parse::<ManifestId>() {
                            store.manifests.insert(manifest_id, manifest);
                        }
                    }
                }
            } else if let Some(id) = key.strip_prefix(MANIFEST_PROOF_KEY_PREFIX) {
                if let Some(TypedValue::Json(value)) = db.get_typed(key) {
                    if let Ok(proof) =
                        serde_json::from_value::<crate::pod::envelope::ProofEnvelope>(value)
                    {
                        if let Ok(manifest_id) = id.parse::<ManifestId>() {
                            proofs.insert(manifest_id, proof);
                        }
                    }
                }
            }
        }
        for (manifest_id, proof) in proofs {
            if let Some(manifest) = store.manifests.get_mut(&manifest_id) {
                manifest.proof = Some(proof);
            }
        }
        store
    }

    pub fn store_chunks(&mut self, db: &mut NucleusDb, chunks: &[Chunk]) -> Result<(), String> {
        let mut writes = Vec::with_capacity(chunks.len());
        for chunk in chunks {
            let key = format!("{CHUNK_KEY_PREFIX}{}", chunk.id);
            let raw = serde_json::to_vec(chunk).map_err(|e| format!("serialize chunk: {e}"))?;
            let (idx, cell) = db.put_typed(&key, TypedValue::Bytes(raw))?;
            writes.push((idx, cell));
            self.chunks.insert(chunk.id.clone(), chunk.clone());
        }
        if !writes.is_empty() {
            db.commit(Delta::new(writes), &[])
                .map_err(|e| format!("commit chunks: {e:?}"))?;
        }
        Ok(())
    }

    pub fn store_manifest(&mut self, db: &mut NucleusDb, manifest: Manifest) -> Result<(), String> {
        let manifest = persist_manifest_with_proof(db, manifest)?;
        self.manifests
            .insert(manifest.manifest_id.clone(), manifest);
        Ok(())
    }

    pub fn get_chunk(&self, chunk_id: &ChunkId) -> Option<&Chunk> {
        self.chunks.get(chunk_id)
    }

    pub fn get_manifest(&self, manifest_id: &ManifestId) -> Option<&Manifest> {
        self.manifests.get(manifest_id)
    }

    pub fn has_chunk(&self, chunk_id: &ChunkId) -> bool {
        self.chunks.contains_key(chunk_id)
    }

    pub fn remove_chunk(&mut self, db: &mut NucleusDb, chunk_id: &ChunkId) -> Result<bool, String> {
        let Some(chunk) = self.chunks.remove(chunk_id) else {
            return Ok(false);
        };
        let key = format!("{CHUNK_KEY_PREFIX}{}", chunk.id);
        let (idx, cell) = db.put_typed(&key, TypedValue::Null)?;
        db.commit(Delta::new(vec![(idx, cell)]), &[])
            .map_err(|e| format!("commit chunk removal: {e:?}"))?;
        Ok(true)
    }

    pub fn list_chunk_ids(&self) -> Vec<ChunkId> {
        self.chunks.keys().cloned().collect()
    }

    pub fn all_chunks(&self) -> Vec<Chunk> {
        self.chunks.values().cloned().collect()
    }

    pub fn stats(&self) -> ChunkStoreStats {
        ChunkStoreStats {
            total_chunks: self.chunks.len(),
            total_bytes: self.chunks.values().map(|chunk| chunk.size).sum(),
            manifest_count: self.manifests.len(),
            active_transfers: self.active_transfers,
        }
    }

    pub fn set_active_transfers(&mut self, active_transfers: usize) {
        self.active_transfers = active_transfers;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::protocol::{NucleusDb, VcBackend};
    use crate::state::State;
    use crate::swarm::chunk_engine::chunk_data;
    use crate::swarm::config::ChunkParams;
    use crate::swarm::manifest::ManifestBuilder;
    use crate::swarm::types::AssetType;

    fn db() -> NucleusDb {
        NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            default_witness_cfg(),
        )
    }

    #[test]
    fn store_and_retrieve_chunks() {
        let mut db = db();
        let chunks = chunk_data(
            b"hello chunk store",
            &ChunkParams {
                chunk_size_bytes: 4,
            },
        );
        let mut store = ChunkStore::new();
        store.store_chunks(&mut db, &chunks).expect("store");
        assert_eq!(
            store.get_chunk(&chunks[0].id).expect("chunk").data,
            chunks[0].data
        );
    }

    #[test]
    fn has_and_missing_chunk() {
        let mut db = db();
        let chunks = chunk_data(
            b"abc",
            &ChunkParams {
                chunk_size_bytes: 2,
            },
        );
        let mut store = ChunkStore::new();
        store.store_chunks(&mut db, &chunks).expect("store");
        assert!(store.has_chunk(&chunks[0].id));
        let missing = ChunkId::from_bytes(b"missing");
        assert!(!store.has_chunk(&missing));
    }

    #[test]
    fn remove_chunk_updates_store() {
        let mut db = db();
        let chunks = chunk_data(
            b"remove me",
            &ChunkParams {
                chunk_size_bytes: 4,
            },
        );
        let mut store = ChunkStore::new();
        store.store_chunks(&mut db, &chunks).expect("store");
        assert!(store.remove_chunk(&mut db, &chunks[0].id).expect("remove"));
        assert!(!store.has_chunk(&chunks[0].id));
    }

    #[test]
    fn stats_report_counts() {
        let mut db = db();
        let chunks = chunk_data(
            b"123456789",
            &ChunkParams {
                chunk_size_bytes: 3,
            },
        );
        let mut store = ChunkStore::new();
        store.store_chunks(&mut db, &chunks).expect("store");
        let stats = store.stats();
        assert_eq!(stats.total_chunks, chunks.len());
        assert_eq!(stats.total_bytes, 9);
    }

    #[test]
    fn list_ids_and_manifests_roundtrip() {
        let mut db = db();
        let chunks = chunk_data(
            b"manifest store",
            &ChunkParams {
                chunk_size_bytes: 5,
            },
        );
        let mut store = ChunkStore::new();
        store.store_chunks(&mut db, &chunks).expect("store");
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let manifest = builder.build(&chunks).expect("manifest");
        store
            .store_manifest(&mut db, manifest.clone())
            .expect("store manifest");
        assert_eq!(store.list_chunk_ids().len(), chunks.len());
        assert!(store.get_manifest(&manifest.manifest_id).is_some());
        let reloaded = ChunkStore::load_from_db(&db);
        assert_eq!(reloaded.list_chunk_ids().len(), chunks.len());
        let reloaded_manifest = reloaded
            .get_manifest(&manifest.manifest_id)
            .expect("reloaded manifest");
        assert!(reloaded_manifest.proof.is_some());
    }
}
