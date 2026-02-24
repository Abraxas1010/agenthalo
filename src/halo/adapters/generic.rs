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
        if line.trim().is_empty() {
            return None;
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
