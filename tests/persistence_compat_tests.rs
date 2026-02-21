use nucleusdb::persistence::{init_wal, load_wal};
use nucleusdb::protocol::{NucleusDb, VcBackend};
use nucleusdb::state::State;
use nucleusdb::witness::WitnessConfig;
use redb::{Database, TableDefinition};
use std::time::{SystemTime, UNIX_EPOCH};

const WAL_META_TABLE: TableDefinition<&str, &[u8]> = TableDefinition::new("nucleusdb_wal_meta");
const WAL_EVENTS_TABLE: TableDefinition<u64, &[u8]> = TableDefinition::new("nucleusdb_wal_events");
const WAL_META_KEY: &str = "meta";

fn mk_cfg() -> WitnessConfig {
    WitnessConfig::with_generated_keys(2, vec!["w1".into(), "w2".into(), "w3".into()])
}

#[test]
fn init_wal_accepts_legacy_meta_without_keymap_field() {
    let db = NucleusDb::new(State::new(vec![10, 20]), VcBackend::BinaryMerkle, mk_cfg());
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let wal_path = std::env::temp_dir().join(format!("nucleusdb_wal_legacy_meta_{stamp}.redb"));

    {
        let database = Database::create(&wal_path).expect("create wal db");
        let wtx = database.begin_write().expect("begin write");
        {
            let mut meta = wtx.open_table(WAL_META_TABLE).expect("meta table");
            let _events = wtx.open_table(WAL_EVENTS_TABLE).expect("events table");
            // Simulate Phase 1 metadata payload where `keymap` did not exist.
            let legacy_meta = serde_json::json!({
                "schema": "nucleusdb/persistence-wal-meta/v1",
                "backend": db.backend.clone(),
                "security_params": db.security_params.clone(),
                "reduction_contracts": db.reduction_contracts.clone(),
                "kzg_trusted_setup": db.kzg_trusted_setup.clone(),
                "initial_state": db.state.clone()
            });
            let raw = serde_json::to_vec(&legacy_meta).expect("serialize");
            meta.insert(WAL_META_KEY, raw.as_slice())
                .expect("insert meta");
        }
        wtx.commit().expect("commit legacy wal");
    }

    init_wal(&wal_path, &db).expect("legacy wal metadata should be accepted");
    let recovered = load_wal(&wal_path, mk_cfg()).expect("load wal");
    assert_eq!(recovered.state.values, db.state.values);
    assert!(recovered.keymap.is_empty());
}
