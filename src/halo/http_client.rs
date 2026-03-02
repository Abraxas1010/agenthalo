use crate::halo::nym;
use crate::halo::privacy_controller::{classify_url, PrivacyLevel};
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const MIXNET_TIMEOUT: Duration = Duration::from_secs(120);

pub fn agent_for_url(url: &str) -> Result<ureq::Agent, String> {
    agent_for_url_with_timeout(url, DEFAULT_TIMEOUT)
}

pub fn agent_for_url_with_timeout(url: &str, timeout: Duration) -> Result<ureq::Agent, String> {
    let level = classify_url(url);
    match level {
        PrivacyLevel::None => Ok(build_direct_agent(timeout)),
        PrivacyLevel::Maximum | PrivacyLevel::P2P => {
            let maybe_proxy = nym::ensure_route_allowed(url)?;
            if let Some(proxy_uri) = maybe_proxy {
                build_socks5_agent(&proxy_uri, timeout.max(MIXNET_TIMEOUT))
            } else {
                Ok(build_direct_agent(timeout))
            }
        }
    }
}

pub fn get(url: &str) -> Result<ureq::RequestBuilder<ureq::typestate::WithoutBody>, String> {
    let agent = agent_for_url(url)?;
    Ok(agent.get(url))
}

pub fn get_with_timeout(
    url: &str,
    timeout: Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithoutBody>, String> {
    let agent = agent_for_url_with_timeout(url, timeout)?;
    Ok(agent.get(url))
}

pub fn post(url: &str) -> Result<ureq::RequestBuilder<ureq::typestate::WithBody>, String> {
    let agent = agent_for_url(url)?;
    Ok(agent.post(url))
}

pub fn post_with_timeout(
    url: &str,
    timeout: Duration,
) -> Result<ureq::RequestBuilder<ureq::typestate::WithBody>, String> {
    let agent = agent_for_url_with_timeout(url, timeout)?;
    Ok(agent.post(url))
}

fn build_direct_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into()
}

fn build_socks5_agent(proxy_uri: &str, timeout: Duration) -> Result<ureq::Agent, String> {
    let proxy = ureq::Proxy::new(proxy_uri)
        .map_err(|e| format!("invalid SOCKS5 proxy URI `{proxy_uri}`: {e}"))?;
    Ok(ureq::Agent::config_builder()
        .proxy(Some(proxy))
        .timeout_global(Some(timeout))
        .build()
        .into())
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
    fn local_url_direct_agent() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _fail = EnvVarGuard::set("NYM_FAIL_CLOSED", Some("true"));
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", None);
        assert!(agent_for_url("http://127.0.0.1:3100/api").is_ok());
    }

    #[test]
    fn external_url_blocks_when_fail_closed_without_proxy() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _fail = EnvVarGuard::set("NYM_FAIL_CLOSED", Some("true"));
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", None);
        let _all = EnvVarGuard::set("ALL_PROXY", None);
        let err = agent_for_url("https://api.openai.com/v1/models").expect_err("must fail closed");
        assert!(err.contains("outbound blocked"));
    }

    #[test]
    fn external_url_uses_proxy_when_available() {
        let _guard = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _fail = EnvVarGuard::set("NYM_FAIL_CLOSED", Some("true"));
        let _socks = EnvVarGuard::set("SOCKS5_PROXY", Some("127.0.0.1:1080"));
        assert!(agent_for_url("https://api.openai.com/v1/models").is_ok());
    }

    #[test]
    fn invalid_proxy_uri_returns_error() {
        let err = build_socks5_agent("socks5://", Duration::from_secs(1))
            .expect_err("invalid URI should error");
        assert!(err.contains("invalid SOCKS5 proxy URI"));
    }
}
