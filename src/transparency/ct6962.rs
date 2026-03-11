use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::security::FormalProvenance;

pub type NodeHash = [u8; 32];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedTreeHead {
    pub tree_size: u64,
    pub root_hash: NodeHash,
    pub timestamp_unix_secs: u64,
    pub sig: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InclusionProof {
    pub leaf_index: u64,
    pub tree_size: u64,
    pub leaf_hash: NodeHash,
    pub path: Vec<NodeHash>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsistencyProof {
    pub old_size: u64,
    pub new_size: u64,
    pub path: Vec<NodeHash>,
}

pub fn sha256(data: &[u8]) -> NodeHash {
    let mut h = Sha256::new();
    h.update(data);
    let digest = h.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn leaf_hash(payload: &[u8]) -> NodeHash {
    let mut bytes = Vec::with_capacity(payload.len() + 1);
    bytes.push(0x00);
    bytes.extend_from_slice(payload);
    sha256(&bytes)
}

pub fn node_hash(left: &NodeHash, right: &NodeHash) -> NodeHash {
    let mut bytes = Vec::with_capacity(65);
    bytes.push(0x01);
    bytes.extend_from_slice(left);
    bytes.extend_from_slice(right);
    sha256(&bytes)
}

pub fn empty_hash() -> NodeHash {
    sha256(&[])
}

fn largest_power_of_two_less_than(n: usize) -> usize {
    if n < 2 {
        return 0;
    }
    let mut k: usize = 1;
    while (k << 1) < n {
        k <<= 1;
    }
    k
}

pub fn merkle_tree_hash(leaves: &[NodeHash]) -> NodeHash {
    let n = leaves.len();
    if n == 0 {
        return empty_hash();
    }
    if n == 1 {
        return leaves[0];
    }
    let k = largest_power_of_two_less_than(n);
    let left = merkle_tree_hash(&leaves[..k]);
    let right = merkle_tree_hash(&leaves[k..]);
    node_hash(&left, &right)
}

pub fn inclusion_path(leaves: &[NodeHash], leaf_index: usize) -> Option<Vec<NodeHash>> {
    let n = leaves.len();
    if leaf_index >= n {
        return None;
    }
    if n <= 1 {
        return Some(vec![]);
    }
    let k = largest_power_of_two_less_than(n);
    if leaf_index < k {
        let mut p = inclusion_path(&leaves[..k], leaf_index)?;
        p.push(merkle_tree_hash(&leaves[k..]));
        Some(p)
    } else {
        let mut p = inclusion_path(&leaves[k..], leaf_index - k)?;
        p.push(merkle_tree_hash(&leaves[..k]));
        Some(p)
    }
}

pub fn make_inclusion_proof(leaves: &[NodeHash], leaf_index: usize) -> Option<InclusionProof> {
    let path = inclusion_path(leaves, leaf_index)?;
    Some(InclusionProof {
        leaf_index: leaf_index as u64,
        tree_size: leaves.len() as u64,
        leaf_hash: leaves[leaf_index],
        path,
    })
}

pub fn verify_inclusion_proof(proof: &InclusionProof, expected_root: &NodeHash) -> bool {
    let tree_size = proof.tree_size as usize;
    let leaf_index = proof.leaf_index as usize;
    if tree_size == 0 || leaf_index >= tree_size {
        return false;
    }

    let mut r = proof.leaf_hash;
    let mut fn_idx = leaf_index;
    let mut sn = tree_size - 1;

    for p in &proof.path {
        if sn == 0 {
            return false;
        }
        if (fn_idx & 1) == 1 || fn_idx == sn {
            r = node_hash(p, &r);
            while fn_idx != 0 && (fn_idx & 1) == 0 {
                fn_idx >>= 1;
                sn >>= 1;
            }
        } else {
            r = node_hash(&r, p);
        }
        fn_idx >>= 1;
        sn >>= 1;
    }

    &r == expected_root && sn == 0
}

fn subproof(leaves: &[NodeHash], m: usize, b: bool) -> Vec<NodeHash> {
    let n = leaves.len();
    if m == n {
        if b {
            return vec![];
        }
        return vec![merkle_tree_hash(leaves)];
    }

    let k = largest_power_of_two_less_than(n);
    if m <= k {
        let mut p = subproof(&leaves[..k], m, b);
        p.push(merkle_tree_hash(&leaves[k..]));
        p
    } else {
        let mut p = subproof(&leaves[k..], m - k, false);
        p.push(merkle_tree_hash(&leaves[..k]));
        p
    }
}

pub fn consistency_path(leaves: &[NodeHash], old_size: usize) -> Option<Vec<NodeHash>> {
    let n = leaves.len();
    if old_size == 0 || old_size > n {
        return None;
    }
    if old_size == n {
        return Some(vec![]);
    }
    Some(subproof(leaves, old_size, true))
}

pub fn make_consistency_proof(leaves: &[NodeHash], old_size: usize) -> Option<ConsistencyProof> {
    let path = consistency_path(leaves, old_size)?;
    Some(ConsistencyProof {
        old_size: old_size as u64,
        new_size: leaves.len() as u64,
        path,
    })
}

pub fn verify_consistency_proof(
    proof: &ConsistencyProof,
    old_root: &NodeHash,
    new_root: &NodeHash,
) -> bool {
    let old_size = proof.old_size as usize;
    let new_size = proof.new_size as usize;

    if old_size == 0 || new_size == 0 || old_size > new_size {
        return false;
    }
    if old_size == new_size {
        return old_root == new_root && proof.path.is_empty();
    }

    let mut consistency_path: Vec<NodeHash> = Vec::with_capacity(proof.path.len() + 1);
    if old_size.is_power_of_two() {
        consistency_path.push(*old_root);
    }
    consistency_path.extend_from_slice(&proof.path);
    if consistency_path.is_empty() {
        return false;
    }

    let mut fn_idx = old_size - 1;
    let mut sn = new_size - 1;
    if (fn_idx & 1) == 1 {
        while (fn_idx & 1) == 1 {
            fn_idx >>= 1;
            sn >>= 1;
        }
    }

    let mut fr = consistency_path[0];
    let mut sr = consistency_path[0];

    for c in &consistency_path[1..] {
        if sn == 0 {
            return false;
        }
        if (fn_idx & 1) == 1 || fn_idx == sn {
            fr = node_hash(c, &fr);
            sr = node_hash(c, &sr);
            if (fn_idx & 1) == 0 {
                while fn_idx != 0 && (fn_idx & 1) == 0 {
                    fn_idx >>= 1;
                    sn >>= 1;
                }
            }
        } else {
            sr = node_hash(&sr, c);
        }
        fn_idx >>= 1;
        sn >>= 1;
    }

    &fr == old_root && &sr == new_root && sn == 0
}

pub fn sth_signature(tree_size: u64, root_hash: &NodeHash, timestamp_unix_secs: u64) -> String {
    let msg = format!(
        "nucleusdb.ct.sth.sig.v1|size={tree_size}|root={}|ts={timestamp_unix_secs}",
        hex_encode(root_hash)
    );
    hex_encode(&sha256(msg.as_bytes()))
}

pub fn make_sth(leaves: &[NodeHash], timestamp_unix_secs: u64) -> SignedTreeHead {
    let root_hash = merkle_tree_hash(leaves);
    let tree_size = leaves.len() as u64;
    SignedTreeHead {
        tree_size,
        root_hash,
        timestamp_unix_secs,
        sig: sth_signature(tree_size, &root_hash, timestamp_unix_secs),
    }
}

pub fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Formal provenance for the RFC6962 transparency layer.
pub fn formal_provenance() -> Vec<FormalProvenance> {
    vec![
        (
            "verifyInclusionProof_sound",
            "HeytingLean.NucleusDB.Transparency.CT6962.verifyInclusionProof_sound",
            Some("HeytingLean.NucleusDB.Transparency.RFC6962.verifyInclusionProof_sound"),
        ),
        (
            "verifyAppendOnlyConsistencyProof_implies_size_extension",
            "HeytingLean.NucleusDB.Transparency.CT6962.verifyAppendOnlyConsistencyProof_implies_size_extension",
            Some(
                "HeytingLean.NucleusDB.Transparency.RFC6962.verifyAppendOnlyConsistencyProof_implies_size_extension",
            ),
        ),
        (
            "leafChainRoot_injective",
            "HeytingLean.NucleusDB.Transparency.CT6962.leafChainRoot_injective",
            Some("HeytingLean.NucleusDB.Transparency.RFC6962.leafChainRoot_injective"),
        ),
        (
            "consistency_implies_prefix",
            "HeytingLean.NucleusDB.Transparency.CT6962.consistency_implies_prefix",
            Some("HeytingLean.NucleusDB.Transparency.RFC6962.consistency_implies_prefix"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_leaves(n: usize) -> Vec<NodeHash> {
        (0..n)
            .map(|i| leaf_hash(format!("leaf-{i}").as_bytes()))
            .collect()
    }

    #[test]
    fn inclusion_roundtrip_small_tree() {
        let leaves = sample_leaves(10);
        let root = merkle_tree_hash(&leaves);

        for idx in 0..leaves.len() {
            let proof = make_inclusion_proof(&leaves, idx).expect("proof");
            assert!(verify_inclusion_proof(&proof, &root));
        }
    }

    #[test]
    fn inclusion_tamper_fails() {
        let leaves = sample_leaves(8);
        let root = merkle_tree_hash(&leaves);
        let mut proof = make_inclusion_proof(&leaves, 3).expect("proof");
        proof.path[0][0] ^= 0x01;
        assert!(!verify_inclusion_proof(&proof, &root));
    }

    #[test]
    fn consistency_roundtrip_small_tree() {
        let leaves = sample_leaves(12);
        let new_root = merkle_tree_hash(&leaves);
        for old_size in 1..leaves.len() {
            let proof = make_consistency_proof(&leaves, old_size).expect("consistency proof");
            let old_root = merkle_tree_hash(&leaves[..old_size]);
            assert!(
                verify_consistency_proof(&proof, &old_root, &new_root),
                "consistency verify failed for old_size={} new_size={}",
                old_size,
                leaves.len()
            );
        }
    }

    #[test]
    fn consistency_tamper_fails() {
        let leaves = sample_leaves(9);
        let old_size = 4;
        let old_root = merkle_tree_hash(&leaves[..old_size]);
        let new_root = merkle_tree_hash(&leaves);
        let mut proof = make_consistency_proof(&leaves, old_size).expect("proof");
        proof.path[0][1] ^= 0x01;
        assert!(!verify_consistency_proof(&proof, &old_root, &new_root));
    }
}
