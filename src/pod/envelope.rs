//! Proof-carrying data envelope for NucleusPOD.
//!
//! A `ProofEnvelope` is a self-contained, serializable unit that bundles:
//! - the data (typed value)
//! - a cryptographic proof (Merkle/IPA/KZG)
//! - the state root and commit height
//! - witness quorum signatures
//! - optional AgentHALO attestation
//! - optional author PUF fingerprint
//! - optional Lean proof reference
//! - optional upstream envelope references (provenance DAG edges)
//!
//! Any consumer can verify the envelope locally without trusting the producer.

use crate::protocol::{NucleusDb, QueryProof, VcBackend};
use crate::transparency::ct6962::NodeHash;
use crate::typed_value::TypedValue;
use crate::{halo::attest::AttestationResult, pod::now_unix};
use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Reference to an upstream envelope used as provenance input.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvelopeRef {
    /// Stable provenance hash of the upstream envelope.
    #[serde(
        serialize_with = "serialize_hex_32",
        deserialize_with = "deserialize_hex_or_array_32"
    )]
    pub envelope_hash: [u8; 32],
    /// Optional key label for human-readable diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Optional commit height hint for diagnostics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit_height: Option<u64>,
}

/// A self-contained proof-carrying data unit.
///
/// Consumers receive this over HTTP or MCP and verify locally.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProofEnvelope {
    /// Schema version for forward compatibility.
    pub version: u32,
    /// The key this data lives at.
    pub key: String,
    /// The typed value.
    pub value: TypedValue,
    /// Which VC backend produced the proof.
    pub backend: VcBackend,
    /// The inclusion/membership proof.
    pub proof: QueryProof,
    /// The state root this proof is against.
    #[serde(
        serialize_with = "serialize_hex_32",
        deserialize_with = "deserialize_hex_or_array_32"
    )]
    pub state_root: NodeHash,
    /// The commit height at time of envelope creation.
    pub commit_height: u64,
    /// Witness quorum signatures on the state root.
    pub witness_sigs: Vec<(String, String)>,
    /// AgentHALO attestation (if available).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attestation: Option<AttestationResult>,
    /// Author PUF fingerprint (if available).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        serialize_with = "serialize_opt_hex_32",
        deserialize_with = "deserialize_opt_hex_or_array_32"
    )]
    pub author_puf: Option<[u8; 32]>,
    /// Lean proof reference (content hash or file path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lean_proof_ref: Option<String>,
    /// Optional upstream envelope references (provenance DAG edges).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream_envelopes: Vec<EnvelopeRef>,
    /// Access grant token that authorized this fetch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_token: Option<String>,
    /// Timestamp of envelope creation (Unix seconds).
    pub created_at: u64,
}

/// Result of verifying a proof envelope locally.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnvelopeVerification {
    /// Whether the envelope schema version is supported.
    #[serde(default = "default_true")]
    pub version_supported: bool,
    /// Whether the cryptographic proof validates against the state root.
    pub proof_valid: bool,
    /// Whether the backend tag matches the proof variant.
    pub backend_consistent: bool,
    /// Number of witness signatures present.
    pub witness_count: usize,
    /// Whether upstream envelope references are syntactically valid.
    #[serde(default = "default_true")]
    pub upstream_refs_valid: bool,
    /// Number of upstream references attached to this envelope.
    #[serde(default)]
    pub upstream_ref_count: usize,
    /// True when the proof check is intentionally shallow (e.g., KZG without trusted setup).
    #[serde(default)]
    pub soft_verified: bool,
    /// Overall verdict: all checks passed.
    pub accepted: bool,
    /// Human-readable rejection reason (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

/// Result of transitive DAG verification for upstream envelope references.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvenanceDagVerification {
    /// Root envelope provenance hash (hex).
    pub root_hash: String,
    /// Whether the root envelope passed local proof checks.
    pub root_accepted: bool,
    /// Number of unique nodes reached (including the root).
    pub visited_nodes: usize,
    /// Number of edges traversed from upstream reference lists.
    pub upstream_edges: usize,
    /// Deepest depth reached during traversal (root = 0).
    pub max_depth_reached: usize,
    /// Number of missing upstream references.
    pub missing_refs: usize,
    /// Number of hash mismatches between reference hash and resolved envelope hash.
    pub hash_mismatches: usize,
    /// Number of resolved nodes that fail local verification.
    pub invalid_nodes: usize,
    /// Whether a cycle was observed in the reference graph.
    pub cycle_detected: bool,
    /// Whether traversal exceeded the caller-provided depth bound.
    pub depth_limit_exceeded: bool,
    /// Whether traversal exceeded the caller-provided node bound.
    #[serde(default)]
    pub nodes_limit_exceeded: bool,
    /// Overall verdict for the transitive graph.
    pub accepted: bool,
}

/// Options for transitive provenance DAG verification.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProvenanceDagVerifyOptions {
    /// Maximum traversal depth (root = 0).
    pub max_depth: usize,
    /// Maximum number of resolved nodes (excluding root).
    pub max_nodes: usize,
    /// Stop traversing immediately on first hash mismatch.
    #[serde(default)]
    pub fail_fast_on_hash_mismatch: bool,
}

impl Default for ProvenanceDagVerifyOptions {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_nodes: 10_000,
            fail_fast_on_hash_mismatch: false,
        }
    }
}

/// Current envelope schema version.
pub const ENVELOPE_VERSION: u32 = 1;

const PROVENANCE_HASH_DOMAIN: &str = "nucleusdb.pod.provenance.envelope.v1";

impl ProofEnvelope {
    /// Build an envelope from an existing NucleusDB query result.
    ///
    /// `key_name` is the human-readable key.
    /// `key_index` is the state-vector index for that key.
    /// `value` is the already-decoded typed value.
    pub fn from_query(
        db: &NucleusDb,
        key_name: &str,
        key_index: usize,
        value: TypedValue,
    ) -> Option<Self> {
        let (raw_value, proof, state_root) = db.query(key_index)?;
        let _ = raw_value; // we already have the decoded TypedValue
        let commit_height = db.entries.len() as u64;
        let witness_sigs = db
            .entries
            .last()
            .map(|e| e.witness_sigs.clone())
            .unwrap_or_default();

        Some(Self {
            version: ENVELOPE_VERSION,
            key: key_name.to_string(),
            value,
            backend: db.backend.clone(),
            proof,
            state_root,
            commit_height,
            witness_sigs,
            attestation: None,
            author_puf: None,
            lean_proof_ref: None,
            upstream_envelopes: vec![],
            grant_token: None,
            created_at: now_unix(),
        })
    }

    /// Return a stable provenance hash for this envelope.
    ///
    /// The hash intentionally excludes transport metadata (`grant_token`) and
    /// issuance time (`created_at`) so references remain stable across exports.
    pub fn provenance_hash(&self) -> Result<[u8; 32], serde_json::Error> {
        let canonical_ref_hashes = canonical_upstream_hashes(&self.upstream_envelopes);
        let payload = EnvelopeHashInput {
            domain: PROVENANCE_HASH_DOMAIN,
            version: self.version,
            key: &self.key,
            value: &self.value,
            backend: &self.backend,
            proof: &self.proof,
            state_root: &self.state_root,
            commit_height: self.commit_height,
            witness_sigs: &self.witness_sigs,
            attestation: &self.attestation,
            author_puf: &self.author_puf,
            lean_proof_ref: &self.lean_proof_ref,
            upstream_hashes: &canonical_ref_hashes,
        };
        let encoded = serde_json::to_vec(&payload)?;
        let mut hasher = Sha256::new();
        hasher.update(PROVENANCE_HASH_DOMAIN.as_bytes());
        hasher.update([0u8]);
        hasher.update(encoded);
        let digest = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        Ok(out)
    }

    /// Return the stable provenance hash as lowercase hex.
    pub fn provenance_hash_hex(&self) -> Result<String, serde_json::Error> {
        self.provenance_hash().map(|h| hex_encode_32(&h))
    }

    /// Build a portable reference to this envelope for use in downstream DAG edges.
    pub fn to_envelope_ref(&self) -> Result<EnvelopeRef, serde_json::Error> {
        Ok(EnvelopeRef {
            envelope_hash: self.provenance_hash()?,
            key: Some(self.key.clone()),
            commit_height: Some(self.commit_height),
        })
    }

    /// Add an upstream envelope reference. Duplicate hashes are ignored.
    pub fn add_upstream_ref(&mut self, reference: EnvelopeRef) {
        if self
            .upstream_envelopes
            .iter()
            .any(|r| r.envelope_hash == reference.envelope_hash)
        {
            return;
        }
        self.upstream_envelopes.push(reference);
        self.upstream_envelopes = canonical_upstream_refs(&self.upstream_envelopes);
    }

    /// Verify the transitive upstream provenance DAG using a caller-supplied resolver.
    ///
    /// `resolve` should return a full `ProofEnvelope` for a given provenance hash.
    /// `max_depth` bounds traversal to prevent untrusted graphs from exhausting resources.
    pub fn verify_provenance_dag<F>(
        &self,
        resolve: F,
        max_depth: usize,
    ) -> Result<ProvenanceDagVerification, serde_json::Error>
    where
        F: FnMut(&[u8; 32]) -> Option<ProofEnvelope>,
    {
        self.verify_provenance_dag_with_options(
            resolve,
            ProvenanceDagVerifyOptions {
                max_depth,
                ..ProvenanceDagVerifyOptions::default()
            },
        )
    }

    /// Verify the transitive upstream provenance DAG with explicit options.
    pub fn verify_provenance_dag_with_options<F>(
        &self,
        mut resolve: F,
        options: ProvenanceDagVerifyOptions,
    ) -> Result<ProvenanceDagVerification, serde_json::Error>
    where
        F: FnMut(&[u8; 32]) -> Option<ProofEnvelope>,
    {
        let root_hash = self.provenance_hash()?;
        let root_accepted = self.verify_locally().accepted;

        let mut stats = DagWalkState {
            upstream_edges: self.upstream_envelopes.len(),
            max_depth_reached: 0,
            missing_refs: 0,
            hash_mismatches: 0,
            invalid_nodes: 0,
            cycle_detected: false,
            depth_limit_exceeded: false,
            nodes_limit_exceeded: false,
            stop_walk: false,
        };
        let mut visited = HashSet::<[u8; 32]>::new();
        let mut path = vec![root_hash];

        for edge in canonical_upstream_refs(&self.upstream_envelopes) {
            if stats.stop_walk {
                break;
            }
            walk_provenance_ref(
                edge.envelope_hash,
                1,
                &options,
                &mut resolve,
                &mut visited,
                &mut path,
                &mut stats,
            )?;
        }

        let visited_nodes = visited.len() + 1; // include root
        let accepted = root_accepted
            && stats.missing_refs == 0
            && stats.hash_mismatches == 0
            && stats.invalid_nodes == 0
            && !stats.cycle_detected
            && !stats.depth_limit_exceeded
            && !stats.nodes_limit_exceeded;

        Ok(ProvenanceDagVerification {
            root_hash: hex_encode_32(&root_hash),
            root_accepted,
            visited_nodes,
            upstream_edges: stats.upstream_edges,
            max_depth_reached: stats.max_depth_reached,
            missing_refs: stats.missing_refs,
            hash_mismatches: stats.hash_mismatches,
            invalid_nodes: stats.invalid_nodes,
            cycle_detected: stats.cycle_detected,
            depth_limit_exceeded: stats.depth_limit_exceeded,
            nodes_limit_exceeded: stats.nodes_limit_exceeded,
            accepted,
        })
    }

    /// Verify this envelope's cryptographic proof locally.
    ///
    /// This does NOT require access to the original database — the envelope
    /// is self-contained. However, the consumer needs the state root to be
    /// trustworthy (via witness signatures or external anchoring).
    pub fn verify_locally(&self) -> EnvelopeVerification {
        let upstream_ref_count = self.upstream_envelopes.len();
        let upstream_ref_validation = validate_upstream_refs(&self.upstream_envelopes);
        let upstream_refs_valid = upstream_ref_validation.is_ok();

        if self.version != ENVELOPE_VERSION {
            return EnvelopeVerification {
                version_supported: false,
                proof_valid: false,
                backend_consistent: false,
                witness_count: self.witness_sigs.len(),
                upstream_refs_valid,
                upstream_ref_count,
                soft_verified: false,
                accepted: false,
                rejection_reason: Some(format!("unsupported envelope version {}", self.version)),
            };
        }

        // Check backend consistency.
        let backend_consistent = matches!(
            (&self.backend, &self.proof),
            (VcBackend::Ipa, QueryProof::Ipa(_))
                | (VcBackend::Kzg, QueryProof::Kzg(_))
                | (VcBackend::BinaryMerkle, QueryProof::BinaryMerkle(_))
        );

        if !backend_consistent {
            return EnvelopeVerification {
                version_supported: true,
                proof_valid: false,
                backend_consistent: false,
                witness_count: self.witness_sigs.len(),
                upstream_refs_valid,
                upstream_ref_count,
                soft_verified: false,
                accepted: false,
                rejection_reason: Some("backend tag does not match proof variant".to_string()),
            };
        }

        // Verify the cryptographic proof.
        let (proof_valid, soft_verified) = match &self.proof {
            QueryProof::BinaryMerkle(p) => {
                use crate::transparency::ct6962::{verify_inclusion_proof, InclusionProof};

                // Reconstruct what the leaf hash should be for the value at this index.
                // We verify the inclusion proof against the claimed state root.
                let ip = InclusionProof {
                    leaf_index: p.index as u64,
                    tree_size: p.tree_size as u64,
                    leaf_hash: p.leaf_hash,
                    path: p.path.clone(),
                };
                if p.tree_size == 0 {
                    // Edge case: empty tree.
                    use crate::transparency::ct6962::merkle_tree_hash;
                    (self.state_root == merkle_tree_hash(&[]), false)
                } else {
                    (verify_inclusion_proof(&ip, &self.state_root), false)
                }
            }
            QueryProof::Ipa(p) => {
                // IPA proof carries the full vector — we can recompute the commitment.
                use crate::vc::ipa::DemoIpa;
                use crate::vc::VC;

                let commitment = DemoIpa::commit(&p.vector);
                let digest = DemoIpa::digest(&commitment);
                (
                    digest == self.state_root && DemoIpa::verify(&commitment, p.index, &p.value, p),
                    false,
                )
            }
            QueryProof::Kzg(p) => {
                // KZG proof is encoded; we cannot re-verify without the trusted setup.
                // Soft-check: require plausibly-encoded compressed BLS12-381 G1 bytes.
                // Full verification requires the consumer to have the same trusted setup.
                let plausible_len = p.proof_encoded.len() == 48;
                let has_non_zero_byte = p.proof_encoded.iter().any(|b| *b != 0);
                (plausible_len && has_non_zero_byte, true)
            }
        };

        let accepted = proof_valid && backend_consistent && upstream_refs_valid;
        let rejection_reason = if !proof_valid {
            if soft_verified {
                Some("kzg soft verification failed basic encoding checks".to_string())
            } else {
                Some("cryptographic proof verification failed".to_string())
            }
        } else if !upstream_refs_valid {
            Some(
                upstream_ref_validation
                    .err()
                    .unwrap_or_else(|| "invalid upstream envelope references".to_string()),
            )
        } else {
            None
        };

        EnvelopeVerification {
            version_supported: true,
            proof_valid,
            backend_consistent,
            witness_count: self.witness_sigs.len(),
            upstream_refs_valid,
            upstream_ref_count,
            soft_verified,
            accepted,
            rejection_reason,
        }
    }
}

#[derive(Serialize)]
struct EnvelopeHashInput<'a> {
    domain: &'static str,
    version: u32,
    key: &'a str,
    value: &'a TypedValue,
    backend: &'a VcBackend,
    proof: &'a QueryProof,
    #[serde(serialize_with = "serialize_hex_32")]
    state_root: &'a [u8; 32],
    commit_height: u64,
    witness_sigs: &'a [(String, String)],
    attestation: &'a Option<AttestationResult>,
    #[serde(serialize_with = "serialize_opt_hex_32")]
    author_puf: &'a Option<[u8; 32]>,
    lean_proof_ref: &'a Option<String>,
    upstream_hashes: &'a [String],
}

struct DagWalkState {
    upstream_edges: usize,
    max_depth_reached: usize,
    missing_refs: usize,
    hash_mismatches: usize,
    invalid_nodes: usize,
    cycle_detected: bool,
    depth_limit_exceeded: bool,
    nodes_limit_exceeded: bool,
    stop_walk: bool,
}

fn walk_provenance_ref<F>(
    expected_hash: [u8; 32],
    depth: usize,
    options: &ProvenanceDagVerifyOptions,
    resolve: &mut F,
    visited: &mut HashSet<[u8; 32]>,
    path: &mut Vec<[u8; 32]>,
    stats: &mut DagWalkState,
) -> Result<(), serde_json::Error>
where
    F: FnMut(&[u8; 32]) -> Option<ProofEnvelope>,
{
    if stats.stop_walk {
        return Ok(());
    }
    if depth > options.max_depth {
        stats.depth_limit_exceeded = true;
        stats.stop_walk = true;
        return Ok(());
    }
    stats.max_depth_reached = stats.max_depth_reached.max(depth);

    if path.contains(&expected_hash) {
        stats.cycle_detected = true;
        return Ok(());
    }
    if !visited.insert(expected_hash) {
        return Ok(());
    }
    if visited.len() > options.max_nodes {
        stats.nodes_limit_exceeded = true;
        stats.stop_walk = true;
        return Ok(());
    }

    let Some(envelope) = resolve(&expected_hash) else {
        stats.missing_refs += 1;
        return Ok(());
    };

    path.push(expected_hash);

    let actual_hash = envelope.provenance_hash()?;
    if actual_hash != expected_hash {
        stats.hash_mismatches += 1;
        if options.fail_fast_on_hash_mismatch {
            stats.stop_walk = true;
            path.pop();
            return Ok(());
        }
    }
    if !envelope.verify_locally().accepted {
        stats.invalid_nodes += 1;
    }

    let edges = canonical_upstream_refs(&envelope.upstream_envelopes);
    stats.upstream_edges += edges.len();
    for edge in edges {
        if stats.stop_walk {
            break;
        }
        walk_provenance_ref(
            edge.envelope_hash,
            depth + 1,
            options,
            resolve,
            visited,
            path,
            stats,
        )?;
    }

    path.pop();
    Ok(())
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(untagged)]
enum HexOrArray32 {
    Hex(String),
    Array([u8; 32]),
}

fn serialize_hex_32<S>(value: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&hex_encode_32(value))
}

fn deserialize_hex_or_array_32<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
where
    D: Deserializer<'de>,
{
    let repr = HexOrArray32::deserialize(deserializer)?;
    match repr {
        HexOrArray32::Array(bytes) => Ok(bytes),
        HexOrArray32::Hex(hex) => parse_hex_32(&hex).map_err(de::Error::custom),
    }
}

fn serialize_opt_hex_32<S>(value: &Option<[u8; 32]>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(bytes) => serializer.serialize_some(&hex_encode_32(bytes)),
        None => serializer.serialize_none(),
    }
}

fn deserialize_opt_hex_or_array_32<'de, D>(deserializer: D) -> Result<Option<[u8; 32]>, D::Error>
where
    D: Deserializer<'de>,
{
    let repr = Option::<HexOrArray32>::deserialize(deserializer)?;
    match repr {
        None => Ok(None),
        Some(HexOrArray32::Array(bytes)) => Ok(Some(bytes)),
        Some(HexOrArray32::Hex(hex)) => parse_hex_32(&hex).map(Some).map_err(de::Error::custom),
    }
}

fn canonical_upstream_refs(references: &[EnvelopeRef]) -> Vec<EnvelopeRef> {
    let mut refs = references.to_vec();
    refs.sort_by(|a, b| {
        a.envelope_hash
            .cmp(&b.envelope_hash)
            .then_with(|| a.key.cmp(&b.key))
            .then_with(|| a.commit_height.cmp(&b.commit_height))
    });
    refs
}

fn canonical_upstream_hashes(references: &[EnvelopeRef]) -> Vec<String> {
    let mut hashes: Vec<[u8; 32]> = references.iter().map(|r| r.envelope_hash).collect();
    hashes.sort();
    hashes.iter().map(hex_encode_32).collect()
}

fn validate_upstream_refs(references: &[EnvelopeRef]) -> Result<(), String> {
    let mut seen = HashSet::<[u8; 32]>::new();
    for reference in references {
        if reference.envelope_hash == [0u8; 32] {
            return Err("upstream envelope reference contains zero hash".to_string());
        }
        if !seen.insert(reference.envelope_hash) {
            return Err(format!(
                "duplicate upstream envelope reference {}",
                hex_encode_32(&reference.envelope_hash)
            ));
        }
    }
    Ok(())
}

fn hex_encode_32(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn parse_hex_32(input: &str) -> Result<[u8; 32], String> {
    let raw = input.strip_prefix("0x").unwrap_or(input);
    if raw.len() != 64 {
        return Err(format!("expected 64 hex chars, got {}", raw.len()));
    }

    let bytes = raw.as_bytes();
    let mut out = [0u8; 32];
    for i in 0..32 {
        let hi = hex_nibble(bytes[2 * i])
            .ok_or_else(|| format!("invalid hex nibble '{}' at {}", bytes[2 * i] as char, 2 * i))?;
        let lo = hex_nibble(bytes[(2 * i) + 1]).ok_or_else(|| {
            format!(
                "invalid hex nibble '{}' at {}",
                bytes[(2 * i) + 1] as char,
                (2 * i) + 1
            )
        })?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(10 + (byte - b'a')),
        b'A'..=b'F' => Some(10 + (byte - b'A')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::default_witness_cfg;
    use crate::protocol::NucleusDb;
    use crate::state::State;
    use std::collections::HashMap;

    fn make_db(backend: VcBackend) -> NucleusDb {
        let state = State::new(vec![42, 99, 7]);
        NucleusDb::new(state, backend, default_witness_cfg())
    }

    #[test]
    fn envelope_roundtrip_binary_merkle() {
        let mut db = make_db(VcBackend::BinaryMerkle);
        // Commit so we have witness sigs.
        let sections = vec![];
        let delta = crate::state::Delta { writes: vec![] };
        let _ = db.commit(delta, &sections);

        let value = TypedValue::Integer(42);
        let envelope = ProofEnvelope::from_query(&db, "test_key", 0, value.clone())
            .expect("query must succeed");

        // Serde roundtrip.
        let json = serde_json::to_string(&envelope).expect("serialize");
        let deserialized: ProofEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.key, "test_key");
        assert_eq!(deserialized.version, ENVELOPE_VERSION);

        // Verify locally.
        let result = deserialized.verify_locally();
        assert!(result.version_supported, "version should be supported");
        assert!(result.backend_consistent, "backend should be consistent");
        assert!(result.proof_valid, "proof should be valid");
        assert!(result.upstream_refs_valid);
        assert_eq!(result.upstream_ref_count, 0);
        assert!(!result.soft_verified, "binary merkle is fully verified");
        assert!(result.accepted, "envelope should be accepted");
    }

    #[test]
    fn envelope_roundtrip_ipa() {
        let mut db = make_db(VcBackend::Ipa);
        let sections = vec![];
        let delta = crate::state::Delta { writes: vec![] };
        let _ = db.commit(delta, &sections);

        let value = TypedValue::Integer(99);
        let envelope =
            ProofEnvelope::from_query(&db, "ipa_key", 1, value).expect("query must succeed");

        let json = serde_json::to_string(&envelope).expect("serialize");
        let deserialized: ProofEnvelope = serde_json::from_str(&json).expect("deserialize");

        let result = deserialized.verify_locally();
        assert!(result.version_supported);
        assert!(result.upstream_refs_valid);
        assert!(!result.soft_verified);
        assert!(result.accepted, "IPA envelope should verify");
    }

    #[test]
    fn envelope_backend_mismatch_rejected() {
        let mut db = make_db(VcBackend::BinaryMerkle);
        let sections = vec![];
        let delta = crate::state::Delta { writes: vec![] };
        let _ = db.commit(delta, &sections);

        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "test_key", 0, value).expect("query must succeed");

        // Tamper: claim IPA backend but proof is BinaryMerkle.
        envelope.backend = VcBackend::Ipa;

        let result = envelope.verify_locally();
        assert!(result.version_supported);
        assert!(!result.backend_consistent);
        assert!(!result.accepted);
    }

    #[test]
    fn envelope_kzg_soft_verify() {
        let mut db = make_db(VcBackend::Kzg);
        let sections = vec![];
        let delta = crate::state::Delta { writes: vec![] };
        let _ = db.commit(delta, &sections);

        let value = TypedValue::Integer(7);
        let envelope =
            ProofEnvelope::from_query(&db, "kzg_key", 2, value).expect("query must succeed");

        let json = serde_json::to_string(&envelope).expect("serialize");
        let deserialized: ProofEnvelope = serde_json::from_str(&json).expect("deserialize");

        let result = deserialized.verify_locally();
        assert!(result.version_supported);
        assert!(result.backend_consistent);
        assert!(result.upstream_refs_valid);
        // KZG is intentionally soft-verified without trusted setup.
        assert!(result.proof_valid);
        assert!(result.soft_verified);
        assert!(result.accepted);
    }

    #[test]
    fn envelope_kzg_soft_verify_rejects_malformed_encoding() {
        let mut db = make_db(VcBackend::Kzg);
        let sections = vec![];
        let delta = crate::state::Delta { writes: vec![] };
        let _ = db.commit(delta, &sections);

        let value = TypedValue::Integer(7);
        let mut envelope =
            ProofEnvelope::from_query(&db, "kzg_key", 2, value).expect("query must succeed");
        match &mut envelope.proof {
            QueryProof::Kzg(p) => p.proof_encoded = vec![1, 2, 3],
            _ => panic!("expected kzg proof"),
        }

        let result = envelope.verify_locally();
        assert!(result.soft_verified);
        assert!(!result.proof_valid);
        assert!(!result.accepted);
    }

    #[test]
    fn typed_value_text_in_envelope() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Text("hello proof".to_string());
        let envelope = ProofEnvelope::from_query(&db, "text_key", 0, value.clone())
            .expect("query must succeed");

        let json = serde_json::to_string(&envelope).expect("serialize");
        let deserialized: ProofEnvelope = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.value, value);
    }

    #[test]
    fn envelope_rejects_unsupported_version() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "test_key", 0, value).expect("query must succeed");
        envelope.version = ENVELOPE_VERSION + 1;

        let result = envelope.verify_locally();
        assert!(!result.version_supported);
        assert!(!result.accepted);
        assert!(result
            .rejection_reason
            .unwrap_or_default()
            .contains("unsupported envelope version"));
    }

    #[test]
    fn envelope_serializes_hashes_as_hex_strings() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "test_key", 0, value).expect("query must succeed");
        envelope.author_puf = Some([0xAB; 32]);

        let json = serde_json::to_value(&envelope).expect("serialize");
        assert!(
            json["state_root"].as_str().is_some(),
            "state_root should serialize as hex string"
        );
        assert_eq!(
            json["author_puf"]
                .as_str()
                .expect("author_puf string")
                .len(),
            64,
            "author_puf hex should be 64 chars"
        );
    }

    #[test]
    fn envelope_deserializes_legacy_array_hashes() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let envelope =
            ProofEnvelope::from_query(&db, "test_key", 0, value).expect("query must succeed");
        let mut json = serde_json::to_value(&envelope).expect("serialize");
        json["state_root"] = serde_json::Value::Array(
            envelope
                .state_root
                .iter()
                .map(|b| serde_json::Value::from(*b))
                .collect(),
        );
        json["author_puf"] = serde_json::Value::Array(
            [0xCDu8; 32]
                .iter()
                .map(|b| serde_json::Value::from(*b))
                .collect(),
        );

        let decoded: ProofEnvelope = serde_json::from_value(json).expect("legacy decode");
        assert_eq!(decoded.state_root, envelope.state_root);
        assert_eq!(decoded.author_puf, Some([0xCD; 32]));
    }

    #[test]
    fn provenance_hash_is_deterministic_for_identical_content() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "deterministic", 0, value).expect("query must succeed");
        envelope.author_puf = Some([0x11; 32]);

        let hash_a = envelope.provenance_hash().expect("hash");
        let hash_b = envelope.provenance_hash().expect("hash");
        assert_eq!(hash_a, hash_b);
        assert_eq!(envelope.provenance_hash_hex().expect("hash hex").len(), 64);
    }

    #[test]
    fn provenance_hash_ignores_upstream_ref_metadata_fields() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "meta", 0, value).expect("query must succeed");
        let ref_hash = [0x77; 32];
        envelope.upstream_envelopes = vec![EnvelopeRef {
            envelope_hash: ref_hash,
            key: Some("alpha".to_string()),
            commit_height: Some(10),
        }];

        let hash_a = envelope.provenance_hash().expect("hash a");
        envelope.upstream_envelopes = vec![EnvelopeRef {
            envelope_hash: ref_hash,
            key: Some("beta".to_string()),
            commit_height: Some(999),
        }];
        let hash_b = envelope.provenance_hash().expect("hash b");

        assert_eq!(hash_a, hash_b);
    }

    #[test]
    fn provenance_hash_is_order_independent_for_upstream_refs() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "order", 0, value).expect("query must succeed");
        let a = EnvelopeRef {
            envelope_hash: [0x01; 32],
            key: Some("a".to_string()),
            commit_height: Some(1),
        };
        let b = EnvelopeRef {
            envelope_hash: [0x02; 32],
            key: Some("b".to_string()),
            commit_height: Some(2),
        };

        envelope.upstream_envelopes = vec![a.clone(), b.clone()];
        let hash_ab = envelope.provenance_hash().expect("hash ab");

        envelope.upstream_envelopes = vec![b, a];
        let hash_ba = envelope.provenance_hash().expect("hash ba");

        assert_eq!(hash_ab, hash_ba);
    }

    #[test]
    fn to_envelope_ref_roundtrip_hex_serde() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let envelope =
            ProofEnvelope::from_query(&db, "ref_key", 0, value).expect("query must succeed");

        let reference = envelope.to_envelope_ref().expect("reference");
        let json = serde_json::to_value(&reference).expect("serialize ref");
        assert!(json["envelope_hash"].as_str().is_some());
        assert_eq!(
            json["envelope_hash"].as_str().expect("hash string").len(),
            64
        );

        let decoded: EnvelopeRef = serde_json::from_value(json).expect("deserialize ref");
        assert_eq!(decoded.envelope_hash, reference.envelope_hash);
        assert_eq!(decoded.key, Some("ref_key".to_string()));
    }

    #[test]
    fn add_upstream_ref_dedupes_by_hash() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut root =
            ProofEnvelope::from_query(&db, "root", 0, value).expect("query must succeed");

        let reference = EnvelopeRef {
            envelope_hash: [0xAB; 32],
            key: Some("child".to_string()),
            commit_height: Some(7),
        };
        root.add_upstream_ref(reference.clone());
        root.add_upstream_ref(reference);
        assert_eq!(root.upstream_envelopes.len(), 1);
    }

    #[test]
    fn verify_locally_rejects_duplicate_upstream_references() {
        let db = make_db(VcBackend::BinaryMerkle);
        let value = TypedValue::Integer(42);
        let mut envelope =
            ProofEnvelope::from_query(&db, "dup", 0, value).expect("query must succeed");
        let dup = EnvelopeRef {
            envelope_hash: [0xCC; 32],
            key: Some("x".to_string()),
            commit_height: Some(1),
        };
        envelope.upstream_envelopes.push(dup.clone());
        envelope.upstream_envelopes.push(dup);

        let result = envelope.verify_locally();
        assert!(!result.upstream_refs_valid);
        assert!(!result.accepted);
        assert_eq!(result.upstream_ref_count, 2);
        assert!(result
            .rejection_reason
            .unwrap_or_default()
            .contains("duplicate upstream envelope reference"));
    }

    #[test]
    fn verify_provenance_dag_accepts_valid_chain() {
        let db = make_db(VcBackend::BinaryMerkle);

        let leaf =
            ProofEnvelope::from_query(&db, "leaf", 0, TypedValue::Integer(42)).expect("leaf query");
        let leaf_ref = leaf.to_envelope_ref().expect("leaf ref");

        let mut mid =
            ProofEnvelope::from_query(&db, "mid", 1, TypedValue::Integer(99)).expect("mid query");
        mid.add_upstream_ref(leaf_ref);
        let mid_ref = mid.to_envelope_ref().expect("mid ref");

        let mut root =
            ProofEnvelope::from_query(&db, "root", 2, TypedValue::Integer(7)).expect("root query");
        root.add_upstream_ref(mid_ref);

        let leaf_hash = leaf.provenance_hash().expect("leaf hash");
        let mid_hash = mid.provenance_hash().expect("mid hash");

        let mut store = HashMap::<[u8; 32], ProofEnvelope>::new();
        store.insert(leaf_hash, leaf);
        store.insert(mid_hash, mid);

        let dag = root
            .verify_provenance_dag(|h| store.get(h).cloned(), 8)
            .expect("dag verify");
        assert!(dag.root_accepted);
        assert_eq!(dag.missing_refs, 0);
        assert_eq!(dag.hash_mismatches, 0);
        assert_eq!(dag.invalid_nodes, 0);
        assert!(!dag.cycle_detected);
        assert!(dag.accepted);
    }

    #[test]
    fn verify_provenance_dag_detects_missing_reference() {
        let db = make_db(VcBackend::BinaryMerkle);
        let mut root =
            ProofEnvelope::from_query(&db, "root", 0, TypedValue::Integer(42)).expect("query");
        root.add_upstream_ref(EnvelopeRef {
            envelope_hash: [0x44; 32],
            key: Some("missing".to_string()),
            commit_height: Some(5),
        });

        let dag = root
            .verify_provenance_dag(|_| None, 4)
            .expect("dag verify should serialize");
        assert_eq!(dag.missing_refs, 1);
        assert!(!dag.accepted);
    }

    #[test]
    fn verify_provenance_dag_detects_hash_mismatch() {
        let db = make_db(VcBackend::BinaryMerkle);
        let mut root =
            ProofEnvelope::from_query(&db, "root", 0, TypedValue::Integer(42)).expect("query");
        let child = ProofEnvelope::from_query(&db, "child", 1, TypedValue::Integer(99))
            .expect("child query");

        root.add_upstream_ref(EnvelopeRef {
            envelope_hash: [0x55; 32],
            key: Some("child".to_string()),
            commit_height: Some(2),
        });

        let dag = root
            .verify_provenance_dag(|_| Some(child.clone()), 4)
            .expect("dag verify");
        assert!(dag.hash_mismatches >= 1);
        assert!(!dag.accepted);
    }

    #[test]
    fn verify_provenance_dag_detects_cycle_and_depth_limit() {
        let db = make_db(VcBackend::BinaryMerkle);

        let mut a = ProofEnvelope::from_query(&db, "a", 0, TypedValue::Integer(42)).expect("a");
        let mut b = ProofEnvelope::from_query(&db, "b", 1, TypedValue::Integer(99)).expect("b");

        let a_seed_ref = a.to_envelope_ref().expect("a seed");
        let b_seed_ref = b.to_envelope_ref().expect("b seed");
        a.upstream_envelopes = vec![b_seed_ref.clone()];
        b.upstream_envelopes = vec![a_seed_ref.clone()];

        let mut root =
            ProofEnvelope::from_query(&db, "root", 2, TypedValue::Integer(7)).expect("root");
        root.upstream_envelopes = vec![a_seed_ref.clone()];

        let mut store = HashMap::<[u8; 32], ProofEnvelope>::new();
        store.insert(a_seed_ref.envelope_hash, a.clone());
        store.insert(b_seed_ref.envelope_hash, b.clone());

        let dag_cycle = root
            .verify_provenance_dag(|h| store.get(h).cloned(), 8)
            .expect("dag verify");
        assert!(dag_cycle.cycle_detected);
        assert!(!dag_cycle.accepted);

        let dag_depth = root
            .verify_provenance_dag(|h| store.get(h).cloned(), 1)
            .expect("dag verify");
        assert!(dag_depth.depth_limit_exceeded);
        assert!(!dag_depth.accepted);
    }

    #[test]
    fn verify_provenance_dag_respects_node_limit() {
        let db = make_db(VcBackend::BinaryMerkle);
        let mut root =
            ProofEnvelope::from_query(&db, "root", 0, TypedValue::Integer(42)).expect("root");
        let child =
            ProofEnvelope::from_query(&db, "child", 1, TypedValue::Integer(99)).expect("child");
        let child_ref = child.to_envelope_ref().expect("child ref");
        root.add_upstream_ref(child_ref.clone());

        let child_hash = child_ref.envelope_hash;
        let dag = root
            .verify_provenance_dag_with_options(
                |h| {
                    if *h == child_hash {
                        Some(child.clone())
                    } else {
                        None
                    }
                },
                ProvenanceDagVerifyOptions {
                    max_depth: 8,
                    max_nodes: 0,
                    fail_fast_on_hash_mismatch: false,
                },
            )
            .expect("dag verify");
        assert!(dag.nodes_limit_exceeded);
        assert!(!dag.accepted);
    }

    #[test]
    fn verify_provenance_dag_fail_fast_on_hash_mismatch() {
        let db = make_db(VcBackend::BinaryMerkle);
        let mut root =
            ProofEnvelope::from_query(&db, "root", 0, TypedValue::Integer(42)).expect("root");
        root.add_upstream_ref(EnvelopeRef {
            envelope_hash: [0xAA; 32],
            key: Some("a".to_string()),
            commit_height: Some(1),
        });
        root.add_upstream_ref(EnvelopeRef {
            envelope_hash: [0xBB; 32],
            key: Some("b".to_string()),
            commit_height: Some(2),
        });

        let child =
            ProofEnvelope::from_query(&db, "child", 1, TypedValue::Integer(99)).expect("child");

        let mut resolve_calls = 0usize;
        let dag = root
            .verify_provenance_dag_with_options(
                |_| {
                    resolve_calls += 1;
                    Some(child.clone())
                },
                ProvenanceDagVerifyOptions {
                    max_depth: 8,
                    max_nodes: 10,
                    fail_fast_on_hash_mismatch: true,
                },
            )
            .expect("dag verify");
        assert_eq!(dag.hash_mismatches, 1);
        assert_eq!(resolve_calls, 1);
        assert!(!dag.accepted);
    }
}
