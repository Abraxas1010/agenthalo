use ed25519_dalek::{
    Signature as Ed25519Signature, Signer as Ed25519Signer, SigningKey as Ed25519SigningKey,
    Verifier as Ed25519Verifier, VerifyingKey as Ed25519VerifyingKey,
};
use ml_dsa::{
    EncodedSignature as MlDsaEncodedSignature, KeyGen, MlDsa65, Signature as MlDsaSignature,
    SigningKey as MlDsaSigningKey, VerifyingKey as MlDsaVerifyingKey,
};
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, BTreeSet};

pub const WITNESS_SIGALG_ED25519: &str = "ed25519";
pub const WITNESS_SIGALG_MLDSA65: &str = "ml_dsa65";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WitnessSignatureAlgorithm {
    Ed25519,
    MlDsa65,
}

impl WitnessSignatureAlgorithm {
    pub fn as_tag(self) -> &'static str {
        match self {
            Self::Ed25519 => WITNESS_SIGALG_ED25519,
            Self::MlDsa65 => WITNESS_SIGALG_MLDSA65,
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            WITNESS_SIGALG_ED25519 => Some(Self::Ed25519),
            WITNESS_SIGALG_MLDSA65 => Some(Self::MlDsa65),
            _ => None,
        }
    }
}

const MLDSA65_CONTEXT: &[u8] = b"";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitnessKeyMaterialSource {
    EnvMasterSeed,
    ProvidedMasterSeed,
    InsecureDefaultSeed,
}

#[derive(Clone, Debug)]
pub struct WitnessConfig {
    pub threshold: usize,
    pub witnesses: Vec<String>,
    pub signing_algorithm: WitnessSignatureAlgorithm,
    pub key_material_source: WitnessKeyMaterialSource,
    pub allowed_algorithms: BTreeSet<WitnessSignatureAlgorithm>,
    ed25519_verifying_keys: BTreeMap<String, Ed25519VerifyingKey>,
    ed25519_signing_keys: BTreeMap<String, Ed25519SigningKey>,
    mldsa65_verifying_keys: BTreeMap<String, MlDsaVerifyingKey<MlDsa65>>,
    mldsa65_signing_keys: BTreeMap<String, MlDsaSigningKey<MlDsa65>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WitnessError {
    MissingSigningKey { witness: String },
    MissingVerifyingKey { witness: String },
    MissingMlDsaSigningKey { witness: String },
    MissingMlDsaVerifyingKey { witness: String },
    InvalidSignatureHex,
    InvalidMlDsaSignatureHex,
    MlDsaSigningFailed,
    UnknownAlgorithmTag { tag: String },
}

impl WitnessConfig {
    pub fn with_generated_keys(threshold: usize, witnesses: Vec<String>) -> Self {
        let (seed, key_material_source) = match std::env::var("NUCLEUSDB_WITNESS_MASTER_SEED") {
            Ok(s) => (s, WitnessKeyMaterialSource::EnvMasterSeed),
            Err(_) => (
                "nucleusdb.dev.witness.seed.v1".to_string(),
                WitnessKeyMaterialSource::InsecureDefaultSeed,
            ),
        };
        let signing_algorithm = std::env::var("NUCLEUSDB_WITNESS_SIGNATURE_ALGORITHM")
            .ok()
            .and_then(|tag| WitnessSignatureAlgorithm::from_tag(tag.trim()))
            .unwrap_or(WitnessSignatureAlgorithm::MlDsa65);
        Self::with_seed_algorithm_source(
            threshold,
            witnesses,
            &seed,
            signing_algorithm,
            key_material_source,
        )
    }

    pub fn with_seed(threshold: usize, witnesses: Vec<String>, seed: &str) -> Self {
        Self::with_seed_algorithm_source(
            threshold,
            witnesses,
            seed,
            WitnessSignatureAlgorithm::MlDsa65,
            WitnessKeyMaterialSource::ProvidedMasterSeed,
        )
    }

    pub fn with_seed_and_algorithm(
        threshold: usize,
        witnesses: Vec<String>,
        seed: &str,
        signing_algorithm: WitnessSignatureAlgorithm,
    ) -> Self {
        Self::with_seed_algorithm_source(
            threshold,
            witnesses,
            seed,
            signing_algorithm,
            WitnessKeyMaterialSource::ProvidedMasterSeed,
        )
    }

    fn with_seed_algorithm_source(
        threshold: usize,
        witnesses: Vec<String>,
        seed: &str,
        signing_algorithm: WitnessSignatureAlgorithm,
        key_material_source: WitnessKeyMaterialSource,
    ) -> Self {
        let mut ed25519_verifying_keys = BTreeMap::new();
        let mut ed25519_signing_keys = BTreeMap::new();
        let mut mldsa65_verifying_keys = BTreeMap::new();
        let mut mldsa65_signing_keys = BTreeMap::new();
        for witness in &witnesses {
            let mut ed_h = Sha512::new();
            ed_h.update(b"nucleusdb.witness.sk.v2|ed25519");
            ed_h.update(seed.as_bytes());
            ed_h.update([0u8]);
            ed_h.update(witness.as_bytes());
            let ed_digest = ed_h.finalize();
            let mut ed_sk_bytes = [0u8; 32];
            ed_sk_bytes.copy_from_slice(&ed_digest[..32]);
            let ed_sk = Ed25519SigningKey::from_bytes(&ed_sk_bytes);
            ed25519_verifying_keys.insert(witness.clone(), ed_sk.verifying_key());
            ed25519_signing_keys.insert(witness.clone(), ed_sk);

            let mut ml_h = Sha512::new();
            ml_h.update(b"nucleusdb.witness.sk.v2|ml_dsa65");
            ml_h.update(seed.as_bytes());
            ml_h.update([0u8]);
            ml_h.update(witness.as_bytes());
            let ml_digest = ml_h.finalize();
            let ml_seed = ml_dsa::Seed::try_from(&ml_digest[..32])
                .expect("ML-DSA seed slice must be exactly 32 bytes");
            let ml_kp = MlDsa65::from_seed(&ml_seed);
            mldsa65_verifying_keys.insert(witness.clone(), ml_kp.verifying_key().clone());
            mldsa65_signing_keys.insert(witness.clone(), ml_kp.signing_key().clone());
        }

        let allowed_algorithms = std::env::var("NUCLEUSDB_WITNESS_ALLOWED_ALGORITHMS")
            .ok()
            .map(|csv| {
                csv.split(',')
                    .filter_map(|tok| WitnessSignatureAlgorithm::from_tag(tok.trim()))
                    .collect::<BTreeSet<_>>()
            })
            .filter(|set| !set.is_empty())
            .unwrap_or_else(|| {
                BTreeSet::from([
                    WitnessSignatureAlgorithm::Ed25519,
                    WitnessSignatureAlgorithm::MlDsa65,
                ])
            });

        let mut allowed_algorithms = allowed_algorithms;
        if !allowed_algorithms.contains(&signing_algorithm) {
            allowed_algorithms.insert(signing_algorithm);
        }

        Self {
            threshold,
            witnesses,
            signing_algorithm,
            key_material_source,
            allowed_algorithms,
            ed25519_verifying_keys,
            ed25519_signing_keys,
            mldsa65_verifying_keys,
            mldsa65_signing_keys,
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
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

fn hex_decode_exact<const N: usize>(hex: &str) -> Result<[u8; N], WitnessError> {
    let input = hex.as_bytes();
    if input.len() != N * 2 {
        return Err(WitnessError::InvalidSignatureHex);
    }
    let mut out = [0u8; N];
    for i in 0..N {
        let hi = hex_nibble(input[2 * i]).ok_or(WitnessError::InvalidSignatureHex)?;
        let lo = hex_nibble(input[(2 * i) + 1]).ok_or(WitnessError::InvalidSignatureHex)?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_decode_dynamic(hex: &str) -> Result<Vec<u8>, WitnessError> {
    let input = hex.as_bytes();
    if (input.len() & 1) == 1 {
        return Err(WitnessError::InvalidSignatureHex);
    }
    let mut out = Vec::with_capacity(input.len() / 2);
    for i in (0..input.len()).step_by(2) {
        let hi = hex_nibble(input[i]).ok_or(WitnessError::InvalidSignatureHex)?;
        let lo = hex_nibble(input[i + 1]).ok_or(WitnessError::InvalidSignatureHex)?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

pub fn parse_algorithm_tag(tag: &str) -> Result<WitnessSignatureAlgorithm, WitnessError> {
    WitnessSignatureAlgorithm::from_tag(tag).ok_or_else(|| WitnessError::UnknownAlgorithmTag {
        tag: tag.to_string(),
    })
}

pub fn default_algorithm_tag() -> String {
    WITNESS_SIGALG_ED25519.to_string()
}

pub fn sign_message(
    cfg: &WitnessConfig,
    witness: &str,
    message: &str,
) -> Result<String, WitnessError> {
    sign_message_with_algorithm(cfg, cfg.signing_algorithm, witness, message)
}

pub fn sign_message_with_algorithm(
    cfg: &WitnessConfig,
    algorithm: WitnessSignatureAlgorithm,
    witness: &str,
    message: &str,
) -> Result<String, WitnessError> {
    match algorithm {
        WitnessSignatureAlgorithm::Ed25519 => {
            let sk = cfg.ed25519_signing_keys.get(witness).ok_or_else(|| {
                WitnessError::MissingSigningKey {
                    witness: witness.to_string(),
                }
            })?;
            let sig = sk.sign(message.as_bytes());
            Ok(hex_encode(&sig.to_bytes()))
        }
        WitnessSignatureAlgorithm::MlDsa65 => {
            let sk = cfg.mldsa65_signing_keys.get(witness).ok_or_else(|| {
                WitnessError::MissingMlDsaSigningKey {
                    witness: witness.to_string(),
                }
            })?;
            let sig = sk
                .sign_deterministic(message.as_bytes(), MLDSA65_CONTEXT)
                .map_err(|_| WitnessError::MlDsaSigningFailed)?;
            Ok(hex_encode(sig.encode().as_slice()))
        }
    }
}

pub fn verify_signature(
    cfg: &WitnessConfig,
    witness: &str,
    message: &str,
    sig_hex: &str,
) -> Result<bool, WitnessError> {
    verify_signature_with_algorithm(cfg, cfg.signing_algorithm, witness, message, sig_hex)
}

pub fn verify_signature_with_algorithm(
    cfg: &WitnessConfig,
    algorithm: WitnessSignatureAlgorithm,
    witness: &str,
    message: &str,
    sig_hex: &str,
) -> Result<bool, WitnessError> {
    match algorithm {
        WitnessSignatureAlgorithm::Ed25519 => {
            let vk = cfg.ed25519_verifying_keys.get(witness).ok_or_else(|| {
                WitnessError::MissingVerifyingKey {
                    witness: witness.to_string(),
                }
            })?;
            let sig = Ed25519Signature::from_bytes(&hex_decode_exact::<64>(sig_hex)?);
            Ok(vk.verify(message.as_bytes(), &sig).is_ok())
        }
        WitnessSignatureAlgorithm::MlDsa65 => {
            let vk = cfg.mldsa65_verifying_keys.get(witness).ok_or_else(|| {
                WitnessError::MissingMlDsaVerifyingKey {
                    witness: witness.to_string(),
                }
            })?;
            let sig_bytes =
                hex_decode_dynamic(sig_hex).map_err(|_| WitnessError::InvalidMlDsaSignatureHex)?;
            let enc = MlDsaEncodedSignature::<MlDsa65>::try_from(sig_bytes.as_slice())
                .map_err(|_| WitnessError::InvalidMlDsaSignatureHex)?;
            let sig = MlDsaSignature::<MlDsa65>::decode(&enc)
                .ok_or(WitnessError::InvalidMlDsaSignatureHex)?;
            Ok(vk.verify_with_context(message.as_bytes(), MLDSA65_CONTEXT, &sig))
        }
    }
}

pub fn verify_quorum(config: &WitnessConfig, message: &str, sigs: &[(String, String)]) -> bool {
    verify_quorum_for_algorithm(config, config.signing_algorithm, message, sigs)
}

pub fn verify_quorum_for_algorithm(
    config: &WitnessConfig,
    algorithm: WitnessSignatureAlgorithm,
    message: &str,
    sigs: &[(String, String)],
) -> bool {
    if !config.allowed_algorithms.contains(&algorithm) {
        return false;
    }
    let allowed: BTreeSet<_> = config.witnesses.iter().cloned().collect();
    let mut ok_unique: BTreeSet<String> = BTreeSet::new();

    for (w, sig) in sigs {
        if !allowed.contains(w) {
            continue;
        }
        if verify_signature_with_algorithm(config, algorithm, w, message, sig).unwrap_or(false) {
            ok_unique.insert(w.clone());
        }
    }

    ok_unique.len() >= config.threshold
}
