use sha2::{Digest, Sha256};

const RANGE_PROOF_DOMAIN: &[u8] = b"agenthalo.range_proof.v1";

/// Range proof guest logic:
/// verifies `min <= value <= max` and returns a deterministic commitment.
pub fn execute(value: u64, min: u64, max: u64) -> Result<Vec<u8>, String> {
    if min > max {
        return Err(format!("invalid range: min={min} > max={max}"));
    }
    if value < min || value > max {
        return Err(format!("value {value} outside range [{min}, {max}]"));
    }

    let mut hasher = Sha256::new();
    hasher.update(RANGE_PROOF_DOMAIN);
    hasher.update(value.to_le_bytes());
    hasher.update(min.to_le_bytes());
    hasher.update(max.to_le_bytes());
    Ok(hasher.finalize().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_accepts_valid_range() {
        let journal = execute(42, 1, 100).expect("range should verify");
        assert_eq!(journal.len(), 32);
    }

    #[test]
    fn execute_rejects_out_of_range_value() {
        let err = execute(101, 1, 100).expect_err("range should fail");
        assert!(err.contains("outside range"));
    }

    #[test]
    fn execute_is_deterministic() {
        let a = execute(7, 0, 10).expect("a");
        let b = execute(7, 0, 10).expect("b");
        assert_eq!(a, b);
    }
}
