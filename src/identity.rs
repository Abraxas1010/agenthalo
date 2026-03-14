use crate::config;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum IdentitySecurityTier {
    MaxSafe,
    LessSafe,
    LowSecurity,
}

pub const DEFAULT_SECURITY_TIER: IdentitySecurityTier = IdentitySecurityTier::MaxSafe;

pub fn default_security_tier_str() -> &'static str {
    DEFAULT_SECURITY_TIER.as_str()
}

impl IdentitySecurityTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MaxSafe => "max-safe",
            Self::LessSafe => "less-safe",
            Self::LowSecurity => "low-security",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "max-safe" | "max_safe" | "maxsafe" => Some(Self::MaxSafe),
            "less-safe" | "less_safe" | "lesssafe" | "balanced" | "a_little_rebellious" => {
                Some(Self::LessSafe)
            }
            "low-security" | "low_security" | "low" | "why-bother" => Some(Self::LowSecurity),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct IdentityConfig {
    pub version: Option<u32>,
    #[serde(default)]
    pub anonymous_mode: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_tier: Option<IdentitySecurityTier>,
    pub device: Option<DeviceIdentity>,
    pub network: Option<NetworkIdentity>,
    #[serde(default)]
    pub social: SocialIdentityConfig,
    #[serde(default)]
    pub super_secure: SuperSecureIdentityConfig,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_address: Option<AgentAddressIdentity>,
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
    pub puf_fingerprint_hex: Option<String>,
    #[serde(default)]
    pub puf_tier: Option<String>,
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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SocialIdentityConfig {
    #[serde(default)]
    pub providers: std::collections::BTreeMap<String, SocialProviderState>,
    pub last_updated: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SocialProviderState {
    #[serde(default)]
    pub selected: bool,
    pub expires_at: Option<u64>,
    pub source: Option<String>,
    pub last_connected_at: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SuperSecureIdentityConfig {
    #[serde(default)]
    pub passkey_enabled: bool,
    #[serde(default)]
    pub security_key_enabled: bool,
    #[serde(default)]
    pub totp_enabled: bool,
    pub totp_label: Option<String>,
    pub last_updated: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AgentAddressIdentity {
    pub evm_address: String,
    pub generated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl IdentityConfig {
    pub fn is_configured(&self) -> bool {
        self.anonymous_mode || self.device.as_ref().map(|d| d.enabled).unwrap_or(false)
    }
}

pub fn network_is_configured(network: &NetworkIdentity) -> bool {
    network.share_local_ip
        || network.share_public_ip
        || network.share_mac
        || network
            .local_ip_hash
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        || network
            .public_ip_hash
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        || !network.mac_addresses.is_empty()
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
    config::ensure_nucleusdb_dir()?;
    let path = config::identity_config_path();
    let json =
        serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize identity config: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write identity config: {e}"))
}
