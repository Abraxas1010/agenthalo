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
    #[serde(default)]
    pub history: Vec<CreditHistoryEntry>,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreditHistoryEntry {
    pub timestamp: u64,
    pub product_slug: String,
    pub units: u64,
    pub total_credits: u64,
    pub remaining_credits: u64,
    pub transaction_id: Option<String>,
    pub mode: String,
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

        let url = self.api_url("/credits/balance");
        let mut response = ureq::get(&url)
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .call()
            .map_err(|e| format!("AgentPMT balance request failed at {url}: {e}"))?;

        let balance: CreditBalance = response
            .body_mut()
            .read_json()
            .map_err(|e| format!("AgentPMT balance parse error at {url}: {e}"))?;
        Ok(balance)
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
            cfg.history.push(CreditHistoryEntry {
                timestamp: now_unix_secs(),
                product_slug: product_slug.to_string(),
                units,
                total_credits: total,
                remaining_credits: remaining,
                transaction_id: Some(format!("stub-{}", uuid::Uuid::new_v4())),
                mode: "stub".to_string(),
            });
            if cfg.history.len() > 200 {
                let keep_from = cfg.history.len() - 200;
                cfg.history.drain(..keep_from);
            }
            save_agentpmt_config(&path, &cfg)?;

            let tx_id = cfg.history.last().and_then(|e| e.transaction_id.clone());
            return Ok(DeductResult {
                success: true,
                remaining_credits: remaining,
                transaction_id: tx_id,
                error: None,
            });
        }

        let url = self.api_url("/credits/deduct");
        let mut response = ureq::post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_key))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .send_json(serde_json::json!({
                "product_slug": product_slug,
                "units": units,
            }))
            .map_err(|e| format!("AgentPMT deduct request failed at {url}: {e}"))?;
        let result: DeductResult = response
            .body_mut()
            .read_json()
            .map_err(|e| format!("AgentPMT deduct parse error at {url}: {e}"))?;
        Ok(result)
    }

    pub fn can_afford(&self, credits_needed: u64) -> Result<bool, String> {
        let balance = self.balance()?;
        Ok(balance.credits >= credits_needed)
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), path)
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
        "attest_onchain" => Some(25),
        "attest_onchain_anon" => Some(75),
        "sign_pq" => Some(1),
        "audit_small" => Some(500),
        "audit_medium" => Some(1500),
        "audit_large" => Some(5000),
        "trust_query" => Some(1),
        "vote" => Some(25),
        "sync" => Some(1),
        "privacy_pool_create" => Some(100),
        "privacy_pool_withdraw" => Some(25),
        "pq_bridge_transfer" => Some(50),
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
        assert_eq!(operation_cost("attest_onchain"), Some(25));
        assert_eq!(operation_cost("attest_onchain_anon"), Some(75));
        assert_eq!(operation_cost("sign_pq"), Some(1));
        assert_eq!(operation_cost("audit_small"), Some(500));
        assert_eq!(operation_cost("audit_medium"), Some(1500));
        assert_eq!(operation_cost("audit_large"), Some(5000));
        assert_eq!(operation_cost("trust_query"), Some(1));
        assert_eq!(operation_cost("vote"), Some(25));
        assert_eq!(operation_cost("sync"), Some(1));
        assert_eq!(operation_cost("privacy_pool_create"), Some(100));
        assert_eq!(operation_cost("privacy_pool_withdraw"), Some(25));
        assert_eq!(operation_cost("pq_bridge_transfer"), Some(50));
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
            history: vec![CreditHistoryEntry {
                timestamp: 1700000000,
                product_slug: "attest".to_string(),
                units: 1,
                total_credits: 10,
                remaining_credits: 490,
                transaction_id: Some("tx-1".to_string()),
                mode: "stub".to_string(),
            }],
        };
        save_agentpmt_config(&path, &config).expect("save config");
        let loaded = load_agentpmt_config(&path).expect("load config");
        assert_eq!(loaded.api_key, "test-key-123");
        assert_eq!(loaded.cached_balance, Some(500));
        assert_eq!(loaded.history.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_without_history_defaults_to_empty_history() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_test_agentpmt_legacy_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let path = dir.join("agentpmt.json");
        std::fs::write(
            &path,
            r#"{"api_key":"legacy","cached_balance":123,"balance_refreshed_at":1700000001}"#,
        )
        .expect("write legacy config");

        let loaded = load_agentpmt_config(&path).expect("load legacy config");
        assert_eq!(loaded.history.len(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
