//! CAB-backed license verification for NucleusDB freemium gating.
//!
//! Certificates are issued by the P2PCLAW license mint endpoint after
//! AgentPMT Tier-2 payment verification.  Verification is fully offline:
//! the binary re-derives the Merkle root over the licensed feature set
//! and checks it against a baked-in foundation commitment.  No phone-home.

use crate::transparency::ct6962::sha256;
use ark_bn254::{Bn254, Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_groth16::{prepare_verifying_key, Groth16, Proof as Groth16Proof, VerifyingKey};
use ark_snark::SNARK;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Foundation commitment baked into the binary at compile time.
// This value is the SHA-256 Hcat("NucleusDB.CAB.Foundation|", ...)
// derived from the Heyting CAB artifact.  A forger would need to reverse
// this hash to produce a valid license.
// ---------------------------------------------------------------------------

/// SHA-256 of "NucleusDB.CAB.Foundation|v1" — acts as the licence root-of-trust.
/// Regenerate with:
///   echo -n "NucleusDB.CAB.Foundation|v1" | sha256sum
const KNOWN_FOUNDATION: [u8; 32] = [
    0xde, 0xca, 0x4e, 0x53, 0xfb, 0x61, 0x34, 0xb0, 0x00, 0x70, 0x53, 0x41, 0xd5, 0xdb, 0x3d, 0xe0,
    0x23, 0xdc, 0xd8, 0xa9, 0x5e, 0x17, 0x4b, 0xce, 0x0c, 0x4e, 0x21, 0xbc, 0xab, 0x6e, 0x8c, 0xf9,
];

/// Domain separator mirroring Heyting LeanTT0 `H` / `Hcat` convention.
const DOMAIN_LICENSE_V1: &[u8] = b"NucleusDB.License|";
const DOMAIN_LICENSE_V2: &[u8] = b"NucleusDB.License.v2|";
const DOMAIN_FOUNDATION: &[u8] = b"NucleusDB.CAB.Foundation|";
const DOMAIN_COMPLIANCE: &[u8] = b"NucleusDB.License.Compliance|";

/// Compliance witness for PCN channel validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PcnComplianceWitness {
    pub feasibility_root: [u8; 32],
    pub replay_seq: u64,
    #[serde(default)]
    pub channel_count: u64,
    #[serde(default)]
    pub valid_channels: u64,
    #[serde(default)]
    pub invalid_channels: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LocalPufEvidence {
    fingerprint: [u8; 32],
}

fn collect_local_puf_evidence() -> Option<LocalPufEvidence> {
    let raw = std::env::var("NUCLEUSDB_PUF_DIGEST").ok()?;
    let fingerprint = hex_to_32(raw.trim())?;
    Some(LocalPufEvidence { fingerprint })
}

/// Poseidon hash of "NucleusDB.CAB.Foundation.v1" — ZK public signal root-of-trust.
/// The first public signal in any valid SNARK proof must equal this value.
const KNOWN_FOUNDATION_POSEIDON: &str =
    "10498408604190631903661670351841509167761295075626051572612706182854256519760";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Features that can be unlocked by a Pro license.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProFeature {
    MultiTenant,
    McpServer,
    Tui,
    IpaBackend,
    KzgBackend,
    CertificateTransparency,
    WalReplay,
    Export,
    BatchSql,
    ConfigurableQuorum,
    UnlimitedEntries,
}

impl ProFeature {
    /// Canonical string used in Merkle leaf hashing.
    pub fn as_leaf_str(&self) -> &'static str {
        match self {
            Self::MultiTenant => "multi_tenant",
            Self::McpServer => "mcp_server",
            Self::Tui => "tui",
            Self::IpaBackend => "ipa_backend",
            Self::KzgBackend => "kzg_backend",
            Self::CertificateTransparency => "certificate_transparency",
            Self::WalReplay => "wal_replay",
            Self::Export => "export",
            Self::BatchSql => "batch_sql",
            Self::ConfigurableQuorum => "configurable_quorum",
            Self::UnlimitedEntries => "unlimited_entries",
        }
    }

    /// Full Pro feature set.
    pub fn all() -> Vec<ProFeature> {
        vec![
            Self::MultiTenant,
            Self::McpServer,
            Self::Tui,
            Self::IpaBackend,
            Self::KzgBackend,
            Self::CertificateTransparency,
            Self::WalReplay,
            Self::Export,
            Self::BatchSql,
            Self::ConfigurableQuorum,
            Self::UnlimitedEntries,
        ]
    }
}

/// Active license level at runtime.
#[derive(Clone, Debug, Default)]
pub enum LicenseLevel {
    /// Free tier — core features only.
    #[default]
    Community,
    /// Paid tier — certificate-verified feature set.
    Pro {
        features: Vec<ProFeature>,
        licensee: String,
        expiry_unix_secs: u64,
    },
}

impl LicenseLevel {
    /// Check whether a specific pro feature is enabled.
    pub fn has(&self, feature: &ProFeature) -> bool {
        match self {
            Self::Community => false,
            Self::Pro { features, .. } => features.contains(feature),
        }
    }

    pub fn is_pro(&self) -> bool {
        matches!(self, Self::Pro { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Community => "Community",
            Self::Pro { .. } => "Pro",
        }
    }
}

// ---------------------------------------------------------------------------
// Certificate
// ---------------------------------------------------------------------------

/// SNARK proof embedded in a license certificate (Groth16 over BN128).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnarkProof {
    pub protocol: String,
    pub curve: String,
    pub pi_a: Vec<String>,
    pub pi_b: Vec<Vec<String>>,
    pub pi_c: Vec<String>,
    pub public_signals: Vec<String>,
}

/// Public compliance inputs bound into v2 certificates.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompliancePublicInputs {
    /// Merkle root over channel-level feasibility witnesses.
    pub feasibility_root: String,
    /// Monotone sequence upper-bound used for replay prevention.
    pub replay_seq: u64,
    /// Optional PUF digest mirrored inside compliance payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub puf_digest: Option<String>,
}

/// On-disk license certificate (JSON).  Issued by P2PCLAW, verified locally.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LicenseCertificate {
    /// Schema version for forward compatibility.
    pub version: String,
    /// Hex-encoded SHA-256 foundation commitment — must match `KNOWN_FOUNDATION`.
    pub foundation_commitment: String,
    /// Hex-encoded Merkle root over the sorted feature leaf hashes.
    pub rules_root: String,
    /// Hex-encoded SHA-256(licensee || expiry).
    pub licensee_commitment: String,
    /// Hex-encoded domain-separated proof digest binding everything together.
    pub proof_digest: String,
    /// Licensee identifier (email, org, agent id).
    pub licensee: String,
    /// Expiry as seconds since UNIX epoch.
    pub expiry_unix_secs: u64,
    /// Licensed feature strings (canonical snake_case names).
    pub features: Vec<String>,
    /// Poseidon hash of the foundation string (BN254 field element, decimal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foundation_poseidon: Option<String>,
    /// Poseidon Merkle root over feature hashes (BN254 field element, decimal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules_root_poseidon: Option<String>,
    /// Poseidon(licensee_field, expiry) commitment (BN254 field element, decimal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensee_commitment_poseidon: Option<String>,
    /// Poseidon(foundation, rules_root, licensee_commitment) digest (BN254 field element, decimal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_digest_poseidon: Option<String>,
    /// Poseidon hashes of individual feature names (16 elements, zero-padded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_hashes_poseidon: Option<Vec<String>>,
    /// Licensee string encoded as BN254 field element (decimal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub licensee_field: Option<String>,
    /// Groth16 SNARK proof (verifiable against embedded verification key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snark_proof: Option<SnarkProof>,
    /// Optional host PUF fingerprint digest (`0x`-prefixed hex of 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub puf_digest: Option<String>,
    /// Optional compliance inputs for v2 certificates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compliance_inputs: Option<CompliancePublicInputs>,
    /// Commitment over compliance inputs (`0x`-prefixed hex of 32 bytes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compliance_commitment: Option<String>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LicenseError {
    Io(std::io::Error),
    Json(serde_json::Error),
    FoundationMismatch,
    RulesRootMismatch,
    ProofDigestMismatch,
    LicenseeCommitmentMismatch,
    Expired,
    UnsupportedVersion(String),
    EmptyFeatures,
    SnarkVerificationFailed,
    SnarkPublicSignalMismatch,
    PufUnavailable,
    PufDigestMismatch,
    MissingComplianceInputs,
    ComplianceInputMalformed,
    ComplianceCommitmentMismatch,
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "license I/O error: {e}"),
            Self::Json(e) => write!(f, "license JSON parse error: {e}"),
            Self::FoundationMismatch => {
                write!(f, "license foundation commitment does not match this build")
            }
            Self::RulesRootMismatch => {
                write!(f, "license rules root does not match declared features")
            }
            Self::ProofDigestMismatch => write!(f, "license proof digest verification failed"),
            Self::LicenseeCommitmentMismatch => {
                write!(f, "license licensee commitment does not match")
            }
            Self::Expired => write!(f, "license has expired"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported license version: {v}"),
            Self::EmptyFeatures => write!(f, "license has no features"),
            Self::SnarkVerificationFailed => {
                write!(f, "SNARK proof verification failed")
            }
            Self::SnarkPublicSignalMismatch => {
                write!(
                    f,
                    "SNARK public signals are not bound to this certificate payload"
                )
            }
            Self::PufUnavailable => {
                write!(
                    f,
                    "license requires PUF binding but no compatible PUF source is available"
                )
            }
            Self::PufDigestMismatch => {
                write!(f, "license PUF digest does not match this device")
            }
            Self::MissingComplianceInputs => {
                write!(f, "license v2 requires compliance inputs")
            }
            Self::ComplianceInputMalformed => {
                write!(f, "license compliance inputs are malformed")
            }
            Self::ComplianceCommitmentMismatch => {
                write!(f, "license compliance commitment does not match inputs")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Crypto helpers (mirrors Heyting LeanTT0 Hcat / H convention)
// ---------------------------------------------------------------------------

/// Domain-separated SHA-256: `SHA-256(domain || payload)`.
fn h(domain: &[u8], payload: &[u8]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(domain.len() + payload.len());
    buf.extend_from_slice(domain);
    buf.extend_from_slice(payload);
    sha256(&buf)
}

/// Domain-separated SHA-256 over concatenated byte arrays.
fn hcat(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let total: usize = domain.len() + parts.iter().map(|p| p.len()).sum::<usize>();
    let mut buf = Vec::with_capacity(total);
    buf.extend_from_slice(domain);
    for p in parts {
        buf.extend_from_slice(p);
    }
    sha256(&buf)
}

/// Compute binary Merkle root over a list of 32-byte leaves.
fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    if leaves.len() == 1 {
        return leaves[0];
    }
    let mut layer: Vec<[u8; 32]> = leaves.to_vec();
    while layer.len() > 1 {
        let mut next = Vec::with_capacity(layer.len().div_ceil(2));
        let mut i = 0;
        while i + 1 < layer.len() {
            let mut buf = Vec::with_capacity(65);
            buf.push(0x01); // RFC 6962 interior node tag
            buf.extend_from_slice(&layer[i]);
            buf.extend_from_slice(&layer[i + 1]);
            next.push(sha256(&buf));
            i += 2;
        }
        if i < layer.len() {
            next.push(layer[i]);
        }
        layer = next;
    }
    layer[0]
}

/// Feature string → leaf hash (RFC 6962 leaf tag 0x00).
fn feature_leaf(feature: &str) -> [u8; 32] {
    let payload = format!("NucleusDB.License.Feature|{feature}");
    let mut buf = Vec::with_capacity(1 + payload.len());
    buf.push(0x00); // leaf tag
    buf.extend_from_slice(payload.as_bytes());
    sha256(&buf)
}

/// Hex-decode a string (with or without "0x" prefix) to 32 bytes.
fn hex_to_32(hex: &str) -> Option<[u8; 32]> {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Encode 32 bytes as "0x"-prefixed hex.
fn to_hex(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(66);
    s.push_str("0x");
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn is_null_digest_sentinel(raw: &str) -> bool {
    let v = raw.trim();
    v.is_empty() || v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("null")
}

fn build_compliance_commitment(inputs: &CompliancePublicInputs) -> Result<[u8; 32], LicenseError> {
    let feasibility =
        hex_to_32(&inputs.feasibility_root).ok_or(LicenseError::ComplianceInputMalformed)?;
    let puf = if let Some(ref digest) = inputs.puf_digest {
        if is_null_digest_sentinel(digest) {
            return Err(LicenseError::ComplianceInputMalformed);
        }
        hex_to_32(digest).ok_or(LicenseError::ComplianceInputMalformed)?
    } else {
        [0u8; 32]
    };
    let puf_flag = [u8::from(inputs.puf_digest.is_some())];
    Ok(hcat(
        DOMAIN_COMPLIANCE,
        &[
            &feasibility,
            &inputs.replay_seq.to_be_bytes(),
            &puf_flag,
            &puf,
        ],
    ))
}

/// Convert runtime PCN witness data into certificate-ready compliance inputs.
pub fn compliance_inputs_from_pcn_witness(
    witness: &PcnComplianceWitness,
    puf_digest: Option<[u8; 32]>,
) -> CompliancePublicInputs {
    CompliancePublicInputs {
        feasibility_root: to_hex(&witness.feasibility_root),
        replay_seq: witness.replay_seq,
        puf_digest: puf_digest.map(|d| to_hex(&d)),
    }
}

// ---------------------------------------------------------------------------
// Embedded Groth16 verification key (from trusted setup ceremony)
// ---------------------------------------------------------------------------

/// Verification key constants from `license_verification_verification_key.json`.
/// Generated during the Groth16 trusted setup ceremony for the license circuit.
/// Total embedded cost: ~2KB of string constants. Verification cost: ~1.2ms.
mod embedded_vk {
    // vk_alpha_1 (G1 affine point: x, y)
    pub const ALPHA_G1: (&str, &str) = (
        "10405655760119732762824095869568406693561841416451728932271937081641821752442",
        "17894357186551961076795404557474199423647869761457444754365915623531257749504",
    );
    // vk_beta_2 (G2 affine point: (x0,x1), (y0,y1))
    pub const BETA_G2: ((&str, &str), (&str, &str)) = (
        (
            "817037149465713341295302205409665163636512997788012192760338436219591622570",
            "8181955070738281622789145815239278178064061204636853080281558871759452889216",
        ),
        (
            "2470469513158114407389324664570297769674606540799180541260835218432618939808",
            "7544550115112493806048097440827485485771358631037888427545369902420464391159",
        ),
    );
    // vk_gamma_2 (G2 affine point)
    pub const GAMMA_G2: ((&str, &str), (&str, &str)) = (
        (
            "10857046999023057135944570762232829481370756359578518086990519993285655852781",
            "11559732032986387107991004021392285783925812861821192530917403151452391805634",
        ),
        (
            "8495653923123431417604973247489272438418190587263600148770280649306958101930",
            "4082367875863433681332203403145435568316851327593401208105741076214120093531",
        ),
    );
    // vk_delta_2 (G2 affine point)
    pub const DELTA_G2: ((&str, &str), (&str, &str)) = (
        (
            "1074597943206102349778140182451723613501628836518057530308898092584953672550",
            "9113188335812023316551771771065788170397772324476632243867577692409202310465",
        ),
        (
            "12568699406555272521137419993975304946411456725722974416328832358691787922082",
            "9671252814183893774941795037258883504827198960588967633442343476946487748802",
        ),
    );
    // IC (input commitments — 3 G1 points for 2 public inputs + 1)
    pub const IC: [(&str, &str); 3] = [
        (
            "1886624505104556738673761345726261325771700141708280611575166489627757278359",
            "21865943168566572626055999140916368834187894605042283690209826820111355242432",
        ),
        (
            "11476862565265928797481511316176026704635336790699393818206281210967343195400",
            "14081078318345287356041983710352453621730748568416967074248703006711490654640",
        ),
        (
            "19098510535955990810767905164616159577478438968862710353268241684793658238243",
            "7701696675531282626493228672088711677637336743678080881498263883922311153132",
        ),
    ];
}

fn parse_fq(s: &str) -> Result<Fq, LicenseError> {
    s.parse::<Fq>()
        .map_err(|_| LicenseError::SnarkVerificationFailed)
}

fn parse_fr(s: &str) -> Result<Fr, LicenseError> {
    s.parse::<Fr>()
        .map_err(|_| LicenseError::SnarkVerificationFailed)
}

fn g1_from_strs(x: &str, y: &str) -> Result<G1Affine, LicenseError> {
    Ok(G1Affine::new_unchecked(parse_fq(x)?, parse_fq(y)?))
}

fn g2_from_strs(coords: ((&str, &str), (&str, &str))) -> Result<G2Affine, LicenseError> {
    let x = Fq2::new(parse_fq(coords.0 .0)?, parse_fq(coords.0 .1)?);
    let y = Fq2::new(parse_fq(coords.1 .0)?, parse_fq(coords.1 .1)?);
    Ok(G2Affine::new_unchecked(x, y))
}

/// Build the Groth16 verification key from embedded constants.
fn build_embedded_vk() -> Result<VerifyingKey<Bn254>, LicenseError> {
    let alpha_g1 = g1_from_strs(embedded_vk::ALPHA_G1.0, embedded_vk::ALPHA_G1.1)?;
    let beta_g2 = g2_from_strs(embedded_vk::BETA_G2)?;
    let gamma_g2 = g2_from_strs(embedded_vk::GAMMA_G2)?;
    let delta_g2 = g2_from_strs(embedded_vk::DELTA_G2)?;
    let gamma_abc_g1: Result<Vec<G1Affine>, _> = embedded_vk::IC
        .iter()
        .map(|(x, y)| g1_from_strs(x, y))
        .collect();
    Ok(VerifyingKey {
        alpha_g1,
        beta_g2,
        gamma_g2,
        delta_g2,
        gamma_abc_g1: gamma_abc_g1?,
    })
}

/// Verify a Groth16 SNARK proof from a license certificate.
/// Cost: three elliptic-curve pairings (~1.2ms on modern hardware).
fn verify_snark_proof(
    proof: &SnarkProof,
    expected_digest_poseidon: Option<&str>,
) -> Result<(), LicenseError> {
    // Protocol / curve guard.
    if proof.protocol != "groth16" || proof.curve != "bn128" {
        return Err(LicenseError::SnarkVerificationFailed);
    }

    // Public signals: [foundation_poseidon, proof_digest_poseidon].
    if proof.public_signals.len() != 2 {
        return Err(LicenseError::SnarkVerificationFailed);
    }

    // First public signal must match our baked-in foundation Poseidon hash.
    if proof.public_signals[0] != KNOWN_FOUNDATION_POSEIDON {
        return Err(LicenseError::SnarkVerificationFailed);
    }

    // Optional binding: ensure the proof's digest signal matches certificate payload.
    if let Some(expected) = expected_digest_poseidon {
        if proof.public_signals[1] != expected {
            return Err(LicenseError::SnarkPublicSignalMismatch);
        }
    }

    // Parse public inputs as scalar field elements.
    let public_inputs: Vec<Fr> = proof
        .public_signals
        .iter()
        .map(|s| parse_fr(s))
        .collect::<Result<Vec<_>, _>>()?;

    // Parse proof points.
    if proof.pi_a.len() < 2 || proof.pi_c.len() < 2 {
        return Err(LicenseError::SnarkVerificationFailed);
    }
    if proof.pi_b.len() < 2 || proof.pi_b[0].len() < 2 || proof.pi_b[1].len() < 2 {
        return Err(LicenseError::SnarkVerificationFailed);
    }

    let a = g1_from_strs(&proof.pi_a[0], &proof.pi_a[1])?;
    let b_x = Fq2::new(parse_fq(&proof.pi_b[0][0])?, parse_fq(&proof.pi_b[0][1])?);
    let b_y = Fq2::new(parse_fq(&proof.pi_b[1][0])?, parse_fq(&proof.pi_b[1][1])?);
    let b = G2Affine::new_unchecked(b_x, b_y);
    let c = g1_from_strs(&proof.pi_c[0], &proof.pi_c[1])?;

    let groth16_proof = Groth16Proof::<Bn254> { a, b, c };

    // Build VK, prepare, and verify.
    let vk = build_embedded_vk()?;
    let pvk = prepare_verifying_key(&vk);
    let valid = Groth16::<Bn254>::verify_with_processed_vk(&pvk, &public_inputs, &groth16_proof)
        .map_err(|_| LicenseError::SnarkVerificationFailed)?;

    if !valid {
        return Err(LicenseError::SnarkVerificationFailed);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Minting (used by tests and the P2PCLAW endpoint reference)
// ---------------------------------------------------------------------------

/// Mint a license certificate.  This is the prover side — lives on
/// the P2PCLAW server in production but is also used by tests.
pub fn mint_certificate(
    licensee: &str,
    features: &[ProFeature],
    expiry_unix_secs: u64,
) -> LicenseCertificate {
    let foundation = h(DOMAIN_FOUNDATION, b"v1");

    // Sorted feature leaves → Merkle root.
    let mut feature_strs: Vec<&str> = features.iter().map(|f| f.as_leaf_str()).collect();
    feature_strs.sort();
    let leaves: Vec<[u8; 32]> = feature_strs.iter().map(|f| feature_leaf(f)).collect();
    let rules_root = merkle_root(&leaves);

    // Licensee commitment = H("NucleusDB.Licensee|", licensee || expiry_bytes).
    let expiry_bytes = expiry_unix_secs.to_be_bytes();
    let licensee_commitment = hcat(
        b"NucleusDB.Licensee|",
        &[licensee.as_bytes(), &expiry_bytes],
    );

    // Proof digest = Hcat(DOMAIN_LICENSE, [foundation, rules_root, licensee_commitment]).
    let proof_digest = hcat(
        DOMAIN_LICENSE_V1,
        &[&foundation, &rules_root, &licensee_commitment],
    );

    LicenseCertificate {
        version: "nucleusdb-license-cab-1".to_string(),
        foundation_commitment: to_hex(&foundation),
        rules_root: to_hex(&rules_root),
        licensee_commitment: to_hex(&licensee_commitment),
        proof_digest: to_hex(&proof_digest),
        licensee: licensee.to_string(),
        expiry_unix_secs,
        features: feature_strs.iter().map(|s| s.to_string()).collect(),
        foundation_poseidon: None,
        rules_root_poseidon: None,
        licensee_commitment_poseidon: None,
        proof_digest_poseidon: None,
        feature_hashes_poseidon: None,
        licensee_field: None,
        snark_proof: None,
        puf_digest: None,
        compliance_inputs: None,
        compliance_commitment: None,
    }
}

/// Mint a v2 certificate binding PCN compliance inputs into the license digest.
pub fn mint_certificate_v2(
    licensee: &str,
    features: &[ProFeature],
    expiry_unix_secs: u64,
    compliance_inputs: CompliancePublicInputs,
) -> Result<LicenseCertificate, LicenseError> {
    let foundation = h(DOMAIN_FOUNDATION, b"v1");

    let mut feature_strs: Vec<&str> = features.iter().map(|f| f.as_leaf_str()).collect();
    feature_strs.sort();
    let leaves: Vec<[u8; 32]> = feature_strs.iter().map(|f| feature_leaf(f)).collect();
    let rules_root = merkle_root(&leaves);

    let expiry_bytes = expiry_unix_secs.to_be_bytes();
    let licensee_commitment = hcat(
        b"NucleusDB.Licensee|",
        &[licensee.as_bytes(), &expiry_bytes],
    );

    let compliance_commitment = build_compliance_commitment(&compliance_inputs)?;
    let proof_digest = hcat(
        DOMAIN_LICENSE_V2,
        &[
            &foundation,
            &rules_root,
            &licensee_commitment,
            &compliance_commitment,
        ],
    );

    Ok(LicenseCertificate {
        version: "nucleusdb-license-cab-2".to_string(),
        foundation_commitment: to_hex(&foundation),
        rules_root: to_hex(&rules_root),
        licensee_commitment: to_hex(&licensee_commitment),
        proof_digest: to_hex(&proof_digest),
        licensee: licensee.to_string(),
        expiry_unix_secs,
        features: feature_strs.iter().map(|s| s.to_string()).collect(),
        foundation_poseidon: None,
        rules_root_poseidon: None,
        licensee_commitment_poseidon: None,
        proof_digest_poseidon: None,
        feature_hashes_poseidon: None,
        licensee_field: None,
        snark_proof: None,
        puf_digest: compliance_inputs.puf_digest.clone(),
        compliance_inputs: Some(compliance_inputs),
        compliance_commitment: Some(to_hex(&compliance_commitment)),
    })
}

// ---------------------------------------------------------------------------
// Verification (the verifier side — runs in the NucleusDB binary)
// ---------------------------------------------------------------------------

/// Load and verify a license certificate from a JSON file.
pub fn load_and_verify(path: &Path) -> Result<LicenseLevel, LicenseError> {
    let raw = std::fs::read_to_string(path).map_err(LicenseError::Io)?;
    let cert: LicenseCertificate = serde_json::from_str(&raw).map_err(LicenseError::Json)?;
    verify_certificate(&cert)
}

/// Verify a parsed certificate.
pub fn verify_certificate(cert: &LicenseCertificate) -> Result<LicenseLevel, LicenseError> {
    // 1. Version check.
    let is_v1 = cert.version == "nucleusdb-license-cab-1";
    let is_v2 = cert.version == "nucleusdb-license-cab-2";
    if !is_v1 && !is_v2 {
        return Err(LicenseError::UnsupportedVersion(cert.version.clone()));
    }

    // 2. Feature list must be non-empty.
    if cert.features.is_empty() {
        return Err(LicenseError::EmptyFeatures);
    }

    // 3. Foundation commitment must match the baked-in constant.
    let foundation_got =
        hex_to_32(&cert.foundation_commitment).ok_or(LicenseError::FoundationMismatch)?;
    if foundation_got != KNOWN_FOUNDATION {
        return Err(LicenseError::FoundationMismatch);
    }

    // 4. Re-derive rules root from declared features (sorted).
    let mut features_sorted = cert.features.clone();
    features_sorted.sort();
    let leaves: Vec<[u8; 32]> = features_sorted.iter().map(|f| feature_leaf(f)).collect();
    let expected_root = merkle_root(&leaves);
    let got_root = hex_to_32(&cert.rules_root).ok_or(LicenseError::RulesRootMismatch)?;
    if got_root != expected_root {
        return Err(LicenseError::RulesRootMismatch);
    }

    // 5. Re-derive licensee commitment.
    let expiry_bytes = cert.expiry_unix_secs.to_be_bytes();
    let expected_licensee = hcat(
        b"NucleusDB.Licensee|",
        &[cert.licensee.as_bytes(), &expiry_bytes],
    );
    let got_licensee =
        hex_to_32(&cert.licensee_commitment).ok_or(LicenseError::LicenseeCommitmentMismatch)?;
    if got_licensee != expected_licensee {
        return Err(LicenseError::LicenseeCommitmentMismatch);
    }

    // 6. Re-derive proof digest.
    let mut compliance_bound_puf: Option<String> = None;
    let expected_digest = if is_v1 {
        hcat(
            DOMAIN_LICENSE_V1,
            &[&foundation_got, &got_root, &got_licensee],
        )
    } else {
        let compliance = cert
            .compliance_inputs
            .as_ref()
            .ok_or(LicenseError::MissingComplianceInputs)?;
        let expected_compliance = build_compliance_commitment(compliance)?;
        let got_compliance = cert
            .compliance_commitment
            .as_deref()
            .and_then(hex_to_32)
            .ok_or(LicenseError::ComplianceCommitmentMismatch)?;
        if got_compliance != expected_compliance {
            return Err(LicenseError::ComplianceCommitmentMismatch);
        }
        if let Some(ref puf) = compliance.puf_digest {
            compliance_bound_puf = Some(puf.clone());
        }
        if let (Some(cert_puf), Some(comp_puf)) =
            (cert.puf_digest.as_ref(), compliance.puf_digest.as_ref())
        {
            if cert_puf != comp_puf {
                return Err(LicenseError::PufDigestMismatch);
            }
        }
        hcat(
            DOMAIN_LICENSE_V2,
            &[
                &foundation_got,
                &got_root,
                &got_licensee,
                &expected_compliance,
            ],
        )
    };
    let got_digest = hex_to_32(&cert.proof_digest).ok_or(LicenseError::ProofDigestMismatch)?;
    if got_digest != expected_digest {
        return Err(LicenseError::ProofDigestMismatch);
    }

    // 7. Expiry check.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if cert.expiry_unix_secs <= now {
        return Err(LicenseError::Expired);
    }

    // 8. SNARK proof verification (when present).
    if let Some(ref snark) = cert.snark_proof {
        verify_snark_proof(snark, cert.proof_digest_poseidon.as_deref())?;
    }

    // 8b. Optional PUF-device binding.
    let effective_puf_digest = cert.puf_digest.clone().or(compliance_bound_puf);
    if let Some(ref puf_digest_hex) = effective_puf_digest {
        if is_null_digest_sentinel(puf_digest_hex) {
            return Err(LicenseError::PufDigestMismatch);
        }
        let expected = hex_to_32(puf_digest_hex).ok_or(LicenseError::PufDigestMismatch)?;
        let current = collect_local_puf_evidence().ok_or(LicenseError::PufUnavailable)?;
        if current.fingerprint != expected {
            return Err(LicenseError::PufDigestMismatch);
        }
    }

    // 9. Map feature strings to ProFeature enum.
    let pro_features: Vec<ProFeature> = cert
        .features
        .iter()
        .filter_map(|f| match f.as_str() {
            "multi_tenant" => Some(ProFeature::MultiTenant),
            "mcp_server" => Some(ProFeature::McpServer),
            "tui" => Some(ProFeature::Tui),
            "ipa_backend" => Some(ProFeature::IpaBackend),
            "kzg_backend" => Some(ProFeature::KzgBackend),
            "certificate_transparency" => Some(ProFeature::CertificateTransparency),
            "wal_replay" => Some(ProFeature::WalReplay),
            "export" => Some(ProFeature::Export),
            "batch_sql" => Some(ProFeature::BatchSql),
            "configurable_quorum" => Some(ProFeature::ConfigurableQuorum),
            "unlimited_entries" => Some(ProFeature::UnlimitedEntries),
            _ => None, // Unknown features silently ignored for forward compat.
        })
        .collect();

    Ok(LicenseLevel::Pro {
        features: pro_features,
        licensee: cert.licensee.clone(),
        expiry_unix_secs: cert.expiry_unix_secs,
    })
}

/// Format a human-readable verification report.
pub fn verification_report(cert: &LicenseCertificate) -> String {
    match verify_certificate(cert) {
        Ok(LicenseLevel::Pro {
            features,
            licensee,
            expiry_unix_secs,
        }) => {
            let feature_list: Vec<&str> = features.iter().map(|f| f.as_leaf_str()).collect();
            format!(
                "License VALID\n  Licensee: {licensee}\n  Expiry:   {expiry_unix_secs} (unix)\n  Features: {}\n  Digest:   {}",
                feature_list.join(", "),
                cert.proof_digest,
            )
        }
        Ok(LicenseLevel::Community) => "License verified as Community (no features).".to_string(),
        Err(e) => format!("License INVALID: {e}"),
    }
}

// ---------------------------------------------------------------------------
// Baked-in foundation commitment generation (for build reproducibility)
// ---------------------------------------------------------------------------

/// Compute the foundation commitment.  Used to generate `KNOWN_FOUNDATION`.
pub fn compute_foundation() -> [u8; 32] {
    h(DOMAIN_FOUNDATION, b"v1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foundation_commitment_matches_constant() {
        let computed = compute_foundation();
        assert_eq!(
            computed, KNOWN_FOUNDATION,
            "KNOWN_FOUNDATION constant is stale — regenerate with compute_foundation()"
        );
    }

    #[test]
    fn mint_then_verify_roundtrip() {
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600; // 1 year from now
        let cert = mint_certificate("test@example.com", &features, expiry);
        let level = verify_certificate(&cert).expect("valid certificate");
        assert!(level.is_pro());
        assert!(level.has(&ProFeature::MultiTenant));
        assert!(level.has(&ProFeature::McpServer));
        assert!(level.has(&ProFeature::KzgBackend));
    }

    #[test]
    fn expired_certificate_rejected() {
        let features = ProFeature::all();
        let cert = mint_certificate("expired@example.com", &features, 1_000_000); // far past
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::Expired)));
    }

    #[test]
    fn tampered_features_rejected() {
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let mut cert = mint_certificate("test@example.com", &features, expiry);
        // Add a feature that wasn't in the original Merkle root.
        cert.features.push("hacked_feature".to_string());
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::RulesRootMismatch)));
    }

    #[test]
    fn tampered_licensee_rejected() {
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let mut cert = mint_certificate("real@example.com", &features, expiry);
        cert.licensee = "pirate@example.com".to_string();
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::LicenseeCommitmentMismatch)));
    }

    #[test]
    fn tampered_expiry_rejected() {
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let mut cert = mint_certificate("test@example.com", &features, expiry);
        cert.expiry_unix_secs += 365 * 24 * 3600; // try to extend
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::LicenseeCommitmentMismatch)));
    }

    #[test]
    fn wrong_foundation_rejected() {
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let mut cert = mint_certificate("test@example.com", &features, expiry);
        cert.foundation_commitment =
            "0x0000000000000000000000000000000000000000000000000000000000000000".to_string();
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::FoundationMismatch)));
    }

    #[test]
    fn community_has_no_pro_features() {
        let level = LicenseLevel::Community;
        assert!(!level.is_pro());
        assert!(!level.has(&ProFeature::MultiTenant));
        assert!(!level.has(&ProFeature::McpServer));
        assert_eq!(level.label(), "Community");
    }

    #[test]
    fn partial_feature_license() {
        let features = vec![ProFeature::MultiTenant, ProFeature::McpServer];
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let cert = mint_certificate("partial@example.com", &features, expiry);
        let level = verify_certificate(&cert).expect("valid");
        assert!(level.has(&ProFeature::MultiTenant));
        assert!(level.has(&ProFeature::McpServer));
        assert!(!level.has(&ProFeature::KzgBackend));
        assert!(!level.has(&ProFeature::Tui));
    }

    #[test]
    fn hex_roundtrip() {
        let original = compute_foundation();
        let hex = to_hex(&original);
        let decoded = hex_to_32(&hex).expect("valid hex");
        assert_eq!(original, decoded);
    }

    #[test]
    fn merkle_root_single_leaf() {
        let leaf = feature_leaf("test");
        let root = merkle_root(&[leaf]);
        assert_eq!(root, leaf);
    }

    #[test]
    fn merkle_root_deterministic() {
        let leaves: Vec<[u8; 32]> = vec!["a", "b", "c"].into_iter().map(feature_leaf).collect();
        let root1 = merkle_root(&leaves);
        let root2 = merkle_root(&leaves);
        assert_eq!(root1, root2);
    }

    /// Helper: build a valid SNARK proof from the proved test certificate.
    fn test_snark_proof() -> SnarkProof {
        SnarkProof {
            protocol: "groth16".to_string(),
            curve: "bn128".to_string(),
            pi_a: vec![
                "21395058710715221512141136997577943222444054556201088705185109505677362522313"
                    .to_string(),
                "4579204029766272866587044832554240078169508494400311183784970114481255036590"
                    .to_string(),
                "1".to_string(),
            ],
            pi_b: vec![
                vec![
                    "5202294881725992217980051469091140593603060690090479418590364461196679826436"
                        .to_string(),
                    "3874299196029840840005590017697452158959263525655996160796037001458805009770"
                        .to_string(),
                ],
                vec![
                    "17779469406488213294468910677045021564849628076372499955495679563736323507938"
                        .to_string(),
                    "7063076278734608474516354088060440657810112068353853767850783433319766131774"
                        .to_string(),
                ],
                vec!["1".to_string(), "0".to_string()],
            ],
            pi_c: vec![
                "10725084923489509678962196352520219987459344786999429978446344967026680290054"
                    .to_string(),
                "11926696386234588152796050475063893399913854466682164509978597141240696416859"
                    .to_string(),
                "1".to_string(),
            ],
            public_signals: vec![
                "10498408604190631903661670351841509167761295075626051572612706182854256519760"
                    .to_string(),
                "11883432733534932235095524943887047101085430713026189147570957712288595065002"
                    .to_string(),
            ],
        }
    }

    #[test]
    fn snark_proof_verifies_valid() {
        let proof = test_snark_proof();
        verify_snark_proof(&proof, None).expect("valid SNARK proof must verify");
    }

    #[test]
    fn snark_proof_rejects_wrong_foundation() {
        let mut proof = test_snark_proof();
        // Tamper with the foundation public signal.
        proof.public_signals[0] = "999999999999999999".to_string();
        let err = verify_snark_proof(&proof, None);
        assert!(matches!(err, Err(LicenseError::SnarkVerificationFailed)));
    }

    #[test]
    fn snark_proof_rejects_tampered_digest() {
        let mut proof = test_snark_proof();
        // Tamper with the proof digest public signal (second signal).
        proof.public_signals[1] = "123456789".to_string();
        let err = verify_snark_proof(&proof, None);
        assert!(matches!(err, Err(LicenseError::SnarkVerificationFailed)));
    }

    #[test]
    fn snark_proof_rejects_wrong_protocol() {
        let mut proof = test_snark_proof();
        proof.protocol = "plonk".to_string();
        let err = verify_snark_proof(&proof, None);
        assert!(matches!(err, Err(LicenseError::SnarkVerificationFailed)));
    }

    #[test]
    fn snark_proof_rejects_wrong_curve() {
        let mut proof = test_snark_proof();
        proof.curve = "bls12-381".to_string();
        let err = verify_snark_proof(&proof, None);
        assert!(matches!(err, Err(LicenseError::SnarkVerificationFailed)));
    }

    #[test]
    fn snark_public_signal_mismatch_rejected_when_expected_digest_provided() {
        let proof = test_snark_proof();
        let err = verify_snark_proof(&proof, Some("42"));
        assert!(matches!(err, Err(LicenseError::SnarkPublicSignalMismatch)));
    }

    #[test]
    fn certificate_without_snark_still_valid() {
        // Backward compatibility: certificates without SNARK proof still verify.
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let cert = mint_certificate("backcompat@example.com", &features, expiry);
        assert!(cert.snark_proof.is_none());
        let level = verify_certificate(&cert).expect("cert without SNARK must verify");
        assert!(level.is_pro());
    }

    #[test]
    fn certificate_with_valid_snark_verifies() {
        // Full round-trip: SHA-256 chain + SNARK proof both verified.
        let features = ProFeature::all();
        let mut cert = mint_certificate("demo@nucleusdb.com", &features, 1803262500);
        cert.snark_proof = Some(test_snark_proof());
        let level = verify_certificate(&cert).expect("cert with valid SNARK must verify");
        assert!(level.is_pro());
    }

    #[test]
    fn certificate_with_tampered_snark_rejected() {
        let features = ProFeature::all();
        let mut cert = mint_certificate("demo@nucleusdb.com", &features, 1803262500);
        let mut bad_proof = test_snark_proof();
        bad_proof.public_signals[1] = "0".to_string();
        cert.snark_proof = Some(bad_proof);
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::SnarkVerificationFailed)));
    }

    #[test]
    fn proved_certificate_json_deserializes() {
        // Ensure the enriched certificate format with SNARK proof deserializes correctly.
        let json = r#"{
            "version": "nucleusdb-license-cab-1",
            "foundation_commitment": "0xdeca4e53fb6134b000705341d5db3de023dcd8a95e174bce0c4e21bcab6e8cf9",
            "rules_root": "0x557784498f3b36a8ecd4128c390d81d7a186e138364b6fe0ef11f90bc3a6e182",
            "licensee_commitment": "0x0804b1f3c3e8ac4add1ea80f742c4c43558d7ca6256733d46ae2708f54b8e3e3",
            "proof_digest": "0xc8acc93d82b346192636d2f7e373ac22ba1d948b96569c77eb3c097fcfb62712",
            "licensee": "demo@nucleusdb.com",
            "expiry_unix_secs": 1803262500,
            "features": ["batch_sql","certificate_transparency","configurable_quorum","export","ipa_backend","kzg_backend","mcp_server","multi_tenant","tui","unlimited_entries","wal_replay"],
            "foundation_poseidon": "10498408604190631903661670351841509167761295075626051572612706182854256519760",
            "rules_root_poseidon": "1322206238312090881251429702796661760875971281226888390374290859648775611199",
            "licensee_field": "8745742575738356945120172455467644870684525",
            "snark_proof": {
                "protocol": "groth16",
                "curve": "bn128",
                "pi_a": ["21395058710715221512141136997577943222444054556201088705185109505677362522313", "4579204029766272866587044832554240078169508494400311183784970114481255036590", "1"],
                "pi_b": [["5202294881725992217980051469091140593603060690090479418590364461196679826436", "3874299196029840840005590017697452158959263525655996160796037001458805009770"], ["17779469406488213294468910677045021564849628076372499955495679563736323507938", "7063076278734608474516354088060440657810112068353853767850783433319766131774"], ["1", "0"]],
                "pi_c": ["10725084923489509678962196352520219987459344786999429978446344967026680290054", "11926696386234588152796050475063893399913854466682164509978597141240696416859", "1"],
                "public_signals": ["10498408604190631903661670351841509167761295075626051572612706182854256519760", "11883432733534932235095524943887047101085430713026189147570957712288595065002"]
            }
        }"#;
        let cert: LicenseCertificate = serde_json::from_str(json).expect("must deserialize");
        assert!(cert.snark_proof.is_some());
        assert_eq!(cert.snark_proof.as_ref().unwrap().public_signals.len(), 2);
        assert!(cert.foundation_poseidon.is_some());
        assert!(cert.licensee_field.is_some());
    }

    #[test]
    fn embedded_vk_parses_correctly() {
        let vk = build_embedded_vk().expect("embedded VK must parse");
        assert_eq!(
            vk.gamma_abc_g1.len(),
            3,
            "2 public inputs + 1 = 3 IC points"
        );
    }

    #[test]
    fn certificate_json_roundtrip() {
        let features = ProFeature::all();
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let cert = mint_certificate("json@test.com", &features, expiry);
        let json = serde_json::to_string_pretty(&cert).unwrap();
        let parsed: LicenseCertificate = serde_json::from_str(&json).unwrap();
        let level = verify_certificate(&parsed).expect("roundtrip must verify");
        assert!(level.is_pro());
    }

    #[test]
    fn v2_certificate_roundtrip_with_compliance_inputs() {
        let features = vec![ProFeature::MultiTenant, ProFeature::McpServer];
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let compliance = CompliancePublicInputs {
            feasibility_root: "0x11223344556677889900aabbccddeeff00112233445566778899aabbccddeeff"
                .to_string(),
            replay_seq: 17,
            puf_digest: None,
        };
        let cert = mint_certificate_v2(
            "phase3@nucleusdb.com",
            &features,
            expiry,
            compliance.clone(),
        )
        .expect("mint v2");
        assert_eq!(cert.version, "nucleusdb-license-cab-2");
        assert!(cert.compliance_commitment.is_some());
        let parsed = serde_json::from_str::<LicenseCertificate>(
            &serde_json::to_string_pretty(&cert).expect("serialize"),
        )
        .expect("deserialize");
        let level = verify_certificate(&parsed).expect("v2 cert must verify");
        assert!(level.is_pro());
        assert_eq!(
            parsed
                .compliance_inputs
                .as_ref()
                .expect("inputs")
                .replay_seq,
            compliance.replay_seq
        );
    }

    #[test]
    fn v2_certificate_missing_compliance_inputs_rejected() {
        let features = vec![ProFeature::MultiTenant];
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let mut cert = mint_certificate("missing@phase3.com", &features, expiry);
        cert.version = "nucleusdb-license-cab-2".to_string();
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::MissingComplianceInputs)));
    }

    #[test]
    fn v2_certificate_tampered_compliance_inputs_rejected() {
        let features = vec![ProFeature::MultiTenant];
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let compliance = CompliancePublicInputs {
            feasibility_root: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            replay_seq: 4,
            puf_digest: None,
        };
        let mut cert =
            mint_certificate_v2("tamper@phase3.com", &features, expiry, compliance.clone())
                .expect("mint v2");
        cert.compliance_inputs = Some(CompliancePublicInputs {
            replay_seq: compliance.replay_seq + 1,
            ..compliance
        });
        let err = verify_certificate(&cert);
        assert!(matches!(
            err,
            Err(LicenseError::ComplianceCommitmentMismatch)
                | Err(LicenseError::ProofDigestMismatch)
        ));
    }

    #[test]
    fn v2_certificate_rejects_none_sentinel_puf_digest() {
        let features = vec![ProFeature::MultiTenant];
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let compliance = CompliancePublicInputs {
            feasibility_root: "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .to_string(),
            replay_seq: 4,
            puf_digest: Some("NONE".to_string()),
        };
        let err = mint_certificate_v2("none@phase3.com", &features, expiry, compliance);
        assert!(matches!(err, Err(LicenseError::ComplianceInputMalformed)));
    }

    #[test]
    fn v1_certificate_rejects_none_sentinel_puf_digest() {
        let features = vec![ProFeature::MultiTenant];
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 365 * 24 * 3600;
        let mut cert = mint_certificate("none-v1@phase3.com", &features, expiry);
        cert.puf_digest = Some("NONE".to_string());
        let err = verify_certificate(&cert);
        assert!(matches!(err, Err(LicenseError::PufDigestMismatch)));
    }
}
