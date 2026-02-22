use nucleusdb::audit::{
    append_evidence_jsonl, bundle_signing_message, create_evidence_bundle, load_evidence_jsonl,
    replay_verify_evidence, ReplayError,
};
use nucleusdb::commitment::{default_commitment_policy, CommitmentPolicy, CommitmentPolicyError};
use nucleusdb::multitenant::{
    MultiTenantError, MultiTenantNucleusDb, MultiTenantPolicy, TenantRole,
};
use nucleusdb::protocol::{CommitError, NucleusDb, VcBackend};
use nucleusdb::security::{
    default_reduction_contracts, ParameterError, ParameterSet, RefinementError,
    SecurityPolicyError, VcProfile,
};
use nucleusdb::sheaf::coherence::LocalSection;
use nucleusdb::state::{Delta, State};
use nucleusdb::vc::kzg::TrustedSetupError;
use nucleusdb::witness::{
    verify_quorum, WitnessConfig, WitnessKeyMaterialSource, WITNESS_SIGALG_MLDSA65,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

fn mk_cfg() -> WitnessConfig {
    WitnessConfig::with_generated_keys(2, vec!["w1".into(), "w2".into(), "w3".into()])
}

fn mk_cfg_with_seed(seed: &str) -> WitnessConfig {
    WitnessConfig::with_seed(2, vec!["w1".into(), "w2".into(), "w3".into()], seed)
}

fn coherent_views() -> Vec<LocalSection> {
    let mut a = BTreeMap::new();
    a.insert("shared".to_string(), 5);
    let mut b = BTreeMap::new();
    b.insert("shared".to_string(), 5);
    vec![
        LocalSection {
            lens_id: "L1".into(),
            kv: a,
        },
        LocalSection {
            lens_id: "L2".into(),
            kv: b,
        },
    ]
}

#[test]
fn commit_query_roundtrip_ipa() {
    let mut db = NucleusDb::new(State::new(vec![1, 2, 3]), VcBackend::Ipa, mk_cfg());
    let entry = db
        .commit(Delta::new(vec![(0, 9), (3, 11)]), &coherent_views())
        .expect("commit should succeed");
    assert_eq!(entry.height, 1);

    let (value, proof, root) = db.query(0).expect("query should exist");
    assert_eq!(value, 9);
    assert!(db.verify_query(0, value, &proof, root));
}

#[test]
fn commit_query_roundtrip_kzg() {
    let mut db = NucleusDb::new(State::new(vec![4, 5]), VcBackend::Kzg, mk_cfg());
    db.commit(Delta::new(vec![(1, 42)]), &coherent_views())
        .expect("commit should succeed");
    let (value, proof, root) = db.query(1).expect("query should exist");
    assert_eq!(value, 42);
    assert!(db.verify_query(1, value, &proof, root));
}

#[test]
fn commit_query_roundtrip_binary_merkle() {
    let mut db = NucleusDb::new(
        State::new(vec![4, 5, 6, 7]),
        VcBackend::BinaryMerkle,
        mk_cfg(),
    );
    db.commit(Delta::new(vec![(2, 99)]), &coherent_views())
        .expect("commit should succeed");
    let (value, proof, root) = db.query(2).expect("query should exist");
    assert_eq!(value, 99);
    assert!(db.verify_query(2, value, &proof, root));
}

#[test]
fn incoherent_sheaf_rejected() {
    let mut db = NucleusDb::new(State::new(vec![0]), VcBackend::Ipa, mk_cfg());
    let mut a = BTreeMap::new();
    let mut b = BTreeMap::new();
    a.insert("k".to_string(), 1);
    b.insert("k".to_string(), 2);
    let views = vec![
        LocalSection {
            lens_id: "L1".into(),
            kv: a,
        },
        LocalSection {
            lens_id: "L2".into(),
            kv: b,
        },
    ];

    let err = db
        .commit(Delta::new(vec![(0, 1)]), &views)
        .expect_err("incoherent views must fail");
    assert_eq!(err, CommitError::SheafIncoherent);
}

#[test]
fn consistency_proof_verification() {
    let mut db = NucleusDb::new(State::new(vec![1]), VcBackend::Ipa, mk_cfg());
    let views = coherent_views();
    let first = db
        .commit(Delta::new(vec![(0, 2)]), &views)
        .expect("first commit");
    let old_sth = first.sth;

    db.commit(Delta::new(vec![(0, 3)]), &views)
        .expect("second commit");

    let (new_sth, proof) = db
        .consistency_from(old_sth.tree_size)
        .expect("consistency proof should exist");
    assert!(db.verify_head_extension(&old_sth, &new_sth, &proof));
}

#[test]
fn tampered_state_root_fails_query_verification() {
    let mut db = NucleusDb::new(State::new(vec![7]), VcBackend::Kzg, mk_cfg());
    db.commit(Delta::new(vec![(0, 8)]), &coherent_views())
        .expect("commit");

    let (value, proof, mut root) = db.query(0).expect("query");
    root[0] ^= 0x01;
    assert!(!db.verify_query(0, value, &proof, root));
}

#[test]
fn with_security_rejects_bad_witness_threshold() {
    let mut cfg = mk_cfg();
    cfg.threshold = 4;
    let err = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Ipa,
        cfg,
        ParameterSet::default(),
        default_reduction_contracts(VcProfile::Ipa),
    )
    .expect_err("threshold above witness count must fail");
    assert_eq!(
        err,
        SecurityPolicyError::Parameter(ParameterError::ThresholdExceedsWitnessSet {
            threshold: 4,
            witnesses: 3
        })
    );
}

#[test]
fn with_security_rejects_commitment_scheme_mismatch() {
    let mut params = ParameterSet::default();
    params.commitment_policy = CommitmentPolicy {
        scheme_id: "kzg".to_string(),
        domain_separator: "nucleusdb.vc.kzg.v1".to_string(),
        max_degree: params.max_vector_len,
    };
    let err = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Ipa,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Ipa),
    )
    .expect_err("IPA profile must reject KZG commitment policy tag");
    assert_eq!(
        err,
        SecurityPolicyError::Parameter(ParameterError::CommitmentPolicy(
            CommitmentPolicyError::SchemeProfileMismatch {
                profile: VcProfile::Ipa,
                scheme_id: "kzg".to_string(),
            }
        ))
    );
}

#[test]
fn with_security_accepts_auto_commitment_policy_for_profile() {
    let max_vector_len = ParameterSet::default().max_vector_len;
    let params = ParameterSet {
        commitment_policy: default_commitment_policy(VcProfile::Ipa, max_vector_len),
        ..Default::default()
    };
    let db = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Ipa,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Ipa),
    )
    .expect("profile-aligned commitment policy should pass");
    assert_eq!(db.security_params.commitment_policy.scheme_id, "ipa");
}

#[test]
fn with_security_rejects_missing_kzg_setup_when_required() {
    let params = ParameterSet {
        require_kzg_trusted_setup: true,
        kzg_trusted_setup_id: None,
        ..Default::default()
    };
    let err = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Kzg,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Kzg),
    )
    .expect_err("kzg setup is mandatory when required");
    assert_eq!(
        err,
        SecurityPolicyError::Parameter(ParameterError::MissingKzgTrustedSetup)
    );
}

#[test]
fn with_security_rejects_missing_kzg_setup_path_when_required() {
    let params = ParameterSet {
        require_kzg_trusted_setup: true,
        kzg_trusted_setup_path: None,
        ..Default::default()
    };
    let err = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Kzg,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Kzg),
    )
    .expect_err("kzg setup path is mandatory when required");
    assert_eq!(
        err,
        SecurityPolicyError::Parameter(ParameterError::MissingKzgTrustedSetupPath)
    );
}

#[test]
fn with_security_rejects_kzg_setup_attestation_mismatch() {
    let params = ParameterSet {
        require_kzg_trusted_setup: true,
        kzg_trusted_setup_attestation_sha512:
            Some("00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad".to_string()),
        ..Default::default()
    };
    let err = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Kzg,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Kzg),
    )
    .expect_err("mismatched setup attestation must fail");
    assert_eq!(
        err,
        SecurityPolicyError::KzgTrustedSetup(TrustedSetupError::ExpectedAttestationMismatch {
            expected: "00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad00bad"
                .to_string(),
            got: "b1ba68a25d64fb0e29348404c7ef8ece8503ae5bd2eb0d8a172ddbc726d70df694cb5f6d323e7435649c35fa365339e93b354262735da11e01c00ad8b17923f1"
                .to_string(),
        })
    );
}

#[test]
fn with_security_rejects_kzg_setup_degree_cap_below_vector_limit() {
    let params = ParameterSet {
        max_vector_len: (1 << 20) + 1,
        ..Default::default()
    };
    let err = NucleusDb::with_security(
        State::new(vec![1]),
        VcBackend::Kzg,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Kzg),
    )
    .expect_err("setup max degree below vector bound must fail");
    assert_eq!(
        err,
        SecurityPolicyError::Parameter(ParameterError::CommitmentPolicy(
            CommitmentPolicyError::DegreeBoundInsufficient {
                required: (1 << 20) + 1,
                max_degree: 1 << 20,
            }
        ))
    );
}

#[test]
fn commit_rejects_oversized_delta_by_refinement_gate() {
    let params = ParameterSet {
        max_delta_writes: 1,
        ..Default::default()
    };

    let mut db = NucleusDb::with_security(
        State::new(vec![1, 2]),
        VcBackend::Ipa,
        mk_cfg(),
        params,
        default_reduction_contracts(VcProfile::Ipa),
    )
    .expect("security policy should be valid");

    let err = db
        .commit(Delta::new(vec![(0, 9), (1, 10)]), &coherent_views())
        .expect_err("delta beyond security bounds must fail");
    assert_eq!(
        err,
        CommitError::SecurityRefinementFailed(RefinementError::DeltaTooLarge { writes: 2, max: 1 })
    );
}

#[test]
fn evidence_roundtrip_and_replay_verification_passes() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_e1, ev1) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("first commit");
    let (_e2, ev2) = db
        .commit_with_evidence(Delta::new(vec![(1, 4)]), &coherent_views())
        .expect("second commit");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("nucleusdb_evidence_{stamp}.jsonl"));

    append_evidence_jsonl(&path, &ev1).expect("append first evidence");
    append_evidence_jsonl(&path, &ev2).expect("append second evidence");
    let loaded = load_evidence_jsonl(&path).expect("load evidence");
    replay_verify_evidence(&loaded, &mk_cfg()).expect("replay verification should pass");
}

#[test]
fn replay_verification_rejects_tampered_witness_signature() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_e1, ev1) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("first commit");
    let (_e2, mut ev2) = db
        .commit_with_evidence(Delta::new(vec![(1, 4)]), &coherent_views())
        .expect("second commit");
    ev2.witness_sigs[0].1 = "bad-signature".to_string();

    let err = replay_verify_evidence(&[ev1, ev2], &mk_cfg()).expect_err("must fail");
    assert_eq!(err, ReplayError::WitnessQuorumFailed { height: 2 });
}

#[test]
fn replay_verification_rejects_unknown_witness_algorithm_tag() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_e1, ev1) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("first commit");
    let (_e2, mut ev2) = db
        .commit_with_evidence(Delta::new(vec![(1, 4)]), &coherent_views())
        .expect("second commit");
    ev2.witness_signature_algorithm = "not-a-real-algorithm".to_string();

    let err = replay_verify_evidence(&[ev1, ev2], &mk_cfg()).expect_err("must fail");
    assert_eq!(
        err,
        ReplayError::InvalidWitnessSignatureAlgorithm {
            height: 2,
            got: "not-a-real-algorithm".to_string(),
        }
    );
}

#[test]
fn replay_verification_rejects_unknown_vc_backend_id() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_e1, ev1) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("first commit");
    let (_e2, mut ev2) = db
        .commit_with_evidence(Delta::new(vec![(1, 4)]), &coherent_views())
        .expect("second commit");
    ev2.vc_backend_id = "unknown_backend".to_string();

    let err = replay_verify_evidence(&[ev1, ev2], &mk_cfg()).expect_err("must fail");
    assert_eq!(
        err,
        ReplayError::InvalidVcBackend {
            height: 2,
            got: "unknown_backend".to_string(),
        }
    );
}

#[test]
fn commit_evidence_records_mldsa_signature_algorithm_by_default() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_entry, ev) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("commit with evidence");
    assert_eq!(ev.witness_signature_algorithm, WITNESS_SIGALG_MLDSA65);
}

#[test]
fn replay_verification_rejects_broken_prev_root_chain() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_e1, ev1) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("first commit");
    let (_e2, mut ev2) = db
        .commit_with_evidence(Delta::new(vec![(1, 4)]), &coherent_views())
        .expect("second commit");
    ev2.prev_state_root = format!("0{}", &ev2.prev_state_root[1..]);

    let err = replay_verify_evidence(&[ev1.clone(), ev2], &mk_cfg()).expect_err("must fail");
    assert_eq!(
        err,
        ReplayError::PrevRootMismatch {
            expected: ev1.state_root.clone(),
            got: format!("0{}", &ev1.state_root[1..]),
        }
    );
}

#[test]
fn evidence_bundle_manifest_is_signed_and_retention_bounded() {
    let mut db = NucleusDb::new(State::new(vec![1, 2]), VcBackend::Ipa, mk_cfg());
    let (_e1, ev1) = db
        .commit_with_evidence(Delta::new(vec![(0, 3)]), &coherent_views())
        .expect("first commit");
    let (_e2, ev2) = db
        .commit_with_evidence(Delta::new(vec![(1, 4)]), &coherent_views())
        .expect("second commit");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let evidence_path =
        std::env::temp_dir().join(format!("nucleusdb_bundle_evidence_{stamp}.jsonl"));
    let bundle_dir = std::env::temp_dir().join(format!("nucleusdb_bundle_out_{stamp}"));

    append_evidence_jsonl(&evidence_path, &ev1).expect("append first evidence");
    append_evidence_jsonl(&evidence_path, &ev2).expect("append second evidence");

    let manifest = create_evidence_bundle(&evidence_path, &bundle_dir, &mk_cfg(), 30)
        .expect("bundle creation should pass");
    assert_eq!(manifest.schema, "nucleusdb/evidence-bundle/v1");
    assert!(manifest.replay_verified);
    assert_eq!(manifest.evidence_records, 2);
    assert!(manifest.delete_after_unix_secs > manifest.created_unix_secs);

    let msg = bundle_signing_message(&manifest.payload_sha512);
    assert!(verify_quorum(&mk_cfg(), &msg, &manifest.witness_signatures));
    assert!(bundle_dir.join("manifest.json").is_file());
    assert!(bundle_dir.join("manifest.sha512").is_file());
    assert!(bundle_dir.join("evidence.jsonl").is_file());
}

#[test]
fn persistence_roundtrip_recovers_state_and_sth() {
    let cfg = mk_cfg();
    let mut db = NucleusDb::new(State::new(vec![2, 4, 6]), VcBackend::Ipa, cfg.clone());
    db.commit(Delta::new(vec![(1, 9)]), &coherent_views())
        .expect("first commit");
    db.commit(Delta::new(vec![(3, 12)]), &coherent_views())
        .expect("second commit");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let snap_path = std::env::temp_dir().join(format!("nucleusdb_snapshot_{stamp}.redb"));
    db.save_persistent(&snap_path)
        .expect("persist snapshot should succeed");

    let recovered = NucleusDb::load_persistent(&snap_path, cfg).expect("load snapshot");
    assert_eq!(recovered.state.values, db.state.values);
    assert_eq!(recovered.entries.len(), db.entries.len());
    assert_eq!(recovered.current_sth(), db.current_sth());

    let (value, proof, root) = recovered.query(1).expect("query");
    assert_eq!(value, 9);
    assert!(recovered.verify_query(1, value, &proof, root));
}

#[test]
fn persistence_roundtrip_preserves_custom_security_policy() {
    let mut params = ParameterSet::default();
    params.max_delta_writes = 5;
    params.max_vector_len = 1024;
    params.commitment_policy = default_commitment_policy(VcProfile::Ipa, params.max_vector_len);
    let reductions = default_reduction_contracts(VcProfile::Ipa);

    let cfg = mk_cfg();
    let mut db = NucleusDb::with_security(
        State::new(vec![1, 2, 3]),
        VcBackend::Ipa,
        cfg.clone(),
        params.clone(),
        reductions.clone(),
    )
    .expect("custom security policy should be valid");
    db.commit(Delta::new(vec![(0, 11)]), &coherent_views())
        .expect("commit");

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let snap_path = std::env::temp_dir().join(format!("nucleusdb_snapshot_policy_{stamp}.redb"));
    db.save_persistent(&snap_path).expect("save snapshot");

    let recovered = NucleusDb::load_persistent(&snap_path, cfg).expect("load snapshot");
    assert_eq!(recovered.security_params, params);
    assert_eq!(recovered.reduction_contracts, reductions);
}

#[test]
fn multi_tenant_isolation_and_auth_enforcement() {
    let manager = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    let tenant_a = NucleusDb::new(
        State::new(vec![1, 2, 3]),
        VcBackend::BinaryMerkle,
        mk_cfg_with_seed("tenant-a-master-seed-v1"),
    );
    let tenant_b = NucleusDb::new(
        State::new(vec![8, 9]),
        VcBackend::BinaryMerkle,
        mk_cfg_with_seed("tenant-b-master-seed-v1"),
    );
    manager
        .register_tenant("tenant_a", "token_a", tenant_a)
        .expect("register tenant_a");
    manager
        .register_tenant("tenant_b", "token_b", tenant_b)
        .expect("register tenant_b");

    manager
        .commit(
            "tenant_a",
            "token_a",
            Delta::new(vec![(1, 77)]),
            &coherent_views(),
        )
        .expect("tenant_a commit");
    let snap_a = manager
        .snapshot_tenant("tenant_a", "token_a")
        .expect("snapshot tenant_a");
    let snap_b = manager
        .snapshot_tenant("tenant_b", "token_b")
        .expect("snapshot tenant_b");

    assert_eq!(snap_a.state_values[1], 77);
    assert_eq!(snap_b.state_values, vec![8, 9]);
    assert_eq!(snap_a.entries, 1);
    assert_eq!(snap_b.entries, 0);

    let err = manager
        .commit(
            "tenant_a",
            "wrong-token",
            Delta::new(vec![(0, 11)]),
            &coherent_views(),
        )
        .expect_err("auth mismatch must fail");
    assert!(matches!(
        err,
        MultiTenantError::TenantAuthFailed { tenant_id } if tenant_id == "tenant_a"
    ));
}

#[test]
fn multi_tenant_parallel_commits_across_tenants() {
    let manager = Arc::new(MultiTenantNucleusDb::new(MultiTenantPolicy::production()));
    manager
        .register_tenant(
            "tenant_a",
            "token_a",
            NucleusDb::new(
                State::new(vec![0]),
                VcBackend::BinaryMerkle,
                mk_cfg_with_seed("parallel-a-master-seed-v1"),
            ),
        )
        .expect("register tenant_a");
    manager
        .register_tenant(
            "tenant_b",
            "token_b",
            NucleusDb::new(
                State::new(vec![0]),
                VcBackend::BinaryMerkle,
                mk_cfg_with_seed("parallel-b-master-seed-v1"),
            ),
        )
        .expect("register tenant_b");

    let a_mgr = Arc::clone(&manager);
    let h1 = thread::spawn(move || {
        for i in 0..40_u64 {
            a_mgr
                .commit(
                    "tenant_a",
                    "token_a",
                    Delta::new(vec![(0, i)]),
                    &coherent_views(),
                )
                .expect("tenant_a commit");
        }
    });
    let b_mgr = Arc::clone(&manager);
    let h2 = thread::spawn(move || {
        for i in 100..140_u64 {
            b_mgr
                .commit(
                    "tenant_b",
                    "token_b",
                    Delta::new(vec![(0, i)]),
                    &coherent_views(),
                )
                .expect("tenant_b commit");
        }
    });
    h1.join().expect("thread a");
    h2.join().expect("thread b");

    let snap_a = manager
        .snapshot_tenant("tenant_a", "token_a")
        .expect("snapshot tenant_a");
    let snap_b = manager
        .snapshot_tenant("tenant_b", "token_b")
        .expect("snapshot tenant_b");
    assert_eq!(snap_a.entries, 40);
    assert_eq!(snap_b.entries, 40);
    assert_eq!(snap_a.state_values[0], 39);
    assert_eq!(snap_b.state_values[0], 139);
}

#[test]
fn multi_tenant_wal_replay_restores_tenant_state() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let wal_path = std::env::temp_dir().join(format!("nucleusdb_tenant_wal_{stamp}.redb"));

    let manager = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    manager
        .register_tenant_with_wal_path(
            "tenant_wal",
            "token_wal",
            NucleusDb::new(
                State::new(vec![10, 20]),
                VcBackend::BinaryMerkle,
                mk_cfg_with_seed("tenant-wal-seed-v1"),
            ),
            Some(wal_path.clone()),
        )
        .expect("register tenant with WAL");

    manager
        .commit(
            "tenant_wal",
            "token_wal",
            Delta::new(vec![(0, 111)]),
            &coherent_views(),
        )
        .expect("commit #1");
    manager
        .commit(
            "tenant_wal",
            "token_wal",
            Delta::new(vec![(1, 222)]),
            &coherent_views(),
        )
        .expect("commit #2");

    let restored = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    restored
        .register_tenant_from_wal(
            "tenant_wal_restored",
            "token_restored",
            mk_cfg_with_seed("tenant-wal-seed-v1"),
            wal_path,
        )
        .expect("register from WAL");

    let snap = restored
        .snapshot_tenant("tenant_wal_restored", "token_restored")
        .expect("restored snapshot");
    assert_eq!(snap.entries, 2);
    assert_eq!(snap.state_values, vec![111, 222]);
}

#[test]
fn multi_tenant_production_policy_rejects_insecure_default_seed() {
    let mut cfg = mk_cfg_with_seed("force-insecure-source-seed");
    cfg.key_material_source = WitnessKeyMaterialSource::InsecureDefaultSeed;
    let manager = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    let err = manager
        .register_tenant(
            "tenant_insecure",
            "token",
            NucleusDb::new(State::new(vec![1]), VcBackend::BinaryMerkle, cfg),
        )
        .expect_err("insecure default seed must be blocked by production policy");
    assert!(matches!(
        err,
        MultiTenantError::TenantPolicyViolation { .. }
    ));
}

#[test]
fn multi_tenant_rbac_enforces_reader_writer_permissions() {
    let manager = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    manager
        .register_tenant(
            "tenant_rbac",
            "admin-token",
            NucleusDb::new(
                State::new(vec![5, 6]),
                VcBackend::BinaryMerkle,
                mk_cfg_with_seed("tenant-rbac-seed-v1"),
            ),
        )
        .expect("register tenant");

    manager
        .register_principal(
            "tenant_rbac",
            "admin",
            "admin-token",
            "reader1",
            "reader-token",
            TenantRole::Reader,
        )
        .expect("register reader");
    manager
        .register_principal(
            "tenant_rbac",
            "admin",
            "admin-token",
            "writer1",
            "writer-token",
            TenantRole::Writer,
        )
        .expect("register writer");

    let (value, proof, root) = manager
        .query_as("tenant_rbac", "reader1", "reader-token", 0)
        .expect("reader query");
    assert_eq!(value, 5);
    assert!(manager
        .verify_query_as(
            "tenant_rbac",
            "reader1",
            "reader-token",
            0,
            value,
            &proof,
            root
        )
        .expect("reader verify"));

    let err = manager
        .commit_as(
            "tenant_rbac",
            "reader1",
            "reader-token",
            Delta::new(vec![(0, 77)]),
            &coherent_views(),
        )
        .expect_err("reader must not be able to commit");
    assert!(matches!(
        err,
        MultiTenantError::TenantPermissionDenied { .. }
    ));

    manager
        .commit_as(
            "tenant_rbac",
            "writer1",
            "writer-token",
            Delta::new(vec![(0, 77)]),
            &coherent_views(),
        )
        .expect("writer commit");
    let snap = manager
        .snapshot_tenant_as("tenant_rbac", "writer1", "writer-token")
        .expect("writer snapshot");
    assert_eq!(snap.state_values[0], 77);
}

#[test]
fn multi_tenant_checkpoint_truncates_wal_and_preserves_state() {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let wal_path = std::env::temp_dir().join(format!("nucleusdb_tenant_wal_ckpt_{stamp}.redb"));
    let snap_path = std::env::temp_dir().join(format!("nucleusdb_tenant_ckpt_{stamp}.redb"));

    let manager = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    manager
        .register_tenant_with_wal_path(
            "tenant_ckpt",
            "admin-token",
            NucleusDb::new(
                State::new(vec![1, 2]),
                VcBackend::BinaryMerkle,
                mk_cfg_with_seed("tenant-ckpt-seed-v1"),
            ),
            Some(wal_path.clone()),
        )
        .expect("register");

    manager
        .commit(
            "tenant_ckpt",
            "admin-token",
            Delta::new(vec![(0, 10)]),
            &coherent_views(),
        )
        .expect("commit #1");
    manager
        .commit(
            "tenant_ckpt",
            "admin-token",
            Delta::new(vec![(1, 20)]),
            &coherent_views(),
        )
        .expect("commit #2");

    manager
        .checkpoint_tenant("tenant_ckpt", "admin", "admin-token", &snap_path)
        .expect("checkpoint");
    manager
        .commit(
            "tenant_ckpt",
            "admin-token",
            Delta::new(vec![(0, 99)]),
            &coherent_views(),
        )
        .expect("commit #3");

    let restored = MultiTenantNucleusDb::new(MultiTenantPolicy::production());
    restored
        .register_tenant_from_wal(
            "tenant_ckpt_restored",
            "admin-restored",
            mk_cfg_with_seed("tenant-ckpt-seed-v1"),
            wal_path,
        )
        .expect("register restored");
    let snap = restored
        .snapshot_tenant("tenant_ckpt_restored", "admin-restored")
        .expect("restored snapshot");
    assert_eq!(snap.state_values, vec![99, 20]);
    assert_eq!(snap.entries, 1);
}
