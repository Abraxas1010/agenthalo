//! AETHER Betti heuristic for binary topology fingerprints.
//!
//! Provenance: `artifacts/aether_verified/rust/aether_betti.rs`
//! Formal basis: `HeytingLean.Bridge.Sharma.AetherBetti.betti_error_bound`

use serde::{Deserialize, Serialize};

fn nat_dist_u8(a: u8, b: u8) -> usize {
    a.abs_diff(b) as usize
}

/// VERIFIED CORE (direct AETHER port).
pub fn detected_loop_at(data: &[u8], i: usize, tol: usize) -> bool {
    if i + 3 >= data.len() {
        return false;
    }
    let a = data[i];
    let b = data[i + 1];
    let c = data[i + 2];
    let d = data[i + 3];
    let close = nat_dist_u8(a, d) <= tol;
    let middle_far = nat_dist_u8(a, b) > tol || nat_dist_u8(a, c) > tol;
    close && middle_far
}

/// VERIFIED CORE (direct AETHER port).
pub fn betti1_heuristic(data: &[u8], tol: usize) -> usize {
    (0..data.len())
        .filter(|index| detected_loop_at(data, *index, tol))
        .count()
}

/// VERIFIED CORE (direct AETHER port).
pub fn betti_error_bound_check(data: &[u8], tol: usize, betti1_exact_or_proxy: usize) -> bool {
    let heuristic = betti1_heuristic(data, tol);
    let overlap = data.len().saturating_sub(3);
    heuristic <= betti1_exact_or_proxy + overlap
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TopoSignature {
    pub betti1_heuristic: usize,
    pub data_len: usize,
    pub tolerance: usize,
    pub formal_error_bound: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompareResult {
    pub within_formal_bound: bool,
    pub absolute_difference: usize,
    pub combined_error_bound: usize,
    pub similarity_score: f64,
    pub warning: Option<String>,
}

pub fn fingerprint(binary: &[u8], tolerance: usize) -> TopoSignature {
    TopoSignature {
        betti1_heuristic: betti1_heuristic(binary, tolerance),
        data_len: binary.len(),
        tolerance,
        formal_error_bound: binary.len().saturating_sub(3),
    }
}

pub fn compare(a: &TopoSignature, b: &TopoSignature) -> CompareResult {
    let absolute_difference = a.betti1_heuristic.abs_diff(b.betti1_heuristic);
    let combined_error_bound = a.formal_error_bound + b.formal_error_bound;
    let denominator = (a.betti1_heuristic.max(b.betti1_heuristic) + combined_error_bound).max(1);
    let similarity_score = 1.0 - (absolute_difference as f64 / denominator as f64);
    let warning = if a.data_len.max(b.data_len) > 100 {
        Some(
            "Betti bound is secondary evidence only for large binaries; retain SHA-256 as the primary authenticator."
                .to_string(),
        )
    } else {
        None
    };
    CompareResult {
        within_formal_bound: absolute_difference <= combined_error_bound,
        absolute_difference,
        combined_error_bound,
        similarity_score: similarity_score.clamp(0.0, 1.0),
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ports_betti_bound_check() {
        let data = [1u8, 9, 8, 2, 7, 6, 1, 2];
        assert!(betti_error_bound_check(&data, 3, 5));
    }

    #[test]
    fn fingerprint_is_deterministic() {
        let data = b"hello topology";
        let first = fingerprint(data, 3);
        let second = fingerprint(data, 3);
        assert_eq!(first.betti1_heuristic, second.betti1_heuristic);
        assert_eq!(first.formal_error_bound, second.formal_error_bound);
    }

    #[test]
    fn compare_is_symmetric() {
        let a = fingerprint(b"abcdabcd", 2);
        let b = fingerprint(b"abceabce", 2);
        let ab = compare(&a, &b);
        let ba = compare(&b, &a);
        assert_eq!(ab.absolute_difference, ba.absolute_difference);
        assert_eq!(ab.combined_error_bound, ba.combined_error_bound);
        assert_eq!(ab.within_formal_bound, ba.within_formal_bound);
    }
}
