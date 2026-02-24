use nucleusdb::halo::auth::{save_credentials, Credentials};
use nucleusdb::halo::pricing::{calculate_cost, default_pricing};
use nucleusdb::halo::runner::AgentRunner;
use nucleusdb::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
use nucleusdb::halo::trace::{
    list_sessions, now_unix_secs, session_events, session_summary, TraceWriter,
};
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
    let code = runner.run(&mut writer).expect("run echo");
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
