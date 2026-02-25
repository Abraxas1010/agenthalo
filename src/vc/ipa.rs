use super::{FieldElem, RootDigest, VC};
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use sha2::{Digest, Sha512};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commitment {
    pub encoded: [u8; 32],
    pub len: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Proof {
    pub index: usize,
    pub value: FieldElem,
    // P1.3: replace full-vector payload with logarithmic IPA opening proof.
    pub vector: Vec<FieldElem>,
}

pub struct DemoIpa;

fn point_digest(point: &[u8; 32]) -> RootDigest {
    let mut h = Sha512::new();
    h.update(b"nucleusdb.ipa.digest.v1");
    h.update(point);
    let out = h.finalize();
    let mut bytes = [0u8; 32];
    bytes.copy_from_slice(&out[..32]);
    bytes
}

fn generator_for_index(i: usize) -> RistrettoPoint {
    let mut h = Sha512::new();
    h.update(b"nucleusdb.ipa.generator.v1");
    h.update((i as u64).to_le_bytes());
    RistrettoPoint::from_hash(h)
}

fn pedersen_vector_commit(v: &[FieldElem]) -> [u8; 32] {
    let mut acc = RistrettoPoint::default();
    for (i, value) in v.iter().enumerate() {
        let g = generator_for_index(i);
        acc += g * Scalar::from(*value);
    }
    acc.compress().to_bytes()
}

impl VC for DemoIpa {
    type Commitment = Commitment;
    type Proof = Proof;

    fn commit(v: &[FieldElem]) -> Self::Commitment {
        Commitment {
            encoded: pedersen_vector_commit(v),
            len: v.len(),
        }
    }

    fn open(v: &[FieldElem], i: usize) -> Self::Proof {
        let value = v.get(i).copied().unwrap_or(0);
        Proof {
            index: i,
            value,
            vector: v.to_vec(),
        }
    }

    fn verify(c: &Self::Commitment, i: usize, value: &FieldElem, p: &Self::Proof) -> bool {
        if p.index != i || p.value != *value {
            return false;
        }
        if p.vector.len() != c.len {
            return false;
        }
        if p.vector.get(i).copied().unwrap_or(0) != *value {
            return false;
        }
        pedersen_vector_commit(&p.vector) == c.encoded
    }

    fn digest(c: &Self::Commitment) -> RootDigest {
        point_digest(&c.encoded)
    }
}
