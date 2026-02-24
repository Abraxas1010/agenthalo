use crate::halo::adapters::{base_event, StreamAdapter};
use crate::halo::schema::{EventType, TraceEvent};

#[derive(Default)]
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl StreamAdapter for ClaudeAdapter {
    fn parse_line(&mut self, line: &str) -> Option<TraceEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let kind = v
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("assistant");

        let mut ev = match kind {
            "thinking" => base_event(EventType::Thinking, v.clone()),
            "tool_use" => base_event(EventType::ToolCall, v.clone()),
            "tool_result" => base_event(EventType::ToolResult, v.clone()),
            "error" => base_event(EventType::Error, v.clone()),
            "system" => base_event(EventType::SystemMessage, v.clone()),
            _ => {
                // Claude's assistant events may carry tool blocks in message.content.
                if contains_content_type(&v, "tool_use") {
                    base_event(EventType::ToolCall, v.clone())
                } else if contains_content_type(&v, "tool_result") {
                    base_event(EventType::ToolResult, v.clone())
                } else {
                    base_event(EventType::Assistant, v.clone())
                }
            }
        };

        if let Some(usage) = v
            .get("message")
            .and_then(|m| m.get("usage"))
            .or_else(|| v.get("usage"))
        {
            ev.input_tokens = usage.get("input_tokens").and_then(|n| n.as_u64());
            ev.output_tokens = usage.get("output_tokens").and_then(|n| n.as_u64());
            ev.cache_read_tokens = usage
                .get("cache_read_input_tokens")
                .and_then(|n| n.as_u64())
                .or_else(|| usage.get("cache_read_tokens").and_then(|n| n.as_u64()));
        }

        if matches!(ev.event_type, EventType::ToolCall | EventType::ToolResult) {
            ev.tool_name = v
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    v.get("message")
                        .and_then(|m| m.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                });
            ev.tool_input = v.get("input").cloned();
            ev.tool_output = v.get("output").cloned();
        }

        Some(ev)
    }

    fn finalize(&mut self) -> Vec<TraceEvent> {
        Vec::new()
    }

    fn agent_name(&self) -> &str {
        "claude"
    }
}

fn contains_content_type(v: &serde_json::Value, want: &str) -> bool {
    v.get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter().any(|item| {
                item.get("type")
                    .and_then(|t| t.as_str())
                    .map(|t| t == want)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}
