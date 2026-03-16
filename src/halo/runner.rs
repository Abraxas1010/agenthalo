use crate::halo::adapters::claude::ClaudeAdapter;
use crate::halo::adapters::codex::CodexAdapter;
use crate::halo::adapters::gemini::GeminiAdapter;
use crate::halo::adapters::generic::GenericAdapter;
use crate::halo::adapters::StreamAdapter;
use crate::verifier::gate as proof_gate;
use crate::halo::detect::{detect_agent, injection_flags, AgentType};
use crate::halo::schema::{EventType, TraceEvent};
use crate::halo::trace::{now_unix_secs, TraceWriter};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

pub struct AgentRunner {
    agent_type: AgentType,
    command: String,
    args: Vec<String>,
}

impl AgentRunner {
    pub fn new(command: String, args: Vec<String>) -> Self {
        let agent_type = detect_agent(&command);
        Self {
            agent_type,
            command,
            args,
        }
    }

    /// Override the auto-detected agent name (e.g. `--agent-name Abraxas`).
    pub fn with_agent_name(mut self, name: &str) -> Self {
        self.agent_type = AgentType::with_name(name);
        self
    }

    pub fn agent_type(&self) -> &AgentType {
        &self.agent_type
    }

    /// Run the agent subprocess, recording all events. Returns (exit_code, detected_model).
    pub fn run(&self, trace_writer: &mut TraceWriter) -> Result<(i32, Option<String>), String> {
        if std::env::var("AGENTHALO_PROOF_GATE_SKIP").is_ok() {
            eprintln!("WARNING: Proof gate enforcement SKIPPED (AGENTHALO_PROOF_GATE_SKIP set)");
            eprintln!("Formal verification checks are disabled. Do NOT use in production.");
        } else {
            let cfg = proof_gate::load_gate_config()?;
            if cfg.enabled {
                let result = proof_gate::check_all_requirements(&cfg);
                if !result.passed {
                    let stale = if result.stale_certificates.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " stale_certificates={}",
                            result.stale_certificates.join(",")
                        )
                    };
                    return Err(format!(
                        "Proof gate failed: {}/{} requirements met.{} Run `agenthalo proof-gate status` for details.",
                        result.total_met, result.total_checked, stale
                    ));
                }
            }
        }

        let mut full_args = injection_flags(&self.agent_type, &self.args);
        full_args.extend(self.args.clone());

        trace_writer.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::BashCommand,
            content: serde_json::json!({
                "command": self.command,
                "args": full_args,
            }),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })?;

        let mut cmd = Command::new(&self.command);
        cmd.args(&full_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("spawn '{} {}': {e}", self.command, full_args.join(" ")))?;

        // Store child PID for signal forwarding.
        let child_pid = Arc::new(AtomicU32::new(child.id()));
        install_signal_forwarder(Arc::clone(&child_pid));

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to capture child stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "failed to capture child stderr".to_string())?;

        let agent = self.agent_type.clone();
        let out_handle = std::thread::spawn(move || -> (Vec<TraceEvent>, Option<String>) {
            let mut events = Vec::new();
            let mut adapter = make_adapter(&agent);
            let rdr = BufReader::new(stdout);
            for line in rdr.lines() {
                let Ok(line) = line else {
                    continue;
                };
                println!("{line}");
                if let Some(ev) = adapter.parse_line(&line) {
                    events.push(ev);
                }
            }
            events.extend(adapter.finalize());
            let model = adapter.detected_model().map(|s| s.to_string());
            (events, model)
        });

        let err_handle = std::thread::spawn(move || -> Vec<TraceEvent> {
            let mut events = Vec::new();
            let rdr = BufReader::new(stderr);
            for line in rdr.lines() {
                let Ok(line) = line else {
                    continue;
                };
                eprintln!("{line}");
                events.push(TraceEvent {
                    seq: 0,
                    timestamp: now_unix_secs(),
                    event_type: EventType::Error,
                    content: serde_json::json!({ "stderr": line }),
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    tool_name: None,
                    tool_input: None,
                    tool_output: None,
                    file_path: None,
                    content_hash: String::new(),
                });
            }
            events
        });

        let status = child
            .wait()
            .map_err(|e| format!("wait child process: {e}"))?;
        let (mut events, detected_model) = out_handle
            .join()
            .map_err(|_| "join stdout reader thread".to_string())?;
        events.extend(
            err_handle
                .join()
                .map_err(|_| "join stderr reader thread".to_string())?,
        );

        events.sort_by_key(|e| (e.timestamp, e.seq));
        for ev in events {
            trace_writer.write_event(ev)?;
        }

        Ok((status.code().unwrap_or(1), detected_model))
    }
}

fn make_adapter(agent: &AgentType) -> Box<dyn StreamAdapter> {
    match agent {
        AgentType::Claude => Box::new(ClaudeAdapter::new()),
        AgentType::Codex => Box::new(CodexAdapter::new()),
        AgentType::Gemini => Box::new(GeminiAdapter::new()),
        AgentType::Generic(name) => Box::new(GenericAdapter::new(name.clone())),
    }
}

/// Install a signal handler that forwards SIGINT/SIGTERM to the child process.
/// On non-Unix platforms this is a no-op.
fn install_signal_forwarder(child_pid: Arc<AtomicU32>) {
    #[cfg(unix)]
    {
        use std::thread;
        thread::spawn(move || {
            // Block until SIGINT or SIGTERM arrives, then forward to child.
            let mut signals = signal_hook::iterator::Signals::new([
                signal_hook::consts::SIGINT,
                signal_hook::consts::SIGTERM,
            ])
            .expect("register signal handlers");
            for sig in signals.forever() {
                let pid = child_pid.load(Ordering::Relaxed);
                if pid != 0 {
                    unsafe {
                        libc::kill(pid as libc::pid_t, sig);
                    }
                }
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = child_pid;
    }
}
