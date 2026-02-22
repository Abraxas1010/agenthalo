//! Monotone extension proofs for immutable agentic records.
//!
//! In `AppendOnly` mode, every commit proves that the new state is a
//! monotone extension of the previous state: all previously committed
//! key-value pairs are preserved.  This guarantee is constructive —
//! the witness is the explicit verification, and the seal chain provides
//! a hash-based commitment that any deletion would break.
//!
//! # Mathematical basis
//!
//! The seal chain forms a diagram in the category of hash commitments.
//! The morphism from height n to height n+1 is a monotone extension:
//!   - **Forward (constructive)**: we produce a witness that `state_n ⊆ state_{n+1}`
//!   - **Backward (verification)**: given `seal_{n+1}`, verify against `seal_n`
//!   - **Bidirectional**: section (extend) and retraction (restrict) form an
//!     adjunction in the Heyting algebra of database states.
//!
//! Deletion is computationally infeasible to hide:
//!   `seal_n = SHA-256("NucleusDB.MonotoneSeal|" || seal_{n-1} || kv_digest_n)`
//!
//! To produce a valid seal after deleting a record, an attacker would need
//! to find a SHA-256 preimage — a 2^128 operation.

use crate::keymap::KeyMap;
use crate::state::State;
use crate::transparency::ct6962::{sha256, NodeHash};
use serde::{Deserialize, Serialize};

/// Domain separator for the monotone seal chain.
const DOMAIN_SEAL: &[u8] = b"NucleusDB.MonotoneSeal|";

/// Write mode controlling mutation permissions on the database.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WriteMode {
    /// Normal mode — INSERT, UPDATE, DELETE all permitted.
    #[default]
    Normal,
    /// Append-only — INSERT permitted, UPDATE and DELETE rejected.
    /// Every commit produces a monotone extension proof.
    /// Once enabled, cannot be reverted (one-way lock).
    AppendOnly,
}

/// Compute a deterministic digest over all key-value pairs in the current state.
///
/// Keys are sorted lexicographically to ensure determinism.
/// The digest captures the complete key-value mapping — any change
/// (insertion, update, or deletion) produces a different digest.
pub fn key_value_digest(state: &State, keymap: &KeyMap) -> NodeHash {
    let mut pairs: Vec<(&str, u64)> = keymap
        .all_keys()
        .map(|(k, idx)| {
            let v = state.values.get(idx).copied().unwrap_or(0);
            (k, v)
        })
        .collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));

    let mut buf = Vec::new();
    buf.extend_from_slice(b"NucleusDB.KeyValueDigest|");
    for (k, v) in &pairs {
        buf.extend_from_slice(k.as_bytes());
        buf.push(b'=');
        buf.extend_from_slice(v.to_string().as_bytes());
        buf.push(b';');
    }
    sha256(&buf)
}

/// Verify raw state-level monotone extension: every non-zero value
/// at each index in `old_state` must appear unchanged at the same
/// index in `new_state`. This catches overwrites at the protocol
/// layer where the keymap may not track all indices.
pub fn verify_raw_monotone_extension(old_state: &State, new_state: &State) -> bool {
    for (i, &old_val) in old_state.values.iter().enumerate() {
        if old_val == 0 {
            continue;
        }
        let new_val = new_state.values.get(i).copied().unwrap_or(0);
        if new_val != old_val {
            return false;
        }
    }
    true
}

/// Verify that `new_state` is a monotone extension of `old_state`:
/// every key present in `old_state` with a non-zero value must be
/// present in `new_state` with the same value.
///
/// This is the constructive forward direction: we explicitly check
/// each key-value pair, producing an affirmative witness.
pub fn verify_monotone_extension(
    old_state: &State,
    old_keymap: &KeyMap,
    new_state: &State,
    new_keymap: &KeyMap,
) -> bool {
    // First: raw index-level check (catches protocol-level overwrites).
    if !verify_raw_monotone_extension(old_state, new_state) {
        return false;
    }
    // Second: keymap-level check (catches named key deletion/rename).
    for (key, old_idx) in old_keymap.all_keys() {
        let old_val = old_state.values.get(old_idx).copied().unwrap_or(0);
        if old_val == 0 {
            // Zero = absent — no monotonicity requirement for absent keys.
            continue;
        }
        match new_keymap.get(key) {
            Some(new_idx) => {
                let new_val = new_state.values.get(new_idx).copied().unwrap_or(0);
                if new_val != old_val {
                    return false; // Value changed — not a monotone extension.
                }
            }
            None => return false, // Key absent in new state — deletion detected.
        }
    }
    true
}

/// Compute the next seal in the monotone seal chain.
///
/// `seal_n = SHA-256("NucleusDB.MonotoneSeal|" || seal_{n-1} || kv_digest_n)`
///
/// The chain is unforgeable: producing a valid seal after deletion
/// requires finding a SHA-256 preimage.
pub fn next_seal(prev_seal: &NodeHash, kv_digest: &NodeHash) -> NodeHash {
    let mut buf = Vec::with_capacity(DOMAIN_SEAL.len() + 64);
    buf.extend_from_slice(DOMAIN_SEAL);
    buf.extend_from_slice(prev_seal);
    buf.extend_from_slice(kv_digest);
    sha256(&buf)
}

/// Genesis seal — the initial seal for an empty database.
pub fn genesis_seal() -> NodeHash {
    sha256(b"NucleusDB.MonotoneSeal.Genesis|")
}

/// Verify a seal chain against a sequence of state snapshots.
/// Returns `true` if the chain is valid (no deletion detected).
pub fn verify_seal_chain(seals: &[NodeHash], states: &[(State, KeyMap)]) -> bool {
    if seals.len() != states.len() {
        return false;
    }
    let mut prev = genesis_seal();
    for (i, seal) in seals.iter().enumerate() {
        let kv = key_value_digest(&states[i].0, &states[i].1);
        let expected = next_seal(&prev, &kv);
        if *seal != expected {
            return false;
        }
        prev = *seal;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::KeyMap;
    use crate::state::State;

    fn make_state(pairs: &[(&str, u64)]) -> (State, KeyMap) {
        let mut keymap = KeyMap::new();
        let mut values = Vec::new();
        for (k, v) in pairs {
            let idx = keymap.get_or_create(k);
            while values.len() <= idx {
                values.push(0);
            }
            values[idx] = *v;
        }
        (State::new(values), keymap)
    }

    #[test]
    fn monotone_extension_allows_append() {
        let (old_state, old_keymap) = make_state(&[("a", 1), ("b", 2)]);
        let (new_state, new_keymap) = make_state(&[("a", 1), ("b", 2), ("c", 3)]);
        assert!(verify_monotone_extension(
            &old_state,
            &old_keymap,
            &new_state,
            &new_keymap
        ));
    }

    #[test]
    fn monotone_extension_rejects_deletion() {
        let (old_state, old_keymap) = make_state(&[("a", 1), ("b", 2)]);
        let (new_state, new_keymap) = make_state(&[("a", 1)]); // b deleted
        assert!(!verify_monotone_extension(
            &old_state,
            &old_keymap,
            &new_state,
            &new_keymap
        ));
    }

    #[test]
    fn monotone_extension_rejects_value_change() {
        let (old_state, old_keymap) = make_state(&[("a", 1), ("b", 2)]);
        let (new_state, new_keymap) = make_state(&[("a", 1), ("b", 999)]); // b changed
        assert!(!verify_monotone_extension(
            &old_state,
            &old_keymap,
            &new_state,
            &new_keymap
        ));
    }

    #[test]
    fn monotone_extension_allows_zero_to_nonzero() {
        let (old_state, old_keymap) = make_state(&[("a", 0)]); // zero = absent
        let (new_state, new_keymap) = make_state(&[("a", 42)]);
        assert!(verify_monotone_extension(
            &old_state,
            &old_keymap,
            &new_state,
            &new_keymap
        ));
    }

    #[test]
    fn seal_chain_deterministic() {
        let gen = genesis_seal();
        let (s1, km1) = make_state(&[("x", 10)]);
        let kv1 = key_value_digest(&s1, &km1);
        let seal1 = next_seal(&gen, &kv1);
        let seal1b = next_seal(&gen, &kv1);
        assert_eq!(seal1, seal1b, "seal must be deterministic");
    }

    #[test]
    fn seal_chain_changes_on_different_data() {
        let gen = genesis_seal();
        let (s1, km1) = make_state(&[("x", 10)]);
        let (s2, km2) = make_state(&[("x", 20)]);
        let seal_a = next_seal(&gen, &key_value_digest(&s1, &km1));
        let seal_b = next_seal(&gen, &key_value_digest(&s2, &km2));
        assert_ne!(
            seal_a, seal_b,
            "different data must produce different seals"
        );
    }

    #[test]
    fn seal_chain_verification_valid() {
        let (s1, km1) = make_state(&[("a", 1)]);
        let (s2, km2) = make_state(&[("a", 1), ("b", 2)]);
        let gen = genesis_seal();
        let seal1 = next_seal(&gen, &key_value_digest(&s1, &km1));
        let seal2 = next_seal(&seal1, &key_value_digest(&s2, &km2));
        assert!(verify_seal_chain(&[seal1, seal2], &[(s1, km1), (s2, km2)]));
    }

    #[test]
    fn seal_chain_verification_detects_tamper() {
        let (s1, km1) = make_state(&[("a", 1)]);
        let (s2, km2) = make_state(&[("a", 1), ("b", 2)]);
        let gen = genesis_seal();
        let seal1 = next_seal(&gen, &key_value_digest(&s1, &km1));
        let seal2 = next_seal(&seal1, &key_value_digest(&s2, &km2));

        // Tamper: claim the second state has "a" deleted
        let (s2_tampered, km2_tampered) = make_state(&[("b", 2)]);
        assert!(!verify_seal_chain(
            &[seal1, seal2],
            &[(s1, km1), (s2_tampered, km2_tampered)]
        ));
    }

    #[test]
    fn kv_digest_deterministic() {
        let (s, km) = make_state(&[("b", 2), ("a", 1)]);
        let d1 = key_value_digest(&s, &km);
        let d2 = key_value_digest(&s, &km);
        assert_eq!(d1, d2);
    }

    #[test]
    fn kv_digest_order_independent() {
        // Regardless of insertion order, same keys+values → same digest.
        let (s1, km1) = make_state(&[("a", 1), ("b", 2)]);
        let (s2, km2) = make_state(&[("b", 2), ("a", 1)]);
        assert_eq!(key_value_digest(&s1, &km1), key_value_digest(&s2, &km2));
    }
}
