use crate::halo::adapters::{base_event, StreamAdapter};
use crate::halo::schema::{EventType, TraceEvent};

#[derive(Default)]
pub struct ClaudeAdapter {
    model: Option<String>,
}

impl ClaudeAdapter {
    pub fn new() -> Self {
        Self { model: None }
    }
}

impl StreamAdapter for ClaudeAdapter {
    fn parse_line(&mut self, line: &str) -> Option<TraceEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;

        // Extract model name from various Claude event locations.
        if self.model.is_none() {
            self.model = v
                .get("model")
                .or_else(|| v.pointer("/message/model"))
                .or_else(|| v.pointer("/event/model"))
                .and_then(|m| m.as_str())
                .map(|s| s.to_string());
        }

        // Handle stream_event wrapper: unwrap inner event.
        let (kind, event_val) = if v
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "stream_event")
            .unwrap_or(false)
        {
            let inner = v.get("event").unwrap_or(&v);
            let inner_type = inner
                .get("type")
                .and_then(|t| t.as_str())
                .or_else(|| {
                    inner
                        .get("delta")
                        .and_then(|d| d.get("type"))
                        .and_then(|t| t.as_str())
                })
                .unwrap_or("assistant");
            (inner_type.to_string(), inner.clone())
        } else {
            let kind = v
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("assistant");
            (kind.to_string(), v.clone())
        };
        let kind = kind.as_str();

        let mut ev = match kind {
            "thinking" => base_event(EventType::Thinking, event_val.clone()),
            "tool_use" => base_event(EventType::ToolCall, event_val.clone()),
            "tool_result" => base_event(EventType::ToolResult, event_val.clone()),
            "error" => base_event(EventType::Error, event_val.clone()),
            "system" => base_event(EventType::SystemMessage, event_val.clone()),
            "text_delta" | "input_json_delta" | "content_block_delta" => {
                // Streaming delta — capture as raw, don't clutter event log.
                return None;
            }
            _ => {
                // Claude's assistant events may carry tool blocks in message.content.
                if contains_content_type(&event_val, "tool_use") {
                    base_event(EventType::ToolCall, event_val.clone())
                } else if contains_content_type(&event_val, "tool_result") {
                    base_event(EventType::ToolResult, event_val.clone())
                } else {
                    base_event(EventType::Assistant, event_val.clone())
                }
            }
        };

        if let Some(usage) = event_val
            .get("message")
            .and_then(|m| m.get("usage"))
            .or_else(|| event_val.get("usage"))
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
            ev.tool_name = event_val
                .get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
                .or_else(|| {
                    event_val
                        .get("message")
                        .and_then(|m| m.get("name"))
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string())
                });
            ev.tool_input = event_val.get("input").cloned();
            ev.tool_output = event_val.get("output").cloned();
        }

        Some(ev)
    }

    fn finalize(&mut self) -> Vec<TraceEvent> {
        Vec::new()
    }

    fn agent_name(&self) -> &str {
        "claude"
    }

    fn detected_model(&self) -> Option<&str> {
        self.model.as_deref()
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
