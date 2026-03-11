use crate::pod::envelope::ProofEnvelope;
use crate::protocol::NucleusDb;
use crate::state::Delta;
use crate::swarm::chunk_engine::verify_chunk;
use crate::swarm::config::ChunkParams;
use crate::swarm::types::{AssetType, Chunk, ChunkId, ManifestId};
use crate::typed_value::TypedValue;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_id: ManifestId,
    pub root_hash: String,
    pub chunk_hashes: Vec<ChunkId>,
    pub total_size: usize,
    pub asset_type: AssetType,
    pub creator_did: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<ProofEnvelope>,
}

impl Manifest {
    pub fn db_key(&self) -> String {
        format!("swarm/manifest/{}", self.manifest_id)
    }

    pub fn proof_db_key(&self) -> String {
        format!("swarm/manifest-proof/{}", self.manifest_id)
    }
}

#[derive(Clone, Debug)]
pub struct ManifestBuilder {
    pub asset_type: AssetType,
    pub creator_did: String,
    pub params: ChunkParams,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ManifestVerification {
    pub root_hash_valid: bool,
    pub proof_valid: bool,
    pub chunk_count: usize,
    pub accepted: bool,
}

impl ManifestBuilder {
    pub fn build(&self, chunks: &[Chunk]) -> Result<Manifest, String> {
        if chunks.is_empty() {
            return Err("manifest requires at least one chunk".to_string());
        }
        let max_chunk_size = self.params.chunk_size_bytes.max(1);
        if chunks.iter().any(|chunk| chunk.size > max_chunk_size) {
            return Err("chunk exceeds manifest chunk size policy".to_string());
        }
        let chunk_hashes = chunks
            .iter()
            .map(|chunk| chunk.id.clone())
            .collect::<Vec<_>>();
        let root_hash = compute_root_hash(&chunk_hashes);
        let manifest_id = ManifestId::from_bytes(root_hash.as_bytes());
        Ok(Manifest {
            manifest_id,
            root_hash,
            chunk_hashes,
            total_size: chunks.iter().map(|chunk| chunk.size).sum(),
            asset_type: self.asset_type.clone(),
            creator_did: self.creator_did.clone(),
            created_at: crate::pod::now_unix(),
            proof: None,
        })
    }

    pub fn build_and_commit(
        &self,
        db: &mut NucleusDb,
        chunks: &[Chunk],
    ) -> Result<Manifest, String> {
        let manifest = self.build(chunks)?;
        persist_manifest_with_proof(db, manifest)
    }
}

pub fn compute_root_hash(chunk_hashes: &[ChunkId]) -> String {
    let mut hasher = blake3::Hasher::new();
    for chunk_hash in chunk_hashes {
        hasher.update(chunk_hash.as_str().as_bytes());
        hasher.update(b"|");
    }
    hasher.finalize().to_hex().to_string()
}

pub fn verify_manifest(manifest: &Manifest, chunks: &[Chunk]) -> ManifestVerification {
    let mut ordered = chunks.to_vec();
    ordered.sort_by_key(|chunk| chunk.index);
    let chunk_sequence_valid = ordered.len() == manifest.chunk_hashes.len()
        && ordered
            .iter()
            .enumerate()
            .all(|(expected, chunk)| chunk.index == expected as u32 && verify_chunk(chunk));
    let chunk_hashes = ordered
        .iter()
        .map(|chunk| ChunkId::from_bytes(&chunk.data))
        .collect::<Vec<_>>();
    let root_hash_valid = chunk_sequence_valid
        && chunk_hashes == manifest.chunk_hashes
        && compute_root_hash(&chunk_hashes) == manifest.root_hash;
    let proof_valid = manifest
        .proof
        .as_ref()
        .map(|proof| proof.verify_locally().accepted)
        .unwrap_or(false);
    ManifestVerification {
        root_hash_valid,
        proof_valid,
        chunk_count: manifest.chunk_hashes.len(),
        accepted: root_hash_valid && (manifest.proof.is_none() || proof_valid),
    }
}

pub fn persist_manifest_with_proof(
    db: &mut NucleusDb,
    mut manifest: Manifest,
) -> Result<Manifest, String> {
    let manifest_key = manifest.db_key();
    let manifest_value =
        serde_json::to_value(&manifest).map_err(|e| format!("serialize manifest: {e}"))?;
    let (idx, cell) = db.put_typed(&manifest_key, TypedValue::Json(manifest_value))?;
    db.commit(Delta::new(vec![(idx, cell)]), &[])
        .map_err(|e| format!("commit manifest: {e:?}"))?;
    let value = db
        .get_typed(&manifest_key)
        .ok_or_else(|| "manifest missing after commit".to_string())?;
    let proof = ProofEnvelope::from_query(db, &manifest_key, idx, value)
        .ok_or_else(|| "manifest proof generation failed".to_string())?;
    let proof_value =
        serde_json::to_value(&proof).map_err(|e| format!("serialize manifest proof: {e}"))?;
    let (proof_idx, proof_cell) =
        db.put_typed(&manifest.proof_db_key(), TypedValue::Json(proof_value))?;
    db.commit(Delta::new(vec![(proof_idx, proof_cell)]), &[])
        .map_err(|e| format!("commit manifest proof: {e:?}"))?;
    manifest.proof = Some(proof);
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::protocol::{NucleusDb, VcBackend};
    use crate::state::State;
    use crate::swarm::chunk_engine::chunk_data;

    fn db() -> NucleusDb {
        NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            default_witness_cfg(),
        )
    }

    #[test]
    fn build_and_verify_manifest() {
        let chunks = chunk_data(
            b"hello manifest",
            &ChunkParams {
                chunk_size_bytes: 4,
            },
        );
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let manifest = builder.build(&chunks).expect("manifest");
        let verification = verify_manifest(&manifest, &chunks);
        assert!(verification.root_hash_valid);
        assert!(verification.accepted);
    }

    #[test]
    fn build_and_commit_manifest_produces_proof() {
        let chunks = chunk_data(
            b"proof manifest",
            &ChunkParams {
                chunk_size_bytes: 5,
            },
        );
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let mut db = db();
        let manifest = builder
            .build_and_commit(&mut db, &chunks)
            .expect("manifest");
        assert!(manifest.proof.is_some());
        assert!(verify_manifest(&manifest, &chunks).accepted);
    }

    #[test]
    fn build_and_commit_persists_proof_record() {
        let chunks = chunk_data(
            b"persist proof",
            &ChunkParams {
                chunk_size_bytes: 4,
            },
        );
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let mut db = db();
        let manifest = builder
            .build_and_commit(&mut db, &chunks)
            .expect("manifest");
        assert!(db.get_typed(&manifest.proof_db_key()).is_some());
    }

    #[test]
    fn manifest_detects_tampered_chunk_list() {
        let mut chunks = chunk_data(
            b"tamper detection",
            &ChunkParams {
                chunk_size_bytes: 3,
            },
        );
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let manifest = builder.build(&chunks).expect("manifest");
        chunks[0].data[0] ^= 0x01;
        chunks[0].id = ChunkId::from_bytes(&chunks[0].data);
        assert!(!verify_manifest(&manifest, &chunks).accepted);
    }

    #[test]
    fn manifest_recomputes_chunk_hash_from_payload() {
        let mut chunks = chunk_data(
            b"tamper detection",
            &ChunkParams {
                chunk_size_bytes: 3,
            },
        );
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let manifest = builder.build(&chunks).expect("manifest");
        chunks[0].data[0] ^= 0x01;
        assert!(!verify_manifest(&manifest, &chunks).accepted);
    }

    #[test]
    fn manifest_chunk_ordering_is_stable() {
        let chunks = chunk_data(
            b"abcdefghijk",
            &ChunkParams {
                chunk_size_bytes: 2,
            },
        );
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let manifest = builder.build(&chunks).expect("manifest");
        assert_eq!(manifest.chunk_hashes.len(), chunks.len());
        assert_eq!(manifest.chunk_hashes[0], chunks[0].id);
    }

    #[test]
    fn manifest_large_payload_has_multiple_chunks() {
        let data = vec![0x55u8; 700_000];
        let chunks = chunk_data(&data, &ChunkParams::default());
        let builder = ManifestBuilder {
            asset_type: AssetType::Dataset,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        let manifest = builder.build(&chunks).expect("manifest");
        assert!(manifest.chunk_hashes.len() > 1);
        assert_eq!(manifest.total_size, data.len());
    }

    #[test]
    fn manifest_rejects_empty_asset() {
        let builder = ManifestBuilder {
            asset_type: AssetType::Binary,
            creator_did: "did:key:test".to_string(),
            params: ChunkParams::default(),
        };
        assert!(builder.build(&[]).is_err());
    }
}
