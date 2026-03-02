use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const AGGREGATION_DOMAIN: &[u8] = b"agenthalo.secure_aggregation.v1";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AggregationPolicy {
    Sum,
    Mean,
    Min,
    Max,
}

fn aggregate(values: &[u64], policy: AggregationPolicy) -> Result<u64, String> {
    if values.is_empty() {
        return Err("secure aggregation requires at least one value".to_string());
    }

    let value = match policy {
        AggregationPolicy::Sum => values.iter().copied().sum(),
        AggregationPolicy::Mean => {
            let sum: u64 = values.iter().copied().sum();
            sum / (values.len() as u64)
        }
        AggregationPolicy::Min => *values
            .iter()
            .min()
            .ok_or_else(|| "missing minimum".to_string())?,
        AggregationPolicy::Max => *values
            .iter()
            .max()
            .ok_or_else(|| "missing maximum".to_string())?,
    };
    Ok(value)
}

/// Secure-aggregation guest logic:
/// applies the selected policy and commits the aggregate value.
pub fn execute(values: &[u64], policy: AggregationPolicy) -> Result<Vec<u8>, String> {
    let aggregate = aggregate(values, policy)?;
    let mut hasher = Sha256::new();
    hasher.update(AGGREGATION_DOMAIN);
    hasher.update((values.len() as u64).to_le_bytes());
    hasher.update(aggregate.to_le_bytes());
    hasher.update(format!("{policy:?}").as_bytes());
    Ok(hasher.finalize().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execute_accepts_sum_policy() {
        let journal = execute(&[2, 3, 5], AggregationPolicy::Sum).expect("sum");
        assert_eq!(journal.len(), 32);
    }

    #[test]
    fn execute_accepts_mean_policy() {
        let a = execute(&[2, 4, 8], AggregationPolicy::Mean).expect("mean");
        let b = execute(&[2, 4, 8], AggregationPolicy::Mean).expect("mean repeat");
        assert_eq!(a, b);
    }

    #[test]
    fn execute_rejects_empty_values() {
        let err = execute(&[], AggregationPolicy::Max).expect_err("empty values");
        assert!(err.contains("at least one value"));
    }
}
