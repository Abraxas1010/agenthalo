use nucleusdb::cli::default_witness_cfg;
use nucleusdb::protocol::{NucleusDb, VcBackend};
use nucleusdb::state::State;
use nucleusdb::vcs::{FileOpInput, QueryFilter, WorkRecordInput, WorkRecordStore};

fn mk_input(ts: u64, author: &str, path: &str, content_hash: &str) -> WorkRecordInput {
    WorkRecordInput {
        parents: vec![],
        author_puf: author.to_string(),
        timestamp: Some(ts),
        op: FileOpInput::Create {
            path: path.to_string(),
            content_hash: content_hash.to_string(),
        },
    }
}

#[test]
fn abraxas_submit_and_query_indices_work() {
    let mut db = NucleusDb::new(
        State::new(vec![]),
        VcBackend::BinaryMerkle,
        default_witness_cfg(),
    );
    let store = WorkRecordStore::new();

    let rec1 = mk_input(
        1_700_000_001,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "src/main.rs",
        "1111111111111111111111111111111111111111111111111111111111111111",
    )
    .into_record(WorkRecordStore::now_unix_secs())
    .expect("record 1 parses");

    let rec2 = mk_input(
        1_700_000_002,
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "src/lib.rs",
        "2222222222222222222222222222222222222222222222222222222222222222",
    )
    .into_record(WorkRecordStore::now_unix_secs())
    .expect("record 2 parses");

    let sub1 = store
        .submit_record(&mut db, rec1)
        .expect("record 1 submitted");
    let _sub2 = store
        .submit_record(&mut db, rec2)
        .expect("record 2 submitted");

    let by_hash = store
        .query_records(
            &db,
            &QueryFilter {
                hash: Some(sub1.hash),
                ..Default::default()
            },
        )
        .into_iter()
        .next()
        .expect("hash query returns row");
    assert_eq!(by_hash.timestamp, 1_700_000_001);
    assert_eq!(by_hash.proof_ref, Some(1));

    let by_path = store.query_records(
        &db,
        &QueryFilter {
            path_prefix: Some("src/".to_string()),
            ..Default::default()
        },
    );
    assert_eq!(by_path.len(), 2);

    let by_author = store.query_records(
        &db,
        &QueryFilter {
            author_puf: Some(
                nucleusdb::vcs::parse_hash_hex(
                    "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                )
                .expect("author hash parses"),
            ),
            ..Default::default()
        },
    );
    assert_eq!(by_author.len(), 1);
    assert_eq!(by_author[0].timestamp, 1_700_000_002);

    let status = store.status(&db);
    assert_eq!(status.record_count, 2);
    assert_eq!(status.latest_timestamp, Some(1_700_000_002));
}
