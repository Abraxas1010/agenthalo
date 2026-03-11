use nucleusdb::pcn::{channel_snapshot, ChannelStatus, SettlementOp};
use nucleusdb::protocol::{NucleusDb, VcBackend};
use nucleusdb::sql::executor::{SqlExecutor, SqlResult};
use nucleusdb::state::State;
use nucleusdb::witness::WitnessConfig;

fn mk_db() -> NucleusDb {
    let cfg = WitnessConfig::with_generated_keys(2, vec!["w1".into(), "w2".into(), "w3".into()]);
    NucleusDb::new(State::new(vec![]), VcBackend::BinaryMerkle, cfg)
}

#[test]
fn pcn_roundtrip_open_update_close() {
    let mut db = mk_db();
    SettlementOp::Open {
        p1: "alice".into(),
        p2: "bob".into(),
        capacity: 100,
    }
    .apply(&mut db)
    .expect("open");
    SettlementOp::Update {
        p1: "alice".into(),
        p2: "bob".into(),
        balance1: 60,
        balance2: 40,
    }
    .apply(&mut db)
    .expect("update");
    SettlementOp::Close {
        p1: "alice".into(),
        p2: "bob".into(),
    }
    .apply(&mut db)
    .expect("close");
    let snap = channel_snapshot(&db, "alice", "bob").expect("snapshot");
    assert_eq!(snap.record.status, ChannelStatus::Closed);
    assert_eq!(snap.record.last_seq, 3);
    assert_eq!(snap.balance1 + snap.balance2, snap.record.capacity);
}

#[test]
fn pcn_append_only_rejects_sql_update() {
    let mut db = mk_db();
    SettlementOp::Open {
        p1: "alice".into(),
        p2: "bob".into(),
        capacity: 100,
    }
    .apply(&mut db)
    .expect("open");

    let mut sql = SqlExecutor::new(&mut db);
    let out = sql.execute("UPDATE data SET value = 1 WHERE key = 'channel:alice:bob';");
    match out {
        SqlResult::Error { message } => assert!(message.contains("AppendOnly")),
        _ => panic!("expected append-only rejection"),
    }
}
