use crate::halo::nym_native;
use crate::halo::privacy_controller::{classify_url, PrivacyLevel};
use serde::{Deserialize, Serialize};
use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::time::Duration;

const DEFAULT_SOCKS5_ADDR: &str = "127.0.0.1:1080";
const HEALTH_TIMEOUT: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NymMode {
    External,
    Local,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NymStatus {
    pub mode: NymMode,
    pub socks5_proxy: Option<String>,
    pub healthy: bool,
    pub fail_closed: bool,
    pub native_enabled: bool,
    pub native_connected: bool,
    pub native_address: Option<String>,
    pub inbound_registered: bool,
    pub cover_traffic_active: bool,
    pub note: String,
}

pub fn status() -> NymStatus {
    let native = nym_native::status_snapshot();
    let fail_closed = is_fail_closed();
    if let Some(proxy) = resolve_socks5_proxy() {
        let healthy = proxy_healthcheck(&proxy);
        let mode = if std::env::var("SOCKS5_PROXY").ok().is_some() {
            NymMode::External
        } else if std::env::var("NYM_BINARY").is_ok() || std::env::var("NYM_CONFIG_DIR").is_ok() {
            NymMode::Local
        } else {
            NymMode::External
        };
        return NymStatus {
            mode,
            socks5_proxy: Some(proxy.clone()),
            healthy: healthy || native.connected,
            fail_closed,
            native_enabled: native.enabled,
            native_connected: native.connected,
            native_address: native.address,
            inbound_registered: native.inbound_registered,
            cover_traffic_active: native.cover_traffic_active,
            note: if healthy {
                "SOCKS5 transport available".to_string()
            } else if native.connected {
                "native mixnet connected; SOCKS5 health check failed".to_string()
            } else {
                "SOCKS5 transport configured but health check failed".to_string()
            },
        };
    }

    if native.connected {
        return NymStatus {
            mode: NymMode::Local,
            socks5_proxy: None,
            healthy: true,
            fail_closed,
            native_enabled: native.enabled,
            native_connected: native.connected,
            native_address: native.address,
            inbound_registered: native.inbound_registered,
            cover_traffic_active: native.cover_traffic_active,
            note: "Native mixnet connected via nym-sdk".to_string(),
        };
    }

    NymStatus {
        mode: NymMode::Disabled,
        socks5_proxy: None,
        healthy: false,
        fail_closed,
        native_enabled: native.enabled,
        native_connected: native.connected,
        native_address: native.address,
        inbound_registered: native.inbound_registered,
        cover_traffic_active: native.cover_traffic_active,
        note: "No SOCKS5 proxy detected".to_string(),
    }
}

/// Returns true when external traffic requiring mixnet routing must be blocked
/// if no healthy SOCKS5 proxy is available.
pub fn is_fail_closed() -> bool {
    // Fail-closed by default: set NYM_FAIL_OPEN=true to allow direct fallback.
    if let Ok(v) = std::env::var("NYM_FAIL_OPEN") {
        if matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ) {
            return false;
        }
    }
    // Legacy compatibility: NYM_FAIL_CLOSED=false means fail-open.
    if let Ok(v) = std::env::var("NYM_FAIL_CLOSED") {
        if matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ) {
            return false;
        }
    }
    true
}

pub fn resolve_socks5_proxy() -> Option<String> {
    if let Ok(raw) = std::env::var("SOCKS5_PROXY") {
        if let Some(uri) = normalize_proxy_uri(&raw) {
            return Some(uri);
        }
    }

    for key in ["ALL_PROXY", "all_proxy", "HTTPS_PROXY", "https_proxy"] {
        if let Ok(raw) = std::env::var(key) {
            let lowered = raw.trim().to_ascii_lowercase();
            if lowered.starts_with("socks5h://")
                || lowered.starts_with("socks5://")
                || lowered.starts_with("socks://")
            {
                if let Some(uri) = normalize_proxy_uri(&raw) {
                    return Some(uri);
                }
            }
        }
    }

    if std::env::var("NYM_BINARY").is_ok() || std::env::var("NYM_CONFIG_DIR").is_ok() {
        eprintln!(
            "[AgentHalo/Nym] auto-detecting SOCKS5 proxy at {DEFAULT_SOCKS5_ADDR} (set SOCKS5_PROXY to override)"
        );
        return Some(format!("socks5h://{DEFAULT_SOCKS5_ADDR}"));
    }

    None
}

pub fn should_route_via_mixnet(url: &str) -> bool {
    matches!(classify_url(url), PrivacyLevel::Maximum | PrivacyLevel::P2P)
}

pub fn ensure_route_allowed(url: &str) -> Result<Option<String>, String> {
    if !should_route_via_mixnet(url) {
        return Ok(None);
    }

    if let Some(proxy) = resolve_socks5_proxy() {
        return Ok(Some(proxy));
    }

    if is_fail_closed() {
        return Err(format!(
            "outbound blocked: `{url}` requires mixnet routing but no SOCKS5 proxy is available"
        ));
    }

    Ok(None)
}

pub fn apply_proxy_env_for_url(cmd: &mut Command, url: &str) -> Result<(), String> {
    let Some(proxy_uri) = ensure_route_allowed(url)? else {
        return Ok(());
    };

    let proxy_no_scheme = proxy_uri
        .strip_prefix("socks5h://")
        .or_else(|| proxy_uri.strip_prefix("socks5://"))
        .or_else(|| proxy_uri.strip_prefix("socks://"))
        .unwrap_or(&proxy_uri)
        .to_string();

    cmd.env("ALL_PROXY", &proxy_uri)
        .env("all_proxy", &proxy_uri)
        .env("HTTPS_PROXY", &proxy_uri)
        .env("https_proxy", &proxy_uri)
        .env("HTTP_PROXY", &proxy_uri)
        .env("http_proxy", &proxy_uri)
        .env("SOCKS5_PROXY", &proxy_no_scheme);
    Ok(())
}

pub fn extract_cast_rpc_url(args: &[String]) -> Option<&str> {
    for pair in args.windows(2) {
        if pair[0] == "--rpc-url" {
            return Some(pair[1].as_str());
        }
    }
    None
}

pub fn apply_proxy_env_for_cast(cmd: &mut Command, args: &[String]) -> Result<(), String> {
    if let Some(url) = extract_cast_rpc_url(args) {
        apply_proxy_env_for_url(cmd, url)?;
    }
    Ok(())
}

pub async fn start_native_transport_if_enabled() -> Result<(), String> {
    nym_native::ensure_connected().await
}

pub async fn send_mixnet_message(
    recipient: &str,
    payload: &[u8],
    include_surbs: u32,
) -> Result<(), String> {
    nym_native::send_message_with_surbs(recipient, payload, include_surbs).await
}

pub async fn send_mixnet_reply(surb_tag: &str, payload: &[u8]) -> Result<(), String> {
    nym_native::send_reply_via_surb(surb_tag, payload).await
}

pub fn subscribe_mixnet_inbound(
) -> Option<tokio::sync::broadcast::Receiver<nym_native::NymInboundMessage>> {
    nym_native::subscribe_inbound()
}

fn normalize_proxy_uri(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("socks5h://") {
        return Some(trimmed.to_string());
    }
    if lowered.starts_with("socks5://") {
        let rest = trimmed.split_once("://").map(|(_, r)| r).unwrap_or(trimmed);
        return Some(format!("socks5h://{rest}"));
    }
    if lowered.starts_with("socks://") {
        let rest = trimmed.split_once("://").map(|(_, r)| r).unwrap_or(trimmed);
        return Some(format!("socks5h://{rest}"));
    }
    if trimmed.contains("://") {
        return None;
    }
    Some(format!("socks5h://{trimmed}"))
}

fn proxy_healthcheck(proxy_uri: &str) -> bool {
    let addr = proxy_uri
        .strip_prefix("socks5h://")
        .or_else(|| proxy_uri.strip_prefix("socks5://"))
        .or_else(|| proxy_uri.strip_prefix("socks://"))
        .unwrap_or(proxy_uri);
    tcp_healthcheck(addr)
}

fn tcp_healthcheck(addr: &str) -> bool {
    if let Ok(sock) = addr.parse::<SocketAddr>() {
        return TcpStream::connect_timeout(&sock, HEALTH_TIMEOUT).is_ok();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
            if let Some(v) = &self.prev {
                std::env::set_var(self.key, v);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn detect_external_proxy() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", Some("127.0.0.1:9050"));
        assert_eq!(
            resolve_socks5_proxy().as_deref(),
            Some("socks5h://127.0.0.1:9050")
        );
    }

    #[test]
    fn normalize_uri_variants() {
        assert_eq!(
            normalize_proxy_uri("127.0.0.1:1080").as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(
            normalize_proxy_uri("socks5://127.0.0.1:1080").as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(
            normalize_proxy_uri("SOCKS5://127.0.0.1:1080").as_deref(),
            Some("socks5h://127.0.0.1:1080")
        );
        assert_eq!(normalize_proxy_uri("http://127.0.0.1:1080"), None);
    }

    #[test]
    fn cast_rpc_url_extracts() {
        let args = vec![
            "call".to_string(),
            "--rpc-url".to_string(),
            "https://sepolia.base.org".to_string(),
        ];
        assert_eq!(
            extract_cast_rpc_url(&args),
            Some("https://sepolia.base.org")
        );
    }

    #[test]
    fn allow_local_without_proxy() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", None);
        let _fail = EnvVarGuard::set("NYM_FAIL_CLOSED", Some("1"));
        let out = ensure_route_allowed("http://127.0.0.1:3100/api").expect("local allowed");
        assert!(out.is_none());
    }

    #[test]
    fn block_external_fail_closed_without_proxy() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", None);
        let _all_proxy = EnvVarGuard::set("ALL_PROXY", None);
        let _fail = EnvVarGuard::set("NYM_FAIL_CLOSED", Some("true"));
        let err =
            ensure_route_allowed("https://sepolia.base.org/rpc").expect_err("should block");
        assert!(err.contains("outbound blocked"));
    }

    #[test]
    fn no_auto_detect_without_env() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", None);
        let _all_proxy = EnvVarGuard::set("ALL_PROXY", None);
        let _https_proxy = EnvVarGuard::set("HTTPS_PROXY", None);
        let _nym_bin = EnvVarGuard::set("NYM_BINARY", None);
        let _nym_cfg = EnvVarGuard::set("NYM_CONFIG_DIR", None);
        assert_eq!(resolve_socks5_proxy(), None);
    }

    #[test]
    fn fail_closed_default_true() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _legacy = EnvVarGuard::set("NYM_FAIL_CLOSED", None);
        let _open = EnvVarGuard::set("NYM_FAIL_OPEN", None);
        assert!(is_fail_closed());
    }

    #[test]
    fn fail_open_override_supported() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _legacy = EnvVarGuard::set("NYM_FAIL_CLOSED", None);
        let _open = EnvVarGuard::set("NYM_FAIL_OPEN", Some("true"));
        assert!(!is_fail_closed());
    }

    #[test]
    fn status_reports_disabled_when_no_proxy_no_native() {
        let _g = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", None);
        let _all_proxy = EnvVarGuard::set("ALL_PROXY", None);
        let _https_proxy = EnvVarGuard::set("HTTPS_PROXY", None);
        let _nym_bin = EnvVarGuard::set("NYM_BINARY", None);
        let _nym_cfg = EnvVarGuard::set("NYM_CONFIG_DIR", None);
        let _native_enabled = EnvVarGuard::set("NYM_NATIVE_ENABLED", None);
        let _legacy = EnvVarGuard::set("NYM_FAIL_CLOSED", None);
        let _open = EnvVarGuard::set("NYM_FAIL_OPEN", None);

        let s = status();
        assert_eq!(s.mode, NymMode::Disabled);
        assert!(!s.healthy);
        assert!(s.fail_closed);
    }
}
