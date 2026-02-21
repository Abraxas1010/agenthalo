pub mod binary_merkle;
pub mod ipa;
pub mod kzg;

pub type FieldElem = u64;
pub type RootDigest = [u8; 32];

pub trait VC {
    type Commitment: Clone;
    type Proof: Clone;

    fn commit(v: &[FieldElem]) -> Self::Commitment;
    fn open(v: &[FieldElem], i: usize) -> Self::Proof;
    fn verify(c: &Self::Commitment, i: usize, value: &FieldElem, p: &Self::Proof) -> bool;
    fn digest(c: &Self::Commitment) -> RootDigest;
}
