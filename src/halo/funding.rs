//! Funding gateway — all customer balance top-ups route through AgentPMT tokens
//! or x402direct (USDC on Base).
//!
//! This module is structurally load-bearing: the metered proxy (`proxy.rs`),
//! customer API key store (`api_keys.rs`), and the dashboard billing UI all
//! depend on `validate_funding_source` to gate balance additions.
//!
//! ## Supported funding channels
//!
//! 1. **AgentPMT token purchase** — customer buys tokens at AgentPMT.com,
//!    which calls our `/api/admin/keys/{key_id}/fund` endpoint with a signed
//!    receipt.  The receipt HMAC is verified against `AGENTPMT_WEBHOOK_SECRET`
//!    before the balance is credited.
//!
//! 2. **x402direct** — customer sends USDC on Base L2 to the operator's
//!    payment address.  The transaction hash is verified on-chain before
//!    the balance is credited.
//!
//! No other funding channel is accepted.  This ensures all revenue flows
//! through AgentPMT or on-chain, making it infeasible to bypass billing.

use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;
const TRANSFER_TOPIC: &str = "0xddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef";

// ---------------------------------------------------------------------------
// Funding source enum — the ONLY accepted payment channels
// ---------------------------------------------------------------------------

/// Every balance addition must declare its funding source.
/// The system rejects any source not in this enum.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FundingSource {
    /// Token purchase through AgentPMT.com
    AgentpmtTokens {
        /// Signed receipt from AgentPMT payment processor.
        receipt_id: String,
        /// Amount in USD credited.
        amount_usd: f64,
        /// HMAC-SHA256 signature over `receipt_id|amount_usd|key_id`.
        signature: String,
    },
    /// On-chain USDC payment via x402direct protocol.
    X402Direct {
        /// Transaction hash on Base L2.
        transaction_hash: String,
        /// Amount in USDC base units (6 decimals).
        amount_base_units: u64,
        /// Network: "base" or "base-sepolia".
        network: String,
    },
    /// Operator manual credit (admin-only, requires sensitive access).
    /// This exists for operator testing/adjustments but is logged and auditable.
    OperatorCredit {
        /// Reason for the credit.
        reason: String,
    },
}

/// Funding validation result.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingValidation {
    pub valid: bool,
    pub amount_usd: f64,
    pub source_type: String,
    pub receipt_id: Option<String>,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// Validation — the single gate for all balance additions
// ---------------------------------------------------------------------------

/// Validate a funding source before crediting a customer's balance.
///
/// This is called by the admin balance endpoint and by the AgentPMT webhook.
/// It is the ONLY path to add funds.  Bypassing this function means the
/// balance addition will not be recorded in the funding ledger.
pub fn validate_funding_source(source: &FundingSource, key_id: &str) -> FundingValidation {
    match source {
        FundingSource::AgentpmtTokens {
            receipt_id,
            amount_usd,
            signature,
            ..
        } => {
            if receipt_id.is_empty() {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "agentpmt_tokens".to_string(),
                    receipt_id: None,
                    error: Some("receipt_id is required".to_string()),
                };
            }
            if *amount_usd <= 0.0 {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "agentpmt_tokens".to_string(),
                    receipt_id: Some(receipt_id.clone()),
                    error: Some("amount_usd must be positive".to_string()),
                };
            }
            if signature.is_empty() {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "agentpmt_tokens".to_string(),
                    receipt_id: Some(receipt_id.clone()),
                    error: Some("signature is required for AgentPMT funding".to_string()),
                };
            }
            if let Err(err) = verify_agentpmt_signature(receipt_id, *amount_usd, key_id, signature)
            {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "agentpmt_tokens".to_string(),
                    receipt_id: Some(receipt_id.clone()),
                    error: Some(format!("AgentPMT signature check error: {err}")),
                };
            }
            FundingValidation {
                valid: true,
                amount_usd: *amount_usd,
                source_type: "agentpmt_tokens".to_string(),
                receipt_id: Some(receipt_id.clone()),
                error: None,
            }
        }
        FundingSource::X402Direct {
            transaction_hash,
            amount_base_units,
            network,
            ..
        } => {
            if !crate::halo::x402::is_valid_tx_hash(transaction_hash) {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "x402_direct".to_string(),
                    receipt_id: None,
                    error: Some("invalid transaction hash format".to_string()),
                };
            }
            if *amount_base_units == 0 {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "x402_direct".to_string(),
                    receipt_id: Some(transaction_hash.clone()),
                    error: Some("amount must be > 0".to_string()),
                };
            }
            if network != "base" && network != "base-sepolia" {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "x402_direct".to_string(),
                    receipt_id: Some(transaction_hash.clone()),
                    error: Some(format!("unsupported network: {network}")),
                };
            }
            if let Err(err) = verify_x402_transaction(transaction_hash, *amount_base_units, network)
            {
                if !crate::halo::onchain::onchain_simulation_enabled() {
                    return FundingValidation {
                        valid: false,
                        amount_usd: 0.0,
                        source_type: "x402_direct".to_string(),
                        receipt_id: Some(transaction_hash.clone()),
                        error: Some(format!("on-chain verification failed: {err}")),
                    };
                }
                eprintln!(
                    "[WARN] x402 transaction verification skipped (onchain simulation mode): {err}"
                );
            }

            // Convert USDC base units (6 decimals) to USD.
            let amount_usd = *amount_base_units as f64 / 1_000_000.0;
            FundingValidation {
                valid: true,
                amount_usd,
                source_type: "x402_direct".to_string(),
                receipt_id: Some(transaction_hash.clone()),
                error: None,
            }
        }
        FundingSource::OperatorCredit { reason } => {
            if reason.is_empty() {
                return FundingValidation {
                    valid: false,
                    amount_usd: 0.0,
                    source_type: "operator_credit".to_string(),
                    receipt_id: None,
                    error: Some("reason is required for operator credits".to_string()),
                };
            }
            // Operator credits are always valid (admin-only endpoint checks auth).
            // Amount comes from the separate amount_usd field on the API request.
            FundingValidation {
                valid: true,
                amount_usd: 0.0, // Caller sets the amount.
                source_type: "operator_credit".to_string(),
                receipt_id: None,
                error: None,
            }
        }
    }
}

fn agentpmt_signature_message(receipt_id: &str, amount_usd: f64, key_id: &str) -> String {
    format!("{}|{:.6}|{}", receipt_id, amount_usd, key_id)
}

pub fn verify_agentpmt_signature(
    receipt_id: &str,
    amount_usd: f64,
    key_id: &str,
    signature_hex: &str,
) -> Result<(), String> {
    let secret = std::env::var("AGENTPMT_WEBHOOK_SECRET")
        .map_err(|_| "AGENTPMT_WEBHOOK_SECRET not configured".to_string())?;
    let message = agentpmt_signature_message(receipt_id, amount_usd, key_id);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC init failed: {e}"))?;
    mac.update(message.as_bytes());
    let sig_bytes = crate::halo::util::hex_decode(signature_hex)?;
    mac.verify_slice(&sig_bytes)
        .map_err(|_| "HMAC signature verification failed".to_string())
}

fn webhook_message(key_id: &str, source: &FundingSource, amount_usd: Option<f64>) -> String {
    match source {
        FundingSource::AgentpmtTokens {
            receipt_id,
            amount_usd,
            signature,
        } => format!(
            "fund|{}|agentpmt_tokens|{}|{:.6}|{}",
            key_id, receipt_id, amount_usd, signature
        ),
        FundingSource::X402Direct {
            transaction_hash,
            amount_base_units,
            network,
        } => format!(
            "fund|{}|x402_direct|{}|{}|{}",
            key_id, transaction_hash, amount_base_units, network
        ),
        FundingSource::OperatorCredit { reason } => format!(
            "fund|{}|operator_credit|{:.6}|{}",
            key_id,
            amount_usd.unwrap_or(0.0),
            reason
        ),
    }
}

pub fn verify_webhook_signature(
    key_id: &str,
    source: &FundingSource,
    amount_usd: Option<f64>,
    signature_header: &str,
) -> Result<(), String> {
    let secret = std::env::var("AGENTPMT_WEBHOOK_SECRET").map_err(|_| {
        "AGENTPMT_WEBHOOK_SECRET not configured; webhook verification disabled".to_string()
    })?;
    let message = webhook_message(key_id, source, amount_usd);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| format!("HMAC init failed: {e}"))?;
    mac.update(message.as_bytes());
    let sig_bytes = crate::halo::util::hex_decode(signature_header)?;
    mac.verify_slice(&sig_bytes)
        .map_err(|_| "webhook signature verification failed".to_string())
}

fn verify_x402_transaction(
    transaction_hash: &str,
    expected_amount_base_units: u64,
    network: &str,
) -> Result<(), String> {
    let rpc_url = match network {
        "base" => {
            std::env::var("BASE_RPC_URL").unwrap_or_else(|_| "https://mainnet.base.org".to_string())
        }
        "base-sepolia" => std::env::var("BASE_SEPOLIA_RPC_URL")
            .unwrap_or_else(|_| "https://sepolia.base.org".to_string()),
        other => return Err(format!("unsupported network: {other}")),
    };

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_getTransactionReceipt",
        "params": [transaction_hash]
    });
    let response = crate::halo::http_client::post(&rpc_url)?
        .content_type("application/json")
        .send_json(payload)
        .map_err(|e| format!("x402 RPC verification failed: {e}"))?;
    let body: serde_json::Value = response
        .into_body()
        .read_json()
        .map_err(|e| format!("x402 RPC parse failed: {e}"))?;
    let receipt = body
        .get("result")
        .ok_or_else(|| format!("eth_getTransactionReceipt missing result: {body}"))?;
    if receipt.is_null() {
        return Err("transaction not found (not yet mined or invalid hash)".to_string());
    }

    let status = receipt
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("0x0");
    if status != "0x1" {
        return Err("transaction reverted on-chain".to_string());
    }

    let logs = receipt
        .get("logs")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "transaction receipt missing logs array".to_string())?;
    let expected_amount = BigUint::from(expected_amount_base_units);
    for log in logs {
        let topic0 = log
            .get("topics")
            .and_then(|v| v.as_array())
            .and_then(|topics| topics.first())
            .and_then(|v| v.as_str());
        if topic0.map(|t| t.eq_ignore_ascii_case(TRANSFER_TOPIC)) != Some(true) {
            continue;
        }
        let data = log.get("data").and_then(|v| v.as_str()).unwrap_or("");
        let Some(amount) = parse_evm_uint256_hex(data) else {
            continue;
        };
        if amount >= expected_amount {
            return Ok(());
        }
    }

    Err(format!(
        "USDC transfer of >= {} base units not found in transaction logs",
        expected_amount_base_units
    ))
}

fn parse_evm_uint256_hex(data: &str) -> Option<BigUint> {
    let hex = data.strip_prefix("0x").unwrap_or(data);
    if hex.is_empty() {
        return None;
    }
    let trimmed = hex.trim_start_matches('0');
    if trimmed.is_empty() {
        return Some(BigUint::from(0u8));
    }
    BigUint::parse_bytes(trimmed.as_bytes(), 16)
}

/// Check whether a funding source is an accepted customer-facing channel.
/// Operator credits are NOT customer-facing (admin-only).
pub fn is_customer_funding_source(source: &FundingSource) -> bool {
    matches!(
        source,
        FundingSource::AgentpmtTokens { .. } | FundingSource::X402Direct { .. }
    )
}

// ---------------------------------------------------------------------------
// Funding ledger — append-only record of all balance changes
// ---------------------------------------------------------------------------

/// A single funding ledger entry.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FundingLedgerEntry {
    pub key_id: String,
    pub source: FundingSource,
    pub amount_usd: f64,
    pub balance_after: f64,
    pub timestamp: u64,
}

/// Append a funding event to the ledger file.
pub fn record_funding(entry: &FundingLedgerEntry) -> Result<(), String> {
    let path = crate::halo::config::halo_dir().join("funding_ledger.jsonl");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create funding ledger dir: {e}"))?;
    }
    let line = serde_json::to_string(entry).map_err(|e| format!("serialize funding entry: {e}"))?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open funding ledger {}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| format!("write funding ledger: {e}"))
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Create a ledger entry from a validated funding event.
pub fn create_ledger_entry(
    key_id: &str,
    source: FundingSource,
    amount_usd: f64,
    balance_after: f64,
) -> FundingLedgerEntry {
    FundingLedgerEntry {
        key_id: key_id.to_string(),
        source,
        amount_usd,
        balance_after,
        timestamp: now_unix(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        let mutex = env_lock();
        let guard = mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        mutex.clear_poison();
        guard
    }

    struct EnvVarGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var(key).ok();
            match value {
                Some(v) => std::env::set_var(key, v),
                None => std::env::remove_var(key),
            }
            Self { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(v) = &self.prev {
                std::env::set_var(self.key, v);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    fn hmac_hex(secret: &str, message: &str) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac init for test message");
        mac.update(message.as_bytes());
        crate::halo::util::hex_encode(&mac.finalize().into_bytes())
    }

    #[test]
    fn agentpmt_funding_valid() {
        let _guard = lock_env();
        let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("test-secret"));
        let sig = hmac_hex(
            "test-secret",
            &agentpmt_signature_message("rcpt_123", 50.0, "cust_abc"),
        );
        let source = FundingSource::AgentpmtTokens {
            receipt_id: "rcpt_123".to_string(),
            amount_usd: 50.0,
            signature: sig,
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(result.valid);
        assert_eq!(result.amount_usd, 50.0);
        assert_eq!(result.source_type, "agentpmt_tokens");
    }

    #[test]
    fn agentpmt_funding_rejects_empty_receipt() {
        let source = FundingSource::AgentpmtTokens {
            receipt_id: "".to_string(),
            amount_usd: 50.0,
            signature: "sig".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
    }

    #[test]
    fn agentpmt_funding_rejects_missing_signature() {
        let _guard = lock_env();
        let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("test-secret"));
        let source = FundingSource::AgentpmtTokens {
            receipt_id: "rcpt_123".to_string(),
            amount_usd: 50.0,
            signature: "".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
    }

    #[test]
    fn x402_funding_valid() {
        let _guard = lock_env();
        let _simulation_guard = EnvVarGuard::set("AGENTHALO_ONCHAIN_SIMULATION", Some("1"));
        let source = FundingSource::X402Direct {
            transaction_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .to_string(),
            amount_base_units: 10_000_000,
            network: "base".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(result.valid);
        assert!((result.amount_usd - 10.0).abs() < 0.001);
    }

    #[test]
    fn x402_funding_rejects_invalid_hash() {
        let _guard = lock_env();
        let source = FundingSource::X402Direct {
            transaction_hash: "0xinvalid".to_string(),
            amount_base_units: 1_000_000,
            network: "base".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
    }

    #[test]
    fn x402_funding_rejects_unknown_network() {
        let _guard = lock_env();
        let source = FundingSource::X402Direct {
            transaction_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .to_string(),
            amount_base_units: 1_000_000,
            network: "polygon".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
    }

    #[test]
    fn operator_credit_valid() {
        let _guard = lock_env();
        let source = FundingSource::OperatorCredit {
            reason: "testing".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(result.valid);
        assert!(!is_customer_funding_source(&source));
    }

    #[test]
    fn operator_credit_rejects_empty_reason() {
        let _guard = lock_env();
        let source = FundingSource::OperatorCredit {
            reason: "".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
    }

    #[test]
    fn agentpmt_funding_rejects_invalid_signature() {
        let _guard = lock_env();
        let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("test-secret"));
        let source = FundingSource::AgentpmtTokens {
            receipt_id: "rcpt_123".to_string(),
            amount_usd: 50.0,
            signature: "deadbeef".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
        assert!(result
            .error
            .unwrap_or_default()
            .contains("signature check error"));
    }

    #[test]
    fn agentpmt_funding_rejects_when_secret_missing() {
        let _guard = lock_env();
        let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", None);
        let source = FundingSource::AgentpmtTokens {
            receipt_id: "rcpt_123".to_string(),
            amount_usd: 50.0,
            signature: "deadbeef".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
        assert!(result
            .error
            .unwrap_or_default()
            .contains("AGENTPMT_WEBHOOK_SECRET not configured"));
    }

    #[test]
    fn webhook_signature_verifies() {
        let _guard = lock_env();
        let _secret = EnvVarGuard::set("AGENTPMT_WEBHOOK_SECRET", Some("test-secret"));
        let source = FundingSource::X402Direct {
            transaction_hash: "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                .to_string(),
            amount_base_units: 1_000_000,
            network: "base".to_string(),
        };
        let msg = webhook_message("key_1", &source, None);
        let sig = hmac_hex("test-secret", &msg);
        assert!(verify_webhook_signature("key_1", &source, None, &sig).is_ok());
    }

    #[test]
    fn customer_funding_channels() {
        assert!(is_customer_funding_source(&FundingSource::AgentpmtTokens {
            receipt_id: "x".to_string(),
            amount_usd: 1.0,
            signature: "s".to_string(),
        }));
        assert!(is_customer_funding_source(&FundingSource::X402Direct {
            transaction_hash: "0x".to_string(),
            amount_base_units: 1,
            network: "base".to_string(),
        }));
        assert!(!is_customer_funding_source(
            &FundingSource::OperatorCredit {
                reason: "test".to_string(),
            }
        ));
    }

    #[test]
    fn funding_ledger_entry_serializes() {
        let entry = create_ledger_entry(
            "cust_123",
            FundingSource::AgentpmtTokens {
                receipt_id: "rcpt_456".to_string(),
                amount_usd: 25.0,
                signature: "sig".to_string(),
            },
            25.0,
            75.0,
        );
        let json = serde_json::to_string(&entry).unwrap();
        let loaded: FundingLedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.key_id, "cust_123");
        assert_eq!(loaded.amount_usd, 25.0);
        assert_eq!(loaded.balance_after, 75.0);
    }

    #[test]
    fn parse_uint256_accepts_zero_padded_64_char_log_data() {
        let data = "0x0000000000000000000000000000000000000000000000000000000000989680";
        let parsed = parse_evm_uint256_hex(data).expect("parse padded uint256");
        assert_eq!(parsed, BigUint::from(10_000_000u64));
    }

    #[test]
    fn parse_uint256_supports_values_larger_than_u128() {
        let data = "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        let parsed = parse_evm_uint256_hex(data).expect("parse uint256 max");
        assert!(parsed > BigUint::from(u128::MAX));
    }
}
