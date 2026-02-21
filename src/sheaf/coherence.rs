use std::collections::{BTreeMap, BTreeSet};

use crate::state::State;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalSection {
    pub lens_id: String,
    pub kv: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheafCoherenceProof {
    pub coherent: bool,
    pub conflicts: Vec<String>,
    pub digest: u64,
}

pub fn build_sheaf_coherence(local_views: &[LocalSection]) -> SheafCoherenceProof {
    let mut seen: BTreeMap<String, u64> = BTreeMap::new();
    let mut conflicts: BTreeSet<String> = BTreeSet::new();

    for view in local_views {
        for (k, v) in &view.kv {
            match seen.get(k) {
                Some(existing) if existing != v => {
                    conflicts.insert(k.clone());
                }
                None => {
                    seen.insert(k.clone(), *v);
                }
                _ => {}
            }
        }
    }

    let coherent = conflicts.is_empty();
    let mut digest = 0u64;
    for (i, key) in seen.keys().enumerate() {
        digest ^= (key.len() as u64).wrapping_mul((i as u64) + 11);
    }
    for (i, key) in conflicts.iter().enumerate() {
        digest = digest.wrapping_add((key.len() as u64).wrapping_mul((i as u64) + 101));
    }

    SheafCoherenceProof {
        coherent,
        conflicts: conflicts.into_iter().collect(),
        digest,
    }
}

pub fn verify_sheaf_coherence(_global: &State, pf: &SheafCoherenceProof) -> bool {
    // P1.2: extend this verifier to check local sections against global state
    // using an explicit key->index model.
    pf.coherent && pf.conflicts.is_empty()
}
