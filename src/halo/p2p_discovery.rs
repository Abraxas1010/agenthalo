use crate::halo::capability_spec::{
    dynamic_topic_for_domain, is_dynamic_capability_topic, normalized_success_rate,
    CapabilityDomain, CapabilityQuery, CapabilitySpec, LiveMetrics,
};
use crate::halo::capability_verification::verify_capability_attestation;
use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::util::{digest_bytes, hex_encode};
use crate::halo::zk_credential;
use libp2p::gossipsub::{IdentTopic, MessageId};
use libp2p::kad::{store::MemoryStore, Behaviour as Kademlia, Quorum, Record, RecordKey};
use libp2p::{gossipsub, PeerId};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

pub const TOPIC_PREFIX: &str = "/agenthalo/capabilities/";
// T23: topic isolation model in lean/NucleusDB/Comms/Protocol/TopicIsolationSpec.lean
const DID_KEY_PREFIX: &str = "did:key:";
const TYPE_ED25519: &str = "Ed25519VerificationKey2020";
const MULTICODEC_ED25519_PUB: &[u8] = &[0xed, 0x01];
const ZK_CREDENTIAL_DID_HASH_DOMAIN: &str = "agenthalo.zk_credential.did_hash.v1";
const MAX_DYNAMIC_SUBSCRIPTIONS: usize = 64;
const MAX_ANNOUNCED_CAPABILITY_SPECS: usize = 64;
const MAX_ATTESTATIONS_PER_SPEC: usize = 32;
const MAX_ANNOUNCEMENT_TTL_SECS: u64 = 86_400;
const MAX_ANNOUNCEMENT_CLOCK_SKEW_SECS: u64 = 300;
const MAX_ANNOUNCEMENT_BYTES: usize = 256 * 1024;

pub fn topic_general() -> String {
    dynamic_topic_for_domain(&CapabilityDomain::new("general", 1))
}

pub fn topic_coding() -> String {
    dynamic_topic_for_domain(&CapabilityDomain::new("code/generate", 1))
}

pub fn topic_research() -> String {
    dynamic_topic_for_domain(&CapabilityDomain::new("research/general", 1))
}

pub fn topic_financial() -> String {
    dynamic_topic_for_domain(&CapabilityDomain::new("finance/analyze", 1))
}

pub fn topic_blockchain() -> String {
    dynamic_topic_for_domain(&CapabilityDomain::new("blockchain/evm", 1))
}

pub fn topic_privacy() -> String {
    dynamic_topic_for_domain(&CapabilityDomain::new("privacy/preserve", 1))
}

pub fn is_allowed_capability_topic(topic: &str) -> bool {
    is_dynamic_capability_topic(topic)
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

impl From<CapabilitySpec> for AgentCapability {
    fn from(value: CapabilitySpec) -> Self {
        let input_types = value
            .input_types
            .iter()
            .map(|ty| format!("{ty:?}"))
            .collect::<Vec<_>>();
        let output_types = value
            .output_types
            .iter()
            .map(|ty| format!("{ty:?}"))
            .collect::<Vec<_>>();
        Self {
            id: value.capability_id.clone(),
            name: value.domain.path.clone(),
            description: format!("Capability domain {}", value.domain.path),
            input_types,
            output_types,
        }
    }
}

impl AgentCapability {
    pub fn to_capability_spec(&self) -> CapabilitySpec {
        let domain_path = if self.name.trim().is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        };
        CapabilitySpec::new(
            CapabilityDomain::new(domain_path, 1),
            self.input_types
                .iter()
                .map(|ty| parse_legacy_type_spec(ty))
                .collect(),
            self.output_types
                .iter()
                .map(|ty| parse_legacy_type_spec(ty))
                .collect(),
            vec![],
        )
    }
}

fn parse_legacy_type_spec(raw: &str) -> crate::halo::capability_spec::TypeSpec {
    match raw {
        "LeanTerm" => crate::halo::capability_spec::TypeSpec::LeanTerm,
        "CoqTerm" => crate::halo::capability_spec::TypeSpec::CoqTerm,
        other => crate::halo::capability_spec::TypeSpec::Text {
            language: Some(other.to_string()),
        },
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentAnnouncement {
    pub peer_id: String,
    pub did: String,
    pub name: String,
    pub description: String,
    pub capabilities: Vec<AgentCapability>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_specs: Vec<CapabilitySpec>,
    #[serde(default)]
    pub multiaddrs: Vec<String>,
    #[serde(default)]
    pub protocols: Vec<String>,
    pub version: String,
    pub timestamp: u64,
    pub ttl: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did_document: Option<DIDDocument>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evm_address: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "binding_proof_sha256"
    )]
    pub binding_proof_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anonymous_membership_proof: Option<zk_credential::AnonymousCredentialProofBundle>,
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
    capability_specs: Vec<CapabilitySpec>,
    multiaddrs: Vec<String>,
    protocols: Vec<String>,
    version: String,
    timestamp: u64,
    ttl: u64,
    did_document: Option<DIDDocument>,
    evm_address: Option<String>,
    #[serde(alias = "binding_proof_sha256")]
    binding_proof_hash: Option<String>,
    anonymous_membership_proof: Option<zk_credential::AnonymousCredentialProofBundle>,
}

impl From<&AgentAnnouncement> for AgentAnnouncementPayload {
    fn from(value: &AgentAnnouncement) -> Self {
        Self {
            peer_id: value.peer_id.clone(),
            did: value.did.clone(),
            name: value.name.clone(),
            description: value.description.clone(),
            capabilities: value.capabilities.clone(),
            capability_specs: value.capability_specs.clone(),
            multiaddrs: value.multiaddrs.clone(),
            protocols: value.protocols.clone(),
            version: value.version.clone(),
            timestamp: value.timestamp,
            ttl: value.ttl,
            did_document: value.did_document.clone(),
            evm_address: value.evm_address.clone(),
            binding_proof_hash: value.binding_proof_hash.clone(),
            anonymous_membership_proof: value.anonymous_membership_proof.clone(),
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

pub fn capability_routing_key(domain: &CapabilityDomain) -> RecordKey {
    RecordKey::new(&format!(
        "/agenthalo/cap/{}/{}",
        domain.path, domain.schema_version
    ))
}

fn capability_routing_keys(domain: &CapabilityDomain) -> Vec<RecordKey> {
    let segments = domain.path.split('/').collect::<Vec<_>>();
    (1..=segments.len())
        .map(|idx| CapabilityDomain::new(segments[..idx].join("/"), domain.schema_version))
        .map(|prefix| capability_routing_key(&prefix))
        .collect()
}

fn agent_capability_key(did: &str) -> RecordKey {
    RecordKey::new(&format!("/agenthalo/agent-caps/{did}"))
}

fn capability_kad_key(capability_id: &str, did: &str) -> RecordKey {
    RecordKey::new(&format!("/agenthalo/capability/{capability_id}/{did}"))
}

static CREDENTIAL_KEYS: OnceLock<zk_credential::CredentialKeypair> = OnceLock::new();

fn credential_keys() -> Result<&'static zk_credential::CredentialKeypair, String> {
    if let Some(keys) = CREDENTIAL_KEYS.get() {
        return Ok(keys);
    }
    let keys = zk_credential::setup_credential_circuit()?;
    let _ = CREDENTIAL_KEYS.set(keys);
    CREDENTIAL_KEYS
        .get()
        .ok_or_else(|| "credential proving keys unavailable".to_string())
}

fn did_hash_for_zk_membership(did: &str) -> String {
    hex_encode(digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, did.as_bytes()).as_slice())
}

fn bounded_announcement_ttl(ttl_secs: u64) -> u64 {
    ttl_secs.min(MAX_ANNOUNCEMENT_TTL_SECS)
}

fn sanitize_live_metrics(metrics: &mut LiveMetrics) {
    metrics.sanitize();
}

fn compare_ranked_specs(
    left_spec: Option<&CapabilitySpec>,
    right_spec: Option<&CapabilitySpec>,
    now: u64,
    attestation_max_age_secs: u64,
) -> std::cmp::Ordering {
    let left_attestations = left_spec
        .map(|spec| spec.verified_attestation_count(now, attestation_max_age_secs))
        .unwrap_or(0);
    let right_attestations = right_spec
        .map(|spec| spec.verified_attestation_count(now, attestation_max_age_secs))
        .unwrap_or(0);
    let left_success = left_spec
        .map(|spec| normalized_success_rate(spec.metrics.success_rate))
        .unwrap_or(0.0);
    let right_success = right_spec
        .map(|spec| normalized_success_rate(spec.metrics.success_rate))
        .unwrap_or(0.0);
    let left_latency = left_spec
        .map(|spec| spec.metrics.latency_p99_ms)
        .unwrap_or(u64::MAX);
    let right_latency = right_spec
        .map(|spec| spec.metrics.latency_p99_ms)
        .unwrap_or(u64::MAX);
    let left_cost = left_spec
        .map(|spec| spec.metrics.cost_microdollars)
        .unwrap_or(u64::MAX);
    let right_cost = right_spec
        .map(|spec| spec.metrics.cost_microdollars)
        .unwrap_or(u64::MAX);

    right_attestations
        .cmp(&left_attestations)
        .then_with(|| right_success.total_cmp(&left_success))
        .then_with(|| left_latency.cmp(&right_latency))
        .then_with(|| left_cost.cmp(&right_cost))
}

fn dht_record_expiry(ttl_secs: u64) -> Option<Instant> {
    let ttl_secs = bounded_announcement_ttl(ttl_secs).max(30);
    Some(Instant::now() + Duration::from_secs(ttl_secs))
}

fn verify_optional_anonymous_membership(announcement: &AgentAnnouncement) -> Result<(), String> {
    let Some(bundle) = announcement.anonymous_membership_proof.as_ref() else {
        return Ok(());
    };

    let expected_leaf = did_hash_for_zk_membership(&announcement.did);
    if bundle.leaf_did_hash != expected_leaf {
        return Err(format!(
            "anonymous membership proof leaf hash does not match announcement DID `{}`",
            announcement.did
        ));
    }

    let keys = credential_keys()?;
    let ok = zk_credential::verify_anonymous_membership_proof(&keys.1, bundle)?;
    if !ok {
        return Err("anonymous membership proof verification failed".to_string());
    }

    Ok(())
}

#[derive(Clone, Debug)]
pub struct AgentDiscovery {
    known_agents: HashMap<String, AgentAnnouncement>,
    subscribed_topics: HashSet<String>,
    gossip_privacy: GossipPrivacy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GossipPrivacy {
    /// Include listen addresses in gossipsub announcements.
    Full,
    /// Omit listen addresses from gossipsub; resolve them via DHT records.
    AddressesViaDhtOnly,
}

impl Default for AgentDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentDiscovery {
    pub fn new() -> Self {
        Self::with_gossip_privacy(GossipPrivacy::AddressesViaDhtOnly)
    }

    pub fn with_gossip_privacy(gossip_privacy: GossipPrivacy) -> Self {
        Self {
            known_agents: HashMap::new(),
            subscribed_topics: HashSet::new(),
            gossip_privacy,
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
        if !is_allowed_capability_topic(topic) {
            return Err(format!(
                "refusing subscription to invalid capability topic `{topic}`"
            ));
        }
        if !self.subscribed_topics.contains(topic)
            && self.subscribed_topics.len() >= MAX_DYNAMIC_SUBSCRIPTIONS
        {
            return Err(format!(
                "refusing to subscribe beyond {MAX_DYNAMIC_SUBSCRIPTIONS} capability topics"
            ));
        }
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
        let gossip_announcement = self.prepare_gossip_announcement(announcement);
        let payload = serde_json::to_vec(&gossip_announcement)
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
        let value = self.serialize_dht_announcement(announcement)?;
        let expiry = dht_record_expiry(announcement.ttl);
        let record = Record {
            key: announcement_kad_key(&announcement.did),
            value: value.clone(),
            publisher: None,
            expires: expiry,
        };
        kademlia
            .put_record(record, Quorum::Majority)
            .map_err(|e| format!("DHT put_record failed: {e}"))?;
        let capability_ids = announcement.capability_ids();
        let capability_index = serde_json::to_vec(&capability_ids)
            .map_err(|e| format!("serialize agent capability index: {e}"))?;
        kademlia
            .put_record(
                Record {
                    key: agent_capability_key(&announcement.did),
                    value: capability_index,
                    publisher: None,
                    expires: expiry,
                },
                Quorum::Majority,
            )
            .map_err(|e| format!("DHT put_record for agent capability index failed: {e}"))?;
        for capability_id in announcement.capability_ids() {
            let record = Record {
                key: capability_kad_key(&capability_id, &announcement.did),
                value: value.clone(),
                publisher: None,
                expires: expiry,
            };
            kademlia.put_record(record, Quorum::Majority).map_err(|e| {
                format!("DHT put_record for capability `{capability_id}` failed: {e}")
            })?;
        }
        for spec in &announcement.capability_specs {
            for key in capability_routing_keys(&spec.domain) {
                kademlia
                    .put_record(
                        Record {
                            key,
                            value: value.clone(),
                            publisher: None,
                            expires: expiry,
                        },
                        Quorum::Majority,
                    )
                    .map_err(|e| {
                        format!(
                            "DHT put_record for capability domain `{}` failed: {e}",
                            spec.domain.path
                        )
                    })?;
            }
        }
        Ok(())
    }

    pub fn lookup_by_did(&self, did: &str, kademlia: &mut Kademlia<MemoryStore>) {
        kademlia.get_record(announcement_kad_key(did));
    }

    pub fn lookup_capability_provider(
        &self,
        capability_id: &str,
        did: &str,
        kademlia: &mut Kademlia<MemoryStore>,
    ) {
        kademlia.get_record(capability_kad_key(capability_id, did));
    }

    pub fn lookup_domain_prefix(
        &self,
        domain_prefix: &str,
        schema_version: u32,
        kademlia: &mut Kademlia<MemoryStore>,
    ) {
        kademlia.get_record(capability_routing_key(&CapabilityDomain::new(
            domain_prefix,
            schema_version,
        )));
    }

    fn prepare_gossip_announcement(&self, announcement: &AgentAnnouncement) -> AgentAnnouncement {
        match self.gossip_privacy {
            GossipPrivacy::Full => announcement.clone(),
            GossipPrivacy::AddressesViaDhtOnly => {
                let mut stripped = announcement.clone();
                stripped.multiaddrs.clear();
                stripped
            }
        }
    }

    fn serialize_dht_announcement(
        &self,
        announcement: &AgentAnnouncement,
    ) -> Result<Vec<u8>, String> {
        serde_json::to_vec(announcement).map_err(|e| format!("serialize announcement for DHT: {e}"))
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
        if record.value.len() > MAX_ANNOUNCEMENT_BYTES {
            return Err(format!(
                "DHT announcement exceeds max size ({MAX_ANNOUNCEMENT_BYTES} bytes)"
            ));
        }
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
        if data.len() > MAX_ANNOUNCEMENT_BYTES {
            return Err(format!(
                "gossipsub announcement exceeds max size ({MAX_ANNOUNCEMENT_BYTES} bytes)"
            ));
        }
        let announcement: AgentAnnouncement = serde_json::from_slice(data)
            .map_err(|e| format!("decode gossipsub message as announcement: {e}"))?;
        self.verify_and_upsert(announcement, resolve_document)
    }

    fn sanitize_attestations<F>(&self, announcement: &mut AgentAnnouncement, resolve_document: &F)
    where
        F: Fn(&str) -> Option<DIDDocument>,
    {
        let subject_did = announcement.did.clone();
        for spec in &mut announcement.capability_specs {
            let capability_id = spec.capability_id.clone();
            spec.attestations.retain(|attestation| {
                if !attestation.passed
                    || attestation.capability_id != capability_id
                    || attestation.challenge_hash.is_empty()
                    || attestation.attester_did == attestation.subject_did
                    || attestation.ed25519_signature.is_empty()
                    || attestation.mldsa65_signature.is_empty()
                    || attestation.subject_did != subject_did
                {
                    return false;
                }
                let attester_document = self
                    .known_agents
                    .get(&attestation.attester_did)
                    .and_then(|known| known.did_document.clone())
                    .or_else(|| resolve_document(&attestation.attester_did));
                let Some(attester_document) = attester_document else {
                    return false;
                };
                verify_capability_attestation(attestation, &attester_document).unwrap_or(false)
            });
        }
    }

    fn sanitize_metrics(&self, announcement: &mut AgentAnnouncement) {
        for spec in &mut announcement.capability_specs {
            sanitize_live_metrics(&mut spec.metrics);
        }
    }

    fn validate_announcement_shape(&self, announcement: &AgentAnnouncement) -> Result<(), String> {
        if announcement.capability_specs.len() > MAX_ANNOUNCED_CAPABILITY_SPECS {
            return Err(format!(
                "announcement for `{}` exceeds max capability_specs ({MAX_ANNOUNCED_CAPABILITY_SPECS})",
                announcement.did
            ));
        }
        for spec in &announcement.capability_specs {
            if spec.attestations.len() > MAX_ATTESTATIONS_PER_SPEC {
                return Err(format!(
                    "announcement for `{}` exceeds max attestations per capability ({MAX_ATTESTATIONS_PER_SPEC})",
                    announcement.did
                ));
            }
        }
        Ok(())
    }

    fn validate_announcement_freshness(
        &self,
        announcement: &AgentAnnouncement,
    ) -> Result<(), String> {
        let now = now_unix();
        if announcement.timestamp > now.saturating_add(MAX_ANNOUNCEMENT_CLOCK_SKEW_SECS) {
            return Err(format!(
                "announcement for `{}` is too far in the future",
                announcement.did
            ));
        }
        let effective_ttl = bounded_announcement_ttl(announcement.ttl);
        if now.saturating_sub(announcement.timestamp) > effective_ttl {
            return Err(format!(
                "announcement for `{}` expired before ingress verification",
                announcement.did
            ));
        }
        Ok(())
    }

    fn verify_and_upsert<F>(
        &mut self,
        mut announcement: AgentAnnouncement,
        resolve_document: F,
    ) -> Result<AgentAnnouncement, String>
    where
        F: Fn(&str) -> Option<DIDDocument>,
    {
        self.validate_announcement_shape(&announcement)?;
        self.validate_announcement_freshness(&announcement)?;
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
        verify_optional_anonymous_membership(&announcement)?;
        announcement.ttl = bounded_announcement_ttl(announcement.ttl);
        self.sanitize_metrics(&mut announcement);
        self.sanitize_attestations(&mut announcement, &resolve_document);
        self.upsert_verified(announcement.clone());
        Ok(announcement)
    }

    pub fn upsert_verified(&mut self, mut announcement: AgentAnnouncement) {
        if self.validate_announcement_shape(&announcement).is_err() {
            return;
        }
        announcement.ttl = bounded_announcement_ttl(announcement.ttl);
        self.sanitize_metrics(&mut announcement);
        self.sanitize_attestations(&mut announcement, &|_| None);
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
                    || announcement
                        .capability_specs
                        .iter()
                        .any(|capability| capability.capability_id == capability_id)
            })
            .cloned()
            .collect()
    }

    pub fn best_capability_match<'a>(
        &self,
        announcement: &'a AgentAnnouncement,
        query: &CapabilityQuery,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> Option<&'a CapabilitySpec> {
        announcement
            .capability_specs
            .iter()
            .find(|spec| spec.satisfies_at(query, now, attestation_max_age_secs))
    }

    pub fn find_by_query(
        &self,
        query: &CapabilityQuery,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> Vec<AgentAnnouncement> {
        self.known_agents
            .values()
            .filter(|announcement| {
                self.best_capability_match(announcement, query, now, attestation_max_age_secs)
                    .is_some()
            })
            .cloned()
            .collect()
    }

    pub fn query_capabilities(
        &mut self,
        query: &CapabilityQuery,
        now: u64,
        attestation_max_age_secs: u64,
        kademlia: &mut Kademlia<MemoryStore>,
        gossipsub: &mut gossipsub::Behaviour,
    ) -> Vec<AgentAnnouncement> {
        let mut matches = self.find_by_query(query, now, attestation_max_age_secs);
        matches.sort_by(|left, right| {
            let left_spec = self.best_capability_match(left, query, now, attestation_max_age_secs);
            let right_spec =
                self.best_capability_match(right, query, now, attestation_max_age_secs);
            compare_ranked_specs(left_spec, right_spec, now, attestation_max_age_secs)
                .then_with(|| left.did.cmp(&right.did))
                .then_with(|| left.peer_id.cmp(&right.peer_id))
        });

        if matches.len() >= query.count as usize {
            matches.truncate(query.count as usize);
            return matches;
        }

        self.lookup_domain_prefix(&query.domain_prefix, 1, kademlia);

        let domain_topic =
            dynamic_topic_for_domain(&CapabilityDomain::new(&query.domain_prefix, 1));
        if !self.is_subscribed(&domain_topic) {
            let _ = self.subscribe(&domain_topic, gossipsub);
        }
        let query_payload = serde_json::to_vec(query).unwrap_or_default();
        let _ = gossipsub.publish(IdentTopic::new(query.topic()), query_payload);

        matches
    }

    pub fn prune_expired(&mut self) {
        let now = now_unix();
        self.known_agents.retain(|_, announcement| {
            now <= announcement.timestamp.saturating_add(announcement.ttl)
        });
    }
}

pub fn hash_did_for_membership(did: &str) -> String {
    did_hash_for_zk_membership(did)
}

impl AgentAnnouncement {
    pub fn capability_ids(&self) -> Vec<String> {
        let mut ids = self
            .capability_specs
            .iter()
            .map(|capability| capability.capability_id.clone())
            .collect::<Vec<_>>();
        for capability in &self.capabilities {
            if !ids.iter().any(|id| id == &capability.id) {
                ids.push(capability.id.clone());
            }
        }
        ids
    }

    pub fn topics(&self) -> Vec<String> {
        let mut topics = vec![topic_general()];
        for spec in &self.capability_specs {
            let topic = spec.topic();
            if !topics.iter().any(|existing| existing == &topic) {
                topics.push(topic);
            }
        }
        topics
    }
}

pub fn announcement_for_identity(
    identity: &DIDIdentity,
    peer_id: PeerId,
    capabilities: Vec<AgentCapability>,
    multiaddrs: Vec<String>,
) -> AgentAnnouncement {
    let capability_specs = capabilities
        .iter()
        .map(AgentCapability::to_capability_spec)
        .collect::<Vec<_>>();
    AgentAnnouncement {
        peer_id: peer_id.to_string(),
        did: identity.did.clone(),
        name: "AgentHalo".to_string(),
        description: "Sovereign privacy-preserving agent".to_string(),
        capabilities,
        capability_specs,
        multiaddrs,
        protocols: vec![
            "/agenthalo/didcomm/1.0.0".to_string(),
            "/agenthalo/discovery/1.0.0".to_string(),
        ],
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: now_unix(),
        ttl: 300,
        did_document: Some(identity.did_document.clone()),
        evm_address: None,
        binding_proof_hash: None,
        anonymous_membership_proof: None,
        ed25519_signature: None,
        mldsa65_signature: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pod::acl::{AccessGrant, GrantPermissions};

    fn seed(byte: u8) -> [u8; 64] {
        [byte; 64]
    }

    fn sample_grant(key_pattern: &str, created_at: u64, expires_at: Option<u64>) -> AccessGrant {
        let grantor_puf = [0x31u8; 32];
        let grantee_puf = [0x41u8; 32];
        let nonce = 7u64;
        let grant_id =
            AccessGrant::compute_id(&grantor_puf, &grantee_puf, key_pattern, created_at, nonce);
        AccessGrant {
            grant_id,
            grantor_puf,
            grantee_puf,
            key_pattern: key_pattern.to_string(),
            permissions: GrantPermissions::read_only(),
            expires_at,
            created_at,
            nonce,
            revoked: false,
        }
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
    fn allowed_capability_topics_are_isolated() {
        assert!(is_allowed_capability_topic(&topic_general()));
        assert!(is_allowed_capability_topic(&topic_coding()));
        assert!(is_allowed_capability_topic(&topic_research()));
        assert!(is_allowed_capability_topic(&topic_financial()));
        assert!(is_allowed_capability_topic(&topic_blockchain()));
        assert!(is_allowed_capability_topic(&topic_privacy()));
        assert!(is_allowed_capability_topic(
            "/agenthalo/capabilities/prove/lean/algebra"
        ));
        assert!(!is_allowed_capability_topic(
            "/agenthalo/credentials/general"
        ));
        assert!(!is_allowed_capability_topic(
            "/agenthalo/capabilities/prove/lean/Bad"
        ));
        assert!(!is_allowed_capability_topic(
            "/agenthalo/capabilities/prove//lean"
        ));
        assert!(!is_allowed_capability_topic(&format!(
            "/agenthalo/capabilities/{}",
            ["deep"; 9].join("/")
        )));
        assert!(!is_allowed_capability_topic("general"));
    }

    #[test]
    fn legacy_agent_capability_roundtrip_preserves_domain_path() {
        let spec = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::CoqTerm],
            vec![],
        );
        let legacy = AgentCapability::from(spec.clone());
        let roundtrip = legacy.to_capability_spec();
        assert_eq!(roundtrip.domain.path, spec.domain.path);
        assert_eq!(roundtrip.input_types, spec.input_types);
        assert_eq!(roundtrip.output_types, spec.output_types);
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
            capability_specs: vec![CapabilitySpec::new(
                CapabilityDomain::new("code/generate", 1),
                vec![],
                vec![],
                vec![],
            )],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
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
            capability_specs: vec![CapabilitySpec::new(
                CapabilityDomain::new("research/general", 1),
                vec![],
                vec![],
                vec![],
            )],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });

        let coding = discovery.find_by_capability("coding");
        assert_eq!(coding.len(), 1);
        assert_eq!(coding[0].peer_id, "peer-1");
    }

    #[test]
    fn find_by_query_matches_typed_capability_specs() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-typed".to_string(),
            did: "did:key:z6Typed".to_string(),
            name: "typed".to_string(),
            description: "typed".to_string(),
            capabilities: vec![],
            capability_specs: vec![CapabilitySpec::new(
                CapabilityDomain::new("prove/lean/algebra", 1),
                vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                vec![],
            )],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });

        let matches = discovery.find_by_query(
            &CapabilityQuery {
                domain_prefix: "prove/lean".to_string(),
                required_inputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_outputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_constraints: vec![],
                min_success_rate: None,
                max_latency_p99_ms: None,
                max_cost_microdollars: None,
                min_attestations: None,
                min_onchain_reputation: None,
                count: 1,
                query_timeout_ms: 200,
            },
            now_unix(),
            3600,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].peer_id, "peer-typed");
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

    #[test]
    fn handle_gossipsub_message_accepts_valid_anonymous_membership_proof() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x71)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);

        let grant = sample_grant("results/*", 1_000_000, Some(2_000_000));
        let current_time = 1_500_000u64;
        let (pk, _) = zk_credential::setup_credential_circuit().expect("setup");
        let leaves = vec![
            digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkA"),
            digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, identity.did.as_bytes()),
            digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkC"),
        ];
        let root = zk_credential::merkle_root(&leaves);
        let path = zk_credential::merkle_path(&leaves, 1).expect("path");
        let witness = zk_credential::AnonymousMembershipWitness {
            leaf_did_hash: hex_encode(leaves[1].as_slice()),
            merkle_path: path
                .iter()
                .map(|value| hex_encode(value.as_slice()))
                .collect(),
            merkle_index: 1,
            merkle_root_hash: hex_encode(root.as_slice()),
        };
        let bundle = zk_credential::prove_anonymous_membership(
            &pk,
            &grant,
            &identity.did,
            GrantPermissions::read_only(),
            current_time,
            &witness,
        )
        .expect("prove anonymous membership");
        announcement.anonymous_membership_proof = Some(bundle);
        sign_announcement(&identity, &mut announcement).expect("sign");

        let payload = serde_json::to_vec(&announcement).expect("serialize");
        let mut discovery = AgentDiscovery::new();
        let accepted = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect("accept with valid anonymous membership proof");
        assert_eq!(accepted.did, identity.did);
    }

    #[test]
    fn handle_gossipsub_message_rejects_invalid_anonymous_membership_proof() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x72)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);

        let grant = sample_grant("results/*", 1_000_000, Some(2_000_000));
        let current_time = 1_500_000u64;
        let (pk, _) = zk_credential::setup_credential_circuit().expect("setup");
        let leaves = vec![
            digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkA"),
            digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, identity.did.as_bytes()),
            digest_bytes(ZK_CREDENTIAL_DID_HASH_DOMAIN, b"did:key:z6MkC"),
        ];
        let root = zk_credential::merkle_root(&leaves);
        let path = zk_credential::merkle_path(&leaves, 1).expect("path");
        let witness = zk_credential::AnonymousMembershipWitness {
            leaf_did_hash: hex_encode(leaves[1].as_slice()),
            merkle_path: path
                .iter()
                .map(|value| hex_encode(value.as_slice()))
                .collect(),
            merkle_index: 1,
            merkle_root_hash: hex_encode(root.as_slice()),
        };
        let mut bundle = zk_credential::prove_anonymous_membership(
            &pk,
            &grant,
            &identity.did,
            GrantPermissions::read_only(),
            current_time,
            &witness,
        )
        .expect("prove anonymous membership");
        bundle.membership_commitment_hash.replace_range(0..1, "f");
        announcement.anonymous_membership_proof = Some(bundle);
        sign_announcement(&identity, &mut announcement).expect("sign");

        let payload = serde_json::to_vec(&announcement).expect("serialize");
        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect_err("invalid anonymous membership proof must fail");
        assert!(err.contains("anonymous membership proof verification failed"));
    }

    #[test]
    fn gossip_announce_strips_multiaddrs_in_dht_only_mode() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x73)).expect("identity");
        let announcement = announcement_for_identity(
            &identity,
            PeerId::random(),
            vec![],
            vec!["/ip4/127.0.0.1/tcp/9090".to_string()],
        );
        let discovery = AgentDiscovery::with_gossip_privacy(GossipPrivacy::AddressesViaDhtOnly);
        let gossip = discovery.prepare_gossip_announcement(&announcement);
        assert!(gossip.multiaddrs.is_empty());
        assert_eq!(announcement.multiaddrs.len(), 1);
    }

    #[test]
    fn gossip_announce_preserves_multiaddrs_in_full_mode() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x74)).expect("identity");
        let announcement = announcement_for_identity(
            &identity,
            PeerId::random(),
            vec![],
            vec!["/ip4/127.0.0.1/tcp/9091".to_string()],
        );
        let discovery = AgentDiscovery::with_gossip_privacy(GossipPrivacy::Full);
        let gossip = discovery.prepare_gossip_announcement(&announcement);
        assert_eq!(gossip.multiaddrs, announcement.multiaddrs);
    }

    #[test]
    fn dht_publish_always_includes_multiaddrs() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x75)).expect("identity");
        let announcement = announcement_for_identity(
            &identity,
            PeerId::random(),
            vec![],
            vec!["/ip4/127.0.0.1/tcp/9092".to_string()],
        );

        let dht_only = AgentDiscovery::with_gossip_privacy(GossipPrivacy::AddressesViaDhtOnly);
        let full = AgentDiscovery::with_gossip_privacy(GossipPrivacy::Full);

        let dht_only_payload = dht_only
            .serialize_dht_announcement(&announcement)
            .expect("serialize dht payload");
        let full_payload = full
            .serialize_dht_announcement(&announcement)
            .expect("serialize dht payload");
        let dht_only_ann: AgentAnnouncement =
            serde_json::from_slice(&dht_only_payload).expect("decode dht only payload");
        let full_ann: AgentAnnouncement =
            serde_json::from_slice(&full_payload).expect("decode full payload");

        assert_eq!(dht_only_ann.multiaddrs, announcement.multiaddrs);
        assert_eq!(full_ann.multiaddrs, announcement.multiaddrs);
    }

    #[test]
    fn discovery_strips_forged_capability_attestations_on_ingest() {
        let subject = crate::halo::did::did_from_genesis_seed(&seed(0x76)).expect("subject");
        let mut announcement =
            announcement_for_identity(&subject, PeerId::random(), vec![], vec![]);
        let mut spec = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![],
        );
        spec.attestations
            .push(crate::halo::capability_spec::CapabilityAttestation {
                attester_did: "did:key:mallory".to_string(),
                subject_did: subject.did.clone(),
                capability_id: spec.capability_id.clone(),
                challenge_hash: "forged".to_string(),
                passed: true,
                verified_at: now_unix(),
                ed25519_signature: vec![0u8; 64],
                mldsa65_signature: vec![0u8; 128],
            });
        announcement.capability_specs = vec![spec];
        sign_announcement(&subject, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let accepted = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect("signed announcement");
        assert!(accepted.capability_specs[0].attestations.is_empty());
    }

    #[test]
    fn discovery_keeps_valid_capability_attestations_on_ingest() {
        let attester = crate::halo::did::did_from_genesis_seed(&seed(0x77)).expect("attester");
        let subject = crate::halo::did::did_from_genesis_seed(&seed(0x78)).expect("subject");

        let mut attester_announcement =
            announcement_for_identity(&attester, PeerId::random(), vec![], vec![]);
        sign_announcement(&attester, &mut attester_announcement).expect("sign attester");
        let attester_payload = serde_json::to_vec(&attester_announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        discovery
            .handle_gossipsub_message(&attester_payload, |_did| None)
            .expect("ingest attester");

        let mut subject_announcement =
            announcement_for_identity(&subject, PeerId::random(), vec![], vec![]);
        let mut spec = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![],
        );
        let attestation = crate::halo::capability_verification::attest_capability(
            &attester,
            &subject.did,
            &spec.capability_id,
            "challenge-ok",
            true,
            now_unix(),
        )
        .expect("attest capability");
        spec.attestations.push(attestation);
        subject_announcement.capability_specs = vec![spec];
        sign_announcement(&subject, &mut subject_announcement).expect("sign subject");
        let subject_payload = serde_json::to_vec(&subject_announcement).expect("serialize");

        let accepted = discovery
            .handle_gossipsub_message(&subject_payload, |_did| None)
            .expect("ingest subject");
        assert_eq!(accepted.capability_specs[0].attestations.len(), 1);
    }

    #[test]
    fn capability_ids_include_typed_and_legacy_forms_without_duplicates() {
        let announcement = AgentAnnouncement {
            peer_id: "peer".to_string(),
            did: "did:key:z6cap".to_string(),
            name: "cap".to_string(),
            description: "cap".to_string(),
            capabilities: vec![AgentCapability {
                id: "legacy-cap".to_string(),
                name: "legacy".to_string(),
                description: "legacy".to_string(),
                input_types: vec![],
                output_types: vec![],
            }],
            capability_specs: vec![CapabilitySpec::new(
                CapabilityDomain::new("prove/lean/algebra", 1),
                vec![],
                vec![],
                vec![],
            )],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        };
        let ids = announcement.capability_ids();
        assert_eq!(ids.len(), 2);
        assert!(ids.iter().any(|id| id == "legacy-cap"));
        assert!(ids.iter().any(|id| id != "legacy-cap"));
    }

    #[test]
    fn announcement_topics_include_general_and_capability_domains() {
        let announcement = AgentAnnouncement {
            peer_id: "peer".to_string(),
            did: "did:key:z6topics".to_string(),
            name: "topics".to_string(),
            description: "topics".to_string(),
            capabilities: vec![],
            capability_specs: vec![CapabilitySpec::new(
                CapabilityDomain::new("prove/lean/algebra", 1),
                vec![],
                vec![],
                vec![],
            )],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        };
        let topics = announcement.topics();
        assert!(topics.iter().any(|topic| topic == &topic_general()));
        assert!(topics
            .iter()
            .any(|topic| topic == "/agenthalo/capabilities/prove/lean/algebra"));
    }

    #[test]
    fn query_capabilities_returns_best_local_matches_first() {
        let mut discovery = AgentDiscovery::new();
        let mut fast = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![],
        );
        fast.metrics.success_rate = 0.99;
        fast.metrics.latency_p99_ms = 50;
        fast.metrics.cost_microdollars = 5;
        fast.attestations = vec![
            crate::halo::capability_spec::CapabilityAttestation {
                attester_did: "did:key:attester-1".to_string(),
                subject_did: "did:key:fast".to_string(),
                capability_id: fast.capability_id.clone(),
                challenge_hash: "h1".to_string(),
                passed: true,
                verified_at: now_unix(),
                ed25519_signature: vec![1],
                mldsa65_signature: vec![1],
            },
            crate::halo::capability_spec::CapabilityAttestation {
                attester_did: "did:key:attester-2".to_string(),
                subject_did: "did:key:fast".to_string(),
                capability_id: fast.capability_id.clone(),
                challenge_hash: "h2".to_string(),
                passed: true,
                verified_at: now_unix(),
                ed25519_signature: vec![1],
                mldsa65_signature: vec![1],
            },
        ];
        let mut slow = fast.clone();
        slow.capability_id = CapabilitySpec::compute_id(
            &CapabilityDomain::new("prove/lean/analysis", 1),
            &slow.input_types,
            &slow.output_types,
            &slow.constraints,
        );
        slow.domain = CapabilityDomain::new("prove/lean/analysis", 1);
        slow.metrics.success_rate = 0.99;
        slow.metrics.latency_p99_ms = 500;
        slow.metrics.cost_microdollars = 50;
        slow.attestations.truncate(1);
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-fast".to_string(),
            did: "did:key:fast".to_string(),
            name: "fast".to_string(),
            description: "fast".to_string(),
            capabilities: vec![],
            capability_specs: vec![fast],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-slow".to_string(),
            did: "did:key:slow".to_string(),
            name: "slow".to_string(),
            description: "slow".to_string(),
            capabilities: vec![],
            capability_specs: vec![slow],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });

        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let config = gossipsub::ConfigBuilder::default()
            .validation_mode(gossipsub::ValidationMode::Strict)
            .build()
            .expect("config");
        let mut gossipsub =
            gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Signed(local_key), config)
                .expect("gossipsub");
        let local_peer = PeerId::random();
        let mut kademlia = Kademlia::new(local_peer, MemoryStore::new(local_peer));

        let matches = discovery.query_capabilities(
            &CapabilityQuery {
                domain_prefix: "prove/lean".to_string(),
                required_inputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_outputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_constraints: vec![],
                min_success_rate: None,
                max_latency_p99_ms: None,
                max_cost_microdollars: None,
                min_attestations: None,
                min_onchain_reputation: None,
                count: 1,
                query_timeout_ms: 200,
            },
            now_unix(),
            3600,
            &mut kademlia,
            &mut gossipsub,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].peer_id, "peer-fast");
    }

    #[test]
    fn query_capabilities_prefers_attested_provider_over_self_reported_metrics() {
        let mut discovery = AgentDiscovery::new();
        let attester_one =
            crate::halo::did::did_from_genesis_seed(&seed(0x81)).expect("attester one");
        let attester_two =
            crate::halo::did::did_from_genesis_seed(&seed(0x82)).expect("attester two");
        let trusted_identity =
            crate::halo::did::did_from_genesis_seed(&seed(0x83)).expect("trusted");
        let flashy_identity = crate::halo::did::did_from_genesis_seed(&seed(0x84)).expect("flashy");
        discovery.upsert_verified(announcement_for_identity(
            &attester_one,
            PeerId::random(),
            vec![],
            vec![],
        ));
        discovery.upsert_verified(announcement_for_identity(
            &attester_two,
            PeerId::random(),
            vec![],
            vec![],
        ));
        let mut trusted = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![],
        );
        trusted.metrics.success_rate = 0.80;
        trusted.metrics.latency_p99_ms = 500;
        trusted.metrics.cost_microdollars = 50;
        trusted.attestations = vec![
            crate::halo::capability_verification::attest_capability(
                &attester_one,
                &trusted_identity.did,
                &trusted.capability_id,
                "h1",
                true,
                now_unix(),
            )
            .expect("attestation 1"),
            crate::halo::capability_verification::attest_capability(
                &attester_two,
                &trusted_identity.did,
                &trusted.capability_id,
                "h2",
                true,
                now_unix(),
            )
            .expect("attestation 2"),
        ];
        let mut flashy = trusted.clone();
        flashy.metrics.success_rate = 1.0;
        flashy.metrics.latency_p99_ms = 1;
        flashy.metrics.cost_microdollars = 0;
        flashy.attestations.clear();
        let mut trusted_announcement =
            announcement_for_identity(&trusted_identity, PeerId::random(), vec![], vec![]);
        trusted_announcement.capability_specs = vec![trusted];
        discovery.upsert_verified(trusted_announcement);
        let mut flashy_announcement =
            announcement_for_identity(&flashy_identity, PeerId::random(), vec![], vec![]);
        flashy_announcement.capability_specs = vec![flashy];
        discovery.upsert_verified(flashy_announcement);

        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let config = gossipsub::ConfigBuilder::default()
            .validation_mode(gossipsub::ValidationMode::Strict)
            .build()
            .expect("config");
        let mut gossipsub =
            gossipsub::Behaviour::new(gossipsub::MessageAuthenticity::Signed(local_key), config)
                .expect("gossipsub");
        let local_peer = PeerId::random();
        let mut kademlia = Kademlia::new(local_peer, MemoryStore::new(local_peer));

        let matches = discovery.query_capabilities(
            &CapabilityQuery {
                domain_prefix: "prove/lean".to_string(),
                required_inputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_outputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_constraints: vec![],
                min_success_rate: None,
                max_latency_p99_ms: None,
                max_cost_microdollars: None,
                min_attestations: None,
                min_onchain_reputation: None,
                count: 1,
                query_timeout_ms: 200,
            },
            now_unix(),
            3600,
            &mut kademlia,
            &mut gossipsub,
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].did, trusted_identity.did);
    }

    #[test]
    fn find_by_query_does_not_match_legacy_capability_without_typed_spec() {
        let mut discovery = AgentDiscovery::new();
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-legacy".to_string(),
            did: "did:key:legacy".to_string(),
            name: "legacy".to_string(),
            description: "legacy".to_string(),
            capabilities: vec![AgentCapability {
                id: "legacy-1".to_string(),
                name: "prove/lean/fake".to_string(),
                description: "legacy".to_string(),
                input_types: vec!["text/plain".to_string()],
                output_types: vec!["text/plain".to_string()],
            }],
            capability_specs: vec![],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });

        let matches = discovery.find_by_query(
            &CapabilityQuery {
                domain_prefix: "prove/lean".to_string(),
                required_inputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_outputs: vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                required_constraints: vec![],
                min_success_rate: None,
                max_latency_p99_ms: None,
                max_cost_microdollars: None,
                min_attestations: None,
                min_onchain_reputation: None,
                count: 1,
                query_timeout_ms: 200,
            },
            now_unix(),
            3600,
        );
        assert!(matches.is_empty());
    }

    #[test]
    fn handle_gossipsub_message_rejects_stale_announcement() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x79)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        announcement.timestamp = 1;
        announcement.ttl = 60;
        sign_announcement(&identity, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect_err("stale announcement must fail");
        assert!(err.contains("expired before ingress verification"));
    }

    #[test]
    fn handle_gossipsub_message_rejects_oversized_capability_catalog() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x7A)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        announcement.capability_specs = (0..=MAX_ANNOUNCED_CAPABILITY_SPECS)
            .map(|idx| {
                CapabilitySpec::new(
                    CapabilityDomain::new(format!("prove/lean/{idx}"), 1),
                    vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                    vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
                    vec![],
                )
            })
            .collect();
        sign_announcement(&identity, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect_err("oversized announcement must fail");
        assert!(err.contains("exceeds max capability_specs"));
    }

    #[test]
    fn handle_gossipsub_message_rejects_oversized_attestation_sets() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x7C)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        let mut spec = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![],
        );
        spec.attestations = (0..=MAX_ATTESTATIONS_PER_SPEC)
            .map(|idx| crate::halo::capability_spec::CapabilityAttestation {
                attester_did: format!("did:key:attester-{idx}"),
                subject_did: identity.did.clone(),
                capability_id: spec.capability_id.clone(),
                challenge_hash: format!("h{idx}"),
                passed: true,
                verified_at: now_unix(),
                ed25519_signature: vec![1],
                mldsa65_signature: vec![2],
            })
            .collect();
        announcement.capability_specs = vec![spec];
        sign_announcement(&identity, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect_err("oversized attestation set must fail");
        assert!(err.contains("exceeds max attestations per capability"));
    }

    #[test]
    fn handle_gossipsub_message_rejects_oversized_payload_before_deserializing() {
        let mut discovery = AgentDiscovery::new();
        let err = discovery
            .handle_gossipsub_message(&vec![b'x'; MAX_ANNOUNCEMENT_BYTES + 1], |_did| None)
            .expect_err("oversized payload must fail");
        assert!(err.contains("exceeds max size"));
    }

    #[test]
    fn ingest_clamps_announcement_ttl_to_maximum() {
        let identity = crate::halo::did::did_from_genesis_seed(&seed(0x7B)).expect("identity");
        let mut announcement =
            announcement_for_identity(&identity, PeerId::random(), vec![], vec![]);
        announcement.ttl = u64::MAX;
        sign_announcement(&identity, &mut announcement).expect("sign");
        let payload = serde_json::to_vec(&announcement).expect("serialize");

        let mut discovery = AgentDiscovery::new();
        let accepted = discovery
            .handle_gossipsub_message(&payload, |_did| None)
            .expect("verified gossip");
        assert_eq!(accepted.ttl, MAX_ANNOUNCEMENT_TTL_SECS);
        assert_eq!(
            discovery
                .known_agents()
                .get(&identity.did)
                .expect("stored")
                .ttl,
            MAX_ANNOUNCEMENT_TTL_SECS
        );
    }

    #[test]
    fn upsert_verified_sanitizes_nan_success_rate() {
        let mut discovery = AgentDiscovery::new();
        let mut spec = CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![crate::halo::capability_spec::TypeSpec::LeanTerm],
            vec![],
        );
        spec.metrics.success_rate = f64::NAN;
        discovery.upsert_verified(AgentAnnouncement {
            peer_id: "peer-nan".to_string(),
            did: "did:key:nan".to_string(),
            name: "nan".to_string(),
            description: "nan".to_string(),
            capabilities: vec![],
            capability_specs: vec![spec],
            multiaddrs: vec![],
            protocols: vec![],
            version: "1".to_string(),
            timestamp: now_unix(),
            ttl: 60,
            did_document: None,
            evm_address: None,
            binding_proof_hash: None,
            anonymous_membership_proof: None,
            ed25519_signature: None,
            mldsa65_signature: None,
        });
        let stored = discovery
            .known_agents()
            .get("did:key:nan")
            .and_then(|announcement| announcement.capability_specs.first())
            .expect("stored spec");
        assert_eq!(stored.metrics.success_rate, 0.0);
    }
}
