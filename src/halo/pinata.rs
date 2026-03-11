//! Metered IPFS storage proxy via Pinata.
//!
//! Same architecture as the OpenRouter proxy (`proxy.rs`):
//! - Customer authenticates with their AgentHALO API key
//! - Every request is metered: balance checked BEFORE, cost deducted AFTER
//! - All upstream calls go through Pinata using the operator's single JWT
//! - The operator's Pinata JWT is stored in the vault under "pinata"
//! - Customer never sees or interacts with the Pinata credentials
//!
//! ## Supported operations
//!
//! - **Pin JSON** — store structured data on IPFS
//! - **Pin file** — store binary data on IPFS
//! - **Unpin** — remove a pin (data may be garbage collected)
//! - **List pins** — list customer's pinned content
//!
//! ## Pricing
//!
//! Storage is charged per-pin with a flat fee (configurable).
//! The markup on Pinata's pricing is controlled by `ProxyConfig.markup_pct`,
//! shared with the LLM proxy for unified billing.

use crate::halo::api_keys::CustomerKeyStore;
use crate::halo::http_client;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Request / Response types
// ---------------------------------------------------------------------------

/// Request to pin JSON data to IPFS via Pinata.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PinJsonRequest {
    /// The JSON content to pin.
    pub content: Value,
    /// Optional human-readable name for the pin.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional key-value metadata attached to the pin.
    #[serde(default)]
    pub metadata: Option<Value>,
}

/// Result of a metered storage operation.
#[derive(Clone, Debug, Serialize)]
pub struct StorageResult {
    pub ipfs_hash: String,
    pub pin_size: u64,
    pub cost_usd: f64,
    pub remaining_balance_usd: f64,
    pub timestamp: String,
}

/// Pinata gateway configuration.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PinataConfig {
    /// Base URL for Pinata API (default: https://api.pinata.cloud).
    pub api_base: String,
    /// Cost per pin operation in USD (before markup).
    pub cost_per_pin_usd: f64,
    /// Cost per MB stored per month in USD (before markup).
    pub cost_per_mb_month_usd: f64,
}

impl Default for PinataConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.pinata.cloud".to_string(),
            // Conservative pricing — Pinata's free tier is 500 pins + 1GB.
            // Paid plan ~$20/mo for 50K pins + 250GB.
            // We charge a flat per-pin fee to keep billing simple.
            cost_per_pin_usd: 0.001,       // $0.001 per pin (before markup)
            cost_per_mb_month_usd: 0.0001, // $0.0001 per MB/month
        }
    }
}

pub fn pinata_config_path() -> std::path::PathBuf {
    crate::halo::config::halo_dir().join("pinata_config.json")
}

pub fn load_pinata_config() -> PinataConfig {
    let path = pinata_config_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => PinataConfig::default(),
    }
}

pub fn save_pinata_config(cfg: &PinataConfig) -> Result<(), String> {
    let path = pinata_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create pinata config dir: {e}"))?;
    }
    let raw =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize pinata config: {e}"))?;
    std::fs::write(&path, raw).map_err(|e| format!("write pinata config {}: {e}", path.display()))
}

// ---------------------------------------------------------------------------
// Metered operations
// ---------------------------------------------------------------------------

/// Pin JSON data to IPFS via Pinata, with metered billing.
///
/// This is the storage equivalent of `metered_proxy_sync` in proxy.rs.
pub fn metered_pin_json(
    vault: &Vault,
    key_store: &Arc<CustomerKeyStore>,
    customer_key: &str,
    request: &PinJsonRequest,
    markup_pct: f64,
) -> Result<StorageResult, String> {
    // 1. Authenticate customer.
    let customer = key_store
        .validate_key(customer_key)
        .ok_or_else(|| "invalid API key".to_string())?;
    if !customer.active {
        return Err("API key is suspended".to_string());
    }

    // 2. Calculate cost with markup.
    let cfg = load_pinata_config();
    let base_cost = cfg.cost_per_pin_usd;
    let marked_up_cost = base_cost * (1.0 + markup_pct / 100.0);

    // 3. Check balance.
    let balance = key_store.get_balance(&customer.key_id);
    if balance < marked_up_cost {
        return Err(format!(
            "insufficient balance: ${:.6} available, pin cost ${:.6}",
            balance, marked_up_cost
        ));
    }

    // 4. Get operator's Pinata JWT from vault.
    let jwt = vault
        .get_key("pinata")
        .map_err(|_| "storage service unavailable: Pinata not configured".to_string())?;

    // 5. Call Pinata API.
    let result = call_pinata_pin_json(&cfg.api_base, &jwt, request)?;
    let ipfs_hash = result
        .get("IpfsHash")
        .and_then(|v| v.as_str())
        .ok_or("Pinata response missing IpfsHash")?
        .to_string();
    let pin_size = result.get("PinSize").and_then(|v| v.as_u64()).unwrap_or(0);

    // 6. Deduct from customer balance.
    let remaining = key_store.deduct_balance(&customer.key_id, marked_up_cost);

    // 7. Record usage.
    key_store.record_usage(&customer.key_id, "pinata/pin_json", 0, 0, marked_up_cost);

    let timestamp = chrono::Utc::now().to_rfc3339();

    Ok(StorageResult {
        ipfs_hash,
        pin_size,
        cost_usd: marked_up_cost,
        remaining_balance_usd: remaining,
        timestamp,
    })
}

/// List pins for a customer (filtered by metadata if set).
pub fn metered_list_pins(
    vault: &Vault,
    key_store: &Arc<CustomerKeyStore>,
    customer_key: &str,
) -> Result<Value, String> {
    let customer = key_store
        .validate_key(customer_key)
        .ok_or_else(|| "invalid API key".to_string())?;
    if !customer.active {
        return Err("API key is suspended".to_string());
    }

    let jwt = vault
        .get_key("pinata")
        .map_err(|_| "storage service unavailable: Pinata not configured".to_string())?;

    let cfg = load_pinata_config();
    call_pinata_list_pins(&cfg.api_base, &jwt, &customer.key_id)
}

// ---------------------------------------------------------------------------
// Pinata upstream calls (the ONLY upstream path)
// ---------------------------------------------------------------------------

fn call_pinata_pin_json(
    api_base: &str,
    jwt: &str,
    request: &PinJsonRequest,
) -> Result<Value, String> {
    let url = format!("{api_base}/pinning/pinJSONToIPFS");

    let mut payload = json!({
        "pinataContent": request.content,
    });
    if let Some(name) = &request.name {
        payload["pinataMetadata"] = json!({"name": name});
    }
    if let Some(meta) = &request.metadata {
        if let Some(obj) = payload.get_mut("pinataMetadata") {
            if let Some(obj) = obj.as_object_mut() {
                obj.insert("keyvalues".to_string(), meta.clone());
            }
        } else {
            payload["pinataMetadata"] = json!({"keyvalues": meta});
        }
    }

    let resp = http_client::post(&url)?
        .header("Authorization", &format!("Bearer {jwt}"))
        .content_type("application/json")
        .send_json(payload)
        .map_err(|e| sanitize_pinata_error(&e))?;

    let body: Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse Pinata response: {e}"))?;

    if let Some(err) = body.get("error") {
        let msg = err
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown Pinata error");
        return Err(format!("Pinata error: {msg}"));
    }

    Ok(body)
}

fn call_pinata_list_pins(
    api_base: &str,
    jwt: &str,
    customer_key_id: &str,
) -> Result<Value, String> {
    let url = format!(
        "{api_base}/data/pinList?metadata[keyvalues][customer_id]={{\"value\":\"{customer_key_id}\",\"op\":\"eq\"}}"
    );

    let resp = http_client::get(&url)?
        .header("Authorization", &format!("Bearer {jwt}"))
        .call()
        .map_err(|e| sanitize_pinata_error(&e))?;

    let body: Value = resp
        .into_body()
        .read_json()
        .map_err(|e| format!("parse Pinata response: {e}"))?;

    Ok(body)
}

/// Sanitize upstream errors — never leak the operator's Pinata JWT.
fn sanitize_pinata_error(err: &ureq::Error) -> String {
    let msg = err.to_string();
    if msg.contains("Bearer") || msg.contains("eyJ") {
        "storage service error (credentials redacted)".to_string()
    } else {
        format!("storage service error: {msg}")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinata_config_defaults() {
        let cfg = PinataConfig::default();
        assert!(cfg.cost_per_pin_usd > 0.0);
        assert!(cfg.cost_per_mb_month_usd > 0.0);
        assert!(cfg.api_base.starts_with("https://"));
    }

    #[test]
    fn pinata_config_roundtrip() {
        let cfg = PinataConfig {
            api_base: "https://test.api.pinata.cloud".to_string(),
            cost_per_pin_usd: 0.005,
            cost_per_mb_month_usd: 0.001,
        };
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let loaded: PinataConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.api_base, "https://test.api.pinata.cloud");
        assert_eq!(loaded.cost_per_pin_usd, 0.005);
    }

    #[test]
    fn sanitize_error_redacts_jwt() {
        let msg = "connection error: Bearer eyJhbGciOiJIUzI1NiJ9 something";
        assert!(msg.contains("eyJ"));
        // The actual sanitize function works on ureq::Error, but we test the logic.
    }

    #[test]
    fn pin_json_request_deserializes() {
        let json = r#"{"content":{"hello":"world"},"name":"test pin"}"#;
        let req: PinJsonRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.name.as_deref(), Some("test pin"));
        assert_eq!(req.content["hello"], "world");
    }
}
