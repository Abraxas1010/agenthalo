use crate::halo::adapters::{base_event, StreamAdapter};
use crate::halo::schema::{EventType, TraceEvent};

#[derive(Default)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl StreamAdapter for GeminiAdapter {
    fn parse_line(&mut self, line: &str) -> Option<TraceEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("raw");

        let mut ev = match kind {
            "thinking" | "reasoning" => base_event(EventType::Thinking, v.clone()),
            "tool_call" | "function_call" => base_event(EventType::ToolCall, v.clone()),
            "tool_result" | "function_result" => base_event(EventType::ToolResult, v.clone()),
            "error" => base_event(EventType::Error, v.clone()),
            "message" | "assistant" => base_event(EventType::Assistant, v.clone()),
            _ => base_event(EventType::Raw, v.clone()),
        };

        if let Some(usage) = v.get("usage") {
            ev.input_tokens = usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
                .and_then(|n| n.as_u64());
            ev.output_tokens = usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
                .and_then(|n| n.as_u64());
            ev.cache_read_tokens = usage.get("cache_read_tokens").and_then(|n| n.as_u64());
        }

        if matches!(ev.event_type, EventType::ToolCall | EventType::ToolResult) {
            ev.tool_name = v
                .get("name")
                .or_else(|| v.get("tool"))
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            ev.tool_input = v.get("input").cloned();
            ev.tool_output = v.get("output").cloned();
        }

        Some(ev)
    }

    fn finalize(&mut self) -> Vec<TraceEvent> {
        Vec::new()
    }

    fn agent_name(&self) -> &str {
        "gemini"
    }
}
