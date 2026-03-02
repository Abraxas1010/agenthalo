use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use libp2p::gossipsub::{IdentTopic, MessageId};
use libp2p::kad::{store::MemoryStore, Behaviour as Kademlia, Quorum, Record, RecordKey};
use libp2p::{gossipsub, PeerId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};

pub const TOPIC_PREFIX: &str = "/agenthalo/capabilities/";
const DID_KEY_PREFIX: &str = "did:key:";
const TYPE_ED25519: &str = "Ed25519VerificationKey2020";
const MULTICODEC_ED25519_PUB: &[u8] = &[0xed, 0x01];

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_document: Option<DIDDocument>,
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
    did_document: Option<DIDDocument>,
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
            did_document: value.did_document.clone(),
        }
    }
}

fn payload_bytes(announcement: &AgentAnnouncement) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&AgentAnnouncementPayload::from(announcement))
        .map_err(|e| format!("serialize announcement payload: {e}"))
}

fn decode_did_key_ed25519_public(did: &str) -> Result<[u8; 32], String> {
    let encoded = did
        .strip_prefix(DID_KEY_PREFIX)
        .ok_or_else(|| "announcement DID is not a did:key identifier".to_string())?;
    let (_, decoded) = multibase::decode(encoded)
        .map_err(|e| format!("multibase decode failed for did:key identifier: {e}"))?;
    if decoded.len() != MULTICODEC_ED25519_PUB.len() + 32 {
        return Err("did:key payload must be Ed25519 multicodec + 32-byte key".to_string());
    }
    if !decoded.starts_with(MULTICODEC_ED25519_PUB) {
        return Err("did:key payload must use Ed25519 multicodec prefix".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded[MULTICODEC_ED25519_PUB.len()..]);
    Ok(out)
}

fn decode_document_ed25519_public(did_document: &DIDDocument) -> Result<[u8; 32], String> {
    let method = did_document
        .verification_method
        .iter()
        .find(|method| method.type_ == TYPE_ED25519)
        .ok_or_else(|| "DID document missing Ed25519 verification method".to_string())?;
    let (_, decoded) = multibase::decode(&method.public_key_multibase)
        .map_err(|e| format!("multibase decode failed for DID Ed25519 key: {e}"))?;
    if decoded.len() != MULTICODEC_ED25519_PUB.len() + 32 {
        return Err("DID Ed25519 key must include multicodec + 32-byte key".to_string());
    }
    if !decoded.starts_with(MULTICODEC_ED25519_PUB) {
        return Err("DID Ed25519 key has unexpected multicodec prefix".to_string());
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&decoded[MULTICODEC_ED25519_PUB.len()..]);
    Ok(out)
}

fn verify_did_document_binding(
    announcement: &AgentAnnouncement,
    did_document: &DIDDocument,
) -> Result<(), String> {
    if did_document.id != announcement.did {
        return Err("announcement DID document id does not match announcement DID".to_string());
    }
    let did_key_ed25519 = decode_did_key_ed25519_public(&announcement.did)?;
    let document_ed25519 = decode_document_ed25519_public(did_document)?;
    if did_key_ed25519 != document_ed25519 {
        return Err(
            "announcement DID document Ed25519 key does not match did:key identifier".to_string(),
        );
    }
    Ok(())
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
        self.ingest_kad_record_with_resolver(record, |_| None)
            .map(|_| ())
    }

    pub fn ingest_kad_record_with_resolver<F>(
        &mut self,
        record: &Record,
        resolve_document: F,
    ) -> Result<AgentAnnouncement, String>
    where
        F: Fn(&str) -> Option<DIDDocument>,
    {
        let announcement: AgentAnnouncement = serde_json::from_slice(&record.value)
            .map_err(|e| format!("decode DHT record as announcement: {e}"))?;
        self.verify_and_upsert(announcement, resolve_document)
    }

    pub fn handle_gossipsub_message<F>(
        &mut self,
        data: &[u8],
        resolve_document: F,
    ) -> Result<AgentAnnouncement, String>
    where
        F: Fn(&str) -> Option<DIDDocument>,
    {
        let announcement: AgentAnnouncement = serde_json::from_slice(data)
            .map_err(|e| format!("decode gossipsub message as announcement: {e}"))?;
        self.verify_and_upsert(announcement, resolve_document)
    }

    fn verify_and_upsert<F>(
        &mut self,
        announcement: AgentAnnouncement,
        resolve_document: F,
    ) -> Result<AgentAnnouncement, String>
    where
        F: Fn(&str) -> Option<DIDDocument>,
    {
        let did_document = announcement
            .did_document
            .clone()
            .or_else(|| resolve_document(&announcement.did))
            .ok_or_else(|| {
                format!(
                    "announcement for `{}` missing DID document for signature verification",
                    announcement.did
                )
            })?;
        verify_did_document_binding(&announcement, &did_document)?;
        let verified = verify_announcement(&announcement, &did_document)?;
        if !verified {
            return Err(format!(
                "announcement signatures failed verification for `{}`",
                announcement.did
            ));
        }
        self.upsert_verified(announcement.clone());
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
        did_document: Some(identity.did_document.clone()),
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
            did_document: None,
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
            did_document: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });

        let coding = discovery.find_by_capability("coding");
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].peer_id, "peer-1");
    }

    #[test]
    fn handle_gossipsub_message_requires_signature_verification() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x53)).expect("identity");
        let announcement = announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        let payload = serde_json::to_vec(&announcement).expect("serialize");
        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect_err("unsigned gossip must fail");
        assert!(err.contains("missing Ed25519 signature"));
    }

    #[test]
    fn handle_gossipsub_message_accepts_signed_announcement() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x54)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        sign_announcement(&identity, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let accepted = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect("verified gossip");
        assert_eq!(accepted.did, identity.did);
        assert!(discovery.known_agents().contains_key(&identity.did));
    }

    #[test]
    fn handle_gossipsub_message_rejects_did_key_mismatch() {
        let alice = crate::halo::did::did_from_genesis_seed(&seed(0x61)).expect("alice");
        let mallory = crate::halo::did::did_from_genesis_seed(&seed(0x62)).expect("mallory");
        let mut announcement =
            announcement_for_identity(&mallory, PeerId::random(), vec![], vec![]);
        announcement.did = alice.did.clone();
        let mut tampered_document = mallory.did_document.clone();
        tampered_document.id = alice.did.clone();
        announcement.did_document = Some(tampered_document);
        sign_announcement(&mallory, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect_err("did:key/document mismatch must fail");
        assert!(err.contains("does not match did:key identifier"));
    }

    #[test]
    fn ingest_kad_record_requires_signature_verification() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x63)).expect("identity");
        let announcement = announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        let value = serde_json::to_vec(&announcement).expect("serialize");
        let record = Record {
            key: announcement_kad_key(&announcement.did),
            value,
            publisher: None,
            expires: None,
        };
        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .ingest_kad_record(&record)
            .expect_err("unsigned KAD record must fail");
        assert!(err.contains("missing Ed25519 signature"));
    }

    #[test]
    fn ingest_kad_record_accepts_signed_announcement() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x64)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        sign_announcement(&identity, &mut announcement).expect("sign");
        let value = serde_json::to_vec(&announcement).expect("serialize");
        let record = Record {
            key: announcement_kad_key(&announcement.did),
            value,
            publisher: None,
            expires: None,
        };
        let mut discovery = AgentDiscovery::new();
        discovery
            .ingest_kad_record(&record)
            .expect("signed KAD record should be accepted");
        assert!(discovery.known_agents().contains_key(&identity.did));
    }
}
