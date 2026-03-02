use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use libp2p::gossipsub::{IdentTopic, MessageId};
use libp2p::kad::{store::MemoryStore, Behaviour as Kademlia, Quorum, Record, RecordKey};
use libp2p::{gossipsub, PeerId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const TOPIC_PREFIX: &str = "/agenthalo/capabilities/";

pub fn topic_general() -> String {
    format!("{TOPIC_PREFIX}general")
}

pub fn topic_coding() -> String {
    format!("{TOPIC_PREFIX}coding")
}

pub fn topic_research() -> String {
    format!("{TOPIC_PREFIX}research")
}

pub fn topic_financial() -> String {
    format!("{TOPIC_PREFIX}financial")
}

pub fn topic_blockchain() -> String {
    format!("{TOPIC_PREFIX}blockchain")
}

pub fn topic_privacy() -> String {
    format!("{TOPIC_PREFIX}privacy")
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentCapability {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub input_types: Vec<String>,
    #[serde(default)]
    pub output_types: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentAnnouncement {
    pub peer_id: String,
    pub did: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<AgentCapability>,
    #[serde(default)]
    pub multiaddrs: Vec<String>,
    #[serde(default)]
    pub protocols: Vec<String>,
    pub version: String,
    pub timestamp: u64,
    pub ttl: u64,
    pub ed25519_signature: Option<Vec<u8>>,
    pub mldsa65_signature: Option<Vec<u8>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct AgentAnnouncementPayload {
    peer_id: String,
    did: String,
    name: String,
    description: String,
    capabilities: Vec<AgentCapability>,
    multiaddrs: Vec<String>,
    protocols: Vec<String>,
    version: String,
    timestamp: u64,
    ttl: u64,
}

impl From<&AgentAnnouncement> for AgentAnnouncementPayload {
    fn from(value: &AgentAnnouncement) -> Self {
        Self {
            peer_id: value.peer_id.clone(),
            did: value.did.clone(),
            name: value.name.clone(),
            description: value.description.clone(),
            capabilities: value.capabilities.clone(),
            multiaddrs: value.multiaddrs.clone(),
            protocols: value.protocols.clone(),
            version: value.version.clone(),
            timestamp: value.timestamp,
            ttl: value.ttl,
        }
    }
}

fn payload_bytes(announcement: &AgentAnnouncement) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&AgentAnnouncementPayload::from(announcement))
        .map_err(|e| format!("serialize announcement payload: {e}"))
}

pub fn sign_announcement(
    identity: &DIDIdentity,
    announcement: &mut AgentAnnouncement,
) -> Result<(), String> {
    let payload = payload_bytes(announcement)?;
    let (ed_sig, pq_sig) = dual_sign(identity, &payload)?;
    announcement.ed25519_signature = Some(ed_sig);
    announcement.mldsa65_signature = Some(pq_sig);
    Ok(())
}

pub fn verify_announcement(
    announcement: &AgentAnnouncement,
    did_document: &DIDDocument,
) -> Result<bool, String> {
    let payload = payload_bytes(announcement)?;
    let ed_sig = announcement
        .ed25519_signature
        .as_ref()
        .ok_or_else(|| "announcement missing Ed25519 signature".to_string())?;
    let pq_sig = announcement
        .mldsa65_signature
        .as_ref()
        .ok_or_else(|| "announcement missing ML-DSA-65 signature".to_string())?;
    dual_verify(did_document, &payload, ed_sig, pq_sig)
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn announcement_kad_key(did: &str) -> RecordKey {
    RecordKey::new(&format!("/agenthalo/agent/{did}"))
}

#[derive(Clone, Debug)]
pub struct AgentDiscovery {
    known_agents: HashMap<String, AgentAnnouncement>,
    subscribed_topics: HashSet<String>,
}

impl Default for AgentDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentDiscovery {
    pub fn new() -> Self {
        Self {
            known_agents: HashMap::new(),
            subscribed_topics: HashSet::new(),
        }
    }

    pub fn known_agents(&self) -> &HashMap<String, AgentAnnouncement> {
        &self.known_agents
    }

    pub fn subscribe(
        &mut self,
        topic: &str,
        gossipsub_behaviour: &mut gossipsub::Behaviour,
    ) -> Result<(), String> {
        let ident = IdentTopic::new(topic.to_string());
        gossipsub_behaviour
            .subscribe(&ident)
            .map_err(|e| format!("subscribe `{topic}`: {e}"))?;
        self.subscribed_topics.insert(topic.to_string());
        Ok(())
    }

    pub fn is_subscribed(&self, topic: &str) -> bool {
        self.subscribed_topics.contains(topic)
    }

    pub fn announce(
        &self,
        topic: &str,
        announcement: &AgentAnnouncement,
        gossipsub_behaviour: &mut gossipsub::Behaviour,
    ) -> Result<MessageId, String> {
        let payload = serde_json::to_vec(announcement)
            .map_err(|e| format!("serialize announcement for gossip: {e}"))?;
        gossipsub_behaviour
            .publish(IdentTopic::new(topic.to_string()), payload)
            .map_err(|e| format!("publish announcement to `{topic}`: {e}"))
    }

    pub fn publish_to_dht(
        &self,
        announcement: &AgentAnnouncement,
        kademlia: &mut Kademlia<MemoryStore>,
    ) -> Result<(), String> {
        let value = serde_json::to_vec(announcement)
            .map_err(|e| format!("serialize announcement for DHT: {e}"))?;
        let record = Record {
            key: announcement_kad_key(&announcement.did),
            value,
            publisher: None,
            expires: None,
        };
        kademlia
            .put_record(record, Quorum::One)
            .map_err(|e| format!("DHT put_record failed: {e}"))?;
        Ok(())
    }

    pub fn lookup_by_did(&self, did: &str, kademlia: &mut Kademlia<MemoryStore>) {
        kademlia.get_record(announcement_kad_key(did));
    }

    pub fn ingest_kad_record(&mut self, record: &Record) -> Result<(), String> {
        let announcement: AgentAnnouncement = serde_json::from_slice(&record.value)
            .map_err(|e| format!("decode DHT record as announcement: {e}"))?;
        self.known_agents
            .insert(announcement.did.clone(), announcement);
        Ok(())
    }

    pub fn handle_gossipsub_message(&mut self, data: &[u8]) -> Result<AgentAnnouncement, String> {
        let announcement: AgentAnnouncement = serde_json::from_slice(data)
            .map_err(|e| format!("decode gossipsub message as announcement: {e}"))?;
        self.known_agents
            .insert(announcement.did.clone(), announcement.clone());
        Ok(announcement)
    }

    pub fn upsert_verified(&mut self, announcement: AgentAnnouncement) {
        self.known_agents
            .insert(announcement.did.clone(), announcement);
    }

    pub fn find_by_capability(&self, capability_id: &str) -> Vec<AgentAnnouncement> {
        self.known_agents
            .values()
            .filter(|announcement| {
                announcement
                    .capabilities
                    .iter()
                    .any(|capability| capability.id == capability_id)
            })
            .cloned()
            .collect()
    }

    pub fn prune_expired(&mut self) {
        let now = now_unix();
        self.known_agents.retain(|_, announcement| {
            now <= announcement.timestamp.saturating_add(announcement.ttl)
        });
    }
}

pub fn hash_did_for_membership(did: &str) -> String {
    let digest = Sha256::digest(did.as_bytes());
    crate::halo::util::hex_encode(digest.as_slice())
}

pub fn announcement_for_identity(
    identity: &DIDIdentity,
    peer_id: PeerId,
    capabilities: Vec<AgentCapability>,
    multiaddrs: Vec<String>,
) -> AgentAnnouncement {
    AgentAnnouncement {
        peer_id: peer_id.to_string(),
        did: identity.did.clone(),
        name: "AgentHalo".to_string(),
        description: "Sovereign privacy-preserving agent".to_string(),
        capabilities,
        multiaddrs,
        protocols: vec![
            "/agenthalo/didcomm/1.0.0".to_string(),
            "/agenthalo/discovery/1.0.0".to_string(),
        ],
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: now_unix(),
        ttl: 300,
        ed25519_signature: None,
        mldsa65_signature: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    #[test]
    fn announcement_sign_verify_roundtrip() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x44)).expect("identity");
        let mut announcement = announcement_for_identity(
            &identity,
            PeerId::random(),
            vec![AgentCapability {
                id: "coding".to_string(),
                name: "Coding".to_string(),
                description: "Writes code".to_string(),
                input_types: vec!["text/plain".to_string()],
                output_types: vec!["text/plain".to_string()],
            }],
            vec![],
        );
        sign_announcement(&identity, &mut announcement).expect("sign");
        let ok = verify_announcement(&announcement, &identity.did_document).expect("verify");
        assert!(ok);
    }

    #[test]
    fn find_by_capability_filters_results() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-1".to_string(),
            did: "did:key:z6Mk1".to_string(),
            name: "a".to_string(),
            description: "a".to_string(),
            capabilities: vec![AgentCapability {
                id: "coding".to_string(),
                name: "coding".to_string(),
                description: "coding".to_string(),
                input_types: vec![],
                output_types: vec![],
            }],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            ed25519_signature: None,
            mldsa65_signature: None,
        });
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-2".to_string(),
            did: "did:key:z6Mk2".to_string(),
            name: "b".to_string(),
            description: "b".to_string(),
            capabilities: vec![AgentCapability {
                id: "research".to_string(),
                name: "research".to_string(),
                description: "research".to_string(),
                input_types: vec![],
                output_types: vec![],
            }],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            ed25519_signature: None,
            mldsa65_signature: None,
        });

        let coding = discovery.find_by_capability("coding");
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].peer_id, "peer-1");
    }
}
