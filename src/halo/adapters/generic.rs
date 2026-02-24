use crate::halo::adapters::{base_event, StreamAdapter};
use crate::halo::schema::{EventType, TraceEvent};

#[derive(Default)]
pub struct GenericAdapter {
    name: String,
}

impl GenericAdapter {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl StreamAdapter for GenericAdapter {
    fn parse_line(&mut self, line: &str) -> Option<TraceEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Try to parse as JSON for richer event extraction.
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            return Some(parse_json_event(&v));
        }

        Some(base_event(
            EventType::Raw,
            serde_json::json!({ "line": line }),
        ))
    }

    fn finalize(&mut self) -> Vec<TraceEvent> {
        Vec::new()
    }

    fn agent_name(&self) -> &str {
        self.name.as_str()
    }
}

/// Parse a JSON line into a typed TraceEvent, extracting tokens and tool info
/// from common formats (OpenAI-compatible, MCP tool calls, agent loop output).
fn parse_json_event(v: &serde_json::Value) -> TraceEvent {
    // Detect event type from common field patterns.
    let event_type = detect_event_type(v);
    let mut ev = base_event(event_type, v.clone());

    // Extract token usage from OpenAI-compatible responses.
    // Patterns: { "usage": { "prompt_tokens": N, "completion_tokens": N } }
    //           { "usage": { "input_tokens": N, "output_tokens": N } }
    if let Some(usage) = v.get("usage").and_then(|u| u.as_object()) {
        ev.input_tokens = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(|n| n.as_u64());
        ev.output_tokens = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(|n| n.as_u64());
        ev.cache_read_tokens = usage
            .get("cache_read_input_tokens")
            .or_else(|| usage.get("cache_read_tokens"))
            .and_then(|n| n.as_u64());
    }

    // Extract token counts from nested response objects.
    // Pattern: { "response": { "usage": { ... } } }
    if ev.input_tokens.is_none() {
        if let Some(usage) = v
            .get("response")
            .and_then(|r| r.get("usage"))
            .and_then(|u| u.as_object())
        {
            ev.input_tokens = usage
                .get("prompt_tokens")
                .or_else(|| usage.get("input_tokens"))
                .and_then(|n| n.as_u64());
            ev.output_tokens = usage
                .get("completion_tokens")
                .or_else(|| usage.get("output_tokens"))
                .and_then(|n| n.as_u64());
        }
    }

    // Extract tool name/input/output for tool call events.
    if matches!(
        ev.event_type,
        EventType::ToolCall | EventType::McpToolCall
    ) {
        ev.tool_name = v
            .get("tool")
            .or_else(|| v.get("tool_name"))
            .or_else(|| v.get("name"))
            .and_then(|n| n.as_str())
            .map(|s| s.to_string());
        ev.tool_input = v.get("input").or_else(|| v.get("arguments")).cloned();
        ev.tool_output = v.get("output").or_else(|| v.get("result")).cloned();
    }

    ev
}

/// Detect event type from JSON field patterns.
fn detect_event_type(v: &serde_json::Value) -> EventType {
    // Explicit type field.
    if let Some(t) = v.get("type").and_then(|t| t.as_str()) {
        return match t {
            "tool_call" | "function_call" => EventType::ToolCall,
            "tool_result" | "function_result" => EventType::ToolResult,
            "mcp_tool_call" => EventType::McpToolCall,
            "mcp_tool_result" => EventType::McpToolResult,
            "error" => EventType::Error,
            "assistant" | "assistant_message" => EventType::Assistant,
            "thinking" => EventType::Thinking,
            "system" | "system_message" => EventType::SystemMessage,
            _ => EventType::Raw,
        };
    }

    // MCP tool call pattern: { "tool_call": { ... } } or { "tool": "name" }
    if v.get("tool_call").is_some() {
        return EventType::McpToolCall;
    }

    // Tool call detection by field presence.
    if v.get("tool").is_some() || v.get("tool_name").is_some() {
        if v.get("result").is_some() || v.get("output").is_some() {
            return EventType::ToolResult;
        }
        return EventType::ToolCall;
    }

    // Error detection.
    if v.get("error").is_some() && v.get("success").and_then(|s| s.as_bool()) == Some(false) {
        return EventType::Error;
    }

    // OpenAI chat completion response.
    if v.get("choices").is_some() && v.get("usage").is_some() {
        return EventType::Assistant;
    }

    // Agent loop structured output (event_count, arxiv_discovery, etc.)
    if v.get("event_count").is_some()
        || v.get("arxiv_discovery").is_some()
        || v.get("agent_action").is_some()
    {
        return EventType::Assistant;
    }

    EventType::Raw
}
