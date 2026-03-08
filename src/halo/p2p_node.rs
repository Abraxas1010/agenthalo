use crate::halo::did::DIDIdentity;
use crate::halo::p2p_discovery::{
    announcement_for_identity, is_allowed_capability_topic, sign_announcement, topic_general,
    AgentDiscovery,
};
use ed25519_dalek::SigningKey as Ed25519SigningKey;
use futures_util::StreamExt;
use libp2p::autonat;
use libp2p::dcutr;
use libp2p::gossipsub::{self, IdentTopic, MessageAuthenticity};
use libp2p::identify;
use libp2p::kad::{self, store::MemoryStore};
use libp2p::mdns;
use libp2p::noise;
use libp2p::relay;
use libp2p::swarm::{NetworkBehaviour, Swarm, SwarmEvent};
use libp2p::yamux;
use libp2p::{identity, Multiaddr, PeerId, SwarmBuilder};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::time::Duration;
use tokio::time::{self, MissedTickBehavior};

pub const PROTOCOL_DIDCOMM: &str = "/agenthalo/didcomm/1.0.0";
pub const PROTOCOL_DISCOVERY: &str = "/agenthalo/discovery/1.0.0";
pub const PROTOCOL_A2A_COMPAT: &str = "/agenthalo/a2a-compat/1.0.0";

const DEFAULT_P2P_PORT: u16 = 9090;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct P2pConfig {
    pub enabled: bool,
    pub listen_port: u16,
    pub bootstrap_peers: Vec<Multiaddr>,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_port: DEFAULT_P2P_PORT,
            bootstrap_peers: Vec::new(),
        }
    }
}

impl P2pConfig {
    pub fn from_env() -> Result<Self, String> {
        let enabled = std::env::var("P2P_ENABLED")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);

        let listen_port = std::env::var("P2P_LISTEN_PORT")
            .ok()
            .and_then(|value| value.trim().parse::<u16>().ok())
            .unwrap_or(DEFAULT_P2P_PORT);

        let bootstrap_peers = std::env::var("P2P_BOOTSTRAP_PEERS")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|segment| {
                        let trimmed = segment.trim();
                        if trimmed.is_empty() {
                            None
                        } else {
                            Multiaddr::from_str(trimmed).ok()
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(Self {
            enabled,
            listen_port,
            bootstrap_peers,
        })
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "HaloBehaviourEvent")]
pub struct HaloBehaviour {
    pub identify: identify::Behaviour,
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub gossipsub: gossipsub::Behaviour,
    pub relay_client: relay::client::Behaviour,
    pub dcutr: dcutr::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub autonat: autonat::Behaviour,
}

#[derive(Debug)]
pub enum HaloBehaviourEvent {
    Identify(Box<identify::Event>),
    Kademlia(Box<kad::Event>),
    Gossipsub(Box<gossipsub::Event>),
    RelayClient(Box<relay::client::Event>),
    Dcutr(Box<dcutr::Event>),
    Mdns(Box<mdns::Event>),
    Autonat(Box<autonat::Event>),
}

impl From<identify::Event> for HaloBehaviourEvent {
    fn from(value: identify::Event) -> Self {
        Self::Identify(Box::new(value))
    }
}

impl From<kad::Event> for HaloBehaviourEvent {
    fn from(value: kad::Event) -> Self {
        Self::Kademlia(Box::new(value))
    }
}

impl From<gossipsub::Event> for HaloBehaviourEvent {
    fn from(value: gossipsub::Event) -> Self {
        Self::Gossipsub(Box::new(value))
    }
}

impl From<relay::client::Event> for HaloBehaviourEvent {
    fn from(value: relay::client::Event) -> Self {
        Self::RelayClient(Box::new(value))
    }
}

impl From<dcutr::Event> for HaloBehaviourEvent {
    fn from(value: dcutr::Event) -> Self {
        Self::Dcutr(Box::new(value))
    }
}

impl From<mdns::Event> for HaloBehaviourEvent {
    fn from(value: mdns::Event) -> Self {
        Self::Mdns(Box::new(value))
    }
}

impl From<autonat::Event> for HaloBehaviourEvent {
    fn from(value: autonat::Event) -> Self {
        Self::Autonat(Box::new(value))
    }
}

pub struct P2pNode {
    swarm: Swarm<HaloBehaviour>,
    peer_id: PeerId,
    config: P2pConfig,
}

impl P2pNode {
    pub fn create(
        ed25519_signing_key: &Ed25519SigningKey,
        config: &P2pConfig,
    ) -> Result<Self, String> {
        let mut keypair_bytes = ed25519_signing_key.to_keypair_bytes();
        let libp2p_ed25519 = identity::ed25519::Keypair::try_from_bytes(&mut keypair_bytes)
            .map_err(|e| format!("convert Ed25519 keypair for libp2p: {e}"))?;
        let identity_keypair = identity::Keypair::from(libp2p_ed25519);
        Self::create_from_identity(identity_keypair, config.clone())
    }

    pub fn create_from_did(identity: &DIDIdentity, config: &P2pConfig) -> Result<Self, String> {
        Self::create(&identity.ed25519_signing_key, config)
    }

    fn create_from_identity(keypair: identity::Keypair, config: P2pConfig) -> Result<Self, String> {
        let peer_id = keypair.public().to_peer_id();
        let mut swarm = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                libp2p::tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .map_err(|e| format!("build TCP transport: {e}"))?
            .with_relay_client(noise::Config::new, yamux::Config::default)
            .map_err(|e| format!("build relay client transport: {e}"))?
            .with_behaviour(|identity_key, relay_client| {
                let local_peer_id = identity_key.public().to_peer_id();
                let mut kad_behaviour =
                    kad::Behaviour::new(local_peer_id, MemoryStore::new(local_peer_id));
                kad_behaviour.set_mode(Some(kad::Mode::Server));

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .validation_mode(gossipsub::ValidationMode::Strict)
                    .build()
                    .map_err(|e| format!("build gossipsub config: {e}"))?;
                let gossipsub = gossipsub::Behaviour::new(
                    MessageAuthenticity::Signed(identity_key.clone()),
                    gossipsub_config,
                )
                .map_err(|e| format!("build gossipsub: {e}"))?;

                let mdns_behaviour =
                    mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id)
                        .map_err(|e| format!("build mDNS: {e}"))?;

                Ok(HaloBehaviour {
                    identify: identify::Behaviour::new(identify::Config::new(
                        PROTOCOL_DIDCOMM.to_string(),
                        identity_key.public(),
                    )),
                    kademlia: kad_behaviour,
                    gossipsub,
                    relay_client,
                    dcutr: dcutr::Behaviour::new(local_peer_id),
                    mdns: mdns_behaviour,
                    autonat: autonat::Behaviour::new(local_peer_id, autonat::Config::default()),
                })
            })
            .map_err(|e| format!("build swarm behaviour: {e}"))?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        let listen: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", config.listen_port)
            .parse()
            .map_err(|e| format!("invalid listen address: {e}"))?;
        swarm
            .listen_on(listen)
            .map_err(|e| format!("listen_on failed: {e}"))?;

        for bootstrap in &config.bootstrap_peers {
            if let Err(error) = swarm.dial(bootstrap.clone()) {
                eprintln!("[AgentHalo/P2P] bootstrap dial failed for {bootstrap}: {error}");
            }
        }

        let topic = IdentTopic::new(topic_general());
        if let Err(error) = swarm.behaviour_mut().gossipsub.subscribe(&topic) {
            eprintln!("[AgentHalo/P2P] subscribe default topic failed: {error}");
        }

        Ok(Self {
            swarm,
            peer_id,
            config,
        })
    }

    pub fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    pub fn config(&self) -> &P2pConfig {
        &self.config
    }

    pub fn listen_addresses(&self) -> Vec<Multiaddr> {
        self.swarm.listeners().cloned().collect()
    }

    pub fn publish(&mut self, topic: &str, payload: Vec<u8>) -> Result<(), String> {
        self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(IdentTopic::new(topic), payload)
            .map_err(|e| format!("publish to `{topic}` failed: {e}"))?;
        Ok(())
    }

    pub fn kademlia_mut(&mut self) -> &mut kad::Behaviour<MemoryStore> {
        &mut self.swarm.behaviour_mut().kademlia
    }

    pub fn gossipsub_mut(&mut self) -> &mut gossipsub::Behaviour {
        &mut self.swarm.behaviour_mut().gossipsub
    }

    pub async fn run(&mut self) -> Result<(), String> {
        loop {
            let Some(event) = self.swarm.next().await else {
                return Err("p2p swarm stream ended unexpectedly".to_string());
            };

            match event {
                SwarmEvent::Behaviour(HaloBehaviourEvent::Identify(event)) => {
                    eprintln!("[AgentHalo/P2P] identify event: {event:?}");
                }
                SwarmEvent::Behaviour(HaloBehaviourEvent::Kademlia(event)) => {
                    eprintln!("[AgentHalo/P2P] kad event: {event:?}");
                }
                SwarmEvent::Behaviour(HaloBehaviourEvent::Gossipsub(event)) => {
                    eprintln!("[AgentHalo/P2P] gossipsub event: {event:?}");
                }
                SwarmEvent::Behaviour(HaloBehaviourEvent::RelayClient(event)) => {
                    eprintln!("[AgentHalo/P2P] relay client event: {event:?}");
                }
                SwarmEvent::Behaviour(HaloBehaviourEvent::Dcutr(event)) => {
                    eprintln!("[AgentHalo/P2P] dcutr event: {event:?}");
                }
                SwarmEvent::Behaviour(HaloBehaviourEvent::Mdns(event)) => match *event {
                    mdns::Event::Discovered(peers) => {
                        for (peer, addr) in peers {
                            self.swarm
                                .behaviour_mut()
                                .kademlia
                                .add_address(&peer, addr.clone());
                            self.swarm
                                .behaviour_mut()
                                .gossipsub
                                .add_explicit_peer(&peer);
                        }
                    }
                    mdns::Event::Expired(peers) => {
                        for (peer, addr) in peers {
                            self.swarm
                                .behaviour_mut()
                                .kademlia
                                .remove_address(&peer, &addr);
                            self.swarm
                                .behaviour_mut()
                                .gossipsub
                                .remove_explicit_peer(&peer);
                        }
                    }
                },
                SwarmEvent::Behaviour(HaloBehaviourEvent::Autonat(event)) => {
                    eprintln!("[AgentHalo/P2P] autonat event: {event:?}");
                }
                SwarmEvent::NewListenAddr { address, .. } => {
                    eprintln!("[AgentHalo/P2P] listening on {address}");
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    eprintln!("[AgentHalo/P2P] connection established to {peer_id}");
                }
                SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                    eprintln!(
                        "[AgentHalo/P2P] outgoing connection error to {:?}: {error}",
                        peer_id
                    );
                }
                SwarmEvent::IncomingConnectionError { error, .. } => {
                    eprintln!("[AgentHalo/P2P] incoming connection error: {error}");
                }
                _ => {}
            }
        }
    }

    pub async fn run_with_discovery(
        &mut self,
        identity: &DIDIdentity,
        discovery: &mut AgentDiscovery,
        reannounce_ttl_secs: u64,
    ) -> Result<(), String> {
        let reannounce_every = Duration::from_secs(reannounce_ttl_secs.max(2) / 2);
        let mut reannounce_tick = time::interval(reannounce_every);
        reannounce_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = reannounce_tick.tick() => {
                    discovery.prune_expired();
                    let mut announcement = announcement_for_identity(
                        identity,
                        self.peer_id,
                        Vec::new(),
                        self.listen_addresses().into_iter().map(|addr| addr.to_string()).collect(),
                    );
                    announcement.ttl = reannounce_ttl_secs;
                    sign_announcement(identity, &mut announcement)?;
                    discovery.upsert_verified(announcement.clone());
                    if let Err(error) = discovery.announce(&topic_general(), &announcement, self.gossipsub_mut()) {
                        eprintln!("[AgentHalo/P2P] periodic gossip announce failed: {error}");
                    }
                    if let Err(error) = discovery.publish_to_dht(&announcement, self.kademlia_mut()) {
                        eprintln!("[AgentHalo/P2P] periodic DHT announce failed: {error}");
                    }
                }
                maybe_event = self.swarm.next() => {
                    let Some(event) = maybe_event else {
                        return Err("p2p swarm stream ended unexpectedly".to_string());
                    };
                    match event {
                        SwarmEvent::Behaviour(HaloBehaviourEvent::Identify(event)) => {
                            eprintln!("[AgentHalo/P2P] identify event: {event:?}");
                        }
                        SwarmEvent::Behaviour(HaloBehaviourEvent::Kademlia(event)) => {
                            match *event {
                                kad::Event::OutboundQueryProgressed { result, .. } => match result {
                                    kad::QueryResult::GetRecord(Ok(
                                        kad::GetRecordOk::FoundRecord(peer_record),
                                    )) => {
                                        match discovery.ingest_kad_record_with_resolver(
                                            &peer_record.record,
                                            |did| {
                                                if did == identity.did {
                                                    Some(identity.did_document.clone())
                                                } else {
                                                    None
                                                }
                                            },
                                        ) {
                                            Ok(announcement) => {
                                                eprintln!(
                                                    "[AgentHalo/P2P] accepted DHT announcement did={}",
                                                    announcement.did
                                                );
                                            }
                                            Err(error) => {
                                                eprintln!(
                                                    "[AgentHalo/P2P] rejected DHT announcement: {error}"
                                                );
                                            }
                                        }
                                    }
                                    kad::QueryResult::GetRecord(Ok(
                                        kad::GetRecordOk::FinishedWithNoAdditionalRecord {
                                            cache_candidates,
                                        },
                                    )) => {
                                        eprintln!(
                                            "[AgentHalo/P2P] DHT get_record finished without additional records (cache candidates: {})",
                                            cache_candidates.len()
                                        );
                                    }
                                    kad::QueryResult::GetRecord(Err(error)) => {
                                        eprintln!("[AgentHalo/P2P] DHT get_record error: {error:?}");
                                    }
                                    other => {
                                        eprintln!("[AgentHalo/P2P] kad query result: {other:?}");
                                    }
                                },
                                other => {
                                    eprintln!("[AgentHalo/P2P] kad event: {other:?}");
                                }
                            }
                        }
                        SwarmEvent::Behaviour(HaloBehaviourEvent::Gossipsub(event)) => match *event {
                            gossipsub::Event::Message {
                                propagation_source,
                                message_id,
                                message,
                            } => {
                                if is_allowed_capability_topic(message.topic.as_str()) {
                                    match discovery.handle_gossipsub_message(&message.data, |did| {
                                        if did == identity.did {
                                            Some(identity.did_document.clone())
                                        } else {
                                            None
                                        }
                                    }) {
                                        Ok(announcement) => {
                                            eprintln!(
                                                "[AgentHalo/P2P] accepted announcement from {propagation_source} did={} msg={message_id}",
                                                announcement.did
                                            );
                                        }
                                        Err(error) => {
                                            eprintln!(
                                                "[AgentHalo/P2P] rejected gossipsub announcement from {propagation_source}: {error}"
                                            );
                                        }
                                    }
                                } else {
                                    eprintln!(
                                        "[AgentHalo/P2P] ignoring gossipsub message on disallowed topic `{}`",
                                        message.topic
                                    );
                                }
                            }
                            other => {
                                eprintln!("[AgentHalo/P2P] gossipsub event: {other:?}");
                            }
                        },
                        SwarmEvent::Behaviour(HaloBehaviourEvent::RelayClient(event)) => {
                            eprintln!("[AgentHalo/P2P] relay client event: {event:?}");
                        }
                        SwarmEvent::Behaviour(HaloBehaviourEvent::Dcutr(event)) => {
                            eprintln!("[AgentHalo/P2P] dcutr event: {event:?}");
                        }
                        SwarmEvent::Behaviour(HaloBehaviourEvent::Mdns(event)) => match *event {
                            mdns::Event::Discovered(peers) => {
                                for (peer, addr) in peers {
                                    self.swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .add_address(&peer, addr.clone());
                                    self.swarm
                                        .behaviour_mut()
                                        .gossipsub
                                        .add_explicit_peer(&peer);
                                }
                            }
                            mdns::Event::Expired(peers) => {
                                for (peer, addr) in peers {
                                    self.swarm
                                        .behaviour_mut()
                                        .kademlia
                                        .remove_address(&peer, &addr);
                                    self.swarm
                                        .behaviour_mut()
                                        .gossipsub
                                        .remove_explicit_peer(&peer);
                                }
                            }
                        },
                        SwarmEvent::Behaviour(HaloBehaviourEvent::Autonat(event)) => {
                            eprintln!("[AgentHalo/P2P] autonat event: {event:?}");
                        }
                        SwarmEvent::NewListenAddr { address, .. } => {
                            eprintln!("[AgentHalo/P2P] listening on {address}");
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            eprintln!("[AgentHalo/P2P] connection established to {peer_id}");
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            eprintln!(
                                "[AgentHalo/P2P] outgoing connection error to {:?}: {error}",
                                peer_id
                            );
                        }
                        SwarmEvent::IncomingConnectionError { error, .. } => {
                            eprintln!("[AgentHalo/P2P] incoming connection error: {error}");
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        let mutex = env_lock();
        let guard = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
            if let Some(prev) = &self.prev {
                std::env::set_var(self.key, prev);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    #[test]
    fn p2p_config_parses_bootstrap_multiaddrs() {
        let _guard = lock_env();
        let _bootstrap = EnvVarGuard::set(
            "P2P_BOOTSTRAP_PEERS",
            Some("/ip4/1.2.3.4/tcp/9090,/ip4/5.6.7.8/tcp/9091"),
        );
        let config = P2pConfig::from_env().expect("config");
        assert_eq!(config.bootstrap_peers.len(), 2);
    }

    #[tokio::test]
    async fn peer_id_derives_from_did_seed() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x2a)).expect("identity");
        let config = P2pConfig::default();
        let node = P2pNode::create_from_did(&identity, &config).expect("p2p node");
        let peer_id_a = node.peer_id().to_string();

        let identity2 = crate::halo::did::did_from_genesis_seed(&seed(0x2a)).expect("identity 2");
        let node2 = P2pNode::create_from_did(&identity2, &config).expect("p2p node 2");
        let peer_id_b = node2.peer_id().to_string();

        assert_eq!(peer_id_a, peer_id_b);
    }

    #[test]
    fn peer_id_and_did_share_same_key_material() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x52)).expect("identity");
        let mut keypair_bytes = identity.ed25519_signing_key.to_keypair_bytes();
        let libp2p_ed25519 = identity::ed25519::Keypair::try_from_bytes(&mut keypair_bytes)
            .expect("libp2p ed25519 keypair");
        let libp2p_public_bytes = libp2p_ed25519.public().to_bytes();

        let method = identity
            .did_document
            .verification_method
            .iter()
            .find(|m| m.type_ == "Ed25519VerificationKey2020")
            .expect("DID Ed25519 verification method");
        let (_, decoded) =
            multibase::decode(&method.public_key_multibase).expect("decode DID Ed25519 key");
        assert!(decoded.starts_with(&[0xed, 0x01]));
        let did_public_bytes: [u8; 32] = decoded[2..]
            .try_into()
            .expect("DID Ed25519 key must be 32 bytes");

        assert_eq!(libp2p_public_bytes, did_public_bytes);
    }
}
