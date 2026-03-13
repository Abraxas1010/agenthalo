use nucleusdb::halo::config;
use nucleusdb::halo::p2pclaw;
use nucleusdb::halo::p2pclaw_bridge;
use nucleusdb::halo::trace::now_unix_secs;
use nucleusdb::halo::vault::Vault;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_env() -> std::sync::MutexGuard<'static, ()> {
    let mutex = env_lock();
    let guard = mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    mutex.clear_poison();
    guard
}

struct EnvVarGuard {
    key: &'static str,
    prev: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: Option<&str>) -> Self {
        let prev = std::env::var(key).ok();
        match value {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        Self { key, prev }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        if let Some(v) = self.prev.as_ref() {
            std::env::set_var(self.key, v);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn temp_home(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "p2pclaw_test_{}_{}_{}",
        tag,
        std::process::id(),
        now_unix_secs()
    ))
}

fn write_wallet_json(path: &Path, key_id: &str, seed_hex: &str) {
    let wallet = json!({
        "version": 1,
        "algorithm": "ml_dsa65",
        "key_id": key_id,
        "public_key_hex": "00",
        "secret_seed_hex": seed_hex,
        "created_at": now_unix_secs(),
    });
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, serde_json::to_vec_pretty(&wallet).unwrap()).unwrap();
}

struct MockP2PClawServer {
    base_url: String,
    shutdown: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl MockP2PClawServer {
    fn spawn() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock p2pclaw");
        listener.set_nonblocking(true).expect("set nonblocking");
        let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_flag = shutdown.clone();
        let handle = thread::spawn(move || {
            while !shutdown_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buffer = [0u8; 8192];
                        let bytes = stream.read(&mut buffer).unwrap_or(0);
                        if bytes == 0 {
                            continue;
                        }
                        let request = String::from_utf8_lossy(&buffer[..bytes]);
                        let mut lines = request.lines();
                        let request_line = lines.next().unwrap_or_default();
                        let mut parts = request_line.split_whitespace();
                        let method = parts.next().unwrap_or_default();
                        let target = parts.next().unwrap_or("/");
                        let path = target.split('?').next().unwrap_or(target);
                        let body = request
                            .split("\r\n\r\n")
                            .nth(1)
                            .unwrap_or_default()
                            .to_string();
                        let (content_type, payload) = match (method, path) {
                            ("GET", "/utf8/briefing") | ("GET", "/dupe/briefing") => (
                                "text/markdown",
                                "# Briefing\nSnowman: \u{2603} and rocket: \u{1F680}".to_string(),
                            ),
                            ("GET", "/briefing") => (
                                "text/markdown",
                                "# Briefing\nLive bridge snapshot.".to_string(),
                            ),
                            ("GET", "/dupe/hive-events") => (
                                "application/json",
                                json!({"events":[
                                    {"id":"evt-1","kind":"published","timestamp":1234},
                                    {"id":"evt-1","kind":"published","timestamp":1234}
                                ]})
                                .to_string(),
                            ),
                            ("GET", "/utf8/investigations")
                            | ("GET", "/dupe/investigations")
                            | ("GET", "/investigations") => (
                                "application/json",
                                json!({"investigations":[{"id":"inv-1","title":"Bridge audit"}]})
                                    .to_string(),
                            ),
                            ("GET", "/error429/mempool") => (
                                "application/json",
                                json!({"error":"rate limited"}).to_string(),
                            ),
                            ("GET", "/utf8/mempool")
                            | ("GET", "/dupe/mempool")
                            | ("GET", "/mempool") => (
                                "application/json",
                                json!({"papers":[{"paperId":"paper-1","title":"Mempool draft"}]})
                                    .to_string(),
                            ),
                            ("GET", "/utf8/hive-events") | ("GET", "/hive-events") => (
                                "application/json",
                                json!({"events":[{"kind":"published","timestamp":1234}]})
                                    .to_string(),
                            ),
                            ("GET", "/agent-rank") => (
                                "application/json",
                                json!({"agent":"agenthalo-mock","rank":"tier1","contributions":7})
                                    .to_string(),
                            ),
                            ("GET", "/agent-briefing") => (
                                "application/json",
                                json!({"agent_id":"agenthalo-mock","summary":"Focused research queue"})
                                    .to_string(),
                            ),
                            ("POST", "/investigations") => {
                                let payload: Value =
                                    serde_json::from_str(&body).unwrap_or_else(|_| json!({}));
                                (
                                    "application/json",
                                    json!({
                                        "id": "created-1",
                                        "title": payload.get("title").and_then(|v| v.as_str()).unwrap_or(""),
                                        "status": "queued"
                                    })
                                    .to_string(),
                                )
                            }
                            ("POST", "/tau/tick") => (
                                "application/json",
                                json!({"status":"ok","accepted":true}).to_string(),
                            ),
                            ("POST", "/chat") => (
                                "application/json",
                                json!({"status":"ok","delivered":true}).to_string(),
                            ),
                            ("POST", "/utf8/publish-paper") | ("POST", "/publish-paper") => (
                                "application/json",
                                json!({"status":"ok","paperId":"published-1"}).to_string(),
                            ),
                            _ => ("application/json", json!({"error":"not found"}).to_string()),
                        };
                        let status_line = if path == "/error429/mempool" {
                            "HTTP/1.1 429 Too Many Requests"
                        } else if payload.contains("\"error\":\"not found\"") {
                            "HTTP/1.1 404 Not Found"
                        } else {
                            "HTTP/1.1 200 OK"
                        };
                        let response = format!(
                            "{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            payload.len(),
                            payload
                        );
                        let _ = stream.write_all(response.as_bytes());
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            base_url,
            shutdown,
            handle: Some(handle),
        }
    }
}

impl Drop for MockP2PClawServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = std::net::TcpStream::connect(self.base_url.trim_start_matches("http://"));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn write_mock_verify_script(dir: &Path) -> PathBuf {
    let path = dir.join("living_agent_verify.py");
    std::fs::write(
        &path,
        r#"#!/usr/bin/env python3
import json
print(json.dumps({
  "paper_sha256": "mock-sha",
  "generated_at": "2026-03-12T00:00:00Z",
  "schema_version": "living-agent-verify-v1",
  "structural": {"score": 0.95, "passed": True, "details": {"word_count": 300}},
  "semantic": {"score": 0.75, "passed": True, "details": {"top_grid_match": "HeytingLean.Mock"}},
  "formal": {"score": 1.0, "passed": True, "details": {"checked": 2, "successes": 2}},
  "composite": {
    "score": 0.75,
    "passed": True,
    "details": {"governing_tier": "semantic", "generated_at": "2026-03-12T00:00:00Z"}
  },
  "report_path": "/tmp/mock-report.json"
}))"#,
    )
    .expect("write mock verifier");
    path
}

#[test]
fn config_roundtrip_without_secret_field() {
    let _guard = lock_env();
    let home = temp_home("roundtrip");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: "http://localhost:3000".to_string(),
        agent_id: "agenthalo-alice".to_string(),
        agent_name: "Alice".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 42,
    };
    p2pclaw::save_config(&cfg).expect("save config");
    let loaded = p2pclaw::load_config().expect("load config");
    assert_eq!(loaded.endpoint_url, "http://localhost:3000");
    assert_eq!(loaded.agent_id, "agenthalo-alice");
    assert_eq!(loaded.agent_name, "Alice");
    assert!(!loaded.auth_configured);
    assert_eq!(loaded.tier, "tier1");
    assert_eq!(loaded.last_connected_at, 42);

    let raw = std::fs::read_to_string(config::p2pclaw_config_path()).expect("read config file");
    assert!(
        !raw.contains("auth_secret"),
        "config must not include secret fields"
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn auth_signature_vectors_match_expected_values() {
    let get_sig =
        p2pclaw::compute_auth_signature("agenthalo-alice", 1_700_000_000_123, "", "s3cr3t")
            .expect("compute get signature");
    assert_eq!(
        get_sig,
        "b989cdb49fbe8981ffcff94316c754d3348b29a19e9f997d633dc1c982b7cd35"
    );

    let body = "{\"title\":\"Hello\",\"content\":\"World\"}";
    let post_sig =
        p2pclaw::compute_auth_signature("agenthalo-alice", 1_700_000_000_123, body, "s3cr3t")
            .expect("compute post signature");
    assert_eq!(
        post_sig,
        "0061411fd82e8fd1f05ed8a326592913db31abb60b14e23f82d44262388caeb0"
    );
}

#[test]
fn validate_endpoint_rejects_non_http_schemes() {
    assert!(p2pclaw::validate_endpoint("http://localhost:3000").is_ok());
    assert!(p2pclaw::validate_endpoint("https://p2pclaw.com").is_ok());

    for bad in [
        "file:///tmp/p2pclaw.sock",
        "ftp://example.com",
        "data:text/plain,hi",
        "not-a-url",
    ] {
        let err = p2pclaw::validate_endpoint(bad).expect_err("must reject non-http(s)");
        assert!(
            err.contains("http:// or https://") || err.contains("invalid P2PCLAW endpoint URL"),
            "unexpected error for {bad}: {err}"
        );
    }
}

#[test]
fn configure_stores_secret_in_vault_when_wallet_available() {
    let _guard = lock_env();
    let home = temp_home("vault");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let wallet_path = config::pq_wallet_path();
    write_wallet_json(
        &wallet_path,
        "p2pclaw-test-key",
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
    );

    let mut cfg = p2pclaw::P2PClawConfig {
        endpoint_url: "http://localhost:3000".to_string(),
        agent_id: "agenthalo-alice".to_string(),
        agent_name: "Alice".to_string(),
        auth_configured: false,
        tier: "tier2".to_string(),
        last_connected_at: 0,
    };
    let configured =
        p2pclaw::configure(&mut cfg, Some("top-secret".to_string())).expect("configure p2pclaw");
    assert!(configured.auth_in_vault);
    assert!(configured.auth_configured);

    let vault = Vault::open(&config::pq_wallet_path(), &config::vault_path()).expect("open vault");
    let saved = vault
        .get_key(p2pclaw::P2PCLAW_VAULT_KEY)
        .expect("read p2pclaw secret from vault");
    assert_eq!(saved, "top-secret");

    let raw = std::fs::read_to_string(config::p2pclaw_config_path()).expect("read config");
    assert!(!raw.contains("top-secret"));
    assert!(!raw.contains("auth_secret_INSECURE"));

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn configure_uses_insecure_fallback_when_vault_unavailable() {
    let _guard = lock_env();
    let home = temp_home("fallback");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let mut cfg = p2pclaw::P2PClawConfig {
        endpoint_url: "http://localhost:3000".to_string(),
        agent_id: "agenthalo-fallback".to_string(),
        agent_name: "Fallback".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };
    let configured = p2pclaw::configure(&mut cfg, Some("fallback-secret".to_string()))
        .expect("configure fallback");
    assert!(!configured.auth_in_vault);
    assert!(configured.auth_configured);

    let raw = std::fs::read_to_string(config::p2pclaw_config_path()).expect("read config");
    assert!(raw.contains("auth_secret_INSECURE"));
    assert!(raw.contains("fallback-secret"));

    let restored = p2pclaw::get_auth_secret(None)
        .expect("read auth secret")
        .expect("fallback secret should exist");
    assert_eq!(restored, "fallback-secret");

    let _ = std::fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn config_file_permissions_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = lock_env();
    let home = temp_home("perm");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: "http://localhost:3000".to_string(),
        agent_id: "agenthalo-perm".to_string(),
        agent_name: "Perm".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };
    p2pclaw::save_config(&cfg).expect("save config");
    let mode = std::fs::metadata(config::p2pclaw_config_path())
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn bridge_state_roundtrip_and_status_report_accounting() {
    let _guard = lock_env();
    let home = temp_home("bridge_state");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let mut state = p2pclaw_bridge::BridgePersistentState::default();
    state.hive_compute_cycles = 6;
    state.local_compute_cycles = 4;
    state.polls_total = 3;
    p2pclaw_bridge::save_state(&state).expect("save bridge state");
    let loaded = p2pclaw_bridge::load_state().expect("load bridge state");
    assert_eq!(loaded.hive_compute_cycles, 6);
    assert_eq!(loaded.local_compute_cycles, 4);

    let status = p2pclaw_bridge::status(None, false).expect("bridge status");
    assert!(!status.configured);
    assert_eq!(status.compute_split_ratio, 0.6);
    assert!(status.nash_compliant);
    assert!(status.capabilities.iter().any(|cap| cap == "briefing"));

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn bridge_config_roundtrip_and_status_surface_loaded_values() {
    let _guard = lock_env();
    let home = temp_home("bridge_config");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let cfg = p2pclaw_bridge::BridgeConfig {
        min_poll_interval_secs: 3,
        max_poll_interval_secs: 9,
        heartbeat_interval_secs: 11,
        event_limit: 7,
        preview_items: 2,
        heyting_verify_script: None,
        heyting_verify_python: None,
        heyting_verify_timeout_secs: Some(120),
    };
    p2pclaw_bridge::save_config(&cfg).expect("save bridge config");
    let loaded = p2pclaw_bridge::load_config().expect("load bridge config");
    assert_eq!(loaded.min_poll_interval_secs, 3);
    assert_eq!(loaded.max_poll_interval_secs, 9);
    assert_eq!(loaded.preview_items, 2);

    let status = p2pclaw_bridge::status(None, false).expect("bridge status");
    assert_eq!(status.bridge_config.event_limit, 7);
    assert!(status.config_path.ends_with("p2pclaw_bridge.json"));

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn client_bridge_endpoints_parse_rank_briefing_and_investigation_create() {
    let _guard = lock_env();
    let server = MockP2PClawServer::spawn();
    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: server.base_url.clone(),
        agent_id: "agenthalo-mock".to_string(),
        agent_name: "Mock".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };

    let rank = p2pclaw::get_agent_rank(&cfg, None).expect("agent rank");
    assert_eq!(rank.rank.as_deref(), Some("tier1"));
    assert_eq!(rank.contributions, Some(7));

    let briefing = p2pclaw::get_agent_briefing(&cfg, None).expect("agent briefing");
    assert_eq!(briefing["summary"], "Focused research queue");

    let investigation =
        p2pclaw::create_investigation(&cfg, "Bridge task", "Check structured bridge path")
            .expect("create investigation");
    assert_eq!(investigation.id.as_deref(), Some("created-1"));
    assert_eq!(investigation.status.as_deref(), Some("queued"));

    let tau = p2pclaw::report_tau_tick(&cfg, 9).expect("tau tick");
    assert_eq!(tau["accepted"], true);
}

#[test]
fn client_rejects_non_success_http_status_even_with_json_body() {
    let _guard = lock_env();
    let server = MockP2PClawServer::spawn();
    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: format!("{}/error429", server.base_url),
        agent_id: "agenthalo-mock".to_string(),
        agent_name: "Mock".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };

    let err = p2pclaw::list_mempool(&cfg).expect_err("429 must not parse as success");
    assert!(err.contains("HTTP 429"), "unexpected error: {err}");
}

#[test]
fn bridge_run_once_polls_and_persists_state() {
    let _guard = lock_env();
    let home = temp_home("bridge_run_once");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let server = MockP2PClawServer::spawn();
    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: server.base_url.clone(),
        agent_id: "agenthalo-mock".to_string(),
        agent_name: "Mock".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };

    let report = p2pclaw_bridge::run_once(&cfg, p2pclaw_bridge::BridgeRunOptions::default())
        .expect("run once");
    assert_eq!(report.investigations_seen, 1);
    assert_eq!(report.mempool_seen, 1);
    assert_eq!(report.events_seen, 1);
    assert_eq!(
        report.briefing_chars,
        "# Briefing\nLive bridge snapshot.".chars().count()
    );
    assert!(report.state_after.next_poll_not_before.is_some());

    let saved = p2pclaw_bridge::load_state().expect("load state after run");
    assert_eq!(saved.polls_total, 1);
    assert_eq!(saved.investigations_seen_total, 1);
    assert_eq!(saved.mempool_seen_total, 1);
    assert_eq!(saved.events_seen_total, 1);
    assert_eq!(saved.hive_compute_cycles, 0);
    assert_eq!(saved.local_compute_cycles, 4);

    let loop_report = p2pclaw_bridge::run_loop(
        &cfg,
        p2pclaw_bridge::BridgeLoopOptions {
            max_iterations: Some(2),
            respect_backoff: false,
            heartbeat: true,
            report_tau_sync: true,
            ..Default::default()
        },
    )
    .expect("run loop");
    assert_eq!(loop_report.iterations, 2);
    assert_eq!(loop_report.reports.len(), 2);
    assert_eq!(loop_report.total_sleep_secs, 0);

    let post_loop = p2pclaw_bridge::load_state().expect("load state after loop");
    assert_eq!(post_loop.polls_total, 3);
    assert_eq!(post_loop.tau_reports_total, 2);
    assert!(post_loop.last_heartbeat_at.is_some());
    assert!(post_loop.last_tau_sync_at.is_some());

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn bridge_publish_dedup_and_utf8_briefing_are_safe() {
    let _guard = lock_env();
    let home = temp_home("bridge_publish_dedup");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let server = MockP2PClawServer::spawn();
    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: format!("{}/utf8", server.base_url),
        agent_id: "agenthalo-mock".to_string(),
        agent_name: "Mock".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };

    let first = p2pclaw_bridge::run_once(
        &cfg,
        p2pclaw_bridge::BridgeRunOptions {
            dry_run: false,
            publish_summary: true,
            ..Default::default()
        },
    )
    .expect("first publish");
    assert_eq!(
        first.briefing_chars,
        "# Briefing\nSnowman: ☃ and rocket: 🚀".chars().count()
    );
    assert!(first
        .actions
        .iter()
        .any(|a| a.kind == "publish_summary" && !a.dry_run));

    let second = p2pclaw_bridge::run_once(
        &cfg,
        p2pclaw_bridge::BridgeRunOptions {
            dry_run: false,
            publish_summary: true,
            ..Default::default()
        },
    )
    .expect("second publish");
    assert!(second
        .actions
        .iter()
        .any(|a| a.kind == "skip_publish_summary"));

    let saved = p2pclaw_bridge::load_state().expect("load state after publish dedup");
    assert_eq!(saved.publications_total, 1);
    assert_eq!(
        saved.last_publication_paper_id.as_deref(),
        Some("published-1")
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn bridge_event_dedup_and_unbounded_loop_respects_shutdown() {
    let _guard = lock_env();
    let home = temp_home("bridge_event_dedup");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let server = MockP2PClawServer::spawn();
    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: format!("{}/dupe", server.base_url),
        agent_id: "agenthalo-mock".to_string(),
        agent_name: "Mock".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };

    let once = p2pclaw_bridge::run_once(&cfg, p2pclaw_bridge::BridgeRunOptions::default())
        .expect("run once with duplicate events");
    assert_eq!(once.events_seen, 1);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = shutdown.clone();
    thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(80));
        shutdown_flag.store(true, Ordering::Relaxed);
    });
    let loop_report = p2pclaw_bridge::run_loop_with_shutdown(
        &cfg,
        p2pclaw_bridge::BridgeLoopOptions {
            respect_backoff: false,
            ..Default::default()
        },
        shutdown,
    )
    .expect("run loop with shutdown");
    assert!(
        loop_report.iterations > 1,
        "loop must keep running without an iteration cap"
    );

    let saved = p2pclaw_bridge::load_state().expect("load deduped state");
    assert_eq!(saved.events_seen_total, 1);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn bridge_publish_verified_paper_uses_full_verifier_and_publishes_content() {
    let _guard = lock_env();
    let home = temp_home("bridge_publish_verified_paper");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).expect("create temp home");
    let _home_guard = EnvVarGuard::set("AGENTHALO_HOME", Some(home.to_str().expect("utf8 home")));

    let server = MockP2PClawServer::spawn();
    let verifier = write_mock_verify_script(&home);
    let bridge_cfg = p2pclaw_bridge::BridgeConfig {
        heyting_verify_script: Some(verifier.clone()),
        ..Default::default()
    };
    p2pclaw_bridge::save_config(&bridge_cfg).expect("save bridge config");

    let cfg = p2pclaw::P2PClawConfig {
        endpoint_url: server.base_url.clone(),
        agent_id: "agenthalo-mock".to_string(),
        agent_name: "Mock".to_string(),
        auth_configured: false,
        tier: "tier1".to_string(),
        last_connected_at: 0,
    };

    let content = format!(
        "# Abstract\nWe prove the bridge can publish verified content. {}\n\n# Methodology\nThe bridge forwards a real paper body into AgentHALO verification and then to P2PCLAW publication. {}\n\n# Results\nThe verified publication returns a paper id and stores full verification metadata. {}",
        "word ".repeat(80),
        "word ".repeat(80),
        "word ".repeat(80),
    );
    let result = p2pclaw_bridge::publish_verified_paper(
        &cfg,
        p2pclaw_bridge::BridgePaperPublishOptions {
            title: "Verified publication".to_string(),
            content,
            dry_run: false,
        },
    )
    .expect("publish verified paper");

    assert_eq!(result.verification.verification_level, "full");
    assert_eq!(result.verification.semantic_score, Some(0.75));
    assert_eq!(result.verification.formal_passed, Some(true));
    assert_eq!(
        result.verification.external_report_path.as_deref(),
        Some("/tmp/mock-report.json")
    );
    assert_eq!(
        result
            .publish_result
            .as_ref()
            .and_then(|value| value.paper_id.as_deref()),
        Some("published-1")
    );
    assert_eq!(result.state_after.publications_total, 1);

    let _ = std::fs::remove_file(verifier);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn discover_verify_script_honors_agenthalo_verify_script_env() {
    let _guard = lock_env();
    let temp = std::env::temp_dir().join(format!(
        "agenthalo_verify_env_{}_{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&temp).expect("create env verify dir");
    let script = temp.join("living_agent_verify.py");
    std::fs::write(&script, "#!/usr/bin/env python3\nprint('ok')\n").expect("write script");
    let _env = EnvVarGuard::set(
        "AGENTHALO_VERIFY_SCRIPT",
        Some(script.to_str().expect("utf8 script")),
    );
    let discovered =
        p2pclaw_bridge::discover_verify_script(&p2pclaw_bridge::BridgeConfig::default())
            .expect("discover env script");
    assert_eq!(discovered, script);
    let _ = std::fs::remove_file(&script);
    let _ = std::fs::remove_dir_all(&temp);
}
