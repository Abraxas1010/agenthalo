use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
            "shell" | "claude" | "codex" | "gemini" => Ok(Self::Cli { cli_name, model }),
            _ => Err(format!(
                "container dispatch requires an explicit container_hookup for agent kind `{agent}`"
            )),
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
