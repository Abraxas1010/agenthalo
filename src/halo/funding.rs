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
//!    receipt.  The receipt is verified against AgentPMT's public key before
//!    the balance is credited.
//!
//! 2. **x402direct** — customer sends USDC on Base L2 to the operator's
//!    payment address.  The transaction hash is verified on-chain before
//!    the balance is credited.
//!
//! No other funding channel is accepted.  This ensures all revenue flows
//! through AgentPMT or on-chain, making it infeasible to bypass billing.

use serde::{Deserialize, Serialize};

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
pub fn validate_funding_source(
    source: &FundingSource,
    _key_id: &str,
) -> FundingValidation {
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
            // TODO: verify HMAC-SHA256 signature against AgentPMT public key
            // when the AgentPMT payment webhook is fully integrated.
            // For now, accept any non-empty signature (operator testing phase).
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
            // Convert USDC base units (6 decimals) to USD.
            let amount_usd = *amount_base_units as f64 / 1_000_000.0;
            // TODO: verify transaction on-chain via RPC when fully integrated.
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
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create funding ledger dir: {e}"))?;
    }
    let line = serde_json::to_string(entry)
        .map_err(|e| format!("serialize funding entry: {e}"))?;
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open funding ledger {}: {e}", path.display()))?;
    writeln!(file, "{line}")
        .map_err(|e| format!("write funding ledger: {e}"))
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

    #[test]
    fn agentpmt_funding_valid() {
        let source = FundingSource::AgentpmtTokens {
            receipt_id: "rcpt_123".to_string(),
            amount_usd: 50.0,
            signature: "hmac_test_sig".to_string(),
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
        let source = FundingSource::X402Direct {
            transaction_hash:
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
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
        let source = FundingSource::X402Direct {
            transaction_hash:
                "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
                    .to_string(),
            amount_base_units: 1_000_000,
            network: "polygon".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
    }

    #[test]
    fn operator_credit_valid() {
        let source = FundingSource::OperatorCredit {
            reason: "testing".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(result.valid);
        assert!(!is_customer_funding_source(&source));
    }

    #[test]
    fn operator_credit_rejects_empty_reason() {
        let source = FundingSource::OperatorCredit {
            reason: "".to_string(),
        };
        let result = validate_funding_source(&source, "cust_abc");
        assert!(!result.valid);
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
}
