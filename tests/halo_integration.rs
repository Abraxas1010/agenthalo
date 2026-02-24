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
