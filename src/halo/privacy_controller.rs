use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyLevel {
    Maximum,
    P2P,
    None,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub force_level: Option<PrivacyLevel>,
    #[serde(default)]
    pub always_maximum: Vec<String>,
    #[serde(default)]
    pub always_none: Vec<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            force_level: None,
            always_maximum: Vec::new(),
            always_none: Vec::new(),
        }
    }
}

impl PrivacyConfig {
    pub fn classify(&self, url: &str) -> PrivacyLevel {
        if let Some(forced) = self.force_level {
            return forced;
        }
        let host = extract_host(url).unwrap_or_default();
        for pat in &self.always_none {
            let p = pat.trim();
            if !p.is_empty() && host.contains(p) {
                return PrivacyLevel::None;
            }
        }
        for pat in &self.always_maximum {
            let p = pat.trim();
            if !p.is_empty() && host.contains(p) {
                return PrivacyLevel::Maximum;
            }
        }
        classify_url(url)
    }
}

pub fn classify_url(url: &str) -> PrivacyLevel {
    let host = extract_host(url);
    match host.as_deref() {
        Some(h) if is_local(h) => PrivacyLevel::None,
        Some(h) if is_peer_endpoint(h) => PrivacyLevel::P2P,
        _ => PrivacyLevel::Maximum,
    }
}

fn extract_host(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_scheme = strip_scheme(trimmed);

    let authority = without_scheme.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return None;
    }

    let authority = authority.split('@').next_back().unwrap_or(authority);
    parse_host_port(authority).map(|(h, _)| h.to_ascii_lowercase())
}

fn strip_scheme(url: &str) -> &str {
    for (prefix, len) in [
        (b"https://" as &[u8], 8),
        (b"http://", 7),
        (b"wss://", 6),
        (b"ws://", 5),
        (b"socks5://", 9),
        (b"tcp://", 6),
    ] {
        if url.len() >= len && url.as_bytes()[..len].eq_ignore_ascii_case(prefix) {
            return &url[len..];
        }
    }
    url
}

fn parse_host_port(authority: &str) -> Option<(String, Option<u16>)> {
    let s = authority.trim();
    if s.is_empty() {
        return None;
    }

    if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = &rest[..end];
        let tail = &rest[end + 1..];
        let port = tail.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
        return Some((host.to_string(), port));
    }

    if let Some((host, port)) = s.rsplit_once(':') {
        if host.contains(':') {
            return Some((s.to_string(), None));
        }
        if let Ok(parsed) = port.parse::<u16>() {
            return Some((host.to_string(), Some(parsed)));
        }
    }

    Some((s.to_string(), None))
}

fn is_local(host: &str) -> bool {
    let lowered = host.to_ascii_lowercase();

    if lowered == "localhost" || lowered == "0.0.0.0" || lowered == "::1" {
        return true;
    }
    if lowered.ends_with(".local") {
        return true;
    }

    if let Ok(ip) = lowered.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_broadcast()
                    || v4 == Ipv4Addr::UNSPECIFIED
            }
            IpAddr::V6(v6) => {
                v6.is_loopback()
                    || v6.is_unique_local()
                    || v6.is_unicast_link_local()
                    || v6.is_unspecified()
            }
        };
    }

    false
}

fn is_peer_endpoint(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    h.starts_with("halo-")
        || h.ends_with(".halo-mesh")
        || h.ends_with(".p2p")
        || h.ends_with(".libp2p")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_localhost_none() {
        assert_eq!(
            classify_url("http://127.0.0.1:3100/api"),
            PrivacyLevel::None
        );
        assert_eq!(classify_url("http://localhost:3000"), PrivacyLevel::None);
        assert_eq!(classify_url("http://[::1]:8080"), PrivacyLevel::None);
    }

    #[test]
    fn classify_private_ip_none() {
        assert_eq!(classify_url("http://10.0.0.9:3000"), PrivacyLevel::None);
        assert_eq!(
            classify_url("https://192.168.1.15/path"),
            PrivacyLevel::None
        );
        assert_eq!(classify_url("http://172.20.0.8"), PrivacyLevel::None);
    }

    #[test]
    fn classify_mesh_p2p() {
        assert_eq!(
            classify_url("tcp://halo-peer-1.halo-mesh:4001"),
            PrivacyLevel::P2P
        );
        assert_eq!(classify_url("https://halo-worker-2"), PrivacyLevel::P2P);
        assert_eq!(classify_url("tcp://node.libp2p:4001"), PrivacyLevel::P2P);
    }

    #[test]
    fn classify_external_maximum() {
        assert_eq!(
            classify_url("https://api.openai.com/v1/models"),
            PrivacyLevel::Maximum
        );
        assert_eq!(
            classify_url("https://sepolia.base.org"),
            PrivacyLevel::Maximum
        );
        assert_eq!(
            classify_url("HTTPS://api.openai.com/v1/models"),
            PrivacyLevel::Maximum
        );
        assert_eq!(classify_url("Http://Example.Com"), PrivacyLevel::Maximum);
        assert_eq!(classify_url("hTTpS://foo.bar"), PrivacyLevel::Maximum);
    }

    #[test]
    fn config_override_force_level() {
        let cfg = PrivacyConfig {
            force_level: Some(PrivacyLevel::None),
            ..PrivacyConfig::default()
        };
        assert_eq!(
            cfg.classify("https://api.openai.com/v1/models"),
            PrivacyLevel::None
        );
    }

    #[test]
    fn config_override_patterns() {
        let cfg = PrivacyConfig {
            force_level: None,
            always_maximum: vec!["example.com".into()],
            always_none: vec!["localhost".into()],
        };
        assert_eq!(cfg.classify("http://localhost:9000"), PrivacyLevel::None);
        assert_eq!(
            cfg.classify("https://foo.example.com"),
            PrivacyLevel::Maximum
        );
    }

    #[test]
    fn always_none_matches_host_not_path() {
        let cfg = PrivacyConfig {
            force_level: None,
            always_maximum: Vec::new(),
            always_none: vec!["127".into()],
        };
        assert_eq!(
            cfg.classify("https://evil.com/path/127/data"),
            PrivacyLevel::Maximum
        );
        assert_eq!(cfg.classify("http://127.0.0.1:3000"), PrivacyLevel::None);
    }

    #[test]
    fn parse_host_port_variants() {
        assert_eq!(
            parse_host_port("example.com:443"),
            Some(("example.com".to_string(), Some(443)))
        );
        assert_eq!(
            parse_host_port("[2001:db8::1]:9000"),
            Some(("2001:db8::1".to_string(), Some(9000)))
        );
        assert_eq!(
            parse_host_port("2001:db8::1"),
            Some(("2001:db8::1".to_string(), None))
        );
    }
}
