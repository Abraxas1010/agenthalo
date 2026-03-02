use sha2::{Digest, Sha256};

const ALGO_DOMAIN: &[u8] = b"agenthalo.algorithm_compliance.v1";

/// Algorithm-compliance guest logic:
/// supports `sha256` and verifies the expected output commitment.
pub fn execute(
    algorithm_id: &str,
    input: &[u8],
    expected_output: &[u8],
) -> Result<Vec<u8>, String> {
    let normalized = algorithm_id.trim().to_ascii_lowercase();
    if normalized != "sha256" {
        return Err(format!(
            "unsupported algorithm `{algorithm_id}` (expected `sha256`)"
        ));
    }

    let computed = Sha256::digest(input);
    if computed.as_slice() != expected_output {
        return Err("algorithm compliance check failed: output mismatch".to_string());
    }

    let mut hasher = Sha256::new();
    hasher.update(ALGO_DOMAIN);
    hasher.update(computed);
    Ok(hasher.finalize().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_accepts_valid_sha256_output() {
        let expected = Sha256::digest(b"hello");
        let journal = execute("sha256", b"hello", expected.as_slice()).expect("compliance");
        assert_eq!(journal.len(), 32);
    }

    #[test]
    fn execute_rejects_output_mismatch() {
        let err = execute("sha256", b"hello", &[0u8; 32]).expect_err("mismatch");
        assert!(err.contains("output mismatch"));
    }

    #[test]
    fn execute_rejects_unknown_algorithm() {
        let err = execute("blake3", b"hello", &[0u8; 32]).expect_err("unsupported");
        assert!(err.contains("unsupported algorithm"));
    }
}
