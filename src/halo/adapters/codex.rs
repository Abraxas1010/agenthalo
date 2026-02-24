use crate::halo::adapters::{base_event, StreamAdapter};
use crate::halo::schema::{EventType, TraceEvent};

#[derive(Default)]
pub struct CodexAdapter;

impl CodexAdapter {
    pub fn new() -> Self {
        Self
    }
}

impl StreamAdapter for CodexAdapter {
    fn parse_line(&mut self, line: &str) -> Option<TraceEvent> {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("raw");

        let mut ev = if kind.contains("error") || kind.ends_with("failed") {
            base_event(EventType::Error, v.clone())
        } else if kind.starts_with("thread.") || kind.starts_with("turn.") {
            base_event(EventType::SystemMessage, v.clone())
        } else if kind.starts_with("item.") {
            let item_kind = v
                .get("item")
                .and_then(|it| it.get("type").or_else(|| it.get("kind")))
                .and_then(|k| k.as_str())
                .unwrap_or("");
            match item_kind {
                "reasoning" => base_event(EventType::Thinking, v.clone()),
                "command" | "bash" => base_event(EventType::BashCommand, v.clone()),
                "tool_call" | "tool_use" => base_event(EventType::ToolCall, v.clone()),
                "tool_result" => base_event(EventType::ToolResult, v.clone()),
                "mcp_tool_call" => base_event(EventType::McpToolCall, v.clone()),
                "mcp_tool_result" => base_event(EventType::McpToolResult, v.clone()),
                "file_change" => base_event(EventType::FileChange, v.clone()),
                "subagent_spawn" => base_event(EventType::SubagentSpawn, v.clone()),
                _ => base_event(EventType::Assistant, v.clone()),
            }
        } else {
            base_event(EventType::Raw, v.clone())
        };

        if let Some(usage) = v.get("usage") {
            ev.input_tokens = usage.get("input_tokens").and_then(|n| n.as_u64());
            ev.output_tokens = usage.get("output_tokens").and_then(|n| n.as_u64());
            ev.cache_read_tokens = usage.get("cache_read_tokens").and_then(|n| n.as_u64());
        }

        if matches!(ev.event_type, EventType::ToolCall | EventType::McpToolCall) {
            ev.tool_name = v
                .pointer("/item/name")
                .or_else(|| v.pointer("/tool/name"))
                .and_then(|n| n.as_str())
                .map(|s| s.to_string());
            ev.tool_input = v.pointer("/item/input").cloned();
        }
        if matches!(
            ev.event_type,
            EventType::ToolResult | EventType::McpToolResult
        ) {
            ev.tool_output = v.pointer("/item/output").cloned();
        }
        if matches!(ev.event_type, EventType::FileChange) {
            ev.file_path = v
                .pointer("/item/path")
                .or_else(|| v.pointer("/path"))
                .and_then(|p| p.as_str())
                .map(|s| s.to_string());
        }

        Some(ev)
    }

    fn finalize(&mut self) -> Vec<TraceEvent> {
        Vec::new()
    }

    fn agent_name(&self) -> &str {
        "codex"
    }
}
