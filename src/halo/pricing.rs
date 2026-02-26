use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: Option<f64>,
}

/// Proxy resale configuration — controls markup and rate limits for external users.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Whether external (non-owner) API access is enabled.
    pub enabled: bool,
    /// Markup percentage applied to base model costs (e.g. 20.0 = 20% markup).
    pub markup_pct: f64,
    /// Maximum requests per minute per API key (0 = unlimited).
    pub rate_limit_rpm: u32,
    /// Maximum tokens per day per API key (0 = unlimited).
    pub daily_token_limit: u64,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            markup_pct: 20.0,
            rate_limit_rpm: 60,
            daily_token_limit: 1_000_000,
        }
    }
}

pub fn proxy_config_path() -> std::path::PathBuf {
    crate::halo::config::halo_dir().join("proxy_config.json")
}

pub fn load_proxy_config() -> ProxyConfig {
    let path = proxy_config_path();
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => ProxyConfig::default(),
    }
}

pub fn save_proxy_config(cfg: &ProxyConfig) -> Result<(), String> {
    let path = proxy_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create proxy config dir: {e}"))?;
    }
    let raw =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize proxy config: {e}"))?;
    std::fs::write(&path, raw)
        .map_err(|e| format!("write proxy config {}: {e}", path.display()))
}

/// Calculate the marked-up cost for external API users.
pub fn calculate_marked_up_cost(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_tokens: u64,
    pricing: &HashMap<String, ModelPricing>,
    markup_pct: f64,
) -> (f64, f64) {
    let base = calculate_cost(model, input_tokens, output_tokens, cache_tokens, pricing);
    let markup = base * (markup_pct / 100.0);
    (base, base + markup)
}

pub fn default_pricing() -> HashMap<String, ModelPricing> {
    let mut m = HashMap::new();
    m.insert(
        "claude-opus-4-6".into(),
        ModelPricing {
            input_per_mtok: 15.0,
            output_per_mtok: 75.0,
            cache_read_per_mtok: Some(1.5),
        },
    );
    m.insert(
        "claude-sonnet-4-6".into(),
        ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: Some(0.3),
        },
    );
    m.insert(
        "claude-haiku-4-5".into(),
        ModelPricing {
            input_per_mtok: 0.8,
            output_per_mtok: 4.0,
            cache_read_per_mtok: Some(0.08),
        },
    );
    m.insert(
        "o3".into(),
        ModelPricing {
            input_per_mtok: 10.0,
            output_per_mtok: 40.0,
            cache_read_per_mtok: None,
        },
    );
    m.insert(
        "o4-mini".into(),
        ModelPricing {
            input_per_mtok: 1.10,
            output_per_mtok: 4.40,
            cache_read_per_mtok: None,
        },
    );
    m.insert(
        "gpt-4.1".into(),
        ModelPricing {
            input_per_mtok: 2.0,
            output_per_mtok: 8.0,
            cache_read_per_mtok: None,
        },
    );
    m.insert(
        "gemini-2.5-pro".into(),
        ModelPricing {
            input_per_mtok: 1.25,
            output_per_mtok: 10.0,
            cache_read_per_mtok: None,
        },
    );
    m.insert(
        "gemini-2.5-flash".into(),
        ModelPricing {
            input_per_mtok: 0.15,
            output_per_mtok: 0.60,
            cache_read_per_mtok: None,
        },
    );
    m
}

pub fn load_or_default(path: &Path) -> Result<HashMap<String, ModelPricing>, String> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("create pricing dir: {e}"))?;
        }
        let defaults = default_pricing();
        let raw = serde_json::to_string_pretty(&defaults)
            .map_err(|e| format!("serialize pricing: {e}"))?;
        std::fs::write(path, raw).map_err(|e| format!("write pricing {}: {e}", path.display()))?;
        return Ok(defaults);
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read pricing {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse pricing {}: {e}", path.display()))
}

pub fn calculate_cost(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_tokens: u64,
    pricing: &HashMap<String, ModelPricing>,
) -> f64 {
    let p = pricing.get(model).cloned().unwrap_or(ModelPricing {
        input_per_mtok: 0.0,
        output_per_mtok: 0.0,
        cache_read_per_mtok: None,
    });
    let input_cost = (input_tokens as f64 / 1_000_000.0) * p.input_per_mtok;
    let output_cost = (output_tokens as f64 / 1_000_000.0) * p.output_per_mtok;
    let cache_cost = (cache_tokens as f64 / 1_000_000.0) * p.cache_read_per_mtok.unwrap_or(0.0);
    input_cost + output_cost + cache_cost
}
