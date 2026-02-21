use crate::transparency::ct6962::{
    inclusion_path, leaf_hash, merkle_tree_hash, verify_inclusion_proof, InclusionProof, NodeHash,
};
use crate::vc::{FieldElem, RootDigest, VC};

#[derive(Clone, Debug)]
pub struct Commitment {
    pub root: NodeHash,
    pub leaf_count: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proof {
    pub index: usize,
    pub leaf_hash: NodeHash,
    pub path: Vec<NodeHash>,
    pub tree_size: usize,
}

pub struct DemoBinaryMerkle;

fn value_leaf(index: usize, value: FieldElem) -> NodeHash {
    let payload = format!("nucleusdb.vc.binary_merkle.leaf.v1|idx={index}|value={value}");
    leaf_hash(payload.as_bytes())
}

fn value_leaves(v: &[FieldElem]) -> Vec<NodeHash> {
    v.iter()
        .enumerate()
        .map(|(i, val)| value_leaf(i, *val))
        .collect()
}

impl VC for DemoBinaryMerkle {
    type Commitment = Commitment;
    type Proof = Proof;

    fn commit(v: &[FieldElem]) -> Self::Commitment {
        let leaves = value_leaves(v);
        let root = merkle_tree_hash(&leaves);
        Commitment {
            root,
            leaf_count: leaves.len(),
        }
    }

    fn open(v: &[FieldElem], i: usize) -> Self::Proof {
        let leaves = value_leaves(v);
        let leaf_hash = leaves.get(i).copied().unwrap_or_else(|| value_leaf(i, 0));
        let path = inclusion_path(&leaves, i).unwrap_or_default();
        Proof {
            index: i,
            leaf_hash,
            path,
            tree_size: leaves.len(),
        }
    }

    fn verify(c: &Self::Commitment, i: usize, value: &FieldElem, p: &Self::Proof) -> bool {
        if p.index != i || p.tree_size != c.leaf_count {
            return false;
        }
        if p.leaf_hash != value_leaf(i, *value) {
            return false;
        }
        if p.tree_size == 0 {
            return c.root == merkle_tree_hash(&[]);
        }
        let proof = InclusionProof {
            leaf_index: p.index as u64,
            tree_size: p.tree_size as u64,
            leaf_hash: p.leaf_hash,
            path: p.path.clone(),
        };
        verify_inclusion_proof(&proof, &c.root)
    }

    fn digest(c: &Self::Commitment) -> RootDigest {
        c.root
    }
}
