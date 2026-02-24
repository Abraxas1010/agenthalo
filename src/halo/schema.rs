use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SessionMetadata {
    pub session_id: String,
    pub agent: String,
    pub model: Option<String>,
    pub started_at: u64,
    pub ended_at: Option<u64>,
    pub prompt: Option<String>,
    pub status: SessionStatus,
    pub user_id: Option<String>,
    pub machine_id: Option<String>,
    pub puf_digest: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct TraceEvent {
    pub seq: u64,
    pub timestamp: u64,
    pub event_type: EventType,
    pub content: serde_json::Value,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub tool_name: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_output: Option<serde_json::Value>,
    pub file_path: Option<String>,
    pub content_hash: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Thinking,
    Assistant,
    ToolCall,
    ToolResult,
    #[serde(alias = "mpc_tool_call")]
    McpToolCall,
    #[serde(alias = "mpc_tool_result")]
    McpToolResult,
    FileChange,
    BashCommand,
    Error,
    SubagentSpawn,
    SystemMessage,
    Raw,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema, Default)]
pub struct SessionSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub estimated_cost_usd: f64,
    pub tool_calls: u64,
    pub mcp_tool_calls: u64,
    pub files_modified: u64,
    pub files_created: u64,
    pub files_read: u64,
    pub bash_commands: u64,
    pub errors: u64,
    pub subagents_spawned: u64,
    pub event_count: u64,
    pub duration_secs: u64,
    pub model: Option<String>,
}

pub const SESSION_PREFIX: &str = "halo:session:";
pub const EVENT_PREFIX: &str = "halo:event:";
pub const SUMMARY_PREFIX: &str = "halo:summary:";
pub const IDX_AGENT_PREFIX: &str = "halo:idx:agent:";
pub const IDX_DATE_PREFIX: &str = "halo:idx:date:";
pub const IDX_MODEL_PREFIX: &str = "halo:idx:model:";
pub const COSTS_DAILY_PREFIX: &str = "halo:costs:daily:";
pub const COSTS_MONTHLY_PREFIX: &str = "halo:costs:monthly:";
