use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const GEMINI_DEFAULT_MODEL_ENV: &str = "AGENTHALO_GEMINI_DEFAULT_MODEL";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchMode {
    Pty,
    Container,
}

impl DispatchMode {
    pub fn from_env() -> Self {
        match std::env::var("AGENTHALO_DISPATCH_MODE")
            .ok()
            .unwrap_or_else(|| "pty".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "container" => Self::Container,
            _ => Self::Pty,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContainerHookupRequest {
    Cli {
        cli_name: String,
        #[serde(default)]
        model: Option<String>,
    },
    Api {
        provider: String,
        model: String,
        api_key_source: String,
        #[serde(default)]
        base_url_override: Option<String>,
    },
    LocalModel {
        model_id: String,
        #[serde(default)]
        vllm_port: Option<u16>,
        #[serde(default)]
        base_url_override: Option<String>,
    },
}

impl ContainerHookupRequest {
    pub fn infer_cli(agent: &str, model: Option<String>) -> Result<Self, String> {
        let cli_name = agent.trim().to_ascii_lowercase();
        match cli_name.as_str() {
            "shell" | "claude" | "codex" | "gemini" => {
                Ok(Self::Cli { cli_name, model }.normalized())
            }
            _ => Err(format!(
                "container dispatch requires an explicit container_hookup for agent kind `{agent}`"
            )),
        }
    }

    pub fn normalized(self) -> Self {
        match self {
            Self::Cli { cli_name, model } => Self::Cli {
                cli_name: cli_name.clone(),
                model: normalize_cli_model(&cli_name, model),
            },
            other => other,
        }
    }

    pub fn agent_type(&self) -> String {
        match self {
            Self::Cli { cli_name, .. } => cli_name.clone(),
            Self::Api { provider, .. } => provider.clone(),
            Self::LocalModel { .. } => "local_model".to_string(),
        }
    }

    pub fn model(&self) -> Option<String> {
        match self {
            Self::Cli { model, .. } => model.clone(),
            Self::Api { model, .. } => Some(model.clone()),
            Self::LocalModel { model_id, .. } => Some(model_id.clone()),
        }
    }
}

pub(crate) fn normalize_cli_model(cli_name: &str, model: Option<String>) -> Option<String> {
    let explicit = model.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    });
    explicit.or_else(|| cli_default_model(cli_name))
}

fn cli_default_model(cli_name: &str) -> Option<String> {
    match cli_name.trim().to_ascii_lowercase().as_str() {
        "gemini" => std::env::var(GEMINI_DEFAULT_MODEL_ENV)
            .ok()
            .and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    struct EnvGuard {
        prior: Option<String>,
    }

    impl EnvGuard {
        fn set(value: Option<&str>) -> Self {
            let prior = std::env::var(GEMINI_DEFAULT_MODEL_ENV).ok();
            match value {
                Some(value) => unsafe { std::env::set_var(GEMINI_DEFAULT_MODEL_ENV, value) },
                None => unsafe { std::env::remove_var(GEMINI_DEFAULT_MODEL_ENV) },
            }
            Self { prior }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.prior.as_deref() {
                Some(value) => unsafe { std::env::set_var(GEMINI_DEFAULT_MODEL_ENV, value) },
                None => unsafe { std::env::remove_var(GEMINI_DEFAULT_MODEL_ENV) },
            }
        }
    }

    #[test]
    fn normalize_cli_model_uses_explicit_trimmed_value() {
        let _lock = env_lock();
        let _guard = EnvGuard::set(Some("gemini-2.5-flash"));
        assert_eq!(
            normalize_cli_model("gemini", Some(" gemini-2.5-pro ".to_string())),
            Some("gemini-2.5-pro".to_string())
        );
    }

    #[test]
    fn normalize_cli_model_uses_env_default_for_gemini() {
        let _lock = env_lock();
        let _guard = EnvGuard::set(Some("gemini-2.5-flash"));
        assert_eq!(
            normalize_cli_model("gemini", None),
            Some("gemini-2.5-flash".to_string())
        );
        assert_eq!(
            normalize_cli_model("gemini", Some("   ".to_string())),
            Some("gemini-2.5-flash".to_string())
        );
    }

    #[test]
    fn normalize_cli_model_returns_none_without_default_or_for_other_clis() {
        let _lock = env_lock();
        let _guard = EnvGuard::set(None);
        assert_eq!(normalize_cli_model("gemini", None), None);
        assert_eq!(normalize_cli_model("claude", None), None);
    }
}
