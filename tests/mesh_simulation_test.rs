//! Network Simulation Test: Sovereign Communications Stack
//!
//! Exercises the full three-layer stack in-process (no Docker required):
//!   1. Mesh network layer (peer registry, discovery, connectivity)
//!   2. DIDComm protocol layer (encrypt/decrypt, session mgmt, envelope bridge)
//!   3. Authorization chain (capability grants, tool dispatch gating)
//!
//! Simulates a three-node mesh: Alice, Bob, Carol.
//! - Alice grants Bob read access via DIDComm capability grant.
//! - Bob calls Alice's query tool via DIDComm-wrapped MCP call.
//! - Carol announces herself, heartbeat exchange between all pairs.
//! - Full envelope exchange lifecycle for ProofEnvelope data.
//! - Session manager tracks all interactions correctly.

use nucleusdb::comms::didcomm::{
    decrypt_message, encrypt_message, verify_envelope_signature, DIDCommMessage, MessageType,
};
use nucleusdb::comms::envelope::{
    unwrap_capability_grant, unwrap_mcp_call, unwrap_mcp_response, unwrap_proof_envelope,
    wrap_capability_grant, wrap_heartbeat, wrap_mcp_call, wrap_mcp_response, wrap_proof_envelope,
};
use nucleusdb::comms::session::SessionManager;
use nucleusdb::container::mesh::{PeerInfo, PeerRegistry, MESH_NETWORK_NAME};
use nucleusdb::halo::did::{did_from_genesis_seed, DIDIdentity};
use nucleusdb::pod::capability::AccessMode;
use nucleusdb::pod::did_acl_bridge::grant_access_to_agent;

fn agent(byte: u8) -> DIDIdentity {
    let seed = [byte; 64];
    did_from_genesis_seed(&seed).expect("agent identity from seed")
}

fn peer_info(id: &DIDIdentity, name: &str) -> PeerInfo {
    PeerInfo {
        agent_id: name.to_string(),
        container_name: name.to_string(),
        did_uri: Some(id.did.clone()),
        mcp_endpoint: format!("http://{name}:8420/mcp"),
        discovery_endpoint: format!("http://{name}:8420/.well-known/nucleus-pod"),
        registered_at: nucleusdb::pod::now_unix(),
        last_seen: nucleusdb::pod::now_unix(),
    }
}

// ────────────────────────────────────────────────────────────────────
// Layer 1: Mesh Network
// ────────────────────────────────────────────────────────────────────

#[test]
fn sim_mesh_three_node_registry() {
    let alice = agent(0xA1);
    let bob = agent(0xA2);
    let carol = agent(0xA3);

    let mut registry = PeerRegistry::new();
    registry.register(peer_info(&alice, "alice"));
    registry.register(peer_info(&bob, "bob"));
    registry.register(peer_info(&carol, "carol"));

    assert_eq!(registry.peers.len(), 3);
    assert!(registry.find("alice").is_some());
    assert!(registry.find("bob").is_some());
    assert!(registry.find("carol").is_some());

    // Each peer sees two others.
    assert_eq!(registry.peers_except("alice").len(), 2);
    assert_eq!(registry.peers_except("bob").len(), 2);
    assert_eq!(registry.peers_except("carol").len(), 2);

    // DID-based lookup.
    assert!(registry.find_by_did(&alice.did).is_some());
    assert!(registry.find_by_did(&bob.did).is_some());
    assert!(registry.find_by_did(&carol.did).is_some());
    assert!(registry.find_by_did("did:key:nonexistent").is_none());
}

#[test]
fn sim_mesh_registry_persistence() {
    let alice = agent(0xB1);
    let bob = agent(0xB2);

    let dir = std::env::temp_dir().join(format!(
        "nucleusdb_sim_registry_{}_{}",
        std::process::id(),
        nucleusdb::pod::now_unix()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("peers.json");

    let mut registry = PeerRegistry::new();
    registry.register(peer_info(&alice, "alice"));
    registry.register(peer_info(&bob, "bob"));
    registry.save(&path).unwrap();

    let loaded = PeerRegistry::load(&path).unwrap();
    assert_eq!(loaded.peers.len(), 2);
    assert_eq!(
        loaded.find("alice").unwrap().did_uri,
        Some(alice.did.clone())
    );
    assert_eq!(loaded.find("bob").unwrap().did_uri, Some(bob.did.clone()));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sim_mesh_deregister_and_prune() {
    let alice = agent(0xB3);
    let bob = agent(0xB4);
    let carol = agent(0xB5);

    let mut registry = PeerRegistry::new();
    registry.register(peer_info(&alice, "alice"));
    registry.register(peer_info(&bob, "bob"));

    let mut stale_carol = peer_info(&carol, "carol");
    stale_carol.last_seen = 1; // epoch + 1s (ancient)
    registry.register(stale_carol);

    assert_eq!(registry.peers.len(), 3);

    // Prune stale: carol's last_seen is 1, cutoff is now.
    let pruned = registry.prune_stale(nucleusdb::pod::now_unix() - 60);
    assert_eq!(pruned, 1);
    assert_eq!(registry.peers.len(), 2);
    assert!(registry.find("carol").is_none());

    // Explicit deregister.
    assert!(registry.deregister("bob"));
    assert_eq!(registry.peers.len(), 1);
    assert!(registry.find("bob").is_none());
    assert!(!registry.deregister("nonexistent"));
}

#[test]
fn sim_mesh_network_name_constant() {
    assert_eq!(MESH_NETWORK_NAME, "halo-mesh");
}

// ────────────────────────────────────────────────────────────────────
// Layer 2: DIDComm Protocol
// ────────────────────────────────────────────────────────────────────

#[test]
fn sim_didcomm_encrypt_decrypt_all_pairs() {
    let alice = agent(0xD1);
    let bob = agent(0xD2);
    let carol = agent(0xD3);

    let pairs = [
        (&alice, &bob, "alice→bob"),
        (&bob, &alice, "bob→alice"),
        (&alice, &carol, "alice→carol"),
        (&carol, &alice, "carol→alice"),
        (&bob, &carol, "bob→carol"),
        (&carol, &bob, "carol→bob"),
    ];

    for (sender, recipient, label) in &pairs {
        let msg = DIDCommMessage {
            id: format!("test-{label}"),
            type_: MessageType::Heartbeat,
            from: sender.did.clone(),
            to: vec![recipient.did.clone()],
            created_time: nucleusdb::pod::now_unix(),
            expires_time: None,
            body: serde_json::json!({"label": label}),
            thid: None,
            pthid: None,
        };
        let envelope = encrypt_message(sender, &recipient.did_document, &msg)
            .unwrap_or_else(|e| panic!("{label} encrypt failed: {e}"));

        // Signature verification without decryption.
        let sig_ok = verify_envelope_signature(&sender.did_document, &envelope)
            .unwrap_or_else(|e| panic!("{label} sig verify failed: {e}"));
        assert!(sig_ok, "{label} signature invalid");

        // Full decryption.
        let decrypted = decrypt_message(recipient, &sender.did_document, &envelope)
            .unwrap_or_else(|e| panic!("{label} decrypt failed: {e}"));
        assert_eq!(decrypted.body["label"], *label);
    }
}

#[test]
fn sim_didcomm_cross_agent_isolation() {
    let alice = agent(0xD4);
    let bob = agent(0xD5);
    let carol = agent(0xD6);

    // Alice encrypts for Bob.
    let msg = DIDCommMessage {
        id: "isolation-test".to_string(),
        type_: MessageType::McpToolCall,
        from: alice.did.clone(),
        to: vec![bob.did.clone()],
        created_time: nucleusdb::pod::now_unix(),
        expires_time: None,
        body: serde_json::json!({"secret": "for-bob-only"}),
        thid: None,
        pthid: None,
    };
    let envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();

    // Carol cannot decrypt Alice→Bob message.
    let result = decrypt_message(&carol, &alice.did_document, &envelope);
    assert!(result.is_err(), "Carol should not decrypt Alice→Bob");

    // Bob can decrypt.
    let decrypted = decrypt_message(&bob, &alice.did_document, &envelope).unwrap();
    assert_eq!(decrypted.body["secret"], "for-bob-only");
}

#[test]
fn sim_didcomm_tamper_detection() {
    let alice = agent(0xD7);
    let bob = agent(0xD8);

    let msg = DIDCommMessage {
        id: "tamper-test".to_string(),
        type_: MessageType::Heartbeat,
        from: alice.did.clone(),
        to: vec![bob.did.clone()],
        created_time: nucleusdb::pod::now_unix(),
        expires_time: None,
        body: serde_json::json!({"value": 42}),
        thid: None,
        pthid: None,
    };
    let mut envelope = encrypt_message(&alice, &bob.did_document, &msg).unwrap();

    // Tamper with ciphertext — should fail signature verification.
    let original_ct = envelope.ciphertext.clone();
    envelope.ciphertext = "AAAA".to_string() + &original_ct[4..];
    let result = decrypt_message(&bob, &alice.did_document, &envelope);
    assert!(result.is_err(), "Tampered ciphertext should be rejected");
}

// ────────────────────────────────────────────────────────────────────
// Layer 2.5: Envelope Bridge (MCP, ProofEnvelope, Capability)
// ────────────────────────────────────────────────────────────────────

#[test]
fn sim_mcp_tool_call_roundtrip() {
    let alice = agent(0xE1);
    let bob = agent(0xE2);

    // Bob calls Alice's nucleusdb_query tool via DIDComm.
    let envelope = wrap_mcp_call(
        &bob,
        &alice.did_document,
        "nucleusdb_query",
        serde_json::json!({"sql": "SELECT * FROM theorems WHERE status = 'proved'"}),
    )
    .unwrap();

    // Alice receives, unwraps, and gets the tool call.
    let (msg_id, tool_name, arguments) =
        unwrap_mcp_call(&alice, &bob.did_document, &envelope).unwrap();
    assert!(!msg_id.is_empty());
    assert_eq!(tool_name, "nucleusdb_query");
    assert_eq!(
        arguments["sql"],
        "SELECT * FROM theorems WHERE status = 'proved'"
    );

    // Alice responds.
    let response_envelope = wrap_mcp_response(
        &alice,
        &bob.did_document,
        "nucleusdb_query",
        serde_json::json!({"rows": [{"name": "Fermat", "status": "proved"}]}),
        Some(&msg_id),
    )
    .unwrap();

    // Bob receives the response.
    let (resp_tool, result) =
        unwrap_mcp_response(&bob, &alice.did_document, &response_envelope).unwrap();
    assert_eq!(resp_tool, "nucleusdb_query");
    assert_eq!(result["rows"][0]["name"], "Fermat");
}

#[test]
fn sim_proof_envelope_exchange() {
    let alice = agent(0xE3);
    let bob = agent(0xE4);

    let proof_data = serde_json::json!({
        "version": 2,
        "key": "attestations/session_42",
        "value_hash": "deadbeef1234",
        "proof_type": "merkle",
        "proof_nodes": ["abc", "def", "ghi"],
        "root_hash": "cafe0000"
    });

    let envelope = wrap_proof_envelope(&alice, &bob.did_document, &proof_data).unwrap();
    let unwrapped = unwrap_proof_envelope(&bob, &alice.did_document, &envelope).unwrap();
    assert_eq!(unwrapped["key"], "attestations/session_42");
    assert_eq!(unwrapped["root_hash"], "cafe0000");
    assert_eq!(unwrapped["proof_nodes"].as_array().unwrap().len(), 3);
}

#[test]
fn sim_capability_grant_and_accept() {
    let alice = agent(0xE5);
    let bob = agent(0xE6);

    // Alice grants Bob read access to "nucleusdb_query" and "nucleusdb_status".
    let cap = grant_access_to_agent(
        &alice,
        &bob.did,
        &[
            "nucleusdb_query".to_string(),
            "nucleusdb_status".to_string(),
        ],
        &[AccessMode::Read],
        3600,
    )
    .expect("grant capability");

    let envelope = wrap_capability_grant(&alice, &bob.did_document, &cap).unwrap();
    let received = unwrap_capability_grant(&bob, &alice.did_document, &envelope).unwrap();
    assert_eq!(received.grantor_did, alice.did);
    assert_eq!(received.grantee_did, bob.did);
    assert_eq!(received.resource_patterns.len(), 2);
    assert!(received
        .resource_patterns
        .contains(&"nucleusdb_query".to_string()));
    assert!(!received.revoked);
}

#[test]
fn sim_heartbeat_three_node() {
    let alice = agent(0xE7);
    let bob = agent(0xE8);
    let carol = agent(0xE9);

    // All six directed heartbeats.
    let pairs: Vec<(&DIDIdentity, &DIDIdentity)> = vec![
        (&alice, &bob),
        (&bob, &alice),
        (&alice, &carol),
        (&carol, &alice),
        (&bob, &carol),
        (&carol, &bob),
    ];

    for (sender, recipient) in &pairs {
        let envelope = wrap_heartbeat(sender, &recipient.did_document).unwrap();
        let decrypted = decrypt_message(recipient, &sender.did_document, &envelope).unwrap();
        assert_eq!(decrypted.type_, MessageType::Heartbeat);
        assert_eq!(decrypted.body["status"], "alive");
        assert!(!decrypted.is_expired());
    }
}

// ────────────────────────────────────────────────────────────────────
// Layer 3: Session Management
// ────────────────────────────────────────────────────────────────────

#[test]
fn sim_session_lifecycle_three_nodes() {
    let _alice = agent(0xF1);
    let bob = agent(0xF2);
    let carol = agent(0xF3);

    let mut mgr = SessionManager::new();

    // Alice establishes sessions with Bob and Carol.
    mgr.establish(bob.did.clone(), bob.did_document.clone());
    mgr.establish(carol.did.clone(), carol.did_document.clone());

    assert!(mgr.has_session(&bob.did));
    assert!(mgr.has_session(&carol.did));
    assert_eq!(mgr.active_peers().len(), 2);

    // Track a request from Alice to Bob.
    mgr.track_request(&bob.did, "req-alice-bob-1", MessageType::McpToolCall, 30)
        .unwrap();

    // Verify pending.
    let bob_session = mgr.get(&bob.did).unwrap();
    assert_eq!(bob_session.pending_requests.len(), 1);

    // Complete the request.
    mgr.complete_request(&bob.did, "req-alice-bob-1").unwrap();
    let bob_session = mgr.get(&bob.did).unwrap();
    assert_eq!(bob_session.pending_requests.len(), 0);
    assert_eq!(bob_session.message_count, 1);

    // Track request to Carol, then do cleanup (keep active, prune stale).
    mgr.track_request(
        &carol.did,
        "req-alice-carol-1",
        MessageType::EnvelopeExchange,
        30,
    )
    .unwrap();

    // Cleanup with generous timeout — nothing pruned.
    let now = nucleusdb::pod::now_unix();
    mgr.cleanup(now, 3600);
    assert_eq!(mgr.active_peers().len(), 2);
}

#[test]
fn sim_session_capability_tracking() {
    let alice = agent(0xF4);
    let bob = agent(0xF5);

    let mut mgr = SessionManager::new();
    mgr.establish(bob.did.clone(), bob.did_document.clone());

    let cap = grant_access_to_agent(
        &alice,
        &bob.did,
        &["*".to_string()],
        &[AccessMode::Read],
        3600,
    )
    .expect("grant capability");

    mgr.add_granted_capability(&bob.did, cap.clone());
    let session = mgr.get(&bob.did).unwrap();
    assert_eq!(session.granted_capabilities.len(), 1);
    assert_eq!(session.granted_capabilities[0].grantor_did, alice.did);
}

// ────────────────────────────────────────────────────────────────────
// Full Scenario: Alice ↔ Bob ↔ Carol End-to-End
// ────────────────────────────────────────────────────────────────────

#[test]
fn sim_full_scenario_grant_query_exchange() {
    // Setup: three agents.
    let alice = agent(0xFA);
    let bob = agent(0xFB);
    let carol = agent(0xFC);

    // 1. Build mesh registry.
    let mut registry = PeerRegistry::new();
    registry.register(peer_info(&alice, "alice"));
    registry.register(peer_info(&bob, "bob"));
    registry.register(peer_info(&carol, "carol"));
    assert_eq!(registry.peers.len(), 3);

    // 2. Alice and Bob establish DIDComm sessions.
    let mut alice_sessions = SessionManager::new();
    let mut bob_sessions = SessionManager::new();
    alice_sessions.establish(bob.did.clone(), bob.did_document.clone());
    bob_sessions.establish(alice.did.clone(), alice.did_document.clone());

    // 3. Alice grants Bob read access via DIDComm.
    let cap = grant_access_to_agent(
        &alice,
        &bob.did,
        &[
            "nucleusdb_query".to_string(),
            "nucleusdb_status".to_string(),
        ],
        &[AccessMode::Read],
        3600,
    )
    .expect("grant capability");

    let grant_env = wrap_capability_grant(&alice, &bob.did_document, &cap).unwrap();
    let received_cap = unwrap_capability_grant(&bob, &alice.did_document, &grant_env).unwrap();
    assert_eq!(received_cap.grantee_did, bob.did);
    bob_sessions.add_received_capability(&alice.did, received_cap);

    // 4. Bob calls Alice's nucleusdb_query via DIDComm.
    let call_env = wrap_mcp_call(
        &bob,
        &alice.did_document,
        "nucleusdb_query",
        serde_json::json!({"sql": "SELECT count(*) FROM proofs"}),
    )
    .unwrap();
    alice_sessions
        .track_request(&bob.did, "call-1", MessageType::McpToolCall, 30)
        .unwrap();

    let (call_id, tool_name, args) =
        unwrap_mcp_call(&alice, &bob.did_document, &call_env).unwrap();
    assert_eq!(tool_name, "nucleusdb_query");
    assert!(!call_id.is_empty());
    assert_eq!(args["sql"], "SELECT count(*) FROM proofs");

    // 5. Alice responds.
    let resp_env = wrap_mcp_response(
        &alice,
        &bob.did_document,
        "nucleusdb_query",
        serde_json::json!({"count": 42}),
        Some(&call_id),
    )
    .unwrap();
    alice_sessions.complete_request(&bob.did, "call-1").unwrap();

    let (resp_tool, resp_result) =
        unwrap_mcp_response(&bob, &alice.did_document, &resp_env).unwrap();
    assert_eq!(resp_tool, "nucleusdb_query");
    assert_eq!(resp_result["count"], 42);

    // 6. Alice sends Bob a ProofEnvelope.
    let proof = serde_json::json!({
        "version": 2,
        "key": "proofs/theorem_42",
        "value_hash": "aaaa",
        "proof_type": "merkle",
        "root_hash": "bbbb"
    });
    let proof_env = wrap_proof_envelope(&alice, &bob.did_document, &proof).unwrap();
    let received_proof = unwrap_proof_envelope(&bob, &alice.did_document, &proof_env).unwrap();
    assert_eq!(received_proof["key"], "proofs/theorem_42");

    // 7. Bob cannot call Carol (no session, no grant — crypto still works
    //    but authorization model would reject).
    let unauthorized_env = wrap_mcp_call(
        &bob,
        &carol.did_document,
        "nucleusdb_execute_sql",
        serde_json::json!({"sql": "DROP TABLE proofs"}),
    )
    .unwrap();
    // Carol can decrypt (crypto is peer-to-peer, not authorization-gated),
    // but the tool name and grant check would reject at the application layer.
    let (_, unauthorized_tool, _) =
        unwrap_mcp_call(&carol, &bob.did_document, &unauthorized_env).unwrap();
    assert_eq!(unauthorized_tool, "nucleusdb_execute_sql");
    // Authorization check: Bob has no capability grant from Carol.
    let carol_sessions = SessionManager::new();
    assert!(!carol_sessions.has_session(&bob.did));

    // 8. Heartbeat round: all pairs alive.
    for (sender, recipient) in [(&alice, &bob), (&bob, &carol), (&carol, &alice)] {
        let hb = wrap_heartbeat(sender, &recipient.did_document).unwrap();
        let dec = decrypt_message(recipient, &sender.did_document, &hb).unwrap();
        assert_eq!(dec.type_, MessageType::Heartbeat);
    }

    // 9. Verify session state.
    let alice_bob_session = alice_sessions.get(&bob.did).unwrap();
    assert_eq!(alice_bob_session.message_count, 1); // completed 1 request
    assert_eq!(alice_bob_session.pending_requests.len(), 0);

    let bob_alice_session = bob_sessions.get(&alice.did).unwrap();
    assert_eq!(bob_alice_session.received_capabilities.len(), 1);
}

#[test]
fn sim_full_scenario_multi_tool_burst() {
    let alice = agent(0xFD);
    let bob = agent(0xFE);

    // Simulate a burst of 10 MCP tool calls.
    let tools = [
        "nucleusdb_query",
        "nucleusdb_status",
        "nucleusdb_verify",
        "nucleusdb_export",
        "nucleusdb_history",
        "nucleusdb_query",
        "nucleusdb_status",
        "nucleusdb_verify",
        "nucleusdb_export",
        "nucleusdb_history",
    ];

    for (i, tool) in tools.iter().enumerate() {
        let env = wrap_mcp_call(
            &bob,
            &alice.did_document,
            tool,
            serde_json::json!({"index": i}),
        )
        .unwrap();
        let (_, name, args) = unwrap_mcp_call(&alice, &bob.did_document, &env).unwrap();
        assert_eq!(name, *tool);
        assert_eq!(args["index"], i);

        // Verify signature on every envelope.
        let sig_ok = verify_envelope_signature(&bob.did_document, &env).unwrap();
        assert!(sig_ok, "signature failed on call {i}");
    }
}
