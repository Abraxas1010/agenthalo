use super::{FieldElem, RootDigest, VC};
use ark_bls12_381::{Bls12_381, Fr};
use ark_ec::{pairing::Pairing, PrimeGroup};
use ark_ff::Field;
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use kzg_mini::{KZGCeremony, KZGProof as MiniKzgProof, Polynomial};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitment {
    pub encoded: Vec<u8>,
    pub degree: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Proof {
    pub index: usize,
    pub value: FieldElem,
    pub degree: usize,
    pub proof_encoded: Vec<u8>,
}

pub struct DemoKzg;

const CURVE_BLS12_381: &str = "bls12-381";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustedSetupArtifact {
    pub setup_id: String,
    pub curve: String,
    pub max_degree: usize,
    pub tau_seed_hex: String,
    pub attestation_sha512: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrustedSetupError {
    Io(String),
    Parse(String),
    CurveMismatch { expected: String, got: String },
    InvalidMaxDegree { max_degree: usize },
    InvalidTauSeedHex { tau_seed_hex: String },
    SetupIdMismatch { expected: String, got: String },
    ArtifactAttestationMismatch { expected: String, got: String },
    ExpectedAttestationMismatch { expected: String, got: String },
    DegreeExceedsSetup { requested: usize, max: usize },
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

fn decode_hex_exact<const N: usize>(s: &str) -> Option<[u8; N]> {
    let bytes = s.as_bytes();
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

fn to_field(v: FieldElem) -> Fr {
    Fr::from(v)
}

fn domain_point(i: usize) -> Fr {
    // 1-based points avoid accidental reuse of x=0 in interpolation edge-cases.
    Fr::from((i as u64) + 1)
}

fn interpolate_vector(values: &[FieldElem]) -> Polynomial<Fr> {
    if values.is_empty() {
        return Polynomial::zero();
    }

    let mut poly = Polynomial::zero();
    for (j, y_raw) in values.iter().enumerate() {
        let xj = domain_point(j);
        let yj = to_field(*y_raw);
        let mut basis = Polynomial::new(vec![Fr::from(1u64)]);
        let mut denom = Fr::from(1u64);

        for m in 0..values.len() {
            if m == j {
                continue;
            }
            let xm = domain_point(m);
            basis = basis.mul(&Polynomial::new(vec![-xm, Fr::from(1u64)]));
            denom *= xj - xm;
        }

        let inv = denom.inverse().unwrap_or(Fr::from(0u64));
        poly = poly.add(&basis.scale(yj * inv));
    }
    poly
}

fn setup_with_degree(degree: usize) -> KZGCeremony<Bls12_381> {
    // Deterministic setup-id derivation for reproducible adapter behavior.
    // In production, load from a trusted setup ceremony artifact.
    let mut h = Sha512::new();
    h.update(b"nucleusdb.kzg.setup.v1");
    h.update((degree as u64).to_le_bytes());
    let digest = h.finalize();
    let mut tau_bytes = [0u8; 8];
    tau_bytes.copy_from_slice(&digest[..8]);
    let tau = Fr::from(u64::from_le_bytes(tau_bytes));
    let g1 = <Bls12_381 as Pairing>::G1::generator();
    let g2 = <Bls12_381 as Pairing>::G2::generator();
    KZGCeremony::<Bls12_381>::setup(degree.max(1), tau, g1, g2)
}

fn setup_from_artifact(
    degree: usize,
    artifact: &TrustedSetupArtifact,
) -> Result<KZGCeremony<Bls12_381>, TrustedSetupError> {
    if degree > artifact.max_degree {
        return Err(TrustedSetupError::DegreeExceedsSetup {
            requested: degree,
            max: artifact.max_degree,
        });
    }
    let tau_seed = decode_hex_exact::<8>(&artifact.tau_seed_hex).ok_or_else(|| {
        TrustedSetupError::InvalidTauSeedHex {
            tau_seed_hex: artifact.tau_seed_hex.clone(),
        }
    })?;
    let tau = Fr::from(u64::from_le_bytes(tau_seed));
    let g1 = <Bls12_381 as Pairing>::G1::generator();
    let g2 = <Bls12_381 as Pairing>::G2::generator();
    Ok(KZGCeremony::<Bls12_381>::setup(degree.max(1), tau, g1, g2))
}

fn encode_g1(point: &<Bls12_381 as Pairing>::G1) -> Vec<u8> {
    let mut out = Vec::new();
    point
        .serialize_compressed(&mut out)
        .expect("serialize G1 commitment");
    out
}

fn decode_g1(bytes: &[u8]) -> Option<<Bls12_381 as Pairing>::G1> {
    <Bls12_381 as Pairing>::G1::deserialize_compressed(bytes).ok()
}

fn encode_proof(proof: &MiniKzgProof<Bls12_381>) -> Vec<u8> {
    let mut out = Vec::new();
    proof
        .proof
        .serialize_compressed(&mut out)
        .expect("serialize KZG proof point");
    out
}

fn decode_proof(
    degree: usize,
    index: usize,
    value: FieldElem,
    encoded: &[u8],
) -> Option<MiniKzgProof<Bls12_381>> {
    let point = domain_point(index);
    let val = Fr::from(value);
    let g1 = decode_g1(encoded)?;
    let _ = degree;
    Some(MiniKzgProof {
        point,
        value: val,
        proof: g1,
    })
}

fn digest_bytes(bytes: &[u8]) -> RootDigest {
    let mut h = Sha512::new();
    h.update(b"nucleusdb.kzg.digest.v1");
    h.update(bytes);
    let d = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&d[..32]);
    out
}

pub fn resolve_trusted_setup_path(path: &str) -> PathBuf {
    let pathbuf = PathBuf::from(path);
    if pathbuf.is_absolute() {
        pathbuf
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(pathbuf)
    }
}

pub fn compute_trusted_setup_attestation_hex(
    setup_id: &str,
    curve: &str,
    max_degree: usize,
    tau_seed_hex: &str,
) -> Result<String, TrustedSetupError> {
    let tau_seed = decode_hex_exact::<8>(tau_seed_hex).ok_or_else(|| {
        TrustedSetupError::InvalidTauSeedHex {
            tau_seed_hex: tau_seed_hex.to_string(),
        }
    })?;
    let mut h = Sha512::new();
    h.update(b"nucleusdb.kzg.attestation.v1");
    h.update(setup_id.as_bytes());
    h.update([0u8]);
    h.update(curve.as_bytes());
    h.update([0u8]);
    h.update((max_degree as u64).to_le_bytes());
    h.update(tau_seed);
    Ok(encode_hex(&h.finalize()))
}

pub fn load_trusted_setup_artifact(path: &Path) -> Result<TrustedSetupArtifact, TrustedSetupError> {
    let raw = fs::read_to_string(path).map_err(|e| {
        TrustedSetupError::Io(format!(
            "failed to read trusted setup {}: {e}",
            path.display()
        ))
    })?;
    serde_json::from_str::<TrustedSetupArtifact>(&raw).map_err(|e| {
        TrustedSetupError::Parse(format!(
            "failed to parse trusted setup {}: {e}",
            path.display()
        ))
    })
}

pub fn validate_trusted_setup_artifact(
    artifact: &TrustedSetupArtifact,
    expected_setup_id: &str,
    expected_attestation: &str,
) -> Result<(), TrustedSetupError> {
    if artifact.curve != CURVE_BLS12_381 {
        return Err(TrustedSetupError::CurveMismatch {
            expected: CURVE_BLS12_381.to_string(),
            got: artifact.curve.clone(),
        });
    }
    if artifact.max_degree == 0 {
        return Err(TrustedSetupError::InvalidMaxDegree { max_degree: 0 });
    }
    if artifact.setup_id != expected_setup_id {
        return Err(TrustedSetupError::SetupIdMismatch {
            expected: expected_setup_id.to_string(),
            got: artifact.setup_id.clone(),
        });
    }

    let recomputed = compute_trusted_setup_attestation_hex(
        &artifact.setup_id,
        &artifact.curve,
        artifact.max_degree,
        &artifact.tau_seed_hex,
    )?;
    if artifact.attestation_sha512 != recomputed {
        return Err(TrustedSetupError::ArtifactAttestationMismatch {
            expected: recomputed,
            got: artifact.attestation_sha512.clone(),
        });
    }
    if artifact.attestation_sha512 != expected_attestation {
        return Err(TrustedSetupError::ExpectedAttestationMismatch {
            expected: expected_attestation.to_string(),
            got: artifact.attestation_sha512.clone(),
        });
    }
    Ok(())
}

pub fn load_and_validate_trusted_setup(
    path: &str,
    expected_setup_id: &str,
    expected_attestation: &str,
) -> Result<TrustedSetupArtifact, TrustedSetupError> {
    let resolved = resolve_trusted_setup_path(path);
    let artifact = load_trusted_setup_artifact(&resolved)?;
    validate_trusted_setup_artifact(&artifact, expected_setup_id, expected_attestation)?;
    Ok(artifact)
}

impl VC for DemoKzg {
    type Commitment = Commitment;
    type Proof = Proof;

    fn commit(v: &[FieldElem]) -> Self::Commitment {
        let poly = interpolate_vector(v);
        let degree = poly.coeffs.len();
        let ceremony = setup_with_degree(degree);
        let cm = ceremony.commit(&poly);
        Commitment {
            encoded: encode_g1(&cm),
            degree,
        }
    }

    fn open(v: &[FieldElem], i: usize) -> Self::Proof {
        let poly = interpolate_vector(v);
        let degree = poly.coeffs.len();
        let ceremony = setup_with_degree(degree);
        let point = domain_point(i);
        let kzg_proof = ceremony.open(&poly, point);
        let value = v.get(i).copied().unwrap_or(0);
        Proof {
            index: i,
            value,
            degree,
            proof_encoded: encode_proof(&kzg_proof),
        }
    }

    fn verify(c: &Self::Commitment, i: usize, value: &FieldElem, p: &Self::Proof) -> bool {
        if p.index != i || p.value != *value {
            return false;
        }
        if p.degree != c.degree {
            return false;
        }
        let ceremony = setup_with_degree(c.degree);
        let commitment = match decode_g1(&c.encoded) {
            Some(v) => v,
            None => return false,
        };
        let proof = match decode_proof(p.degree, p.index, p.value, &p.proof_encoded) {
            Some(v) => v,
            None => return false,
        };
        let expected_point = domain_point(i);
        let expected_value = Fr::from(*value);
        if proof.point != expected_point || proof.value != expected_value {
            return false;
        }
        ceremony.verify(commitment, &proof)
    }

    fn digest(c: &Self::Commitment) -> RootDigest {
        digest_bytes(&c.encoded)
    }
}

impl DemoKzg {
    pub fn commit_trusted(
        v: &[FieldElem],
        setup: &TrustedSetupArtifact,
    ) -> Result<Commitment, TrustedSetupError> {
        let poly = interpolate_vector(v);
        let degree = poly.coeffs.len();
        let ceremony = setup_from_artifact(degree, setup)?;
        let cm = ceremony.commit(&poly);
        Ok(Commitment {
            encoded: encode_g1(&cm),
            degree,
        })
    }

    pub fn open_trusted(
        v: &[FieldElem],
        i: usize,
        setup: &TrustedSetupArtifact,
    ) -> Result<Proof, TrustedSetupError> {
        let poly = interpolate_vector(v);
        let degree = poly.coeffs.len();
        let ceremony = setup_from_artifact(degree, setup)?;
        let point = domain_point(i);
        let kzg_proof = ceremony.open(&poly, point);
        let value = v.get(i).copied().unwrap_or(0);
        Ok(Proof {
            index: i,
            value,
            degree,
            proof_encoded: encode_proof(&kzg_proof),
        })
    }

    pub fn verify_trusted(
        c: &Commitment,
        i: usize,
        value: &FieldElem,
        p: &Proof,
        setup: &TrustedSetupArtifact,
    ) -> Result<bool, TrustedSetupError> {
        if p.index != i || p.value != *value {
            return Ok(false);
        }
        if p.degree != c.degree {
            return Ok(false);
        }
        let ceremony = setup_from_artifact(c.degree, setup)?;
        let commitment = match decode_g1(&c.encoded) {
            Some(v) => v,
            None => return Ok(false),
        };
        let proof = match decode_proof(p.degree, p.index, p.value, &p.proof_encoded) {
            Some(v) => v,
            None => return Ok(false),
        };
        let expected_point = domain_point(i);
        let expected_value = Fr::from(*value);
        if proof.point != expected_point || proof.value != expected_value {
            return Ok(false);
        }
        Ok(ceremony.verify(commitment, &proof))
    }
}
