use crate::halo::config;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct UserProfile {
    pub display_name: Option<String>,
    pub avatar_type: Option<String>,
    pub avatar_data: Option<String>,
    #[serde(default)]
    pub name_locked: bool,
    #[serde(default)]
    pub name_revision: u64,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

impl UserProfile {
    pub fn has_name(&self) -> bool {
        self.display_name
            .as_ref()
            .map(|name| !name.trim().is_empty())
            .unwrap_or(false)
    }
}

pub fn load() -> UserProfile {
    let path = config::profile_path();
    if !path.exists() {
        return UserProfile::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(profile: &UserProfile) -> Result<(), String> {
    config::ensure_halo_dir()?;
    let path = config::profile_path();
    let json =
        serde_json::to_string_pretty(profile).map_err(|e| format!("serialize profile: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write profile: {e}"))
}
