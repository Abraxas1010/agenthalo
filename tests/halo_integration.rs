use nucleusdb::halo::attest::{attest_session, AttestationRequest};
use nucleusdb::halo::audit::{audit_contract_source, AuditRequest, AuditSize};
use nucleusdb::halo::auth::{save_credentials, Credentials};
use nucleusdb::halo::circuit::{
    load_or_setup_attestation_keys, prove_attestation, verify_attestation_proof,
};
use nucleusdb::halo::onchain::{
    load_onchain_config, query_attestation, save_onchain_config, OnchainConfig,
};
use nucleusdb::halo::pq::{
    keygen_pq_with_paths, sign_pq_payload_with_paths, verify_detached_signature, PqStoragePaths,
};
use nucleusdb::halo::pricing::{calculate_cost, default_pricing};
use nucleusdb::halo::runner::AgentRunner;
use nucleusdb::halo::schema::{
    EventType, PaidOperation, SessionMetadata, SessionStatus, TraceEvent,
};
use nucleusdb::halo::trace::{
    list_sessions, now_unix_secs, paid_operations, session_events, session_summary, TraceWriter,
};
use nucleusdb::halo::trust::query_trust_score;
use nucleusdb::halo::wrap::{unwrap_agent, wrap_agent};
use std::path::PathBuf;

fn temp_db_path(tag: &str) -> PathBuf {
    let stamp = format!("{}-{}-{}", tag, std::process::id(), now_unix_secs());
    std::env::temp_dir().join(format!("agenthalo_{stamp}.ndb"))
}

#[test]
fn halo_generic_recording_roundtrip() {
    let db_path = temp_db_path("generic");
    let mut writer = TraceWriter::new(&db_path).expect("trace writer init");
    let started = now_unix_secs();
    let meta = SessionMetadata {
        session_id: format!("sess-{started}"),
        agent: "echo".to_string(),
        model: None,
        started_at: started,
        ended_at: None,
        prompt: Some("hello world".to_string()),
        status: SessionStatus::Running,
        user_id: None,
        machine_id: None,
        puf_digest: None,
    };
    writer.start_session(meta).expect("start session");

    let runner = AgentRunner::new("echo".to_string(), vec!["hello".to_string()]);
    let (code, _detected_model) = runner.run(&mut writer).expect("run echo");
    assert_eq!(code, 0);

    let summary = writer
        .end_session(SessionStatus::Completed)
        .expect("end session");
    assert!(summary.event_count >= 1);

    let sessions = list_sessions(&db_path).expect("list sessions");
    assert!(!sessions.is_empty());
}

#[test]
fn halo_trace_schema_readback() {
    let db_path = temp_db_path("schema");
    let mut writer = TraceWriter::new(&db_path).expect("trace writer init");
    let started = now_unix_secs();
    let sid = format!("sess-{started}-schema");

    writer
        .start_session(SessionMetadata {
            session_id: sid.clone(),
            agent: "generic".to_string(),
            model: Some("gpt-4.1".to_string()),
            started_at: started,
            ended_at: None,
            prompt: Some("demo".to_string()),
            status: SessionStatus::Running,
            user_id: Some("u1".to_string()),
            machine_id: Some("m1".to_string()),
            puf_digest: None,
        })
        .expect("start");

    writer
        .write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Assistant,
            content: serde_json::json!({"text": "hi"}),
            input_tokens: Some(10),
            output_tokens: Some(20),
            cache_read_tokens: Some(0),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
        .expect("write event");

    writer
        .end_session(SessionStatus::Completed)
        .expect("end session");

    let summary = session_summary(&db_path, &sid)
        .expect("summary query")
        .expect("summary exists");
    assert_eq!(summary.total_input_tokens, 10);
    assert_eq!(summary.total_output_tokens, 20);

    let events = session_events(&db_path, &sid).expect("events query");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, EventType::Assistant);
    assert!(!events[0].content_hash.is_empty());
}

#[test]
fn halo_cost_calculation_matches_expected() {
    let pricing = default_pricing();
    let cost = calculate_cost("gpt-4.1", 1_000_000, 2_000_000, 0, &pricing);
    assert!((cost - 18.0).abs() < 1e-9);
}

#[test]
fn halo_wrap_unwrap_edits_shell_rc() {
    let path = std::env::temp_dir().join(format!(
        "agenthalo_wrap_{}_{}.rc",
        std::process::id(),
        now_unix_secs()
    ));

    wrap_agent("claude", &path).expect("wrap");
    let wrapped = std::fs::read_to_string(&path).expect("read wrapped");
    assert!(wrapped.contains("AGENTHALO_WRAP_CLAUDE"));
    assert!(wrapped.contains("alias claude='agenthalo run claude'"));

    unwrap_agent("claude", &path).expect("unwrap");
    let unwrapped = std::fs::read_to_string(&path).expect("read unwrapped");
    assert!(!unwrapped.contains("AGENTHALO_WRAP_CLAUDE"));
    assert!(!unwrapped.contains("agenthalo run claude"));
}

#[test]
fn halo_eventtype_accepts_legacy_mpc_alias() {
    let raw = serde_json::json!({
        "seq": 7,
        "timestamp": 1771900000u64,
        "event_type": "mpc_tool_call",
        "content": {"name": "legacy_call"},
        "input_tokens": null,
        "output_tokens": null,
        "cache_read_tokens": null,
        "tool_name": "legacy_tool",
        "tool_input": null,
        "tool_output": null,
        "file_path": null,
        "content_hash": "abc"
    });
    let ev: TraceEvent = serde_json::from_value(raw).expect("deserialize legacy alias");
    assert_eq!(ev.event_type, EventType::McpToolCall);
    let out = serde_json::to_value(&ev).expect("serialize event");
    assert_eq!(
        out.get("event_type").and_then(|v| v.as_str()),
        Some("mcp_tool_call")
    );
}

#[cfg(unix)]
#[test]
fn halo_save_credentials_enforces_0600_on_existing_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!(
        "agenthalo_creds_perms_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join("credentials.json");
    std::fs::write(&path, "{\"api_key\":\"old\"}").expect("seed credentials");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("seed insecure perms");

    save_credentials(
        &path,
        &Credentials {
            api_key: Some("new-key".to_string()),
            oauth_token: None,
            oauth_provider: None,
            user_id: None,
            created_at: now_unix_secs(),
        },
    )
    .expect("save credentials");

    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "credentials mode should be 0600");
}

#[test]
fn halo_attestation_engine_roundtrip() {
    let db_path = temp_db_path("attestation");
    let mut writer = TraceWriter::new(&db_path).expect("trace writer");
    let sid = format!("sess-attest-{}", now_unix_secs());
    writer
        .start_session(SessionMetadata {
            session_id: sid.clone(),
            agent: "echo".to_string(),
            model: Some("gpt-4.1".to_string()),
            started_at: now_unix_secs(),
            ended_at: None,
            prompt: Some("attest me".to_string()),
            status: SessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })
        .expect("start session");
    writer
        .write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Assistant,
            content: serde_json::json!({"text":"hello"}),
            input_tokens: Some(1),
            output_tokens: Some(1),
            cache_read_tokens: Some(0),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
        .expect("write");
    writer
        .end_session(SessionStatus::Completed)
        .expect("end session");

    let sid_for_assert = sid.clone();
    let full = attest_session(
        &db_path,
        AttestationRequest {
            session_id: sid.clone(),
            anonymous: false,
        },
    )
    .expect("full attestation");
    let anon = attest_session(
        &db_path,
        AttestationRequest {
            session_id: sid,
            anonymous: true,
        },
    )
    .expect("anon attestation");

    assert_eq!(full.event_count, 1);
    assert!(!full.merkle_root.is_empty());
    assert_eq!(full.session_id.as_deref(), Some(sid_for_assert.as_str()));
    assert!(anon.session_id.is_none());
    assert!(anon.blinded_session_ref.is_some());
}

#[test]
fn halo_audit_engine_detects_reentrancy() {
    let source = r#"
        pragma solidity ^0.8.0;
        contract V {
            mapping(address => uint256) public balances;
            function withdraw() public {
                (bool ok,) = msg.sender.call{value: balances[msg.sender]}("");
                require(ok);
                balances[msg.sender] = 0;
            }
        }
    "#;
    let result = audit_contract_source(
        source,
        AuditRequest {
            contract_path: "inline.sol".to_string(),
            size: AuditSize::Small,
        },
    )
    .expect("audit");
    assert!(
        result
            .findings
            .iter()
            .any(|f| f.category == "reentrancy-cei-violation"),
        "expected CEI finding: {:?}",
        result.findings
    );
    assert!(result.risk_score > 0.0);
}

#[test]
fn halo_paid_operation_write_and_read() {
    let db_path = temp_db_path("paid");
    let mut writer = TraceWriter::new(&db_path).expect("writer");
    writer
        .record_paid_operation(PaidOperation {
            operation_id: "op-paid-1".to_string(),
            timestamp: now_unix_secs(),
            operation_type: "attest".to_string(),
            credits_spent: 10,
            usd_equivalent: 0.10,
            session_id: Some("sess-1".to_string()),
            result_digest: Some("deadbeef".to_string()),
            success: true,
            error: None,
        })
        .expect("record paid");

    let ops = paid_operations(&db_path).expect("read paid");
    assert_eq!(ops.len(), 1);
    assert_eq!(ops[0].operation_type, "attest");
    assert_eq!(ops[0].credits_spent, 10);
}

#[test]
fn halo_trust_query_reports_score() {
    let db_path = temp_db_path("trust-score");
    let mut writer = TraceWriter::new(&db_path).expect("writer");
    writer
        .start_session(SessionMetadata {
            session_id: "sess-trust-1".to_string(),
            agent: "codex".to_string(),
            model: None,
            started_at: now_unix_secs(),
            ended_at: None,
            prompt: None,
            status: SessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })
        .expect("start");
    writer.end_session(SessionStatus::Completed).expect("end");
    writer
        .record_paid_operation(PaidOperation {
            operation_id: "op-trust-1".to_string(),
            timestamp: now_unix_secs(),
            operation_type: "attest".to_string(),
            credits_spent: 10,
            usd_equivalent: 0.1,
            session_id: Some("sess-trust-1".to_string()),
            result_digest: Some("deadbeef".to_string()),
            success: true,
            error: None,
        })
        .expect("record");

    let score = query_trust_score(&db_path, Some("sess-trust-1")).expect("trust score");
    assert!(score.score > 0.0 && score.score <= 1.0);
    assert_eq!(score.attestation_count, 1);
}

#[test]
fn halo_pq_keygen_and_sign_detached_roundtrip() {
    let root = std::env::temp_dir().join(format!(
        "agenthalo_pq_integration_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create temp root");
    let paths = PqStoragePaths {
        wallet_path: root.join("pq_wallet.json"),
        signatures_dir: root.join("signatures"),
    };

    let key = keygen_pq_with_paths(&paths, false).expect("keygen");
    let payload = b"integration-test-message";
    let (sig, _sig_path) =
        sign_pq_payload_with_paths(&paths, payload, "message", Some("inline".to_string()))
            .expect("sign");
    assert_eq!(sig.key_id, key.key_id);
    assert!(
        verify_detached_signature(payload, &sig.public_key_hex, &sig.signature_hex)
            .expect("verify")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn halo_groth16_attestation_proof_roundtrip() {
    let db_path = temp_db_path("groth16-roundtrip");
    let mut writer = TraceWriter::new(&db_path).expect("writer");
    let sid = format!("sess-groth16-{}", now_unix_secs());
    writer
        .start_session(SessionMetadata {
            session_id: sid.clone(),
            agent: "codex".to_string(),
            model: Some("gpt-5".to_string()),
            started_at: now_unix_secs(),
            ended_at: None,
            prompt: Some("prove".to_string()),
            status: SessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })
        .expect("start");
    writer
        .write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Assistant,
            content: serde_json::json!({"text":"phase4"}),
            input_tokens: Some(3),
            output_tokens: Some(5),
            cache_read_tokens: Some(0),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
        .expect("event");
    writer.end_session(SessionStatus::Completed).expect("end");

    let att = attest_session(
        &db_path,
        AttestationRequest {
            session_id: sid,
            anonymous: false,
        },
    )
    .expect("attest");
    let (pk, vk, _info) = load_or_setup_attestation_keys(Some(64)).expect("keys");
    let proof = prove_attestation(&pk, &att).expect("prove");
    assert!(verify_attestation_proof(&vk, &proof).expect("verify"));
    assert_eq!(proof.public_inputs.len(), 5);
}

#[test]
fn halo_groth16_proof_is_deterministic() {
    let db_path = temp_db_path("groth16-det");
    let mut writer = TraceWriter::new(&db_path).expect("writer");
    let sid = format!("sess-groth16-det-{}", now_unix_secs());
    writer
        .start_session(SessionMetadata {
            session_id: sid.clone(),
            agent: "codex".to_string(),
            model: None,
            started_at: now_unix_secs(),
            ended_at: None,
            prompt: None,
            status: SessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })
        .expect("start");
    writer
        .write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Assistant,
            content: serde_json::json!({"text":"deterministic"}),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
        .expect("event");
    writer.end_session(SessionStatus::Completed).expect("end");
    let att = attest_session(
        &db_path,
        AttestationRequest {
            session_id: sid,
            anonymous: false,
        },
    )
    .expect("attest");
    let (pk, _vk, _info) = load_or_setup_attestation_keys(Some(64)).expect("keys");
    let a = prove_attestation(&pk, &att).expect("prove a");
    let b = prove_attestation(&pk, &att).expect("prove b");
    assert_eq!(a.proof_hex, b.proof_hex);
    assert_eq!(a.proof_words, b.proof_words);
}

#[test]
fn halo_onchain_config_management() {
    let root = std::env::temp_dir().join(format!(
        "agenthalo_onchain_cfg_test_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("dir");
    let path = root.join("onchain.json");
    let cfg = OnchainConfig {
        contract_address: "0x1111111111111111111111111111111111111111".to_string(),
        ..OnchainConfig::default()
    };
    save_onchain_config(&path, &cfg).expect("save");
    let loaded = load_onchain_config(&path).expect("load");
    assert_eq!(loaded.contract_address, cfg.contract_address);

    std::env::set_var("AGENTHALO_ONCHAIN_SIMULATION", "1");
    let status = query_attestation(&loaded, &"00".repeat(32)).expect("query");
    assert!(status.is_some());
    std::env::remove_var("AGENTHALO_ONCHAIN_SIMULATION");
    let _ = std::fs::remove_dir_all(&root);
}
