use crate::halo::capability_spec::{
    CapabilityAttestation, CapabilityConstraint, CapabilitySpec, TypeSpec,
};
use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::util::{digest_bytes, hex_encode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet, VecDeque};

const CHALLENGE_HASH_DOMAIN: &str = "agenthalo.capability.challenge.v1";
const ATTESTATION_HASH_DOMAIN: &str = "agenthalo.capability.attestation.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChallengeKind {
    DeterministicText,
    DeterministicJson,
    TypeOnly,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum ChallengeInput {
    None,
    Text(String),
    Json(Value),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityChallenge {
    pub challenge_id: String,
    pub capability_id: String,
    pub kind: ChallengeKind,
    pub input: ChallengeInput,
    pub expected_output: Value,
    pub issued_at: u64,
    pub expires_at: u64,
    pub nonce: String,
    pub required_constraints: Vec<CapabilityConstraint>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChallengeResponse {
    pub output_type: TypeSpec,
    pub payload: Value,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub completed_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChallengeResult {
    pub passed: bool,
    pub challenge_hash: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DomainChallengeBank {
    templates: HashMap<String, Vec<ChallengeKind>>,
}

impl DomainChallengeBank {
    pub fn with_defaults() -> Self {
        let mut bank = Self::default();
        bank.register("prove/lean", vec![ChallengeKind::DeterministicText]);
        bank.register("translate", vec![ChallengeKind::DeterministicJson]);
        bank.register("analyze", vec![ChallengeKind::TypeOnly]);
        bank
    }

    pub fn register(&mut self, domain_prefix: &str, templates: Vec<ChallengeKind>) {
        self.templates.insert(domain_prefix.to_string(), templates);
    }

    pub fn issue_for_spec(
        &self,
        spec: &CapabilitySpec,
        now: u64,
        ttl_secs: u64,
    ) -> CapabilityChallenge {
        let kind = self
            .templates
            .iter()
            .find(|(prefix, _)| spec.domain.matches_prefix(prefix))
            .and_then(|(_, templates)| templates.first().cloned())
            .unwrap_or_else(|| default_kind_for_spec(spec));
        issue_challenge(spec, kind, now, ttl_secs)
    }
}

fn default_kind_for_spec(spec: &CapabilitySpec) -> ChallengeKind {
    match spec.output_types.first() {
        Some(TypeSpec::JsonSchema { .. }) => ChallengeKind::DeterministicJson,
        Some(TypeSpec::Text { .. }) | Some(TypeSpec::LeanTerm) | Some(TypeSpec::CoqTerm) => {
            ChallengeKind::DeterministicText
        }
        _ => ChallengeKind::TypeOnly,
    }
}

fn challenge_nonce(spec: &CapabilitySpec, now: u64) -> String {
    let raw = format!("{}:{now}:{}", spec.capability_id, spec.domain.path);
    hex_encode(&digest_bytes(CHALLENGE_HASH_DOMAIN, raw.as_bytes()))
}

pub fn issue_challenge(
    spec: &CapabilitySpec,
    kind: ChallengeKind,
    now: u64,
    ttl_secs: u64,
) -> CapabilityChallenge {
    let nonce = challenge_nonce(spec, now);
    let expected_output = match kind {
        ChallengeKind::DeterministicText => {
            Value::String(format!("capability:{}:{}", spec.capability_id, nonce))
        }
        ChallengeKind::DeterministicJson => serde_json::json!({
            "capability_id": spec.capability_id,
            "nonce": nonce,
            "ok": true,
        }),
        ChallengeKind::TypeOnly => Value::Null,
    };
    let input = match kind {
        ChallengeKind::DeterministicText => {
            ChallengeInput::Text(format!("solve:{}", spec.domain.path))
        }
        ChallengeKind::DeterministicJson => {
            ChallengeInput::Json(serde_json::json!({"domain": spec.domain.path}))
        }
        ChallengeKind::TypeOnly => ChallengeInput::None,
    };
    let mut challenge = CapabilityChallenge {
        challenge_id: String::new(),
        capability_id: spec.capability_id.clone(),
        kind,
        input,
        expected_output,
        issued_at: now,
        expires_at: now.saturating_add(ttl_secs),
        nonce,
        required_constraints: spec.constraints.clone(),
    };
    challenge.challenge_id = challenge_hash(&challenge);
    challenge
}

pub fn challenge_hash(challenge: &CapabilityChallenge) -> String {
    let raw = serde_json::to_vec(&serde_json::json!({
        "capability_id": challenge.capability_id,
        "kind": challenge.kind,
        "input": challenge.input,
        "expected_output": challenge.expected_output,
        "issued_at": challenge.issued_at,
        "expires_at": challenge.expires_at,
        "nonce": challenge.nonce,
        "required_constraints": challenge.required_constraints,
    }))
    .unwrap_or_default();
    hex_encode(&digest_bytes(CHALLENGE_HASH_DOMAIN, &raw))
}

pub fn verify_challenge_response(
    spec: &CapabilitySpec,
    challenge: &CapabilityChallenge,
    response: &ChallengeResponse,
) -> ChallengeResult {
    let mut reasons = Vec::new();
    if challenge.capability_id != spec.capability_id {
        reasons.push("challenge capability_id does not match spec".to_string());
    }
    if response.completed_at > challenge.expires_at {
        reasons.push("challenge response expired".to_string());
    }
    if !spec
        .output_types
        .iter()
        .any(|output_type| output_type == &response.output_type)
    {
        reasons.push("response type is not declared by capability spec".to_string());
    }

    match challenge.kind {
        ChallengeKind::DeterministicText => {
            if response.payload != challenge.expected_output {
                reasons.push("deterministic text challenge output mismatch".to_string());
            }
        }
        ChallengeKind::DeterministicJson => {
            if response.payload != challenge.expected_output {
                reasons.push("deterministic json challenge output mismatch".to_string());
            }
        }
        ChallengeKind::TypeOnly => {
            if !payload_matches_type(&response.payload, &response.output_type) {
                reasons.push("response payload does not match declared output type".to_string());
            }
        }
    }

    for constraint in &challenge.required_constraints {
        if let Some(reason) = constraint_violation(constraint, challenge, response) {
            reasons.push(reason);
        }
    }

    ChallengeResult {
        passed: reasons.is_empty(),
        challenge_hash: challenge_hash(challenge),
        reasons,
    }
}

fn payload_matches_type(payload: &Value, ty: &TypeSpec) -> bool {
    match ty {
        TypeSpec::Bytes { .. } => payload.as_str().is_some(),
        TypeSpec::JsonSchema { .. } => payload.is_object() || payload.is_array(),
        TypeSpec::LeanTerm | TypeSpec::CoqTerm => payload.as_str().is_some(),
        TypeSpec::Text { .. } => payload.as_str().is_some(),
        TypeSpec::Vector { dimensions } => payload
            .as_array()
            .map(|items| items.len() == *dimensions as usize && items.iter().all(|v| v.is_number()))
            .unwrap_or(false),
    }
}

fn constraint_violation(
    constraint: &CapabilityConstraint,
    challenge: &CapabilityChallenge,
    response: &ChallengeResponse,
) -> Option<String> {
    match constraint {
        CapabilityConstraint::KernelFaithful { kernel } => {
            let actual = response
                .metadata
                .get("kernel")
                .map(String::as_str)
                .unwrap_or("");
            if actual == kernel {
                None
            } else {
                Some(format!("kernel mismatch: expected {kernel}, got {actual}"))
            }
        }
        CapabilityConstraint::ZeroSorry => {
            let text = response
                .payload
                .as_str()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if text.contains("sorry") || text.contains("admit") {
                Some("response violated ZeroSorry constraint".to_string())
            } else {
                None
            }
        }
        CapabilityConstraint::MaxLatencyMs(max_ms) => {
            let elapsed_ms = response
                .completed_at
                .saturating_sub(challenge.issued_at)
                .saturating_mul(1000);
            if elapsed_ms <= *max_ms {
                None
            } else {
                Some(format!("response exceeded MaxLatencyMs({max_ms})"))
            }
        }
        CapabilityConstraint::RequiresIndex { index_name } => {
            let indexes = response
                .metadata
                .get("indexes")
                .map(String::as_str)
                .unwrap_or("");
            if indexes.split(',').any(|index| index.trim() == index_name) {
                None
            } else {
                Some(format!("required index `{index_name}` missing"))
            }
        }
        CapabilityConstraint::Custom { key, value } => {
            let actual = response.metadata.get(key).map(String::as_str).unwrap_or("");
            if actual == value {
                None
            } else {
                Some(format!("custom constraint `{key}` mismatch"))
            }
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct CapabilityAttestationPayload<'a> {
    attester_did: &'a str,
    subject_did: &'a str,
    capability_id: &'a str,
    challenge_hash: &'a str,
    passed: bool,
    verified_at: u64,
}

fn attestation_payload_bytes(
    attester_did: &str,
    subject_did: &str,
    capability_id: &str,
    challenge_hash: &str,
    passed: bool,
    verified_at: u64,
) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&CapabilityAttestationPayload {
        attester_did,
        subject_did,
        capability_id,
        challenge_hash,
        passed,
        verified_at,
    })
    .map_err(|e| format!("serialize capability attestation payload: {e}"))
}

pub fn attest_capability(
    attester_identity: &DIDIdentity,
    subject_did: &str,
    capability_id: &str,
    challenge_hash: &str,
    passed: bool,
    verified_at: u64,
) -> Result<CapabilityAttestation, String> {
    let payload = attestation_payload_bytes(
        &attester_identity.did,
        subject_did,
        capability_id,
        challenge_hash,
        passed,
        verified_at,
    )?;
    let _payload_hash = hex_encode(&digest_bytes(ATTESTATION_HASH_DOMAIN, &payload));
    let (ed25519_signature, mldsa65_signature) = dual_sign(attester_identity, &payload)?;
    Ok(CapabilityAttestation {
        attester_did: attester_identity.did.clone(),
        subject_did: subject_did.to_string(),
        capability_id: capability_id.to_string(),
        challenge_hash: challenge_hash.to_string(),
        passed,
        verified_at,
        ed25519_signature,
        mldsa65_signature,
    })
}

pub fn verify_capability_attestation(
    attestation: &CapabilityAttestation,
    attester_document: &DIDDocument,
) -> Result<bool, String> {
    let payload = attestation_payload_bytes(
        &attestation.attester_did,
        &attestation.subject_did,
        &attestation.capability_id,
        &attestation.challenge_hash,
        attestation.passed,
        attestation.verified_at,
    )?;
    dual_verify(
        attester_document,
        &payload,
        &attestation.ed25519_signature,
        &attestation.mldsa65_signature,
    )
}

pub fn attestation_decay_weight(
    attestation: &CapabilityAttestation,
    now: u64,
    half_life_secs: u64,
) -> f64 {
    if !attestation.passed || half_life_secs == 0 {
        return 0.0;
    }
    let age = now.saturating_sub(attestation.verified_at) as f64;
    0.5_f64.powf(age / half_life_secs as f64)
}

pub fn transitive_trust(
    subject_did: &str,
    attestations: &[CapabilityAttestation],
    trust_graph: &HashMap<String, f64>,
    decay_per_hop: f64,
    max_hops: u32,
) -> f64 {
    if max_hops == 0 || decay_per_hop <= 0.0 {
        return 0.0;
    }

    let mut by_subject: HashMap<&str, Vec<&CapabilityAttestation>> = HashMap::new();
    for attestation in attestations.iter().filter(|attestation| attestation.passed) {
        by_subject
            .entry(attestation.subject_did.as_str())
            .or_default()
            .push(attestation);
    }

    let mut queue = VecDeque::from([(subject_did.to_string(), 1_u32)]);
    let mut visited = HashSet::from([subject_did.to_string()]);
    let mut best_by_attester: HashMap<String, f64> = HashMap::new();

    while let Some((current, hops)) = queue.pop_front() {
        if hops > max_hops {
            continue;
        }
        let Some(edges) = by_subject.get(current.as_str()) else {
            continue;
        };
        let hop_weight = decay_per_hop.powi(hops as i32);
        for attestation in edges {
            if let Some(direct_trust) = trust_graph.get(&attestation.attester_did) {
                let weighted = direct_trust.max(0.0) * hop_weight;
                best_by_attester
                    .entry(attestation.attester_did.clone())
                    .and_modify(|existing| *existing = existing.max(weighted))
                    .or_insert(weighted);
            }

            if hops < max_hops && visited.insert(attestation.attester_did.clone()) {
                queue.push_back((attestation.attester_did.clone(), hops + 1));
            }
        }
    }

    best_by_attester.values().sum::<f64>().min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::capability_spec::{CapabilityDomain, CapabilitySpec};

    fn sample_spec() -> CapabilitySpec {
        CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![TypeSpec::LeanTerm],
            vec![TypeSpec::Text {
                language: Some("lean".to_string()),
            }],
            vec![
                CapabilityConstraint::KernelFaithful {
                    kernel: "lean4".to_string(),
                },
                CapabilityConstraint::ZeroSorry,
            ],
        )
    }

    #[test]
    fn challenge_response_respects_constraints() {
        let spec = sample_spec();
        let challenge = issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60);
        let ok = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text {
                    language: Some("lean".to_string()),
                },
                payload: challenge.expected_output.clone(),
                metadata: HashMap::from([
                    ("kernel".to_string(), "lean4".to_string()),
                    ("indexes".to_string(), "mathlib,lean_index".to_string()),
                ]),
                completed_at: 100,
            },
        );
        assert!(ok.passed, "unexpected failure: {:?}", ok.reasons);

        let bad = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text {
                    language: Some("lean".to_string()),
                },
                payload: Value::String("sorry".to_string()),
                metadata: HashMap::from([("kernel".to_string(), "lean4".to_string())]),
                completed_at: 100,
            },
        );
        assert!(!bad.passed);
        assert!(bad
            .reasons
            .iter()
            .any(|reason| reason.contains("ZeroSorry") || reason.contains("mismatch")));
    }

    #[test]
    fn attestation_roundtrip_verifies() {
        let attester = crate::halo::did::did_from_genesis_seed(&[0x51; 64]).expect("attester");
        let attestation = attest_capability(
            &attester,
            "did:key:subject",
            "capability-1",
            "challenge-hash",
            true,
            500,
        )
        .expect("attest");
        let ok = verify_capability_attestation(&attestation, &attester.did_document)
            .expect("verify attestation");
        assert!(ok);
    }

    #[test]
    fn challenge_bank_selects_registered_domain_template() {
        let spec = sample_spec();
        let bank = DomainChallengeBank::with_defaults();
        let challenge = bank.issue_for_spec(&spec, 100, 60);
        assert_eq!(challenge.kind, ChallengeKind::DeterministicText);
        assert_eq!(challenge.challenge_id, challenge_hash(&challenge));
    }

    #[test]
    fn transitive_trust_decays_by_hop_distance() {
        let attestations = vec![
            CapabilityAttestation {
                attester_did: "did:key:b".to_string(),
                subject_did: "did:key:c".to_string(),
                capability_id: "cap-1".to_string(),
                challenge_hash: "h1".to_string(),
                passed: true,
                verified_at: 100,
                ed25519_signature: vec![],
                mldsa65_signature: vec![],
            },
            CapabilityAttestation {
                attester_did: "did:key:a".to_string(),
                subject_did: "did:key:b".to_string(),
                capability_id: "cap-1".to_string(),
                challenge_hash: "h2".to_string(),
                passed: true,
                verified_at: 100,
                ed25519_signature: vec![],
                mldsa65_signature: vec![],
            },
        ];
        let trust_graph = HashMap::from([
            ("did:key:a".to_string(), 0.9),
            ("did:key:b".to_string(), 0.7),
        ]);

        let score = transitive_trust("did:key:c", &attestations, &trust_graph, 0.7, 3);
        assert!(score > 0.8, "score={score}");
        assert!(score <= 1.0);
    }
}
