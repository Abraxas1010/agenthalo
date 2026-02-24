pub mod claude;
pub mod codex;
pub mod gemini;
pub mod generic;

use crate::halo::schema::TraceEvent;

pub trait StreamAdapter: Send {
    fn parse_line(&mut self, line: &str) -> Option<TraceEvent>;
    fn finalize(&mut self) -> Vec<TraceEvent>;
    fn agent_name(&self) -> &str;
}

pub fn base_event(
    event_type: crate::halo::schema::EventType,
    content: serde_json::Value,
) -> TraceEvent {
    TraceEvent {
        seq: 0,
        timestamp: crate::halo::trace::now_unix_secs(),
        event_type,
        content,
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        tool_name: None,
        tool_input: None,
        tool_output: None,
        file_path: None,
        content_hash: String::new(),
    }
}
