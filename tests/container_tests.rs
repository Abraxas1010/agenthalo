use nucleusdb::cockpit::pty_manager::PtyManager;
use nucleusdb::container::builder::parse_channel_list;
use nucleusdb::container::launcher::{Channel, MonitorConfig};
use nucleusdb::container::{
    AgentHookup, ApiAgentHookup, CliAgentHookup, ContainerAgentLock, LocalModelHookup,
};
use nucleusdb::halo::config;
use nucleusdb::halo::schema::{EventType, TraceEvent};
use nucleusdb::halo::trace::session_events;
use nucleusdb::test_support::{lock_env, EnvVarGuard, MockOpenAiServer};
use std::collections::BTreeSet;
use std::sync::Arc;

#[test]
fn parse_channel_list_valid() {
    let channels = parse_channel_list("chat,payments,tools").expect("parse");
    assert_eq!(
        channels,
        vec![Channel::Chat, Channel::Payments, Channel::Tools]
    );
}

#[test]
fn monitor_config_csv() {
    let cfg = MonitorConfig {
        channels: vec![Channel::Chat, Channel::State],
        agent_id: "agent-a".to_string(),
        max_nesting_depth: 3,
    };
    assert_eq!(cfg.channels_csv(), "chat,state");
}

fn trace_shape(events: &[TraceEvent]) -> Vec<(EventType, BTreeSet<String>, BTreeSet<String>)> {
    events
        .iter()
        .map(|event| {
            let raw = serde_json::to_value(event).expect("serialize trace event");
            let top_keys = raw
                .as_object()
                .expect("trace event object")
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>();
            let content_keys = event
                .content
                .as_object()
                .map(|obj| obj.keys().cloned().collect::<BTreeSet<_>>())
                .unwrap_or_default();
            (event.event_type.clone(), top_keys, content_keys)
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn agent_hookup_trace_schema_uniformity() {
    let _guard = lock_env();

    let cli_home = tempfile::tempdir().expect("cli tempdir");
    let _cli_env = EnvVarGuard::set("AGENTHALO_HOME", cli_home.path().to_str());
    let cli = CliAgentHookup::with_trace_path(
        "shell",
        Arc::new(PtyManager::new(4)),
        None,
        &config::db_path(),
    )
    .expect("cli hookup");
    let mut cli_lock = ContainerAgentLock::load_or_create("container-cli").expect("cli lock");
    cli.start(&mut cli_lock).await.expect("cli start");
    cli.send_prompt("printf 'cli response\\n'")
        .await
        .expect("cli prompt");
    cli.stop().await.expect("cli stop");
    let cli_trace_id = cli.trace_session_id().expect("cli trace id");
    let cli_events = session_events(cli.trace_db_path(), &cli_trace_id).expect("cli events");
    drop(_cli_env);

    let api_home = tempfile::tempdir().expect("api tempdir");
    let _api_env = EnvVarGuard::set("AGENTHALO_HOME", api_home.path().to_str());
    let api_server = MockOpenAiServer::spawn("openrouter/test-model", "api response");
    let api = ApiAgentHookup::with_base_url(
        "openrouter",
        "openrouter/test-model",
        "literal-test-key",
        Some(api_server.base_url.clone()),
        &config::db_path(),
    )
    .expect("api hookup");
    let mut api_lock = ContainerAgentLock::load_or_create("container-api").expect("api lock");
    api.start(&mut api_lock).await.expect("api start");
    api.send_prompt("api prompt").await.expect("api prompt");
    api.stop().await.expect("api stop");
    let api_trace_id = api.trace_session_id().expect("api trace id");
    let api_events = session_events(api.trace_db_path(), &api_trace_id).expect("api events");
    drop(_api_env);

    let local_home = tempfile::tempdir().expect("local tempdir");
    let _local_env = EnvVarGuard::set("AGENTHALO_HOME", local_home.path().to_str());
    let local_server = MockOpenAiServer::spawn("test/local-model", "local response");
    let local = LocalModelHookup::with_base_url(
        "test/local-model",
        8000,
        Some(local_server.base_url.clone()),
        &config::db_path(),
    )
    .expect("local hookup");
    let mut local_lock = ContainerAgentLock::load_or_create("container-local").expect("local lock");
    local.start(&mut local_lock).await.expect("local start");
    local
        .send_prompt("local prompt")
        .await
        .expect("local prompt");
    local.stop().await.expect("local stop");
    let local_trace_id = local.trace_session_id().expect("local trace id");
    let local_events =
        session_events(local.trace_db_path(), &local_trace_id).expect("local events");

    let cli_shape = trace_shape(&cli_events);
    let api_shape = trace_shape(&api_events);
    let local_shape = trace_shape(&local_events);

    assert_eq!(
        cli_shape.iter().map(|(ty, _, _)| ty).collect::<Vec<_>>(),
        vec![
            &EventType::AgentInitialized,
            &EventType::PromptSent,
            &EventType::ResponseReceived,
            &EventType::AgentDeinitialized,
        ]
    );
    assert_eq!(api_shape, cli_shape);
    assert_eq!(local_shape, cli_shape);
}
