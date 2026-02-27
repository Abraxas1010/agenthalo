use crate::halo::config;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IdentityConfig {
    pub version: Option<u32>,
    #[serde(default)]
    pub anonymous_mode: bool,
    pub device: Option<DeviceIdentity>,
    pub network: Option<NetworkIdentity>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DeviceIdentity {
    #[serde(default)]
    pub enabled: bool,
    pub browser_fingerprint: Option<String>,
    #[serde(default)]
    pub selected_components: Vec<String>,
    pub composite_fingerprint_hex: Option<String>,
    #[serde(default)]
    pub entropy_bits: u32,
    pub last_collected: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NetworkIdentity {
    #[serde(default)]
    pub share_local_ip: bool,
    #[serde(default)]
    pub share_public_ip: bool,
    #[serde(default)]
    pub share_mac: bool,
    pub local_ip_hash: Option<String>,
    pub public_ip_hash: Option<String>,
    #[serde(default)]
    pub mac_addresses: Vec<String>,
}

impl IdentityConfig {
    pub fn is_configured(&self) -> bool {
        self.anonymous_mode || self.device.as_ref().map(|d| d.enabled).unwrap_or(false)
    }
}

pub fn load() -> IdentityConfig {
    let path = config::identity_config_path();
    if !path.exists() {
        return IdentityConfig::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &IdentityConfig) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let path = config::identity_config_path();
    let json =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize identity config: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write identity config: {e}"))
}
