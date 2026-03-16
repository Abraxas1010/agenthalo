use nucleusdb::cockpit::pty_manager::PtyManager;
use nucleusdb::container::launcher::{Channel, MeshConfig, MonitorConfig};
use nucleusdb::container::{
    parse_channel_list, AgentHookup, ApiAgentHookup, CliAgentHookup, ContainerAgentLock,
    LocalModelHookup,
};
use nucleusdb::halo::config;
use nucleusdb::halo::schema::{EventType, TraceEvent};
use nucleusdb::halo::trace::session_events;
use nucleusdb::mcp::tools::{
    NucleusDbMcpService, OrchestratorLaunchRequest, SubsidiaryListRequest,
};
use nucleusdb::orchestrator::subsidiary_registry::SubsidiaryRegistry;
use nucleusdb::test_support::{lock_env, EnvVarGuard, MockOpenAiServer};
use rmcp::handler::server::wrapper::Parameters;
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

#[test]
fn mesh_config_default_respects_operator_env_overrides() {
    let _guard = lock_env();
    let _volume = EnvVarGuard::set(
        "AGENTHALO_CONTAINER_REGISTRY_VOLUME",
        Some("phase7-mesh-volume"),
    );
    let _port = EnvVarGuard::set("AGENTHALO_CONTAINER_MCP_PORT", Some("43123"));

    let cfg = MeshConfig::default();
    assert_eq!(
        cfg.registry_volume,
        std::path::PathBuf::from("phase7-mesh-volume")
    );
    assert_eq!(cfg.mcp_port, 43123);
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

#[tokio::test(flavor = "current_thread")]
async fn operator_subsidiary_list_filters_owned_sessions() {
    let _guard = lock_env();

    let home = tempfile::tempdir().expect("home tempdir");
    let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
    let _container = EnvVarGuard::set("NUCLEUSDB_MESH_AGENT_ID", Some("operator-container"));

    let db_path = config::halo_dir().join("subsidiary_list_test.ndb");
    let service = NucleusDbMcpService::new(&db_path).expect("service");
    let rmcp::Json(operator) = service
        .orchestrator_launch(Parameters(OrchestratorLaunchRequest {
            agent: "shell".to_string(),
            agent_name: "operator".to_string(),
            working_dir: None,
            env: Default::default(),
            timeout_secs: Some(30),
            model: None,
            trace: Some(false),
            capabilities: vec!["operator".to_string()],
            dispatch_mode: None,
            container_hookup: None,
            admission_mode: None,
        }))
        .await
        .expect("launch operator");

    let run_dir = std::env::temp_dir().join("agenthalo-native");
    std::fs::create_dir_all(&run_dir).expect("create run dir");
    for (session_id, agent_id) in [
        ("sess-owned-int", "peer-owned"),
        ("sess-other-int", "peer-other"),
    ] {
        let session_dir = run_dir.join(session_id);
        std::fs::create_dir_all(&session_dir).expect("create session dir");
        let path = session_dir.join("session.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&nucleusdb::container::SessionInfo {
                session_id: session_id.to_string(),
                container_id: format!("ctr-{session_id}"),
                image: "nucleusdb-agent:test".to_string(),
                agent_id: agent_id.to_string(),
                host_sock: std::env::temp_dir().join(format!("{session_id}.sock")),
                started_at_unix: 1,
                mesh_port: Some(3000),
                pid: None,
                log_path: None,
            })
            .expect("encode session"),
        )
        .expect("write session");
    }

    let mut registry = SubsidiaryRegistry::load_or_create(&operator.agent_id).expect("registry");
    registry.register_provision(
        "sess-owned-int".to_string(),
        "ctr-sess-owned-int".to_string(),
        "peer-owned".to_string(),
    );
    registry.save().expect("save registry");

    let rmcp::Json(listed) = service
        .subsidiary_list(Parameters(SubsidiaryListRequest {
            operator_agent_id: operator.agent_id.clone(),
        }))
        .await
        .expect("subsidiary list");
    assert_eq!(listed.count, 1);
    assert_eq!(listed.subsidiaries[0].session_id, "sess-owned-int");

    let _ = std::fs::remove_dir_all(run_dir.join("sess-owned-int"));
    let _ = std::fs::remove_dir_all(run_dir.join("sess-other-int"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}.wal", db_path.display()));
}
