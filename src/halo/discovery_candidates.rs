use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum DiscoveryChannel {
    Mdns,
    DnsTxt { domain: String },
    Hardcoded,
    Cached,
    EnvVar,
    PeerGossip { from_peer: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiscoveryCandidate {
    pub peer_id: String,
    pub addrs: Vec<Multiaddr>,
    pub channels: Vec<DiscoveryChannel>,
    pub discovered_at: u64,
    pub trust: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct CachedCandidate {
    peer_id: String,
    #[serde(default)]
    addrs: Vec<String>,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn parse_discovery_entries(raw: &str) -> Vec<(String, Vec<Multiaddr>)> {
    raw.split(';')
        .filter_map(|entry| {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return None;
            }
            let (peer_id, addrs) = trimmed.split_once('=')?;
            let parsed_addrs = addrs
                .split(',')
                .filter_map(|addr| Multiaddr::from_str(addr.trim()).ok())
                .collect::<Vec<_>>();
            if parsed_addrs.is_empty() {
                None
            } else {
                Some((peer_id.trim().to_string(), parsed_addrs))
            }
        })
        .collect()
}

fn discovery_cache_path() -> Option<PathBuf> {
    let home = std::env::var_os("AGENTHALO_HOME")?;
    Some(PathBuf::from(home).join("discovery_cache.json"))
}

async fn mdns_discover() -> Vec<(String, Vec<Multiaddr>)> {
    Vec::new()
}

async fn dns_txt_discover() -> Vec<(String, Vec<Multiaddr>)> {
    std::env::var("AGENTHALO_DISCOVERY_DNS_PEERS")
        .ok()
        .map(|raw| parse_discovery_entries(&raw))
        .unwrap_or_default()
}

async fn hardcoded_discover() -> Vec<(String, Vec<Multiaddr>)> {
    std::env::var("AGENTHALO_DISCOVERY_HARDCODED_PEERS")
        .ok()
        .map(|raw| parse_discovery_entries(&raw))
        .unwrap_or_default()
}

async fn cached_discover() -> Vec<(String, Vec<Multiaddr>)> {
    let Some(path) = discovery_cache_path() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(entries) = serde_json::from_str::<Vec<CachedCandidate>>(&raw) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|entry| {
            let addrs = entry
                .addrs
                .into_iter()
                .filter_map(|addr| Multiaddr::from_str(&addr).ok())
                .collect::<Vec<_>>();
            (entry.peer_id, addrs)
        })
        .filter(|(_, addrs)| !addrs.is_empty())
        .collect()
}

async fn env_var_discover() -> Vec<(String, Vec<Multiaddr>)> {
    std::env::var("AGENTHALO_DISCOVERY_PEERS")
        .ok()
        .map(|raw| parse_discovery_entries(&raw))
        .unwrap_or_default()
}

pub fn merge_discovery_candidates(
    sources: Vec<(DiscoveryChannel, Vec<(String, Vec<Multiaddr>)>)>,
    discovered_at: u64,
) -> Vec<DiscoveryCandidate> {
    let mut by_peer: HashMap<String, DiscoveryCandidate> = HashMap::new();

    for (channel, candidates) in sources {
        for (peer_id, addrs) in candidates {
            by_peer
                .entry(peer_id.clone())
                .and_modify(|candidate| {
                    if !candidate.channels.contains(&channel) {
                        candidate.channels.push(channel.clone());
                    }
                    for addr in &addrs {
                        if !candidate.addrs.contains(addr) {
                            candidate.addrs.push(addr.clone());
                        }
                    }
                })
                .or_insert_with(|| DiscoveryCandidate {
                    peer_id,
                    addrs,
                    channels: vec![channel.clone()],
                    discovered_at,
                    trust: 0.0,
                });
        }
    }

    let mut out = by_peer.into_values().collect::<Vec<_>>();
    out.sort_by(|a, b| a.peer_id.cmp(&b.peer_id));
    out
}

pub async fn discover_candidates() -> Vec<DiscoveryCandidate> {
    let (mdns, dns, hardcoded, cached, env) = tokio::join!(
        mdns_discover(),
        dns_txt_discover(),
        hardcoded_discover(),
        cached_discover(),
        env_var_discover(),
    );
    merge_discovery_candidates(
        vec![
            (DiscoveryChannel::Mdns, mdns),
            (
                DiscoveryChannel::DnsTxt {
                    domain: "agenthalo.discovery".to_string(),
                },
                dns,
            ),
            (DiscoveryChannel::Hardcoded, hardcoded),
            (DiscoveryChannel::Cached, cached),
            (DiscoveryChannel::EnvVar, env),
        ],
        now_unix(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_deduplicates_peers_and_preserves_zero_trust() {
        let addr_a: Multiaddr = "/ip4/127.0.0.1/tcp/9001".parse().expect("addr a");
        let addr_b: Multiaddr = "/ip4/127.0.0.1/tcp/9002".parse().expect("addr b");
        let merged = merge_discovery_candidates(
            vec![
                (
                    DiscoveryChannel::EnvVar,
                    vec![("peer-a".to_string(), vec![addr_a.clone()])],
                ),
                (
                    DiscoveryChannel::Hardcoded,
                    vec![("peer-a".to_string(), vec![addr_b.clone()])],
                ),
            ],
            100,
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].trust, 0.0);
        assert_eq!(merged[0].channels.len(), 2);
        assert!(merged[0].addrs.contains(&addr_a));
        assert!(merged[0].addrs.contains(&addr_b));
    }

    #[test]
    fn parser_accepts_peer_equals_multiaddr_lists() {
        let entries = parse_discovery_entries(
            "peer-a=/ip4/127.0.0.1/tcp/9001,/ip4/127.0.0.1/tcp/9002;peer-b=/dns4/example.com/tcp/443",
        );
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].0, "peer-a");
        assert_eq!(entries[0].1.len(), 2);
        assert_eq!(entries[1].0, "peer-b");
    }
}
