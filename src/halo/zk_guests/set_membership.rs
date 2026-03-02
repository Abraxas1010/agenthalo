use crate::halo::util::digest_bytes;
use sha2::{Digest, Sha256};

const MERKLE_DOMAIN: &str = "agenthalo.zk_credential.merkle.v1";
const MEMBERSHIP_DOMAIN: &[u8] = b"agenthalo.set_membership.v1";

fn merkle_parent(left: &[u8; 32], right: &[u8; 32]) -> [u8; 32] {
    let mut payload = [0u8; 64];
    payload[..32].copy_from_slice(left);
    payload[32..].copy_from_slice(right);
    digest_bytes(MERKLE_DOMAIN, &payload)
}

fn verify_merkle_membership(
    leaf: &[u8; 32],
    path: &[[u8; 32]],
    mut index: u64,
    root: &[u8; 32],
) -> bool {
    let mut cur = *leaf;
    for sibling in path {
        cur = if index.is_multiple_of(2) {
            merkle_parent(&cur, sibling)
        } else {
            merkle_parent(sibling, &cur)
        };
        index /= 2;
    }
    &cur == root
}

/// Set-membership guest logic:
/// verifies Merkle membership and returns a deterministic commitment hash.
pub fn execute(
    candidate_hash: [u8; 32],
    merkle_root: [u8; 32],
    merkle_path: &[[u8; 32]],
    merkle_index: u64,
) -> Result<Vec<u8>, String> {
    if !verify_merkle_membership(&candidate_hash, merkle_path, merkle_index, &merkle_root) {
        return Err("candidate is not a member of the committed set".to_string());
    }

    let mut hasher = Sha256::new();
    hasher.update(MEMBERSHIP_DOMAIN);
    hasher.update(candidate_hash);
    hasher.update(merkle_root);
    hasher.update(merkle_index.to_le_bytes());
    for sibling in merkle_path {
        hasher.update(sibling);
    }
    Ok(hasher.finalize().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(bytes: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"leaf");
        hasher.update(bytes);
        let out = hasher.finalize();
        let mut digest = [0u8; 32];
        digest.copy_from_slice(&out);
        digest
    }

    #[test]
    fn execute_accepts_valid_merkle_membership() {
        let l0 = leaf(b"a");
        let l1 = leaf(b"b");
        let l2 = leaf(b"c");
        let p01 = merkle_parent(&l0, &l1);
        let p22 = merkle_parent(&l2, &l2);
        let root = merkle_parent(&p01, &p22);
        let path = vec![l0, p22];
        let journal = execute(l1, root, &path, 1).expect("membership should verify");
        assert_eq!(journal.len(), 32);
    }

    #[test]
    fn execute_rejects_invalid_merkle_membership() {
        let l0 = leaf(b"a");
        let l1 = leaf(b"b");
        let l2 = leaf(b"c");
        let p01 = merkle_parent(&l0, &l1);
        let p22 = merkle_parent(&l2, &l2);
        let root = merkle_parent(&p01, &p22);
        let bad_path = vec![l2, p22];
        let err = execute(l1, root, &bad_path, 1).expect_err("membership should fail");
        assert!(err.contains("not a member"));
    }
}
