use nucleusdb::vcs::{
    analyze_records, export_state_to_worktree, hash_hex, parse_hash_hex, FileOp, WorkRecord,
};
use std::path::PathBuf;

fn rec(ts: u64, op: FileOp, hash_hex_str: &str) -> WorkRecord {
    WorkRecord {
        hash: parse_hash_hex(hash_hex_str).unwrap(),
        parents: vec![],
        author_puf: [0u8; 32],
        timestamp: ts,
        op,
        proof_ref: None,
    }
}

#[test]
fn analyze_detects_same_timestamp_conflict() {
    let h1 =
        parse_hash_hex("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let h2 =
        parse_hash_hex("2222222222222222222222222222222222222222222222222222222222222222").unwrap();
    let records = vec![
        rec(
            10,
            FileOp::Create {
                path: "src/lib.rs".to_string(),
                content_hash: h1,
            },
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        rec(
            10,
            FileOp::Modify {
                path: "src/lib.rs".to_string(),
                old_hash: h1,
                new_hash: h2,
                patch: None,
            },
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    ];

    let snapshot = analyze_records(&records);
    assert_eq!(snapshot.record_count, 2);
    assert_eq!(snapshot.conflict_count, 1);
    assert_eq!(snapshot.conflicts[0].path, "src/lib.rs");
}

#[test]
fn export_materializes_latest_state() {
    let h1 =
        parse_hash_hex("1111111111111111111111111111111111111111111111111111111111111111").unwrap();
    let h2 =
        parse_hash_hex("2222222222222222222222222222222222222222222222222222222222222222").unwrap();
    let records = vec![
        rec(
            10,
            FileOp::Create {
                path: "a.txt".to_string(),
                content_hash: h1,
            },
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
        rec(
            11,
            FileOp::Modify {
                path: "a.txt".to_string(),
                old_hash: h1,
                new_hash: h2,
                patch: None,
            },
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
        rec(
            12,
            FileOp::Delete {
                path: "gone.txt".to_string(),
                content_hash: h1,
            },
            "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        ),
    ];
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "nucleusdb_abraxas_merge_agent_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let stats = export_state_to_worktree(&records, &dir).unwrap();
    assert_eq!(stats.written_files, 1);
    assert!(PathBuf::from(&dir).join("a.txt").exists());
    let body = std::fs::read_to_string(PathBuf::from(&dir).join("a.txt")).unwrap();
    assert!(body.contains(&hash_hex(&h2)));
    let _ = std::fs::remove_dir_all(&dir);
}
