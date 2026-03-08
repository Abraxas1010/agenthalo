use crate::halo::util::{digest_bytes, hex_encode};
use serde::{Deserialize, Serialize};

const CAPABILITY_ID_DOMAIN: &str = "agenthalo.capability.id.v1";
const CAPABILITY_QUERY_HASH_DOMAIN: &str = "agenthalo.capability.query.v1";
pub const CAPABILITY_TOPIC_PREFIX: &str = "/agenthalo/capabilities/";

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDomain {
    pub path: String,
    pub schema_version: u32,
}

impl CapabilityDomain {
    pub fn new(path: impl Into<String>, schema_version: u32) -> Self {
        Self {
            path: normalize_domain_path(&path.into()),
            schema_version,
        }
    }

    pub fn matches_prefix(&self, prefix: &str) -> bool {
        let normalized_prefix = normalize_domain_path(prefix);
        if normalized_prefix.is_empty() {
            return true;
        }
        self.path == normalized_prefix
            || self
                .path
                .strip_prefix(&(normalized_prefix.clone() + "/"))
                .is_some()
    }

    pub fn topic(&self) -> String {
        dynamic_topic_for_domain(self)
    }
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum TypeSpec {
    Bytes { mime: String },
    JsonSchema { schema_id: String },
    LeanTerm,
    CoqTerm,
    Text { language: Option<String> },
    Vector { dimensions: u32 },
}

#[derive(Clone, Debug, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub enum CapabilityConstraint {
    KernelFaithful { kernel: String },
    ZeroSorry,
    MaxLatencyMs(u64),
    RequiresIndex { index_name: String },
    Custom { key: String, value: String },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LiveMetrics {
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub success_rate: f64,
    pub latency_p50_ms: u64,
    pub latency_p99_ms: u64,
    pub cost_microdollars: u64,
    pub last_active: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onchain_reputation: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityAttestation {
    pub attester_did: String,
    pub subject_did: String,
    pub capability_id: String,
    pub challenge_hash: String,
    pub passed: bool,
    pub verified_at: u64,
    pub ed25519_signature: Vec<u8>,
    pub mldsa65_signature: Vec<u8>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilitySpec {
    pub capability_id: String,
    pub domain: CapabilityDomain,
    pub input_types: Vec<TypeSpec>,
    pub output_types: Vec<TypeSpec>,
    pub constraints: Vec<CapabilityConstraint>,
    pub metrics: LiveMetrics,
    #[serde(default)]
    pub attestations: Vec<CapabilityAttestation>,
}

impl CapabilitySpec {
    pub fn new(
        domain: CapabilityDomain,
        input_types: Vec<TypeSpec>,
        output_types: Vec<TypeSpec>,
        constraints: Vec<CapabilityConstraint>,
    ) -> Self {
        let capability_id = Self::compute_id(&domain, &input_types, &output_types, &constraints);
        Self {
            capability_id,
            domain,
            input_types,
            output_types,
            constraints,
            metrics: LiveMetrics::default(),
            attestations: Vec::new(),
        }
    }

    pub fn compute_id(
        domain: &CapabilityDomain,
        input_types: &[TypeSpec],
        output_types: &[TypeSpec],
        constraints: &[CapabilityConstraint],
    ) -> String {
        let canonical = serde_json::json!({
            "domain": domain,
            "input_types": input_types,
            "output_types": output_types,
            "constraints": constraints,
        });
        let raw = serde_json::to_vec(&canonical).unwrap_or_default();
        hex_encode(&digest_bytes(CAPABILITY_ID_DOMAIN, &raw))
    }

    pub fn satisfies(&self, query: &CapabilityQuery) -> bool {
        self.satisfies_at(query, u64::MAX, u64::MAX)
    }

    pub fn satisfies_at(
        &self,
        query: &CapabilityQuery,
        now: u64,
        attestation_max_age_secs: u64,
    ) -> bool {
        if !self.domain.matches_prefix(&query.domain_prefix) {
            return false;
        }
        if !query
            .required_inputs
            .iter()
            .all(|required| self.input_types.iter().any(|actual| actual == required))
        {
            return false;
        }
        if !query
            .required_outputs
            .iter()
            .all(|required| self.output_types.iter().any(|actual| actual == required))
        {
            return false;
        }
        if !query
            .required_constraints
            .iter()
            .all(|required| self.constraints.iter().any(|actual| actual == required))
        {
            return false;
        }
        if let Some(min_success_rate) = query.min_success_rate {
            if self.metrics.success_rate < min_success_rate {
                return false;
            }
        }
        if let Some(max_latency_p99_ms) = query.max_latency_p99_ms {
            if self.metrics.latency_p99_ms > max_latency_p99_ms {
                return false;
            }
        }
        if let Some(max_cost_microdollars) = query.max_cost_microdollars {
            if self.metrics.cost_microdollars > max_cost_microdollars {
                return false;
            }
        }
        if let Some(min_attestations) = query.min_attestations {
            if self.verified_attestation_count(now, attestation_max_age_secs) < min_attestations {
                return false;
            }
        }
        if let Some(min_onchain_reputation) = query.min_onchain_reputation {
            if self.metrics.onchain_reputation.unwrap_or(0.0) < min_onchain_reputation {
                return false;
            }
        }
        true
    }

    pub fn verified_attestation_count(&self, now: u64, max_age_secs: u64) -> usize {
        self.attestations
            .iter()
            .filter(|attestation| {
                attestation.passed && now.saturating_sub(attestation.verified_at) <= max_age_secs
            })
            .count()
    }

    pub fn topic(&self) -> String {
        self.domain.topic()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityQuery {
    pub domain_prefix: String,
    pub required_inputs: Vec<TypeSpec>,
    pub required_outputs: Vec<TypeSpec>,
    pub required_constraints: Vec<CapabilityConstraint>,
    pub min_success_rate: Option<f64>,
    pub max_latency_p99_ms: Option<u64>,
    pub max_cost_microdollars: Option<u64>,
    pub min_attestations: Option<usize>,
    pub min_onchain_reputation: Option<f64>,
    pub count: u32,
    pub query_timeout_ms: u64,
}

impl CapabilityQuery {
    pub fn hash(&self) -> String {
        let raw = serde_json::to_vec(self).unwrap_or_default();
        hex_encode(&digest_bytes(CAPABILITY_QUERY_HASH_DOMAIN, &raw))
    }

    pub fn topic(&self) -> String {
        format!("{CAPABILITY_TOPIC_PREFIX}query/{}", self.hash())
    }
}

pub fn normalize_domain_path(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("/")
}

pub fn dynamic_topic_for_domain(domain: &CapabilityDomain) -> String {
    format!(
        "{CAPABILITY_TOPIC_PREFIX}{}",
        normalize_domain_path(&domain.path)
    )
}

pub fn is_dynamic_capability_topic(topic: &str) -> bool {
    let Some(rest) = topic.strip_prefix(CAPABILITY_TOPIC_PREFIX) else {
        return false;
    };
    !rest.is_empty()
        && !rest.starts_with('/')
        && !rest.ends_with('/')
        && rest.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> CapabilitySpec {
        CapabilitySpec::new(
            CapabilityDomain::new("prove/lean/algebra", 1),
            vec![TypeSpec::LeanTerm],
            vec![TypeSpec::LeanTerm],
            vec![
                CapabilityConstraint::KernelFaithful {
                    kernel: "lean4".to_string(),
                },
                CapabilityConstraint::ZeroSorry,
            ],
        )
    }

    #[test]
    fn capability_id_is_stable_for_same_spec() {
        let spec = sample_spec();
        let id_again = CapabilitySpec::compute_id(
            &spec.domain,
            &spec.input_types,
            &spec.output_types,
            &spec.constraints,
        );
        assert_eq!(spec.capability_id, id_again);
    }

    #[test]
    fn domain_prefix_matching_is_hierarchical() {
        let domain = CapabilityDomain::new("prove/lean/algebra", 1);
        assert!(domain.matches_prefix("prove"));
        assert!(domain.matches_prefix("prove/lean"));
        assert!(domain.matches_prefix("prove/lean/algebra"));
        assert!(!domain.matches_prefix("translate/coq"));
    }

    #[test]
    fn query_satisfaction_checks_metrics_and_attestations() {
        let mut spec = sample_spec();
        spec.metrics.success_rate = 0.97;
        spec.metrics.latency_p99_ms = 800;
        spec.metrics.cost_microdollars = 40;
        spec.attestations.push(CapabilityAttestation {
            attester_did: "did:key:attester".to_string(),
            subject_did: "did:key:subject".to_string(),
            capability_id: spec.capability_id.clone(),
            challenge_hash: "abc".to_string(),
            passed: true,
            verified_at: 1_000,
            ed25519_signature: vec![1],
            mldsa65_signature: vec![2],
        });
        let query = CapabilityQuery {
            domain_prefix: "prove/lean".to_string(),
            required_inputs: vec![TypeSpec::LeanTerm],
            required_outputs: vec![TypeSpec::LeanTerm],
            required_constraints: vec![CapabilityConstraint::ZeroSorry],
            min_success_rate: Some(0.9),
            max_latency_p99_ms: Some(1000),
            max_cost_microdollars: Some(100),
            min_attestations: Some(1),
            min_onchain_reputation: None,
            count: 1,
            query_timeout_ms: 250,
        };
        assert!(spec.satisfies_at(&query, 1_010, 120));
        assert!(!spec.satisfies_at(&query, 2_000, 120));
    }

    #[test]
    fn dynamic_topic_validation_accepts_hierarchical_domains() {
        assert!(is_dynamic_capability_topic(
            "/agenthalo/capabilities/prove/lean/algebra"
        ));
        assert!(is_dynamic_capability_topic(
            "/agenthalo/capabilities/query/abcdef0123"
        ));
        assert!(!is_dynamic_capability_topic("/agenthalo/capabilities//bad"));
        assert!(!is_dynamic_capability_topic("/agenthalo/capabilities/Bad"));
        assert!(!is_dynamic_capability_topic("general"));
    }
}
