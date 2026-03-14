use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CreditTransaction {
    pub transaction_id: String,
    pub user_did: String,
    pub amount: f64,
    pub reason: String,
    pub timestamp_unix: i64,
}

#[derive(Debug)]
pub enum CreditError {
    Io(String),
    Serde(String),
}

impl std::fmt::Display for CreditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) | Self::Serde(err) => f.write_str(err),
        }
    }
}

impl std::error::Error for CreditError {}

fn credits_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_unix_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn compute_lambda_cost(source_bytes: usize, dep_count: usize) -> f64 {
    let source_kb = source_bytes as f64 / 1024.0;
    let raw = 5.0 + 0.5 * source_kb + dep_count as f64;
    raw.clamp(1.0, 100.0)
}

pub fn compile_credit_award(source_bytes: usize, dep_count: usize) -> f64 {
    compute_lambda_cost(source_bytes, dep_count) / 10.0
}

pub fn net_cost(download_cost: f64, earned_credits: f64) -> f64 {
    (download_cost - earned_credits).max(0.0)
}

pub fn award_compile_credits(
    user_did: &str,
    task_id: &str,
    source_bytes: usize,
    dep_count: usize,
) -> Result<CreditTransaction, CreditError> {
    let txn_id = format!("compile-award-{task_id}");
    if let Some(existing) = get_transaction(&txn_id)? {
        return Ok(existing);
    }
    let txn = CreditTransaction {
        transaction_id: txn_id,
        user_did: user_did.to_string(),
        amount: compile_credit_award(source_bytes, dep_count),
        reason: format!("HeytingLean compile: {task_id}"),
        timestamp_unix: now_unix_secs(),
    };
    let _guard = credits_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut txns = load_transactions_map()?;
    let existing = txns
        .entry(txn.transaction_id.clone())
        .or_insert_with(|| txn.clone())
        .clone();
    persist_transactions_map(&txns)?;
    Ok(existing)
}

pub fn award_compile_credits_for_result(
    user_did: &str,
    task_id: &str,
    source_bytes: usize,
    dep_count: usize,
    status: &str,
) -> Result<Option<CreditTransaction>, CreditError> {
    if status != "success" {
        return Ok(None);
    }
    award_compile_credits(user_did, task_id, source_bytes, dep_count).map(Some)
}

pub fn get_transaction(txn_id: &str) -> Result<Option<CreditTransaction>, CreditError> {
    let _guard = credits_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let txns = load_transactions_map()?;
    Ok(txns.get(txn_id).cloned())
}

fn load_transactions_map() -> Result<BTreeMap<String, CreditTransaction>, CreditError> {
    let path = crate::halo::config::lambda_credits_path();
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| CreditError::Io(format!("read {}: {e}", path.display())))?;
    serde_json::from_str(&raw)
        .map_err(|e| CreditError::Serde(format!("parse {}: {e}", path.display())))
}

fn persist_transactions_map(txns: &BTreeMap<String, CreditTransaction>) -> Result<(), CreditError> {
    let path = crate::halo::config::lambda_credits_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CreditError::Io(format!("create {}: {e}", parent.display())))?;
    }
    let raw = serde_json::to_string_pretty(txns)
        .map_err(|e| CreditError::Serde(format!("encode lambda credits: {e}")))?;
    std::fs::write(&path, raw)
        .map_err(|e| CreditError::Io(format!("write {}: {e}", path.display())))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};

    #[test]
    fn test_lambda_credits_formula() {
        assert!((compile_credit_award(1024, 0) - 0.55).abs() < 1e-9);
        assert!((compile_credit_award(102400, 10) - 6.5).abs() < 1e-9);
        assert_eq!(net_cost(5.0, 7.0), 0.0);
    }

    #[test]
    fn test_credit_idempotency() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
        let first = award_compile_credits("did:test", "task-1", 1024, 1).expect("first award");
        let second = award_compile_credits("did:test", "task-1", 1024, 1).expect("second award");
        assert_eq!(first, second);
    }

    #[test]
    fn test_no_credit_on_failed_compile() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
        let awarded =
            award_compile_credits_for_result("did:test", "task-2", 1024, 1, "build_failed")
                .expect("award call");
        assert!(awarded.is_none());
        assert!(get_transaction("compile-award-task-2")
            .expect("txn lookup")
            .is_none());
    }
}
