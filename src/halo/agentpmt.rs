//! AgentPMT credit system client.
//!
//! Phase 0 uses a local stub mode for credit simulation.
//! Set `AGENTHALO_AGENTPMT_STUB=1` to enable local balance/deduction behavior.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const AGENTPMT_API_BASE: &str = "https://www.agentpmt.com/api";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentPmtConfig {
    pub api_key: String,
    pub cached_balance: Option<u64>,
    pub balance_refreshed_at: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreditBalance {
    pub credits: u64,
    pub currency: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeductResult {
    pub success: bool,
    pub remaining_credits: u64,
    pub transaction_id: Option<String>,
    pub error: Option<String>,
}

pub struct AgentPmtClient {
    api_key: String,
    base_url: String,
}

impl AgentPmtClient {
    pub fn new(api_key: String) -> Self {
        let base_url = std::env::var("AGENTHALO_AGENTPMT_URL")
            .unwrap_or_else(|_| AGENTPMT_API_BASE.to_string());
        Self { api_key, base_url }
    }

    pub fn from_config() -> Option<Self> {
        let config = load_agentpmt_config(&agentpmt_config_path()).ok()?;
        if config.api_key.trim().is_empty() {
            return None;
        }
        Some(Self::new(config.api_key))
    }

    pub fn balance(&self) -> Result<CreditBalance, String> {
        if self.api_key.trim().is_empty() {
            return Err("AgentPMT API key is empty".to_string());
        }
        if stub_mode_enabled() {
            let cfg = load_agentpmt_config(&agentpmt_config_path())?;
            let credits = cfg.cached_balance.unwrap_or(10_000);
            return Ok(CreditBalance {
                credits,
                currency: "credits".to_string(),
            });
        }

        Err(format!(
            "AgentPMT API not yet connected at {}. Set AGENTHALO_AGENTPMT_STUB=1 for local stub mode.",
            self.base_url
        ))
    }

    pub fn deduct(&self, product_slug: &str, units: u64) -> Result<DeductResult, String> {
        if self.api_key.trim().is_empty() {
            return Err("AgentPMT API key is empty".to_string());
        }
        let unit_cost = operation_cost(product_slug)
            .ok_or_else(|| format!("unknown AgentPMT product slug: {product_slug}"))?;
        let total = unit_cost.saturating_mul(units);

        if stub_mode_enabled() {
            let path = agentpmt_config_path();
            let mut cfg = load_agentpmt_config(&path)?;
            let balance = cfg.cached_balance.unwrap_or(10_000);
            if balance < total {
                return Ok(DeductResult {
                    success: false,
                    remaining_credits: balance,
                    transaction_id: None,
                    error: Some("insufficient credits".to_string()),
                });
            }

            let remaining = balance - total;
            cfg.cached_balance = Some(remaining);
            cfg.balance_refreshed_at = Some(now_unix_secs());
            save_agentpmt_config(&path, &cfg)?;

            return Ok(DeductResult {
                success: true,
                remaining_credits: remaining,
                transaction_id: Some(format!("stub-{}", uuid::Uuid::new_v4())),
                error: None,
            });
        }

        Err(format!(
            "AgentPMT API not yet connected at {}. Set AGENTHALO_AGENTPMT_STUB=1 for local stub mode.",
            self.base_url
        ))
    }

    pub fn can_afford(&self, credits_needed: u64) -> Result<bool, String> {
        let balance = self.balance()?;
        Ok(balance.credits >= credits_needed)
    }
}

pub fn agentpmt_config_path() -> PathBuf {
    crate::halo::config::halo_dir().join("agentpmt.json")
}

pub fn load_agentpmt_config(path: &Path) -> Result<AgentPmtConfig, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read agentpmt config {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse agentpmt config {}: {e}", path.display()))
}

pub fn save_agentpmt_config(path: &Path, config: &AgentPmtConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create agentpmt config dir: {e}"))?;
    }
    let raw = serde_json::to_string_pretty(config)
        .map_err(|e| format!("serialize agentpmt config: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("write agentpmt config {}: {e}", path.display()))
}

pub fn is_agentpmt_configured() -> bool {
    load_agentpmt_config(&agentpmt_config_path()).is_ok()
}

pub fn operation_cost(operation: &str) -> Option<u64> {
    match operation {
        "attest" => Some(10),
        "attest_anon" | "attest_anonymous" => Some(50),
        "sign_pq" => Some(1),
        "audit_small" => Some(500),
        "audit_medium" => Some(1500),
        "audit_large" => Some(5000),
        "trust_query" => Some(1),
        "vote" => Some(25),
        "sync" => Some(1),
        "license_starter" => Some(900),
        "license_professional" => Some(2900),
        "license_enterprise" => Some(9900),
        _ => None,
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn stub_mode_enabled() -> bool {
    std::env::var("AGENTHALO_AGENTPMT_STUB")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_costs_are_defined() {
        assert_eq!(operation_cost("attest"), Some(10));
        assert_eq!(operation_cost("attest_anon"), Some(50));
        assert_eq!(operation_cost("sign_pq"), Some(1));
        assert_eq!(operation_cost("audit_small"), Some(500));
        assert_eq!(operation_cost("audit_medium"), Some(1500));
        assert_eq!(operation_cost("audit_large"), Some(5000));
        assert_eq!(operation_cost("trust_query"), Some(1));
        assert_eq!(operation_cost("vote"), Some(25));
        assert_eq!(operation_cost("sync"), Some(1));
        assert_eq!(operation_cost("unknown"), None);
    }

    #[test]
    fn config_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_test_agentpmt_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("agentpmt.json");

        let config = AgentPmtConfig {
            api_key: "test-key-123".to_string(),
            cached_balance: Some(500),
            balance_refreshed_at: Some(1700000000),
        };
        save_agentpmt_config(&path, &config).expect("save config");
        let loaded = load_agentpmt_config(&path).expect("load config");
        assert_eq!(loaded.api_key, "test-key-123");
        assert_eq!(loaded.cached_balance, Some(500));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
