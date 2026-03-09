use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DEFAULT_CHUNK_SIZE: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChunkParams {
    pub chunk_size_bytes: usize,
}

impl Default for ChunkParams {
    fn default() -> Self {
        Self {
            chunk_size_bytes: DEFAULT_CHUNK_SIZE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SwarmConfig {
    pub chunk_params: ChunkParams,
    pub bitswap_enabled: bool,
    pub chunk_credit_cost: u64,
    pub require_grants: bool,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        Self {
            chunk_params: ChunkParams::default(),
            bitswap_enabled: true,
            chunk_credit_cost: 1,
            require_grants: false,
        }
    }
}

impl SwarmConfig {
    pub fn from_env() -> Self {
        let chunk_size_bytes = std::env::var("HALO_CHUNK_SIZE_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_CHUNK_SIZE);
        let bitswap_enabled = std::env::var("HALO_BITSWAP_ENABLED")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(true);
        let chunk_credit_cost = std::env::var("HALO_CHUNK_CREDIT_COST")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1);
        let require_grants = std::env::var("HALO_BITSWAP_REQUIRE_GRANTS")
            .ok()
            .map(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            })
            .unwrap_or(false);
        Self {
            chunk_params: ChunkParams { chunk_size_bytes },
            bitswap_enabled,
            chunk_credit_cost,
            require_grants,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
            if let Some(prev) = &self.prev {
                std::env::set_var(self.key, prev);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn default_chunk_size_matches_spec() {
        assert_eq!(ChunkParams::default().chunk_size_bytes, 256 * 1024);
    }

    #[test]
    fn config_parses_require_grants_from_env() {
        let _guard = env_lock().lock().expect("lock env");
        let _require = EnvVarGuard::set("HALO_BITSWAP_REQUIRE_GRANTS", Some("true"));
        let cfg = SwarmConfig::from_env();
        assert!(cfg.require_grants);
    }
}
