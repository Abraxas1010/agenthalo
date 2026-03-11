use crate::halo::capability_spec::{
    CapabilityAttestation, CapabilityConstraint, CapabilitySpec, TypeSpec,
};
use crate::halo::did::{dual_sign, dual_verify, DIDDocument, DIDIdentity};
use crate::halo::util::{digest_bytes, hex_encode};
use deunicode::deunicode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet, VecDeque};
#[cfg(not(test))]
use std::fs;
use std::sync::{Mutex, OnceLock};
use url::Url;

const CHALLENGE_HASH_DOMAIN: &str = "agenthalo.capability.challenge.v1";
const ZERO_SORRY_PAYLOAD_HASH_DOMAIN: &str = "agenthalo.capability.zero_sorry.payload.v1";
const CHALLENGE_RATE_LIMIT_PER_MIN: usize = 10;
const CHALLENGE_RATE_LIMIT_WINDOW_SECS: u64 = 60;

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
        bank.register("analyze", vec![ChallengeKind::DeterministicJson]);
        bank
    }

    pub fn register(&mut self, domain_prefix: &str, templates: Vec<ChallengeKind>) {
        self.templates.insert(domain_prefix.to_string(), templates);
    }

    pub fn issue_for_spec(
        &self,
        spec: &CapabilitySpec,
        target_scope: &str,
        now: u64,
        ttl_secs: u64,
    ) -> Result<CapabilityChallenge, String> {
        enforce_challenge_rate_limit(target_scope, &spec.capability_id, now)?;
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
        _ => ChallengeKind::DeterministicJson,
    }
}

fn challenge_rate_state() -> &'static Mutex<HashMap<String, VecDeque<u64>>> {
    static RATE_STATE: OnceLock<Mutex<HashMap<String, VecDeque<u64>>>> = OnceLock::new();
    RATE_STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

#[cfg(not(test))]
fn load_challenge_rate_store_from_disk() -> Result<HashMap<String, VecDeque<u64>>, String> {
    let path = crate::halo::config::challenge_rate_store_path();
    match fs::read(&path) {
        Ok(raw) => {
            let parsed = serde_json::from_slice::<HashMap<String, Vec<u64>>>(&raw)
                .map_err(|e| format!("parse challenge rate store at {}: {e}", path.display()))?;
            Ok(parsed
                .into_iter()
                .map(|(key, values)| (key, VecDeque::from(values)))
                .collect())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(format!(
            "read challenge rate store at {}: {e}",
            path.display()
        )),
    }
}

#[cfg(test)]
fn load_challenge_rate_store_from_disk() -> Result<HashMap<String, VecDeque<u64>>, String> {
    Ok(HashMap::new())
}

#[cfg(not(test))]
fn persist_challenge_rate_store_to_disk(
    store: &HashMap<String, VecDeque<u64>>,
) -> Result<(), String> {
    crate::halo::config::ensure_halo_dir()?;
    let path = crate::halo::config::challenge_rate_store_path();
    let tmp_path = path.with_extension("json.tmp");
    let serializable = store
        .iter()
        .map(|(key, values)| (key.clone(), values.iter().copied().collect::<Vec<_>>()))
        .collect::<HashMap<_, _>>();
    let raw = serde_json::to_vec(&serializable)
        .map_err(|e| format!("serialize challenge rate store: {e}"))?;
    fs::write(&tmp_path, raw).map_err(|e| {
        format!(
            "write challenge rate store temp file {}: {e}",
            tmp_path.display()
        )
    })?;
    fs::rename(&tmp_path, &path)
        .map_err(|e| format!("persist challenge rate store to {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
fn persist_challenge_rate_store_to_disk(
    _store: &HashMap<String, VecDeque<u64>>,
) -> Result<(), String> {
    Ok(())
}

fn challenge_rate_key(target_scope: &str, capability_id: &str) -> String {
    format!("{target_scope}:{capability_id}")
}

fn prune_challenge_rate_store(store: &mut HashMap<String, VecDeque<u64>>, now: u64) {
    store.retain(|_, entries| {
        while let Some(front) = entries.front().copied() {
            if now.saturating_sub(front) > CHALLENGE_RATE_LIMIT_WINDOW_SECS {
                let _ = entries.pop_front();
            } else {
                break;
            }
        }
        !entries.is_empty()
    });
}

fn enforce_challenge_rate_limit(
    target_scope: &str,
    capability_id: &str,
    now: u64,
) -> Result<(), String> {
    let mut guard = challenge_rate_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !cfg!(test) {
        *guard = load_challenge_rate_store_from_disk()?;
    }
    prune_challenge_rate_store(&mut guard, now);
    let key = challenge_rate_key(target_scope, capability_id);
    let entries = guard.entry(key).or_default();
    if entries.len() >= CHALLENGE_RATE_LIMIT_PER_MIN {
        persist_challenge_rate_store_to_disk(&guard)?;
        return Err(format!(
            "challenge issuance for target `{target_scope}` capability `{capability_id}` exceeded {CHALLENGE_RATE_LIMIT_PER_MIN}/minute"
        ));
    }
    entries.push_back(now);
    persist_challenge_rate_store_to_disk(&guard)?;
    Ok(())
}

fn challenge_nonce() -> Result<String, String> {
    let mut random = [0u8; 32];
    getrandom::getrandom(&mut random).map_err(|e| format!("challenge nonce rng: {e}"))?;
    Ok(hex_encode(&random))
}

pub fn issue_challenge(
    spec: &CapabilitySpec,
    kind: ChallengeKind,
    now: u64,
    ttl_secs: u64,
) -> Result<CapabilityChallenge, String> {
    let nonce = challenge_nonce()?;
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
    Ok(challenge)
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
            reasons.push(
                "TypeOnly challenges are informational only and cannot build trust".to_string(),
            );
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
            let proof_ref = response
                .metadata
                .get("zero_sorry_proof_ref")
                .map(String::as_str)
                .unwrap_or("");
            let expected_payload_hash = zero_sorry_payload_hash(&response.payload);
            if response
                .metadata
                .get("zero_sorry_verified")
                .map(String::as_str)
                != Some("true")
            {
                Some("ZeroSorry requires kernel-level zero_sorry_verified=true".to_string())
            } else if !valid_zero_sorry_proof_ref(proof_ref) {
                Some(
                "ZeroSorry requires a structured zero_sorry_proof_ref (proof://, cab://, or did:)"
                    .to_string(),
            )
            } else if response
                .metadata
                .get("zero_sorry_payload_hash")
                .map(String::as_str)
                != Some(expected_payload_hash.as_str())
            {
                Some("ZeroSorry payload hash metadata does not match response payload".to_string())
            } else if payload_contains_banned_token(&response.payload, "sorry")
                || payload_contains_banned_token(&response.payload, "admit")
            {
                Some("response violated ZeroSorry constraint".to_string())
            } else {
                None
            }
        }
        CapabilityConstraint::MaxLatencyMs(max_ms) => {
            let elapsed_ms = response
                .metadata
                .get("elapsed_ms")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or_else(|| response.completed_at.saturating_sub(challenge.issued_at));
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

fn zero_sorry_payload_hash(payload: &Value) -> String {
    let raw = serde_json::to_vec(payload).unwrap_or_default();
    let digest = digest_bytes(ZERO_SORRY_PAYLOAD_HASH_DOMAIN, &raw);
    let mut sha = Sha256::new();
    sha.update(digest);
    hex_encode(&sha.finalize())
}

fn normalized_payload_text(payload: &Value) -> String {
    let mut text = String::new();
    collect_payload_text(payload, &mut text);
    let stripped = text
        .chars()
        .filter(|ch| !matches!(*ch, '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{FEFF}'))
        .collect::<String>();
    deunicode(&stripped).to_ascii_lowercase()
}

fn payload_contains_banned_token(payload: &Value, token: &str) -> bool {
    let normalized = normalized_payload_text(payload);
    if normalized.contains(token) {
        return true;
    }
    let compact = normalized
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic())
        .collect::<String>();
    compact.contains(token)
}

fn valid_zero_sorry_proof_ref(proof_ref: &str) -> bool {
    let Ok(parsed) = Url::parse(proof_ref) else {
        return false;
    };
    matches!(parsed.scheme(), "proof" | "cab" | "did")
        && (!parsed.host_str().unwrap_or_default().is_empty() || !parsed.path().is_empty())
}

fn collect_payload_text(payload: &Value, out: &mut String) {
    match payload {
        Value::String(text) => {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(text);
        }
        Value::Array(items) => {
            for item in items {
                collect_payload_text(item, out);
            }
        }
        Value::Object(map) => {
            for value in map.values() {
                collect_payload_text(value, out);
            }
        }
        _ => {}
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
    if attester_identity.did == subject_did {
        return Err("self-attestation is not allowed".to_string());
    }
    let payload = attestation_payload_bytes(
        &attester_identity.did,
        subject_did,
        capability_id,
        challenge_hash,
        passed,
        verified_at,
    )?;
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
    let steps = now.saturating_sub(attestation.verified_at) / half_life_secs;
    0.5_f64.powi(steps.min(i32::MAX as u64) as i32)
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

    best_by_attester
        .values()
        .copied()
        .fold(0.0_f64, f64::max)
        .min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::capability_spec::{CapabilityDomain, CapabilitySpec};
    use crate::test_support::lock_env;

    fn reset_challenge_rate_state_for_tests() {
        let mutex = challenge_rate_state();
        let mut guard = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
        mutex.clear_poison();
    }

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
        reset_challenge_rate_state_for_tests();
        let spec = sample_spec();
        let challenge =
            issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60).expect("challenge");
        let payload_hash = zero_sorry_payload_hash(&challenge.expected_output);
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
                    ("zero_sorry_verified".to_string(), "true".to_string()),
                    (
                        "zero_sorry_proof_ref".to_string(),
                        "proof://lean/zero-sorry".to_string(),
                    ),
                    ("zero_sorry_payload_hash".to_string(), payload_hash.clone()),
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
                metadata: HashMap::from([
                    ("kernel".to_string(), "lean4".to_string()),
                    ("zero_sorry_verified".to_string(), "true".to_string()),
                    (
                        "zero_sorry_proof_ref".to_string(),
                        "proof://lean/zero-sorry".to_string(),
                    ),
                    (
                        "zero_sorry_payload_hash".to_string(),
                        zero_sorry_payload_hash(&Value::String("sorry".to_string())),
                    ),
                ]),
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
        let _guard = lock_env();
        reset_challenge_rate_state_for_tests();
        let spec = sample_spec();
        let bank = DomainChallengeBank::with_defaults();
        let challenge = bank
            .issue_for_spec(&spec, "did:key:target", 100, 60)
            .expect("challenge");
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
        assert!(score > 0.4, "score={score}");
        assert!(score < 0.8, "score={score}");
        assert!(score <= 1.0);
    }

    #[test]
    fn challenge_nonce_is_unique_within_same_second() {
        let _guard = lock_env();
        reset_challenge_rate_state_for_tests();
        let spec = sample_spec();
        let first = issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60)
            .expect("first challenge");
        let second = issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60)
            .expect("second challenge");
        assert_ne!(first.nonce, second.nonce);
        assert_ne!(first.challenge_id, second.challenge_id);
        assert_ne!(first.expected_output, second.expected_output);
    }

    #[test]
    fn type_only_challenges_do_not_build_trust() {
        let spec = CapabilitySpec::new(
            CapabilityDomain::new("analyze/general", 1),
            vec![TypeSpec::Text { language: None }],
            vec![TypeSpec::Text { language: None }],
            vec![],
        );
        let challenge =
            issue_challenge(&spec, ChallengeKind::TypeOnly, 100, 60).expect("type only challenge");
        let result = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text { language: None },
                payload: Value::String("anything".to_string()),
                metadata: HashMap::new(),
                completed_at: 100,
            },
        );
        assert!(!result.passed);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("informational only")));
    }

    #[test]
    fn challenge_rate_limit_enforced_per_capability() {
        let _guard = lock_env();
        reset_challenge_rate_state_for_tests();
        let target_scope = "did:key:rate-limit-test-a";
        let capability_id = sample_spec().capability_id;
        let key = challenge_rate_key(target_scope, &capability_id);
        {
            let mutex = challenge_rate_state();
            let mut guard = mutex
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.insert(key, VecDeque::from((100..110).collect::<Vec<_>>()));
            mutex.clear_poison();
        }
        let err = enforce_challenge_rate_limit(target_scope, &capability_id, 110)
            .expect_err("11th challenge in a minute must fail");
        assert!(err.contains("exceeded 10/minute"));
    }

    #[test]
    fn challenge_rate_limit_is_scoped_per_target() {
        let _guard = lock_env();
        reset_challenge_rate_state_for_tests();
        let spec = sample_spec();
        let bank = DomainChallengeBank::default();
        for now in 100..110 {
            bank.issue_for_spec(&spec, "did:key:rate-scope-test-a", now, 60)
                .expect("target a within limit");
        }
        bank.issue_for_spec(&spec, "did:key:rate-scope-test-b", 110, 60)
            .expect("target b should not be blocked by target a");
    }

    #[test]
    fn zero_sorry_scans_nested_json_payloads() {
        let spec = sample_spec();
        let challenge =
            issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60).expect("challenge");
        let payload = serde_json::json!({
            "proof": {
                "body": "intro h\nexact so\u{200B}rry"
            }
        });
        let result = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text {
                    language: Some("lean".to_string()),
                },
                payload: payload.clone(),
                metadata: HashMap::from([
                    ("kernel".to_string(), "lean4".to_string()),
                    ("zero_sorry_verified".to_string(), "true".to_string()),
                    (
                        "zero_sorry_proof_ref".to_string(),
                        "proof://lean/zero-sorry".to_string(),
                    ),
                    (
                        "zero_sorry_payload_hash".to_string(),
                        zero_sorry_payload_hash(&payload),
                    ),
                ]),
                completed_at: 100,
            },
        );
        assert!(!result.passed);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("ZeroSorry")));
    }

    #[test]
    fn zero_sorry_rejects_confusable_sorry_spellings() {
        let spec = sample_spec();
        let challenge =
            issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60).expect("challenge");
        let payload = Value::String("exact \u{0441}orry".to_string());
        let result = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text {
                    language: Some("lean".to_string()),
                },
                payload: payload.clone(),
                metadata: HashMap::from([
                    ("kernel".to_string(), "lean4".to_string()),
                    ("zero_sorry_verified".to_string(), "true".to_string()),
                    (
                        "zero_sorry_proof_ref".to_string(),
                        "proof://lean/zero-sorry".to_string(),
                    ),
                    (
                        "zero_sorry_payload_hash".to_string(),
                        zero_sorry_payload_hash(&payload),
                    ),
                ]),
                completed_at: 100,
            },
        );
        assert!(!result.passed);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("ZeroSorry")));
    }

    #[test]
    fn zero_sorry_requires_structured_proof_ref() {
        let spec = sample_spec();
        let challenge =
            issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60).expect("challenge");
        let payload = challenge.expected_output.clone();
        let result = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text {
                    language: Some("lean".to_string()),
                },
                payload: payload.clone(),
                metadata: HashMap::from([
                    ("kernel".to_string(), "lean4".to_string()),
                    ("zero_sorry_verified".to_string(), "true".to_string()),
                    ("zero_sorry_proof_ref".to_string(), "x".to_string()),
                    (
                        "zero_sorry_payload_hash".to_string(),
                        zero_sorry_payload_hash(&payload),
                    ),
                ]),
                completed_at: 100,
            },
        );
        assert!(!result.passed);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("structured zero_sorry_proof_ref")));
    }

    #[test]
    fn zero_sorry_rejects_combining_mark_variants() {
        let spec = sample_spec();
        let challenge =
            issue_challenge(&spec, ChallengeKind::DeterministicText, 100, 60).expect("challenge");
        let payload = Value::String("exact s\u{0338}orry".to_string());
        let result = verify_challenge_response(
            &spec,
            &challenge,
            &ChallengeResponse {
                output_type: TypeSpec::Text {
                    language: Some("lean".to_string()),
                },
                payload: payload.clone(),
                metadata: HashMap::from([
                    ("kernel".to_string(), "lean4".to_string()),
                    ("zero_sorry_verified".to_string(), "true".to_string()),
                    (
                        "zero_sorry_proof_ref".to_string(),
                        "proof://lean/zero-sorry".to_string(),
                    ),
                    (
                        "zero_sorry_payload_hash".to_string(),
                        zero_sorry_payload_hash(&payload),
                    ),
                ]),
                completed_at: 100,
            },
        );
        assert!(!result.passed);
        assert!(result
            .reasons
            .iter()
            .any(|reason| reason.contains("ZeroSorry")));
    }

    #[test]
    fn challenge_rate_store_prunes_expired_keys() {
        let _guard = lock_env();
        reset_challenge_rate_state_for_tests();
        let mutex = challenge_rate_state();
        let mut guard = mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(
            "did:key:old-prune-test:cap".to_string(),
            VecDeque::from(vec![100]),
        );
        guard.insert(
            "did:key:new-prune-test:cap".to_string(),
            VecDeque::from(vec![1000]),
        );
        prune_challenge_rate_store(&mut guard, 1000);
        assert!(guard.contains_key("did:key:new-prune-test:cap"));
        assert!(!guard.contains_key("did:key:old-prune-test:cap"));
    }

    #[test]
    fn self_attestation_is_rejected() {
        let identity = crate::halo::did::did_from_genesis_seed(&[0x52; 64]).expect("identity");
        let err = attest_capability(
            &identity,
            &identity.did,
            "capability-1",
            "challenge-hash",
            true,
            500,
        )
        .expect_err("self-attestation must fail");
        assert!(err.contains("self-attestation"));
    }
}
