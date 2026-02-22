use crate::protocol::CommitEntry;
use crate::transparency::ct6962::{
    hex_encode, sth_signature, verify_consistency_proof, ConsistencyProof, NodeHash,
};
use crate::witness::{
    default_algorithm_tag, parse_algorithm_tag, sign_message, verify_quorum_for_algorithm,
    WitnessConfig, WitnessError,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::fmt::{Display, Formatter};
use std::fs;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsistencyEvidence {
    pub old_size: u64,
    pub new_size: u64,
    pub old_root: String,
    pub new_root: String,
    pub proof: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitEvidence {
    pub height: u64,
    pub prev_state_root: String,
    pub state_root: String,
    pub delta_digest: u64,
    pub cert_digest: u64,
    pub sheaf_coherence_digest: u64,
    #[serde(default)]
    pub vc_backend_id: String,
    #[serde(default)]
    pub vc_scheme_id: String,
    #[serde(default)]
    pub vc_domain_separator: String,
    #[serde(default)]
    pub vc_max_degree: usize,
    pub sth_size: u64,
    pub sth_root: String,
    pub sth_timestamp_unix_secs: u64,
    pub sth_sig: String,
    #[serde(default = "default_algorithm_tag")]
    pub witness_signature_algorithm: String,
    pub witness_sigs: Vec<(String, String)>,
    pub consistency_with_prev: Option<ConsistencyEvidence>,
}

#[derive(Debug)]
pub enum EvidenceError {
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl Display for EvidenceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
        }
    }
}

impl std::error::Error for EvidenceError {}

impl From<std::io::Error> for EvidenceError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for EvidenceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayError {
    EmptyEvidence,
    HeightMismatch { expected: u64, got: u64 },
    PrevRootMismatch { expected: String, got: String },
    SthSizeMismatch { expected: u64, got: u64 },
    InvalidSthSignature { expected: String, got: String },
    InvalidHashHex { height: u64 },
    MissingConsistencyEvidence { height: u64 },
    UnexpectedConsistencyEvidenceAtGenesis,
    InvalidConsistencyEvidence { height: u64 },
    InvalidWitnessSignatureAlgorithm { height: u64, got: String },
    InvalidVcBackend { height: u64, got: String },
    WitnessQuorumFailed { height: u64 },
}

impl Display for ReplayError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyEvidence => write!(f, "empty evidence"),
            Self::HeightMismatch { expected, got } => {
                write!(f, "height mismatch: expected {expected}, got {got}")
            }
            Self::PrevRootMismatch { expected, got } => {
                write!(f, "prev root mismatch: expected {expected}, got {got}")
            }
            Self::SthSizeMismatch { expected, got } => {
                write!(f, "sth size mismatch: expected {expected}, got {got}")
            }
            Self::InvalidSthSignature { expected, got } => {
                write!(f, "invalid sth signature: expected {expected}, got {got}")
            }
            Self::InvalidHashHex { height } => write!(f, "invalid hash hex at height {height}"),
            Self::MissingConsistencyEvidence { height } => {
                write!(f, "missing consistency evidence at height {height}")
            }
            Self::UnexpectedConsistencyEvidenceAtGenesis => {
                write!(f, "unexpected consistency evidence at genesis")
            }
            Self::InvalidConsistencyEvidence { height } => {
                write!(f, "invalid consistency evidence at height {height}")
            }
            Self::InvalidWitnessSignatureAlgorithm { height, got } => {
                write!(
                    f,
                    "invalid witness signature algorithm at height {height}: {got}"
                )
            }
            Self::InvalidVcBackend { height, got } => {
                write!(f, "invalid vc backend id at height {height}: {got}")
            }
            Self::WitnessQuorumFailed { height } => {
                write!(f, "witness quorum failed at height {height}")
            }
        }
    }
}

impl std::error::Error for ReplayError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceBundleManifest {
    pub schema: String,
    pub created_unix_secs: u64,
    pub retention_days: u32,
    pub delete_after_unix_secs: u64,
    pub source_evidence_path: String,
    pub evidence_file: String,
    pub evidence_records: usize,
    pub evidence_sha512: String,
    pub replay_verified: bool,
    pub payload_sha512: String,
    pub witness_threshold: usize,
    pub witness_signatures: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum BundleError {
    Evidence(EvidenceError),
    Replay(ReplayError),
    Io(std::io::Error),
    Json(serde_json::Error),
    Time(String),
    WitnessSigning(WitnessError),
    BundleWitnessQuorumFailed,
}

impl Display for BundleError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evidence(e) => write!(f, "{e}"),
            Self::Replay(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Json(e) => write!(f, "json error: {e}"),
            Self::Time(e) => write!(f, "time error: {e}"),
            Self::WitnessSigning(e) => write!(f, "witness signing error: {e:?}"),
            Self::BundleWitnessQuorumFailed => write!(f, "bundle witness quorum failed"),
        }
    }
}

impl std::error::Error for BundleError {}

impl From<EvidenceError> for BundleError {
    fn from(value: EvidenceError) -> Self {
        Self::Evidence(value)
    }
}

impl From<ReplayError> for BundleError {
    fn from(value: ReplayError) -> Self {
        Self::Replay(value)
    }
}

impl From<std::io::Error> for BundleError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

fn decode_hex_exact<const N: usize>(hex: &str) -> Option<[u8; N]> {
    let bytes = hex.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for i in 0..N {
        let hi = hex_nibble(bytes[2 * i])?;
        let lo = hex_nibble(bytes[(2 * i) + 1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn msg_for_entry(e: &CommitEvidence) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        e.height, e.prev_state_root, e.state_root, e.sth_root, e.sth_timestamp_unix_secs
    )
}

impl CommitEvidence {
    pub fn from_entry(
        prev: Option<&CommitEntry>,
        entry: &CommitEntry,
        consistency_with_prev: Option<&ConsistencyProof>,
    ) -> Self {
        let consistency_with_prev = match (prev, consistency_with_prev) {
            (Some(prev_entry), Some(p)) => Some(ConsistencyEvidence {
                old_size: p.old_size,
                new_size: p.new_size,
                old_root: hex_encode(&prev_entry.sth.root_hash),
                new_root: hex_encode(&entry.sth.root_hash),
                proof: p.path.iter().map(|h| hex_encode(h)).collect(),
            }),
            _ => None,
        };

        Self {
            height: entry.height,
            prev_state_root: hex_encode(&entry.prev_state_root),
            state_root: hex_encode(&entry.state_root),
            delta_digest: entry.delta_digest,
            cert_digest: entry.cert_digest,
            sheaf_coherence_digest: entry.sheaf_coherence_digest,
            vc_backend_id: entry.vc_backend_id.clone(),
            vc_scheme_id: entry.vc_scheme_id.clone(),
            vc_domain_separator: entry.vc_domain_separator.clone(),
            vc_max_degree: entry.vc_max_degree,
            sth_size: entry.sth.tree_size,
            sth_root: hex_encode(&entry.sth.root_hash),
            sth_timestamp_unix_secs: entry.sth.timestamp_unix_secs,
            sth_sig: entry.sth.sig.clone(),
            witness_signature_algorithm: entry.witness_signature_algorithm.clone(),
            witness_sigs: entry.witness_sigs.clone(),
            consistency_with_prev,
        }
    }
}

fn normalize_backend_id(entry: &CommitEvidence) -> Option<&str> {
    if !entry.vc_backend_id.trim().is_empty() {
        return Some(entry.vc_backend_id.trim());
    }
    let scheme = entry.vc_scheme_id.trim();
    match scheme {
        "ipa" => Some("ipa"),
        "kzg" => Some("kzg"),
        _ => None,
    }
}

pub fn append_evidence_jsonl(
    path: impl AsRef<Path>,
    e: &CommitEvidence,
) -> Result<(), EvidenceError> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut f, e)?;
    f.write_all(b"\n")?;
    Ok(())
}

pub fn load_evidence_jsonl(path: impl AsRef<Path>) -> Result<Vec<CommitEvidence>, EvidenceError> {
    let f = OpenOptions::new().read(true).open(path)?;
    let reader = BufReader::new(f);
    let mut out = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str::<CommitEvidence>(&line)?);
    }
    Ok(out)
}

pub fn replay_verify_evidence(
    evidence: &[CommitEvidence],
    witness_cfg: &WitnessConfig,
) -> Result<(), ReplayError> {
    if evidence.is_empty() {
        return Err(ReplayError::EmptyEvidence);
    }

    for (i, e) in evidence.iter().enumerate() {
        let expected_height = (i as u64) + 1;
        if e.height != expected_height {
            return Err(ReplayError::HeightMismatch {
                expected: expected_height,
                got: e.height,
            });
        }
        if e.sth_size != e.height {
            return Err(ReplayError::SthSizeMismatch {
                expected: e.height,
                got: e.sth_size,
            });
        }
        let sth_root = decode_hex_exact::<32>(&e.sth_root)
            .ok_or(ReplayError::InvalidHashHex { height: e.height })?;
        let expected_sig = sth_signature(e.sth_size, &sth_root, e.sth_timestamp_unix_secs);
        if e.sth_sig != expected_sig {
            return Err(ReplayError::InvalidSthSignature {
                expected: expected_sig,
                got: e.sth_sig.clone(),
            });
        }
        if e.vc_scheme_id.trim().is_empty() || e.vc_domain_separator.trim().is_empty() {
            return Err(ReplayError::InvalidConsistencyEvidence { height: e.height });
        }
        if e.vc_max_degree == 0 {
            return Err(ReplayError::InvalidConsistencyEvidence { height: e.height });
        }
        let backend_id = normalize_backend_id(e).ok_or_else(|| ReplayError::InvalidVcBackend {
            height: e.height,
            got: e.vc_backend_id.clone(),
        })?;
        if backend_id != "ipa" && backend_id != "kzg" && backend_id != "binary_merkle" {
            return Err(ReplayError::InvalidVcBackend {
                height: e.height,
                got: backend_id.to_string(),
            });
        }
        let sig_alg = parse_algorithm_tag(&e.witness_signature_algorithm).map_err(|_| {
            ReplayError::InvalidWitnessSignatureAlgorithm {
                height: e.height,
                got: e.witness_signature_algorithm.clone(),
            }
        })?;

        if i == 0 {
            if e.consistency_with_prev.is_some() {
                return Err(ReplayError::UnexpectedConsistencyEvidenceAtGenesis);
            }
            if !verify_quorum_for_algorithm(
                witness_cfg,
                sig_alg,
                &msg_for_entry(e),
                &e.witness_sigs,
            ) {
                return Err(ReplayError::WitnessQuorumFailed { height: e.height });
            }
            continue;
        }

        let prev = &evidence[i - 1];
        if e.prev_state_root != prev.state_root {
            return Err(ReplayError::PrevRootMismatch {
                expected: prev.state_root.clone(),
                got: e.prev_state_root.clone(),
            });
        }

        let c = e
            .consistency_with_prev
            .as_ref()
            .ok_or(ReplayError::MissingConsistencyEvidence { height: e.height })?;
        if c.old_size != prev.sth_size || c.new_size != e.sth_size {
            return Err(ReplayError::InvalidConsistencyEvidence { height: e.height });
        }
        if c.old_root != prev.sth_root || c.new_root != e.sth_root {
            return Err(ReplayError::InvalidConsistencyEvidence { height: e.height });
        }
        let old_root = decode_hex_exact::<32>(&c.old_root)
            .ok_or(ReplayError::InvalidHashHex { height: e.height })?;
        let new_root = decode_hex_exact::<32>(&c.new_root)
            .ok_or(ReplayError::InvalidHashHex { height: e.height })?;
        let mut path: Vec<NodeHash> = Vec::with_capacity(c.proof.len());
        for node_hex in &c.proof {
            let node = decode_hex_exact::<32>(node_hex)
                .ok_or(ReplayError::InvalidHashHex { height: e.height })?;
            path.push(node);
        }
        let proof = ConsistencyProof {
            old_size: c.old_size,
            new_size: c.new_size,
            path,
        };
        if !verify_consistency_proof(&proof, &old_root, &new_root) {
            return Err(ReplayError::InvalidConsistencyEvidence { height: e.height });
        }
        if !verify_quorum_for_algorithm(witness_cfg, sig_alg, &msg_for_entry(e), &e.witness_sigs) {
            return Err(ReplayError::WitnessQuorumFailed { height: e.height });
        }
    }

    Ok(())
}

fn sha512_hex(bytes: &[u8]) -> String {
    let mut h = Sha512::new();
    h.update(bytes);
    hex_encode(&h.finalize())
}

pub fn bundle_signing_message(payload_sha512: &str) -> String {
    format!("nucleusdb:evidence-bundle:sha512:{payload_sha512}")
}

pub fn create_evidence_bundle(
    evidence_path: impl AsRef<Path>,
    out_dir: impl AsRef<Path>,
    witness_cfg: &WitnessConfig,
    retention_days: u32,
) -> Result<EvidenceBundleManifest, BundleError> {
    let evidence_path = evidence_path.as_ref();
    let out_dir = out_dir.as_ref();
    let evidence = load_evidence_jsonl(evidence_path)?;
    replay_verify_evidence(&evidence, witness_cfg)?;

    let evidence_bytes = fs::read(evidence_path)?;
    let evidence_sha512 = sha512_hex(&evidence_bytes);
    let created_unix_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| BundleError::Time(format!("clock before epoch: {e}")))?
        .as_secs();
    let delete_after_unix_secs = created_unix_secs + (retention_days as u64 * 24 * 60 * 60);

    let payload_seed = format!(
        "{}|{}|{}|{}|{}",
        "nucleusdb/evidence-bundle/v1",
        created_unix_secs,
        retention_days,
        evidence.len(),
        evidence_sha512
    );
    let payload_sha512 = sha512_hex(payload_seed.as_bytes());
    let sig_msg = bundle_signing_message(&payload_sha512);
    let mut witness_signatures: Vec<(String, String)> = Vec::new();
    for w in witness_cfg.witnesses.iter().take(witness_cfg.threshold) {
        let sig = sign_message(witness_cfg, w, &sig_msg).map_err(BundleError::WitnessSigning)?;
        witness_signatures.push((w.clone(), sig));
    }

    if !verify_quorum_for_algorithm(
        witness_cfg,
        witness_cfg.signing_algorithm,
        &sig_msg,
        &witness_signatures,
    ) {
        return Err(BundleError::BundleWitnessQuorumFailed);
    }

    fs::create_dir_all(out_dir)?;
    let evidence_out = out_dir.join("evidence.jsonl");
    fs::write(&evidence_out, &evidence_bytes)?;

    let manifest = EvidenceBundleManifest {
        schema: "nucleusdb/evidence-bundle/v1".to_string(),
        created_unix_secs,
        retention_days,
        delete_after_unix_secs,
        source_evidence_path: evidence_path.display().to_string(),
        evidence_file: evidence_out
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "evidence.jsonl".to_string()),
        evidence_records: evidence.len(),
        evidence_sha512,
        replay_verified: true,
        payload_sha512,
        witness_threshold: witness_cfg.threshold,
        witness_signatures,
    };

    let manifest_json = serde_json::to_vec_pretty(&manifest)?;
    fs::write(out_dir.join("manifest.json"), &manifest_json)?;
    fs::write(
        out_dir.join("manifest.sha512"),
        format!("{}\n", sha512_hex(&manifest_json)),
    )?;
    Ok(manifest)
}
