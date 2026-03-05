use crate::cockpit::pty_manager::{PtySession, SessionEvent};
use crate::halo::adapters::claude::ClaudeAdapter;
use crate::halo::adapters::codex::CodexAdapter;
use crate::halo::adapters::gemini::GeminiAdapter;
use crate::halo::adapters::generic::GenericAdapter;
use crate::halo::adapters::StreamAdapter;
use crate::halo::schema::{
    EventType, SessionMetadata, SessionStatus as HaloSessionStatus, TraceEvent,
};
use crate::halo::trace::TraceWriter;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct TaskRunOutcome {
    pub output: String,
    pub exit_code: i32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_cost_usd: f64,
    pub trace_session_id: Option<String>,
}

pub struct TraceBridge {
    adapter: Box<dyn StreamAdapter>,
    writer: Option<TraceWriter>,
    line_buf: Vec<u8>,
    output_buf: Vec<u8>,
}

impl TraceBridge {
    pub fn new(
        agent_type: &str,
        trace_db_path: &Path,
        trace_session_id: &str,
        prompt: &str,
        enabled: bool,
    ) -> Result<Self, String> {
        let adapter = match agent_type {
            "claude" => Box::new(ClaudeAdapter::new()) as Box<dyn StreamAdapter>,
            "codex" => Box::new(CodexAdapter::new()) as Box<dyn StreamAdapter>,
            "gemini" => Box::new(GeminiAdapter::new()) as Box<dyn StreamAdapter>,
            other => Box::new(GenericAdapter::new(other.to_string())) as Box<dyn StreamAdapter>,
        };
        let writer = if enabled {
            let mut writer = TraceWriter::new(trace_db_path)?;
            writer.start_session(SessionMetadata {
                session_id: trace_session_id.to_string(),
                agent: agent_type.to_string(),
                model: None,
                started_at: crate::pod::now_unix(),
                ended_at: None,
                prompt: Some(prompt.to_string()),
                status: HaloSessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })?;
            Some(writer)
        } else {
            None
        };
        Ok(Self {
            adapter,
            writer,
            line_buf: Vec::new(),
            output_buf: Vec::new(),
        })
    }

    fn write_trace_event(&mut self, event: TraceEvent) -> Result<(), String> {
        if let Some(writer) = self.writer.as_mut() {
            writer.write_event(event)?;
        }
        Ok(())
    }

    fn process_line(&mut self, line: &str) -> Result<(), String> {
        if let Some(event) = self.adapter.parse_line(line) {
            self.write_trace_event(event)?;
        } else {
            self.write_trace_event(TraceEvent {
                seq: 0,
                timestamp: crate::halo::trace::now_unix_secs(),
                event_type: EventType::Raw,
                content: serde_json::json!({ "line": line }),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: None,
                tool_input: None,
                tool_output: None,
                file_path: None,
                content_hash: String::new(),
            })?;
        }
        Ok(())
    }

    pub fn process_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.output_buf.extend_from_slice(bytes);
        self.line_buf.extend_from_slice(bytes);
        while let Some(pos) = self.line_buf.iter().position(|b| *b == b'\n') {
            let line = String::from_utf8_lossy(&self.line_buf[..pos]).to_string();
            self.line_buf.drain(..=pos);
            self.process_line(&line)?;
        }
        Ok(())
    }

    pub fn finalize(&mut self, status: HaloSessionStatus) -> Result<Option<String>, String> {
        if !self.line_buf.is_empty() {
            let line = String::from_utf8_lossy(&self.line_buf).to_string();
            self.line_buf.clear();
            self.process_line(&line)?;
        }
        let tail = self.adapter.finalize();
        for event in tail {
            self.write_trace_event(event)?;
        }
        if let Some(model) = self.adapter.detected_model().map(str::to_string) {
            if let Some(writer) = self.writer.as_mut() {
                writer.update_session_model(&model);
            }
        }
        if let Some(writer) = self.writer.as_mut() {
            let _ = writer.end_session(status)?;
        }
        Ok(self.adapter.detected_model().map(str::to_string))
    }

    pub fn output_text(&self) -> String {
        String::from_utf8_lossy(&self.output_buf).to_string()
    }
}

pub async fn collect_task_output(
    session: Arc<PtySession>,
    agent_type: &str,
    trace_db_path: &Path,
    trace_session_id: &str,
    prompt: &str,
    trace_enabled: bool,
    timeout_secs: u64,
) -> Result<TaskRunOutcome, String> {
    let mut bridge = TraceBridge::new(
        agent_type,
        trace_db_path,
        trace_session_id,
        prompt,
        trace_enabled,
    )?;
    let mut rx = session.subscribe_output();
    let timeout = std::time::Duration::from_secs(timeout_secs.max(1));
    let deadline = tokio::time::Instant::now() + timeout;
    let exit_code = loop {
        let remain = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remain.is_zero() {
            let _ = session.terminate();
            bridge.finalize(HaloSessionStatus::Interrupted)?;
            return Err(format!("task timeout after {timeout_secs}s"));
        }
        let event = tokio::time::timeout(remain, rx.recv())
            .await
            .map_err(|_| format!("task timeout after {timeout_secs}s"))?;
        match event {
            Ok(SessionEvent::Output(bytes)) => {
                bridge.process_bytes(&bytes)?;
            }
            Ok(SessionEvent::Status(crate::cockpit::session::SessionStatus::Done {
                exit_code: code,
            })) => {
                break code;
            }
            Ok(SessionEvent::Status(crate::cockpit::session::SessionStatus::Error { message })) => {
                bridge.finalize(HaloSessionStatus::Failed)?;
                return Err(message);
            }
            Ok(SessionEvent::Status(_)) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => {
                break session.poll_exit_status().unwrap_or(1);
            }
        }
    };

    let final_status = if exit_code == 0 {
        HaloSessionStatus::Completed
    } else {
        HaloSessionStatus::Failed
    };
    bridge.finalize(final_status)?;
    let telemetry = session.telemetry_snapshot();
    Ok(TaskRunOutcome {
        output: bridge.output_text(),
        exit_code,
        input_tokens: telemetry.estimated_input_tokens,
        output_tokens: telemetry.estimated_output_tokens,
        estimated_cost_usd: telemetry.estimated_cost_usd,
        trace_session_id: if trace_enabled {
            Some(trace_session_id.to_string())
        } else {
            None
        },
    })
}
