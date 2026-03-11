use crate::protocol::NucleusDb;
use crate::state::Delta;
use crate::transparency::ct6962::NodeHash;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::time::{SystemTime, UNIX_EPOCH};

const RECORD_PREFIX: &str = "abraxas:record:";
const AUTHOR_INDEX_PREFIX: &str = "abraxas:idx:author:";
const PATH_INDEX_PREFIX: &str = "abraxas:idx:path:";
const TS_INDEX_PREFIX: &str = "abraxas:idx:ts:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRecord {
    /// Content hash (SHA-256) of this record.
    pub hash: [u8; 32],
    /// Parent record hashes (0 = genesis, 1+ = sequential/merge).
    pub parents: Vec<[u8; 32]>,
    /// Author PUF digest (hardware-bound identity).
    pub author_puf: [u8; 32],
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    /// Operation type.
    pub op: FileOp,
    /// Merkle inclusion proof reference (NucleusDB commit height).
    pub proof_ref: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileOp {
    Create {
        path: String,
        content_hash: [u8; 32],
    },
    Modify {
        path: String,
        old_hash: [u8; 32],
        new_hash: [u8; 32],
        patch: Option<Vec<u8>>,
    },
    Delete {
        path: String,
        content_hash: [u8; 32],
    },
    Rename {
        old_path: String,
        new_path: String,
        content_hash: [u8; 32],
    },
}

impl FileOp {
    fn paths(&self) -> Vec<&str> {
        match self {
            FileOp::Create { path, .. } => vec![path.as_str()],
            FileOp::Modify { path, .. } => vec![path.as_str()],
            FileOp::Delete { path, .. } => vec![path.as_str()],
            FileOp::Rename {
                old_path, new_path, ..
            } => vec![old_path.as_str(), new_path.as_str()],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkRecordInput {
    pub parents: Vec<String>,
    pub author_puf: String,
    pub timestamp: Option<u64>,
    pub op: FileOpInput,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileOpInput {
    Create {
        path: String,
        content_hash: String,
    },
    Modify {
        path: String,
        old_hash: String,
        new_hash: String,
        patch: Option<Vec<u8>>,
    },
    Delete {
        path: String,
        content_hash: String,
    },
    Rename {
        old_path: String,
        new_path: String,
        content_hash: String,
    },
}

impl WorkRecordInput {
    pub fn into_record(self, now: u64) -> Result<WorkRecord, String> {
        let parents = self
            .parents
            .iter()
            .map(|h| parse_hash_hex(h))
            .collect::<Result<Vec<_>, _>>()?;
        let author_puf = parse_hash_hex(&self.author_puf)?;
        let op = self.op.try_into()?;
        Ok(WorkRecord {
            hash: [0u8; 32],
            parents,
            author_puf,
            timestamp: self.timestamp.unwrap_or(now),
            op,
            proof_ref: None,
        })
    }
}

impl TryFrom<FileOpInput> for FileOp {
    type Error = String;

    fn try_from(value: FileOpInput) -> Result<Self, Self::Error> {
        match value {
            FileOpInput::Create { path, content_hash } => Ok(FileOp::Create {
                path,
                content_hash: parse_hash_hex(&content_hash)?,
            }),
            FileOpInput::Modify {
                path,
                old_hash,
                new_hash,
                patch,
            } => Ok(FileOp::Modify {
                path,
                old_hash: parse_hash_hex(&old_hash)?,
                new_hash: parse_hash_hex(&new_hash)?,
                patch,
            }),
            FileOpInput::Delete { path, content_hash } => Ok(FileOp::Delete {
                path,
                content_hash: parse_hash_hex(&content_hash)?,
            }),
            FileOpInput::Rename {
                old_path,
                new_path,
                content_hash,
            } => Ok(FileOp::Rename {
                old_path,
                new_path,
                content_hash: parse_hash_hex(&content_hash)?,
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkRecordView {
    pub hash: String,
    pub parents: Vec<String>,
    pub author_puf: String,
    pub timestamp: u64,
    pub op: FileOp,
    pub proof_ref: Option<u64>,
}

impl From<&WorkRecord> for WorkRecordView {
    fn from(value: &WorkRecord) -> Self {
        Self {
            hash: hash_hex(&value.hash),
            parents: value.parents.iter().map(hash_hex).collect(),
            author_puf: hash_hex(&value.author_puf),
            timestamp: value.timestamp,
            op: value.op.clone(),
            proof_ref: value.proof_ref,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueryFilter {
    pub hash: Option<[u8; 32]>,
    pub author_puf: Option<[u8; 32]>,
    pub path_prefix: Option<String>,
    pub start_timestamp: Option<u64>,
    pub end_timestamp: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmitResult {
    pub hash: [u8; 32],
    pub proof_ref: u64,
    pub commit_height: u64,
    pub state_root: NodeHash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreStatus {
    pub record_count: usize,
    pub latest_hash: Option<[u8; 32]>,
    pub latest_timestamp: Option<u64>,
    pub sth_tree_size: u64,
    pub sth_root: Option<NodeHash>,
    pub sth_timestamp: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct WorkRecordStore;

impl WorkRecordStore {
    pub fn new() -> Self {
        Self
    }

    pub fn now_unix_secs() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    pub fn compute_hash(record: &WorkRecord) -> [u8; 32] {
        #[derive(Serialize)]
        struct Canonical<'a> {
            parents: &'a Vec<[u8; 32]>,
            author_puf: [u8; 32],
            timestamp: u64,
            op: &'a FileOp,
        }
        let canonical = Canonical {
            parents: &record.parents,
            author_puf: record.author_puf,
            timestamp: record.timestamp,
            op: &record.op,
        };
        let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
        let digest = Sha256::digest(&bytes);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
    }

    pub fn submit_record(
        &self,
        db: &mut NucleusDb,
        mut record: WorkRecord,
    ) -> Result<SubmitResult, String> {
        record.hash = Self::compute_hash(&record);
        let predicted_height = (db.entries.len() as u64) + 1;
        record.proof_ref = Some(predicted_height);

        let rec_hash_hex = hash_hex(&record.hash);
        let author_hex = hash_hex(&record.author_puf);
        let payload = serde_json::to_vec(&record).map_err(|e| format!("serialize record: {e}"))?;
        let chunks = bytes_to_u64_chunks(&payload);

        let mut writes = Vec::new();
        writes.push((record_len_key(&rec_hash_hex), payload.len() as u64));
        for (i, chunk) in chunks.iter().enumerate() {
            writes.push((record_chunk_key(&rec_hash_hex, i), *chunk));
        }

        let ts = record.timestamp;
        writes.push((author_index_key(&author_hex, ts, &rec_hash_hex), 1));
        writes.push((timestamp_index_key(ts, &rec_hash_hex), 1));
        for path in record.op.paths() {
            let path_hex = bytes_hex(path.as_bytes());
            writes.push((path_index_key(&path_hex, ts, &rec_hash_hex), 1));
        }

        let delta_writes = writes
            .into_iter()
            .map(|(key, value)| {
                let idx = db.keymap.get_or_create(&key);
                (idx, value)
            })
            .collect();

        let entry = db
            .commit(Delta::new(delta_writes), &[])
            .map_err(|e| format!("commit work record: {e:?}"))?;

        Ok(SubmitResult {
            hash: record.hash,
            proof_ref: entry.height,
            commit_height: entry.height,
            state_root: entry.state_root,
        })
    }

    pub fn get_record(&self, db: &NucleusDb, hash: &[u8; 32]) -> Option<WorkRecord> {
        let hash_hex = hash_hex(hash);
        let len_key = record_len_key(&hash_hex);
        let len = self.read_value_by_key(db, &len_key)? as usize;
        let chunk_count = len.div_ceil(8);
        let mut chunks = Vec::with_capacity(chunk_count);
        for i in 0..chunk_count {
            let key = record_chunk_key(&hash_hex, i);
            let value = self.read_value_by_key(db, &key)?;
            chunks.push(value);
        }
        let mut bytes = u64_chunks_to_bytes(&chunks);
        bytes.truncate(len);
        let rec: WorkRecord = serde_json::from_slice(&bytes).ok()?;
        if rec.hash == *hash {
            Some(rec)
        } else {
            None
        }
    }

    pub fn query_records(&self, db: &NucleusDb, filter: &QueryFilter) -> Vec<WorkRecord> {
        if let Some(hash) = filter.hash {
            let one = self.get_record(db, &hash);
            return one
                .into_iter()
                .filter(|r| self.matches_filter(r, filter))
                .collect();
        }

        let mut out = Vec::new();
        for hash in self.all_record_hashes(db) {
            if let Some(rec) = self.get_record(db, &hash) {
                if self.matches_filter(&rec, filter) {
                    out.push(rec);
                }
            }
        }

        out.sort_by_key(|r| (r.timestamp, r.hash));
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        out
    }

    pub fn status(&self, db: &NucleusDb) -> StoreStatus {
        let hashes = self.all_record_hashes(db);
        let mut latest: Option<(u64, [u8; 32])> = None;
        for hash in &hashes {
            if let Some(rec) = self.get_record(db, hash) {
                match latest {
                    Some((ts, _)) if rec.timestamp <= ts => {}
                    _ => latest = Some((rec.timestamp, *hash)),
                }
            }
        }

        let sth = db.current_sth();
        StoreStatus {
            record_count: hashes.len(),
            latest_hash: latest.map(|(_, h)| h),
            latest_timestamp: latest.map(|(ts, _)| ts),
            sth_tree_size: sth.as_ref().map(|s| s.tree_size).unwrap_or(0),
            sth_root: sth.as_ref().map(|s| s.root_hash),
            sth_timestamp: sth.map(|s| s.timestamp_unix_secs),
        }
    }

    fn matches_filter(&self, rec: &WorkRecord, filter: &QueryFilter) -> bool {
        if let Some(author) = filter.author_puf {
            if rec.author_puf != author {
                return false;
            }
        }
        if let Some(start) = filter.start_timestamp {
            if rec.timestamp < start {
                return false;
            }
        }
        if let Some(end) = filter.end_timestamp {
            if rec.timestamp > end {
                return false;
            }
        }
        if let Some(prefix) = &filter.path_prefix {
            let matches = rec.op.paths().iter().any(|p| p.starts_with(prefix));
            if !matches {
                return false;
            }
        }
        true
    }

    fn read_value_by_key(&self, db: &NucleusDb, key: &str) -> Option<u64> {
        let idx = db.keymap.get(key)?;
        db.state.values.get(idx).copied()
    }

    fn all_record_hashes(&self, db: &NucleusDb) -> Vec<[u8; 32]> {
        let mut set: BTreeSet<[u8; 32]> = BTreeSet::new();
        for (k, _) in db.keymap.all_keys() {
            if let Some(hash) = parse_record_len_key(k) {
                set.insert(hash);
            }
        }
        set.into_iter().collect()
    }
}

fn record_len_key(hash_hex: &str) -> String {
    format!("{RECORD_PREFIX}{hash_hex}:len")
}

fn record_chunk_key(hash_hex: &str, idx: usize) -> String {
    format!("{RECORD_PREFIX}{hash_hex}:chunk:{idx}")
}

fn parse_record_len_key(key: &str) -> Option<[u8; 32]> {
    if !key.starts_with(RECORD_PREFIX) || !key.ends_with(":len") {
        return None;
    }
    let trimmed = key
        .strip_prefix(RECORD_PREFIX)?
        .strip_suffix(":len")?
        .trim();
    parse_hash_hex(trimmed).ok()
}

fn author_index_key(author_hex: &str, ts: u64, hash_hex: &str) -> String {
    format!("{AUTHOR_INDEX_PREFIX}{author_hex}:{ts}:{hash_hex}")
}

fn path_index_key(path_hex: &str, ts: u64, hash_hex: &str) -> String {
    format!("{PATH_INDEX_PREFIX}{path_hex}:{ts}:{hash_hex}")
}

fn timestamp_index_key(ts: u64, hash_hex: &str) -> String {
    format!("{TS_INDEX_PREFIX}{ts}:{hash_hex}")
}

pub fn hash_hex(hash: &[u8; 32]) -> String {
    bytes_hex(hash)
}

pub fn bytes_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(hex_digit((b >> 4) & 0x0f));
        out.push(hex_digit(b & 0x0f));
    }
    out
}

fn hex_digit(v: u8) -> char {
    match v {
        0..=9 => (b'0' + v) as char,
        _ => (b'a' + (v - 10)) as char,
    }
}

pub fn parse_hash_hex(raw: &str) -> Result<[u8; 32], String> {
    let s = raw.trim();
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 64 {
        return Err(format!(
            "expected 32-byte hex hash (64 chars), got {} chars",
            s.len()
        ));
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = hex_nibble(s.as_bytes()[2 * i])?;
        let lo = hex_nibble(s.as_bytes()[2 * i + 1])?;
        *byte = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Result<u8, String> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(10 + (b - b'a')),
        b'A'..=b'F' => Ok(10 + (b - b'A')),
        _ => Err(format!("invalid hex digit '{}'", b as char)),
    }
}

fn bytes_to_u64_chunks(bytes: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    for chunk in bytes.chunks(8) {
        let mut buf = [0u8; 8];
        buf[..chunk.len()].copy_from_slice(chunk);
        out.push(u64::from_be_bytes(buf));
    }
    out
}

fn u64_chunks_to_bytes(chunks: &[u64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(chunks.len() * 8);
    for c in chunks {
        out.extend_from_slice(&c.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::protocol::VcBackend;
    use crate::state::State;

    fn sample_record(ts: u64, path: &str, author: &str) -> WorkRecord {
        let author_puf = parse_hash_hex(author).unwrap();
        let content_hash =
            parse_hash_hex("1111111111111111111111111111111111111111111111111111111111111111")
                .unwrap();
        WorkRecord {
            hash: [0u8; 32],
            parents: vec![],
            author_puf,
            timestamp: ts,
            op: FileOp::Create {
                path: path.to_string(),
                content_hash,
            },
            proof_ref: None,
        }
    }

    #[test]
    fn hash_hex_roundtrip() {
        let h = parse_hash_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            .unwrap();
        assert_eq!(
            hash_hex(&h),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
    }

    #[test]
    fn submit_and_query_roundtrip() {
        let mut db = NucleusDb::new(
            State::new(vec![]),
            VcBackend::BinaryMerkle,
            default_witness_cfg(),
        );
        let store = WorkRecordStore::new();

        let rec1 = sample_record(
            1700000000,
            "src/main.rs",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        );
        let rec2 = sample_record(
            1700000005,
            "src/lib.rs",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        );

        let sub1 = store.submit_record(&mut db, rec1).unwrap();
        let _sub2 = store.submit_record(&mut db, rec2).unwrap();

        let loaded1 = store.get_record(&db, &sub1.hash).unwrap();
        assert_eq!(loaded1.timestamp, 1700000000);
        assert_eq!(loaded1.proof_ref, Some(1));

        let f = QueryFilter {
            path_prefix: Some("src/".to_string()),
            ..Default::default()
        };
        let rows = store.query_records(&db, &f);
        assert_eq!(rows.len(), 2);

        let status = store.status(&db);
        assert_eq!(status.record_count, 2);
        assert_eq!(status.latest_timestamp, Some(1700000005));
    }
}
