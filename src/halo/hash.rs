//! Cryptographic hash dispatch for PQ-hardened content hashing.
//!
//! Post-quantum threat model: Grover's algorithm halves collision resistance
//! (SHA-256: 128-bit → 64-bit quantum collision). SHA-512 provides 256-bit
//! collision resistance → 128-bit post-quantum.
//!
//! New entries use SHA-512 by default. Old entries remain SHA-256 and are
//! verified via algorithm dispatch.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};

/// Hash algorithm for content integrity hashing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
}

impl HashAlgorithm {
    /// The algorithm used for all NEW entries (post-PQ hardening).
    pub const CURRENT: HashAlgorithm = HashAlgorithm::Sha512;

    /// Parse from optional string field. `None` or "sha256" → Sha256.
    pub fn from_field(field: Option<&str>) -> Self {
        match field {
            Some("sha512") => HashAlgorithm::Sha512,
            _ => HashAlgorithm::Sha256,
        }
    }

    /// Serialize to the string stored in `hash_algorithm` fields.
    pub fn as_str(&self) -> &'static str {
        match self {
            HashAlgorithm::Sha256 => "sha256",
            HashAlgorithm::Sha512 => "sha512",
        }
    }
}

/// Hash bytes and return raw digest.
pub fn hash_bytes(algo: &HashAlgorithm, data: &[u8]) -> Vec<u8> {
    match algo {
        HashAlgorithm::Sha256 => Sha256::digest(data).to_vec(),
        HashAlgorithm::Sha512 => Sha512::digest(data).to_vec(),
    }
}

/// Hash bytes and return hex-encoded digest string.
pub fn hash_hex(algo: &HashAlgorithm, data: &[u8]) -> String {
    crate::halo::util::hex_encode(&hash_bytes(algo, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_produces_64_hex_chars() {
        let h = hash_hex(&HashAlgorithm::Sha256, b"test");
        assert_eq!(h.len(), 64);
    }

    #[test]
    fn sha512_produces_128_hex_chars() {
        let h = hash_hex(&HashAlgorithm::Sha512, b"test");
        assert_eq!(h.len(), 128);
    }

    #[test]
    fn from_field_defaults_to_sha256() {
        assert_eq!(HashAlgorithm::from_field(None), HashAlgorithm::Sha256);
        assert_eq!(
            HashAlgorithm::from_field(Some("sha256")),
            HashAlgorithm::Sha256
        );
    }

    #[test]
    fn from_field_parses_sha512() {
        assert_eq!(
            HashAlgorithm::from_field(Some("sha512")),
            HashAlgorithm::Sha512
        );
    }

    #[test]
    fn current_is_sha512() {
        assert_eq!(HashAlgorithm::CURRENT, HashAlgorithm::Sha512);
    }
}
