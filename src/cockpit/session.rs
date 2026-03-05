use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionStatus {
    #[default]
    Starting,
    Active,
    Done {
        exit_code: i32,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    pub status: SessionStatus,
    pub created_at: u64,
    pub cols: u16,
    pub rows: u16,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub input_bytes: u64,
    #[serde(default)]
    pub output_bytes: u64,
    #[serde(default)]
    pub estimated_input_tokens: u64,
    #[serde(default)]
    pub estimated_output_tokens: u64,
    #[serde(default)]
    pub estimated_cost_usd: f64,
    #[serde(default)]
    pub runtime_secs: u64,
    #[serde(default)]
    pub trace_flushed: bool,
}
