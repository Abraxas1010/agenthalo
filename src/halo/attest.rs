use crate::halo::config;
use crate::halo::hash::{self, HashAlgorithm};
use crate::halo::schema::TraceEvent;
use crate::halo::trace::{list_sessions, now_unix_secs, session_events};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AttestationRequest {
    pub session_id: String,
    pub anonymous: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AttestationResult {
    pub session_id: Option<String>,
    pub blinded_session_ref: Option<String>,
    pub merkle_root: String,
    pub event_count: u64,
    pub content_hashes: Vec<String>,
    pub witness_algorithm: String,
    pub attestation_digest: String,
    pub timestamp: u64,
    pub anonymous: bool,
    pub proof_type: String,
    pub anonymous_membership_proof: Option<AnonymousMembershipProof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groth16_proof: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub groth16_public_inputs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tx_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contract_address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chain: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnonymousMembershipProof {
    pub membership_root: String,
    pub leaf_hash: String,
    pub tree_size: u64,
    pub leaf_index: u64,
    pub steps: Vec<AnonymousMembershipStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct AnonymousMembershipStep {
    pub sibling_hash: String,
    pub sibling_is_left: bool,
}

pub fn resolve_session_id(db_path: &Path, session_id: Option<&str>) -> Result<String, String> {
    if let Some(session_id) = session_id {
        let trimmed = session_id.trim();
        if trimmed.is_empty() {
            return Err("session id cannot be empty".to_string());
        }
        return Ok(trimmed.to_string());
    }
    let sessions = list_sessions(db_path)?;
    let latest = sessions
        .first()
        .ok_or_else(|| "no recorded sessions available to attest".to_string())?;
    Ok(latest.session_id.clone())
}

pub fn attest_session(
    db_path: &Path,
    request: AttestationRequest,
) -> Result<AttestationResult, String> {
    let events = session_events(db_path, &request.session_id)?;
    if events.is_empty() {
        return Err(format!(
            "session {} has no trace events",
            request.session_id
        ));
    }

    let mut verified_hashes = Vec::with_capacity(events.len());
    for event in &events {
        verify_event_hash(event)?;
        verified_hashes.push(event.content_hash.to_ascii_lowercase());
    }

    let merkle_root = merkle_root_from_hashes(&verified_hashes)?;
    let timestamp = now_unix_secs();
    let blinded = blinded_session_reference(&request.session_id);
    let anonymous_membership_proof = if request.anonymous {
        let sessions = list_sessions(db_path)?;
        Some(build_anonymous_membership_proof(&sessions, &blinded)?)
    } else {
        None
    };
    let public_ref = if request.anonymous {
        blinded.clone()
    } else {
        request.session_id.clone()
    };
    let attestation_digest =
        digest_attestation(&public_ref, &merkle_root, verified_hashes.len() as u64);

    Ok(AttestationResult {
        session_id: if request.anonymous {
            None
        } else {
            Some(request.session_id)
        },
        blinded_session_ref: if request.anonymous {
            Some(blinded)
        } else {
            None
        },
        merkle_root,
        event_count: verified_hashes.len() as u64,
        content_hashes: verified_hashes,
        witness_algorithm: "ML-DSA-65".to_string(),
        attestation_digest,
        timestamp,
        anonymous: request.anonymous,
        proof_type: if request.anonymous {
            "merkle-sha512+anon-membership".to_string()
        } else {
            "merkle-sha512".to_string()
        },
        anonymous_membership_proof,
        groth16_proof: None,
        groth16_public_inputs: None,
        tx_hash: None,
        contract_address: None,
        block_number: None,
        chain: None,
    })
}

pub fn save_attestation(
    canonical_session_id: &str,
    result: &AttestationResult,
) -> Result<PathBuf, String> {
    config::ensure_halo_dir()?;
    config::ensure_attestations_dir()?;
    let path = config::attestations_dir().join(format!("{canonical_session_id}.json"));
    let raw =
        serde_json::to_vec_pretty(result).map_err(|e| format!("serialize attestation: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write attestation {}: {e}", path.display()))?;
    Ok(path)
}

fn verify_event_hash(event: &TraceEvent) -> Result<(), String> {
    if event.content_hash.trim().is_empty() {
        return Err(format!(
            "session event {} has empty content_hash",
            event.seq
        ));
    }
    let encoded = serde_json::to_vec(&event.content)
        .map_err(|e| format!("serialize event {} content: {e}", event.seq))?;
    // Detect hash algorithm from length: 128 hex chars = SHA-512, 64 = SHA-256.
    let algo = if event.content_hash.len() > 64 {
        HashAlgorithm::Sha512
    } else {
        HashAlgorithm::Sha256
    };
    let expected = hash::hash_hex(&algo, &encoded);
    if expected != event.content_hash.to_ascii_lowercase() {
        return Err(format!(
            "content hash mismatch at seq {}: expected {}, got {}",
            event.seq, expected, event.content_hash
        ));
    }
    Ok(())
}

fn blinded_session_reference(session_id: &str) -> String {
    let payload = format!("agenthalo.attestation.blind.v1:{session_id}");
    hash::hash_hex(&HashAlgorithm::CURRENT, payload.as_bytes())
}

fn digest_attestation(public_session_ref: &str, merkle_root: &str, event_count: u64) -> String {
    let payload =
        format!("agenthalo.attestation.digest.v1:{public_session_ref}:{merkle_root}:{event_count}");
    hash::hash_hex(&HashAlgorithm::CURRENT, payload.as_bytes())
}

fn merkle_root_from_hashes(content_hashes: &[String]) -> Result<String, String> {
    if content_hashes.is_empty() {
        return Err("cannot build Merkle root from empty hash list".to_string());
    }
    let algo = &HashAlgorithm::CURRENT;
    let mut level = Vec::with_capacity(content_hashes.len());
    for h in content_hashes {
        let bytes = hex_decode_var(h)?;
        level.push(leaf_hash(algo, &bytes));
    }

    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut idx = 0usize;
        while idx < level.len() {
            if idx + 1 < level.len() {
                next.push(node_hash(algo, &level[idx], &level[idx + 1]));
            } else {
                next.push(level[idx].clone());
            }
            idx += 2;
        }
        level = next;
    }
    Ok(hex_encode(&level[0]))
}

fn leaf_hash(algo: &HashAlgorithm, content_hash: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(1 + content_hash.len());
    input.push(0x00);
    input.extend_from_slice(content_hash);
    hash::hash_bytes(algo, &input)
}

fn node_hash(algo: &HashAlgorithm, left: &[u8], right: &[u8]) -> Vec<u8> {
    let mut input = Vec::with_capacity(1 + left.len() + right.len());
    input.push(0x01);
    input.extend_from_slice(left);
    input.extend_from_slice(right);
    hash::hash_bytes(algo, &input)
}

fn anon_leaf_hash(algo: &HashAlgorithm, blinded_ref: &[u8]) -> Vec<u8> {
    let domain = b"agenthalo.anon.membership.leaf.v1";
    let mut input = Vec::with_capacity(domain.len() + 1 + blinded_ref.len());
    input.extend_from_slice(domain);
    input.push(0u8);
    input.extend_from_slice(blinded_ref);
    hash::hash_bytes(algo, &input)
}

fn anon_node_hash(algo: &HashAlgorithm, left: &[u8], right: &[u8]) -> Vec<u8> {
    let domain = b"agenthalo.anon.membership.node.v1";
    let mut input = Vec::with_capacity(domain.len() + 1 + left.len() + right.len());
    input.extend_from_slice(domain);
    input.push(0u8);
    input.extend_from_slice(left);
    input.extend_from_slice(right);
    hash::hash_bytes(algo, &input)
}

fn build_anonymous_membership_proof(
    sessions: &[crate::halo::schema::SessionMetadata],
    target_blinded_ref_hex: &str,
) -> Result<AnonymousMembershipProof, String> {
    let algo = &HashAlgorithm::CURRENT;
    let mut leaves: Vec<(String, Vec<u8>)> = sessions
        .iter()
        .map(|s| {
            let blinded = blinded_session_reference(&s.session_id);
            let bytes = hex_decode_var(&blinded)?;
            Ok((blinded, anon_leaf_hash(algo, &bytes)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    if leaves.is_empty() {
        return Err("cannot build anonymous membership proof: no sessions found".to_string());
    }
    leaves.sort_by(|a, b| a.0.cmp(&b.0));

    let target_leaf = {
        let target_bytes = hex_decode_var(target_blinded_ref_hex)?;
        anon_leaf_hash(algo, &target_bytes)
    };
    let mut target_index = leaves
        .iter()
        .position(|(_, leaf)| *leaf == target_leaf)
        .ok_or_else(|| "target session not found in anonymous membership tree".to_string())?;

    let mut level: Vec<Vec<u8>> = leaves.iter().map(|(_, leaf)| leaf.clone()).collect();
    let mut steps = Vec::new();
    while level.len() > 1 {
        if target_index % 2 == 0 {
            if target_index + 1 < level.len() {
                steps.push(AnonymousMembershipStep {
                    sibling_hash: hex_encode(&level[target_index + 1]),
                    sibling_is_left: false,
                });
            }
        } else {
            steps.push(AnonymousMembershipStep {
                sibling_hash: hex_encode(&level[target_index - 1]),
                sibling_is_left: true,
            });
        }

        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut i = 0usize;
        while i < level.len() {
            if i + 1 < level.len() {
                next.push(anon_node_hash(algo, &level[i], &level[i + 1]));
            } else {
                next.push(level[i].clone());
            }
            i += 2;
        }
        target_index /= 2;
        level = next;
    }
    let membership_root = level
        .first()
        .map(|h| hex_encode(h))
        .ok_or_else(|| "failed to build anonymous membership root".to_string())?;

    let leaf_index = leaves
        .iter()
        .position(|(_, leaf)| *leaf == target_leaf)
        .ok_or_else(|| "target session not found for membership proof".to_string())?
        as u64;

    Ok(AnonymousMembershipProof {
        membership_root,
        leaf_hash: hex_encode(&target_leaf),
        tree_size: leaves.len() as u64,
        leaf_index,
        steps,
    })
}

pub fn verify_anonymous_membership_proof(proof: &AnonymousMembershipProof) -> Result<bool, String> {
    // Detect algorithm from hash length: 128 hex = SHA-512, 64 hex = SHA-256.
    let algo = if proof.leaf_hash.len() > 64 {
        HashAlgorithm::Sha512
    } else {
        HashAlgorithm::Sha256
    };
    let mut acc = hex_decode_var(&proof.leaf_hash)?;
    for step in &proof.steps {
        let sibling = hex_decode_var(&step.sibling_hash)?;
        acc = if step.sibling_is_left {
            anon_node_hash(&algo, &sibling, &acc)
        } else {
            anon_node_hash(&algo, &acc, &sibling)
        };
    }
    Ok(hex_encode(&acc).eq_ignore_ascii_case(&proof.membership_root))
}

/// Decode variable-length hex string to bytes (supports both SHA-256 and SHA-512 lengths).
fn hex_decode_var(s: &str) -> Result<Vec<u8>, String> {
    if s.is_empty() || !s.len().is_multiple_of(2) {
        return Err(format!("invalid hex hash length {}", s.len()));
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for chunk in s.as_bytes().chunks_exact(2) {
        let hi = hex_nibble(chunk[0]).ok_or_else(|| format!("invalid hex character in {s}"))?;
        let lo = hex_nibble(chunk[1]).ok_or_else(|| format!("invalid hex character in {s}"))?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
    use crate::halo::trace::TraceWriter;
    use serde_json::json;

    fn temp_db_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "agenthalo_attest_{tag}_{}_{}.ndb",
            std::process::id(),
            now_unix_secs()
        ))
    }

    fn write_session_with_events(
        db_path: &Path,
        session_id: &str,
        bad_hash: bool,
    ) -> Result<(), String> {
        let mut writer = TraceWriter::new(db_path)?;
        writer.start_session(SessionMetadata {
            session_id: session_id.to_string(),
            agent: "test-agent".to_string(),
            model: Some("gpt-test".to_string()),
            started_at: now_unix_secs(),
            ended_at: None,
            prompt: Some("attest test".to_string()),
            status: SessionStatus::Running,
            user_id: None,
            machine_id: None,
            puf_digest: None,
        })?;

        writer.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Assistant,
            content: json!({"text":"hello"}),
            input_tokens: Some(1),
            output_tokens: Some(2),
            cache_read_tokens: Some(0),
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: if bad_hash {
                "00".repeat(32)
            } else {
                String::new()
            },
        })?;

        writer.end_session(SessionStatus::Completed)?;
        Ok(())
    }

    #[test]
    fn merkle_root_deterministic() {
        let db_path = temp_db_path("det");
        let session = format!("sess-det-{}", now_unix_secs());
        write_session_with_events(&db_path, &session, false).expect("seed events");

        let first = attest_session(
            &db_path,
            AttestationRequest {
                session_id: session.clone(),
                anonymous: false,
            },
        )
        .expect("first attestation");
        let second = attest_session(
            &db_path,
            AttestationRequest {
                session_id: session,
                anonymous: false,
            },
        )
        .expect("second attestation");

        assert_eq!(first.merkle_root, second.merkle_root);
        assert_eq!(first.attestation_digest, second.attestation_digest);
    }

    #[test]
    fn attestation_digest_includes_session_identity() {
        let db_path = temp_db_path("digest");
        let s1 = format!("sess-a-{}", now_unix_secs());
        let s2 = format!("sess-b-{}", now_unix_secs());
        write_session_with_events(&db_path, &s1, false).expect("seed first");
        write_session_with_events(&db_path, &s2, false).expect("seed second");

        let r1 = attest_session(
            &db_path,
            AttestationRequest {
                session_id: s1,
                anonymous: false,
            },
        )
        .expect("attest first");
        let r2 = attest_session(
            &db_path,
            AttestationRequest {
                session_id: s2,
                anonymous: false,
            },
        )
        .expect("attest second");
        assert_ne!(r1.attestation_digest, r2.attestation_digest);
    }

    #[test]
    fn anonymous_omits_session_id() {
        let db_path = temp_db_path("anon");
        let session = format!("sess-anon-{}", now_unix_secs());
        write_session_with_events(&db_path, &session, false).expect("seed session");

        let result = attest_session(
            &db_path,
            AttestationRequest {
                session_id: session,
                anonymous: true,
            },
        )
        .expect("anonymous attestation");

        assert!(result.session_id.is_none());
        assert!(result.blinded_session_ref.is_some());
        assert_eq!(result.proof_type, "merkle-sha512+anon-membership");
        let membership = result
            .anonymous_membership_proof
            .as_ref()
            .expect("membership proof exists");
        assert!(
            verify_anonymous_membership_proof(membership).expect("verify membership proof"),
            "membership proof should verify"
        );
    }

    #[test]
    fn empty_session_returns_error() {
        let db_path = temp_db_path("empty");
        let session = format!("sess-empty-{}", now_unix_secs());
        let mut writer = TraceWriter::new(&db_path).expect("writer");
        writer
            .start_session(SessionMetadata {
                session_id: session.clone(),
                agent: "test".to_string(),
                model: None,
                started_at: now_unix_secs(),
                ended_at: None,
                prompt: None,
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })
            .expect("start");
        writer.end_session(SessionStatus::Completed).expect("end");

        let err = attest_session(
            &db_path,
            AttestationRequest {
                session_id: session,
                anonymous: false,
            },
        )
        .expect_err("must fail for empty session");
        assert!(err.contains("has no trace events"));
    }

    #[test]
    fn content_hash_verification_detects_tamper() {
        let db_path = temp_db_path("tamper");
        let session = format!("sess-tamper-{}", now_unix_secs());
        write_session_with_events(&db_path, &session, true).expect("seed tampered event");

        let err = attest_session(
            &db_path,
            AttestationRequest {
                session_id: session,
                anonymous: false,
            },
        )
        .expect_err("attestation should reject tampered content hash");
        assert!(err.contains("content hash mismatch"));
    }
}
