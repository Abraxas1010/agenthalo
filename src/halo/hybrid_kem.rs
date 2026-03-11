//! Hybrid KEM combining X25519 + ML-KEM-768.
//!
//! Follows IETF Composite ML-KEM construction:
//!   combined_ss = HKDF-SHA512(
//!     salt = "AgentHALO-HybridKEM-v2",
//!     ikm  = x25519_ss || mlkem_ss || mlkem_ct,
//!     info = <caller-provided context>
//!   )
//!
//! The ML-KEM ciphertext is included in the KDF input per the IETF composite
//! KEM spec to bind the shared secret to the specific encapsulation, preventing
//! ciphertext substitution attacks.

use hkdf::Hkdf;
use ml_kem::kem::{Decapsulate, Encapsulate};
use ml_kem::{Encoded, EncodedSizeUser, KemCore, MlKem768};
use rand_core::OsRng;
use sha2::Sha512;
use x25519_dalek::{PublicKey as X25519PublicKey, StaticSecret as X25519StaticSecret};
use zeroize::Zeroize;

const HYBRID_KEM_SALT: &[u8] = b"AgentHALO-HybridKEM-v2";

pub type MlKem768EncapsulationKey = <MlKem768 as KemCore>::EncapsulationKey;
pub type MlKem768DecapsulationKey = <MlKem768 as KemCore>::DecapsulationKey;

pub struct HybridKemEncapResult {
    /// The combined shared secret (32 bytes), used to derive the CEK.
    pub shared_secret: [u8; 32],
    /// X25519 ephemeral public key (32 bytes) — sent to recipient.
    pub x25519_ephemeral_pk: [u8; 32],
    /// ML-KEM-768 ciphertext (1088 bytes) — sent to recipient.
    /// None if classical-only fallback.
    pub mlkem_ciphertext: Option<Vec<u8>>,
}

pub struct HybridKemDecapResult {
    /// The combined shared secret (32 bytes).
    pub shared_secret: [u8; 32],
}

#[derive(Debug)]
pub enum HybridKemError {
    MlKemEncapFailed(String),
    MlKemDecapFailed(String),
    MlKemCtDeserFailed(String),
    MlKemEkDeserFailed(String),
    HkdfExpandFailed,
    MissingMlKemKey,
}

impl std::fmt::Display for HybridKemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MlKemEncapFailed(e) => write!(f, "ML-KEM encapsulation failed: {e}"),
            Self::MlKemDecapFailed(e) => write!(f, "ML-KEM decapsulation failed: {e}"),
            Self::MlKemCtDeserFailed(e) => {
                write!(f, "ML-KEM ciphertext deserialization failed: {e}")
            }
            Self::MlKemEkDeserFailed(e) => {
                write!(f, "ML-KEM encapsulation key deserialization failed: {e}")
            }
            Self::HkdfExpandFailed => write!(f, "HKDF expand failed"),
            Self::MissingMlKemKey => write!(
                f,
                "ML-KEM decapsulation key required for hybrid decap but ciphertext present"
            ),
        }
    }
}

impl std::error::Error for HybridKemError {}

/// Combine X25519 shared secret with optional ML-KEM shared secret via HKDF.
fn combine_shared_secrets(
    x25519_ss: &[u8],
    mlkem_ss: Option<&[u8]>,
    mlkem_ct: Option<&[u8]>,
    info: &[u8],
) -> Result<[u8; 32], HybridKemError> {
    let mut ikm = Vec::with_capacity(32 + 32 + 1088);
    ikm.extend_from_slice(x25519_ss);
    if let (Some(ss), Some(ct)) = (mlkem_ss, mlkem_ct) {
        ikm.extend_from_slice(ss);
        ikm.extend_from_slice(ct);
    }

    let hk = Hkdf::<Sha512>::new(Some(HYBRID_KEM_SALT), &ikm);
    let mut combined = [0u8; 32];
    hk.expand(info, &mut combined)
        .map_err(|_| HybridKemError::HkdfExpandFailed)?;
    ikm.zeroize();
    Ok(combined)
}

/// Encapsulate: sender side.
///
/// If `recipient_mlkem_ek` is Some, produces a hybrid KEM shared secret
/// combining X25519 + ML-KEM-768. If None, falls back to classical X25519 only.
pub fn hybrid_encap(
    recipient_x25519_pk: &X25519PublicKey,
    recipient_mlkem_ek: Option<&MlKem768EncapsulationKey>,
    info: &[u8],
) -> Result<HybridKemEncapResult, HybridKemError> {
    let mut rng = OsRng;

    // X25519 ephemeral key agreement
    let x25519_ephemeral_sk = X25519StaticSecret::random_from_rng(rng);
    let x25519_ephemeral_pk = X25519PublicKey::from(&x25519_ephemeral_sk);
    let x25519_ss = x25519_ephemeral_sk.diffie_hellman(recipient_x25519_pk);

    let (mlkem_ss_bytes, mlkem_ct_raw) = if let Some(ek) = recipient_mlkem_ek {
        let (ct, ss) = ek
            .encapsulate(&mut rng)
            .map_err(|_| HybridKemError::MlKemEncapFailed("encapsulate returned error".into()))?;
        (Some(ss), Some(ct.as_slice().to_vec()))
    } else {
        eprintln!(
            "warning: hybrid_encap: recipient lacks ML-KEM-768 key, \
             falling back to classical X25519 only"
        );
        (None, None)
    };

    let combined = combine_shared_secrets(
        x25519_ss.as_bytes(),
        mlkem_ss_bytes.as_ref().map(|ss| ss.as_slice()),
        mlkem_ct_raw.as_deref(),
        info,
    )?;

    Ok(HybridKemEncapResult {
        shared_secret: combined,
        x25519_ephemeral_pk: x25519_ephemeral_pk.to_bytes(),
        mlkem_ciphertext: mlkem_ct_raw,
    })
}

/// Decapsulate: recipient side.
///
/// If `mlkem_ciphertext` is Some, performs hybrid decapsulation combining
/// X25519 + ML-KEM-768. If None, classical X25519 only.
pub fn hybrid_decap(
    x25519_ephemeral_pk: &[u8; 32],
    mlkem_ciphertext: Option<&[u8]>,
    own_x25519_sk: &X25519StaticSecret,
    own_mlkem_dk: Option<&MlKem768DecapsulationKey>,
    info: &[u8],
) -> Result<HybridKemDecapResult, HybridKemError> {
    let peer_pk = X25519PublicKey::from(*x25519_ephemeral_pk);
    let x25519_ss = own_x25519_sk.diffie_hellman(&peer_pk);

    let mlkem_ss_bytes = if let Some(ct_bytes) = mlkem_ciphertext {
        let dk = own_mlkem_dk.ok_or(HybridKemError::MissingMlKemKey)?;
        let ct: ml_kem::Ciphertext<MlKem768> = ct_bytes.try_into().map_err(|_| {
            HybridKemError::MlKemCtDeserFailed(format!(
                "expected 1088 bytes, got {}",
                ct_bytes.len()
            ))
        })?;
        let ss = dk
            .decapsulate(&ct)
            .map_err(|_| HybridKemError::MlKemDecapFailed("decapsulate returned error".into()))?;
        Some(ss)
    } else {
        None
    };

    let combined = combine_shared_secrets(
        x25519_ss.as_bytes(),
        mlkem_ss_bytes.as_ref().map(|ss| ss.as_slice()),
        mlkem_ciphertext,
        info,
    )?;

    Ok(HybridKemDecapResult {
        shared_secret: combined,
    })
}

/// Deserialize an ML-KEM-768 encapsulation key from raw bytes.
pub fn mlkem768_ek_from_bytes(raw: &[u8]) -> Result<MlKem768EncapsulationKey, HybridKemError> {
    let arr: Encoded<MlKem768EncapsulationKey> = raw.try_into().map_err(|_| {
        HybridKemError::MlKemEkDeserFailed(format!(
            "expected encapsulation key bytes, got {} bytes",
            raw.len()
        ))
    })?;
    Ok(MlKem768EncapsulationKey::from_bytes(&arr))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_keypair() -> (
        X25519StaticSecret,
        X25519PublicKey,
        MlKem768DecapsulationKey,
    ) {
        let x25519_sk = X25519StaticSecret::random_from_rng(OsRng);
        let x25519_pk = X25519PublicKey::from(&x25519_sk);
        let (dk, _) = MlKem768::generate(&mut OsRng);
        (x25519_sk, x25519_pk, dk)
    }

    const TEST_INFO: &[u8] = b"test-info";

    #[test]
    fn hybrid_kem_roundtrip() {
        let (x25519_sk, x25519_pk, dk) = test_keypair();
        let ek = dk.encapsulation_key();

        let encap = hybrid_encap(&x25519_pk, Some(ek), TEST_INFO).unwrap();
        assert!(encap.mlkem_ciphertext.is_some());

        let decap = hybrid_decap(
            &encap.x25519_ephemeral_pk,
            encap.mlkem_ciphertext.as_deref(),
            &x25519_sk,
            Some(&dk),
            TEST_INFO,
        )
        .unwrap();

        assert_eq!(encap.shared_secret, decap.shared_secret);
    }

    #[test]
    fn hybrid_kem_different_recipients_different_ss() {
        let (_, x25519_pk1, dk1) = test_keypair();
        let (_, x25519_pk2, dk2) = test_keypair();
        let ek1 = dk1.encapsulation_key();
        let ek2 = dk2.encapsulation_key();

        let r1 = hybrid_encap(&x25519_pk1, Some(ek1), TEST_INFO).unwrap();
        let r2 = hybrid_encap(&x25519_pk2, Some(ek2), TEST_INFO).unwrap();

        assert_ne!(r1.shared_secret, r2.shared_secret);
    }

    #[test]
    fn hybrid_kem_ct_is_1088_bytes() {
        let (_, x25519_pk, dk) = test_keypair();
        let ek = dk.encapsulation_key();

        let r = hybrid_encap(&x25519_pk, Some(ek), TEST_INFO).unwrap();
        assert_eq!(r.mlkem_ciphertext.as_ref().unwrap().len(), 1088);
    }

    #[test]
    fn classical_fallback_when_no_mlkem_key() {
        let (x25519_sk, x25519_pk, _) = test_keypair();

        let encap = hybrid_encap(&x25519_pk, None, TEST_INFO).unwrap();
        assert!(encap.mlkem_ciphertext.is_none());

        let decap = hybrid_decap(
            &encap.x25519_ephemeral_pk,
            None,
            &x25519_sk,
            None,
            TEST_INFO,
        )
        .unwrap();

        assert_eq!(encap.shared_secret, decap.shared_secret);
    }

    #[test]
    fn ek_serialization_roundtrip() {
        let (_, _, dk) = test_keypair();
        let ek = dk.encapsulation_key();
        let raw = ek.as_bytes().to_vec();
        let recovered = mlkem768_ek_from_bytes(&raw).unwrap();
        assert_eq!(ek.as_bytes(), recovered.as_bytes());
    }

    #[test]
    fn tampered_ct_produces_different_ss() {
        let (x25519_sk, x25519_pk, dk) = test_keypair();
        let ek = dk.encapsulation_key();

        let encap = hybrid_encap(&x25519_pk, Some(ek), TEST_INFO).unwrap();
        let mut tampered_ct = encap.mlkem_ciphertext.clone().unwrap();
        tampered_ct[0] ^= 0xFF;

        // ML-KEM uses implicit rejection — decap "succeeds" but produces wrong SS
        let decap = hybrid_decap(
            &encap.x25519_ephemeral_pk,
            Some(&tampered_ct),
            &x25519_sk,
            Some(&dk),
            TEST_INFO,
        )
        .unwrap();

        assert_ne!(encap.shared_secret, decap.shared_secret);
    }

    #[test]
    fn bad_ek_bytes_rejected() {
        let result = mlkem768_ek_from_bytes(&[0u8; 10]);
        assert!(result.is_err());
    }

    #[test]
    fn hkdf_sha512_produces_same_length_key() {
        let combined = combine_shared_secrets(
            &[0x11; 32],
            Some(&[0x22; 32]),
            Some(&[0x33; 1088]),
            TEST_INFO,
        )
        .expect("combine secrets");
        assert_eq!(combined.len(), 32);
    }
}
