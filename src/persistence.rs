use crate::keymap::KeyMap;
use crate::protocol::{CommitEntry, NucleusDb, VcBackend};
use crate::security::{ParameterSet, ReductionContract};
use crate::state::{apply, Delta, State};
use crate::transparency::ct6962::{NodeHash, SignedTreeHead};
use crate::vc::kzg::TrustedSetupArtifact;
use crate::witness::WitnessConfig;
use redb::{Database, ReadableTable, TableDefinition};
use serde::{Deserialize, Serialize};
use std::path::Path;

const SNAP_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("nucleusdb_snapshot");
const SNAP_KEY: &str = "latest";
const WAL_META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("nucleusdb_wal_meta");
const WAL_EVENTS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("nucleusdb_wal_events");
const WAL_META_KEY: &str = "meta";

#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Redb(String),
    Json(serde_json::Error),
    MissingSnapshot,
    MissingWalMeta,
    WalMetaMismatch { reason: String },
    SchemaMismatch { expected: String, got: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotV1 {
    schema: String,
    backend: VcBackend,
    security_params: ParameterSet,
    reduction_contracts: Vec<ReductionContract>,
    kzg_trusted_setup: Option<TrustedSetupArtifact>,
    state: State,
    entries: Vec<CommitEntry>,
    ct_leaves: Vec<NodeHash>,
    current_sth: Option<SignedTreeHead>,
    #[serde(default)]
    keymap: Option<KeyMap>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WalMetaV1 {
    schema: String,
    backend: VcBackend,
    security_params: ParameterSet,
    reduction_contracts: Vec<ReductionContract>,
    kzg_trusted_setup: Option<TrustedSetupArtifact>,
    initial_state: State,
    #[serde(default)]
    keymap: Option<KeyMap>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WalEventV1 {
    schema: String,
    seq: u64,
    delta: Delta,
    entry: CommitEntry,
    ct_leaf: NodeHash,
}

fn map_redb<E: std::fmt::Display>(e: E) -> PersistenceError {
    PersistenceError::Redb(e.to_string())
}

pub fn save_snapshot(path: &Path, db: &NucleusDb) -> Result<(), PersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(PersistenceError::Io)?;
    }
    let database = Database::create(path).map_err(map_redb)?;
    let wtx = database.begin_write().map_err(map_redb)?;
    {
        let mut table = wtx.open_table(SNAP_TABLE).map_err(map_redb)?;
        let payload = SnapshotV1 {
            schema: "nucleusdb/persistence-snapshot/v1".to_string(),
            backend: db.backend.clone(),
            security_params: db.security_params.clone(),
            reduction_contracts: db.reduction_contracts.clone(),
            kzg_trusted_setup: db.kzg_trusted_setup.clone(),
            state: db.state.clone(),
            entries: db.entries.clone(),
            ct_leaves: db.ct_leaves.clone(),
            current_sth: db.current_sth.clone(),
            keymap: Some(db.keymap.clone()),
        };
        let raw = serde_json::to_vec(&payload).map_err(PersistenceError::Json)?;
        table.insert(SNAP_KEY, raw.as_slice()).map_err(map_redb)?;
    }
    wtx.commit().map_err(map_redb)?;
    Ok(())
}

pub fn load_snapshot(
    path: &Path,
    witness_cfg: WitnessConfig,
) -> Result<NucleusDb, PersistenceError> {
    let database = Database::open(path).map_err(map_redb)?;
    let rtx = database.begin_read().map_err(map_redb)?;
    let table = rtx.open_table(SNAP_TABLE).map_err(map_redb)?;
    let raw = table
        .get(SNAP_KEY)
        .map_err(map_redb)?
        .ok_or(PersistenceError::MissingSnapshot)?;
    let snapshot: SnapshotV1 =
        serde_json::from_slice(raw.value()).map_err(PersistenceError::Json)?;
    if snapshot.schema != "nucleusdb/persistence-snapshot/v1" {
        return Err(PersistenceError::SchemaMismatch {
            expected: "nucleusdb/persistence-snapshot/v1".to_string(),
            got: snapshot.schema,
        });
    }

    let mut db = NucleusDb::new(snapshot.state.clone(), snapshot.backend, witness_cfg);
    db.security_params = snapshot.security_params;
    db.reduction_contracts = snapshot.reduction_contracts;
    db.kzg_trusted_setup = snapshot.kzg_trusted_setup;
    db.state = snapshot.state;
    db.entries = snapshot.entries;
    db.ct_leaves = snapshot.ct_leaves;
    db.current_sth = snapshot.current_sth;
    db.keymap = snapshot.keymap.unwrap_or_default();
    Ok(db)
}

pub fn init_wal(path: &Path, db: &NucleusDb) -> Result<(), PersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(PersistenceError::Io)?;
    }
    let database = Database::create(path).map_err(map_redb)?;
    let wtx = database.begin_write().map_err(map_redb)?;
    {
        let mut meta = wtx.open_table(WAL_META_TABLE).map_err(map_redb)?;
        let _events = wtx.open_table(WAL_EVENTS_TABLE).map_err(map_redb)?;
        let expected = WalMetaV1 {
            schema: "nucleusdb/persistence-wal-meta/v1".to_string(),
            backend: db.backend.clone(),
            security_params: db.security_params.clone(),
            reduction_contracts: db.reduction_contracts.clone(),
            kzg_trusted_setup: db.kzg_trusted_setup.clone(),
            initial_state: db.state.clone(),
            keymap: Some(db.keymap.clone()),
        };
        let got_existing: Option<WalMetaV1> = {
            let existing = meta.get(WAL_META_KEY).map_err(map_redb)?;
            existing
                .map(|v| serde_json::from_slice(v.value()).map_err(PersistenceError::Json))
                .transpose()?
        };
        if let Some(got) = got_existing {
            if got.schema != "nucleusdb/persistence-wal-meta/v1" {
                return Err(PersistenceError::SchemaMismatch {
                    expected: "nucleusdb/persistence-wal-meta/v1".to_string(),
                    got: got.schema,
                });
            }
            if got.backend != expected.backend
                || got.security_params != expected.security_params
                || got.reduction_contracts != expected.reduction_contracts
                || got.kzg_trusted_setup != expected.kzg_trusted_setup
                || got.initial_state != expected.initial_state
                || got.keymap != expected.keymap
            {
                return Err(PersistenceError::WalMetaMismatch {
                    reason: "existing WAL metadata does not match tenant configuration".to_string(),
                });
            }
        } else {
            let raw = serde_json::to_vec(&expected).map_err(PersistenceError::Json)?;
            meta.insert(WAL_META_KEY, raw.as_slice())
                .map_err(map_redb)?;
        }
    }
    wtx.commit().map_err(map_redb)?;
    Ok(())
}

pub fn append_wal_event(
    path: &Path,
    delta: &Delta,
    db: &NucleusDb,
    entry: &CommitEntry,
) -> Result<(), PersistenceError> {
    let database = Database::create(path).map_err(map_redb)?;
    let wtx = database.begin_write().map_err(map_redb)?;
    {
        let meta = wtx.open_table(WAL_META_TABLE).map_err(map_redb)?;
        let mut events = wtx.open_table(WAL_EVENTS_TABLE).map_err(map_redb)?;
        let meta_raw = meta
            .get(WAL_META_KEY)
            .map_err(map_redb)?
            .ok_or(PersistenceError::MissingWalMeta)?;
        let meta_val: WalMetaV1 =
            serde_json::from_slice(meta_raw.value()).map_err(PersistenceError::Json)?;
        if meta_val.schema != "nucleusdb/persistence-wal-meta/v1" {
            return Err(PersistenceError::SchemaMismatch {
                expected: "nucleusdb/persistence-wal-meta/v1".to_string(),
                got: meta_val.schema,
            });
        }

        let seq = db.entries.len() as u64;
        let ct_leaf = db
            .ct_leaves
            .last()
            .copied()
            .ok_or(PersistenceError::WalMetaMismatch {
                reason: "cannot append WAL event: ct_leaves is empty after commit".to_string(),
            })?;
        let payload = WalEventV1 {
            schema: "nucleusdb/persistence-wal-event/v1".to_string(),
            seq,
            delta: delta.clone(),
            entry: entry.clone(),
            ct_leaf,
        };
        let raw = serde_json::to_vec(&payload).map_err(PersistenceError::Json)?;
        events.insert(seq, raw.as_slice()).map_err(map_redb)?;
    }
    wtx.commit().map_err(map_redb)?;
    Ok(())
}

pub fn load_wal(path: &Path, witness_cfg: WitnessConfig) -> Result<NucleusDb, PersistenceError> {
    let database = Database::open(path).map_err(map_redb)?;
    let rtx = database.begin_read().map_err(map_redb)?;
    let meta = rtx.open_table(WAL_META_TABLE).map_err(map_redb)?;
    let events = rtx.open_table(WAL_EVENTS_TABLE).map_err(map_redb)?;

    let meta_raw = meta
        .get(WAL_META_KEY)
        .map_err(map_redb)?
        .ok_or(PersistenceError::MissingWalMeta)?;
    let wal_meta: WalMetaV1 =
        serde_json::from_slice(meta_raw.value()).map_err(PersistenceError::Json)?;
    if wal_meta.schema != "nucleusdb/persistence-wal-meta/v1" {
        return Err(PersistenceError::SchemaMismatch {
            expected: "nucleusdb/persistence-wal-meta/v1".to_string(),
            got: wal_meta.schema,
        });
    }

    let mut db = NucleusDb::new(
        wal_meta.initial_state.clone(),
        wal_meta.backend.clone(),
        witness_cfg,
    );
    db.security_params = wal_meta.security_params;
    db.reduction_contracts = wal_meta.reduction_contracts;
    db.kzg_trusted_setup = wal_meta.kzg_trusted_setup;
    db.state = wal_meta.initial_state;
    db.keymap = wal_meta.keymap.unwrap_or_default();

    for row in events.iter().map_err(map_redb)? {
        let (k, v) = row.map_err(map_redb)?;
        let key_seq = k.value();
        let event: WalEventV1 =
            serde_json::from_slice(v.value()).map_err(PersistenceError::Json)?;
        if event.schema != "nucleusdb/persistence-wal-event/v1" {
            return Err(PersistenceError::SchemaMismatch {
                expected: "nucleusdb/persistence-wal-event/v1".to_string(),
                got: event.schema,
            });
        }
        if event.seq != key_seq {
            return Err(PersistenceError::WalMetaMismatch {
                reason: format!(
                    "WAL event sequence mismatch for key {key_seq}: payload has {}",
                    event.seq
                ),
            });
        }
        if event.entry.height != key_seq {
            return Err(PersistenceError::WalMetaMismatch {
                reason: format!(
                    "WAL event commit height mismatch for key {key_seq}: entry has height {}",
                    event.entry.height
                ),
            });
        }
        db.state = apply(&db.state, &event.delta);
        db.entries.push(event.entry);
        db.ct_leaves.push(event.ct_leaf);
        db.current_sth = db.entries.last().map(|e| e.sth.clone());
    }

    Ok(db)
}

pub fn truncate_wal(path: &Path, db: &NucleusDb) -> Result<(), PersistenceError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(PersistenceError::Io)?;
    }
    let tmp_suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = path.with_extension(format!("swap.{tmp_suffix}.tmp"));
    init_wal(&tmp_path, db)?;
    match std::fs::rename(&tmp_path, path) {
        Ok(()) => Ok(()),
        Err(first_err) => {
            if path.exists() {
                std::fs::remove_file(path).map_err(PersistenceError::Io)?;
                std::fs::rename(&tmp_path, path).map_err(PersistenceError::Io)?;
                Ok(())
            } else {
                let _ = std::fs::remove_file(&tmp_path);
                Err(PersistenceError::Io(first_err))
            }
        }
    }
}
