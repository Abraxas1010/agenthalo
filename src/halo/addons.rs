use crate::halo::config;
use crate::halo::trace::now_unix_secs;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddonConfig {
    pub p2pclaw_enabled: bool,
    pub agentpmt_workflows_enabled: bool,
    pub updated_at: u64,
}

impl Default for AddonConfig {
    fn default() -> Self {
        Self {
            p2pclaw_enabled: false,
            agentpmt_workflows_enabled: false,
            updated_at: now_unix_secs(),
        }
    }
}

pub fn load_or_default() -> AddonConfig {
    load_addons(&config::addons_path()).unwrap_or_default()
}

pub fn load_addons(path: &Path) -> Result<AddonConfig, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("read addons {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("parse addons {}: {e}", path.display()))
}

pub fn save_addons(path: &Path, cfg: &AddonConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create addons dir {}: {e}", parent.display()))?;
    }
    let raw = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize addons: {e}"))?;
    std::fs::write(path, raw).map_err(|e| format!("write addons {}: {e}", path.display()))
}

pub fn set_enabled(name: &str, enabled: bool) -> Result<AddonConfig, String> {
    let mut cfg = load_or_default();
    match name {
        "p2pclaw" => cfg.p2pclaw_enabled = enabled,
        "agentpmt-workflows" => cfg.agentpmt_workflows_enabled = enabled,
        _ => return Err(format!("unknown add-on: {name}")),
    }
    cfg.updated_at = now_unix_secs();
    save_addons(&config::addons_path(), &cfg)?;
    Ok(cfg)
}

pub fn is_enabled(name: &str) -> Result<bool, String> {
    let cfg = load_or_default();
    match name {
        "p2pclaw" => Ok(cfg.p2pclaw_enabled),
        "agentpmt-workflows" => Ok(cfg.agentpmt_workflows_enabled),
        _ => Err(format!("unknown add-on: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addons_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "agenthalo_addons_{}_{}",
            std::process::id(),
            now_unix_secs()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("addons.json");
        let cfg = AddonConfig {
            p2pclaw_enabled: true,
            agentpmt_workflows_enabled: false,
            updated_at: now_unix_secs(),
        };
        save_addons(&path, &cfg).expect("save");
        let loaded = load_addons(&path).expect("load");
        assert!(loaded.p2pclaw_enabled);
        assert!(!loaded.agentpmt_workflows_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
