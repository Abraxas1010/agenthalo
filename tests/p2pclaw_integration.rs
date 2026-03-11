use nucleusdb::halo::config;
use nucleusdb::halo::p2pclaw;
use nucleusdb::halo::trace::now_unix_secs;
use nucleusdb::halo::vault::Vault;
use serde_json::json;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

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
