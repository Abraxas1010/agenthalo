use crate::cockpit::pty_manager::PtyManager;
use crate::container::{AgentHookupKind, ContainerAgentLock};
use crate::dashboard::mcp_bridge;
use crate::halo::config;
use crate::halo::http_client;
use crate::halo::local_models::{self, LocalBackendType, ServeRequest};
use crate::halo::pricing;
use crate::halo::proxy::{ChatCompletionRequest, Message};
use crate::halo::schema::{EventType, SessionMetadata, SessionStatus, TraceEvent};
use crate::halo::trace::{now_unix_secs, TraceWriter};
use crate::halo::vault::Vault;
use crate::orchestrator::agent_pool::{AgentPool, LaunchSpec};
use crate::orchestrator::trace_bridge::collect_task_output;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tempfile::TempDir;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct ToolCallRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub output: Option<Value>,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct AgentResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallRecord>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct UsageSummary {
    prompt_tokens: u64,
    completion_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHealth {
    pub ready: bool,
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
}

#[async_trait]
pub trait AgentHookup: Send + Sync {
    fn name(&self) -> &str;
    fn kind(&self) -> AgentHookupKind;
    async fn start(&self, lock: &mut ContainerAgentLock) -> Result<String, String>;
    async fn send_prompt(&self, prompt: &str) -> Result<AgentResponse, String>;
    async fn stop(&self) -> Result<(), String>;
    async fn health(&self) -> AgentHealth;
}

#[derive(Clone, Debug, Default)]
struct HookupRuntimeState {
    agent_id: Option<String>,
    container_id: Option<String>,
    trace_session_id: Option<String>,
    last_trace_session_id: Option<String>,
    started_at_unix: Option<u64>,
    total_cost_usd: f64,
    total_input_tokens: u64,
    total_output_tokens: u64,
    active: bool,
}

struct HookupTrace {
    trace_db_path: PathBuf,
    lock_path: PathBuf,
    writer: Mutex<TraceWriter>,
    runtime: Mutex<HookupRuntimeState>,
}

struct McpInjection {
    _config_dir: TempDir,
    working_dir: PathBuf,
    endpoint: String,
    secret: String,
}

impl McpInjection {
    fn prepare() -> Result<Self, String> {
        let (endpoint, secret) = mcp_bridge::running_session_endpoint()?;
        let config_dir = tempfile::tempdir().map_err(|e| format!("create MCP config dir: {e}"))?;
        let working_dir = config_dir.path().to_path_buf();

        // Resolve the stdio bridge script path (lives in the repo's scripts/ dir)
        let bridge_script = Self::resolve_bridge_script()?;

        // Use stdio bridge for all agents — it auto-discovers the running
        // MCP server's port and secret from /proc/<pid>/environ, so no
        // hardcoded credentials leak into config files.
        let claude_config = json!({
            "mcpServers": {
                "agenthalo": {
                    "command": "python3",
                    "args": [bridge_script],
                    "env": {
                        "AGENTHALO_MCP_PORT": Self::extract_port(&endpoint),
                        "AGENTHALO_MCP_SECRET": secret,
                        "AGENTHALO_MCP_BRIDGE_AUTO_START": "0"
                    }
                }
            }
        });
        std::fs::write(
            working_dir.join(".mcp.json"),
            serde_json::to_vec_pretty(&claude_config)
                .map_err(|e| format!("serialize Claude MCP config: {e}"))?,
        )
        .map_err(|e| format!("write Claude MCP config: {e}"))?;

        let codex_dir = working_dir.join(".codex");
        std::fs::create_dir_all(&codex_dir)
            .map_err(|e| format!("create Codex MCP config dir {}: {e}", codex_dir.display()))?;
        let port = Self::extract_port(&endpoint);
        std::fs::write(
            codex_dir.join("config.toml"),
            format!(
                "[[mcp_servers]]\n\
name = \"agenthalo\"\n\
command = \"python3\"\n\
args = [\"{bridge_script}\"]\n\
\n\
[mcp_servers.env]\n\
AGENTHALO_MCP_PORT = \"{port}\"\n\
AGENTHALO_MCP_SECRET = \"{secret}\"\n\
AGENTHALO_MCP_BRIDGE_AUTO_START = \"0\"\n"
            ),
        )
        .map_err(|e| format!("write Codex MCP config: {e}"))?;

        let gemini_dir = working_dir.join(".gemini");
        std::fs::create_dir_all(&gemini_dir)
            .map_err(|e| format!("create Gemini MCP config dir {}: {e}", gemini_dir.display()))?;
        let gemini_config = json!({
            "mcpServers": {
                "agenthalo": {
                    "command": "python3",
                    "args": [bridge_script],
                    "env": {
                        "AGENTHALO_MCP_PORT": port,
                        "AGENTHALO_MCP_SECRET": secret,
                        "AGENTHALO_MCP_BRIDGE_AUTO_START": "0"
                    }
                }
            }
        });
        std::fs::write(
            gemini_dir.join("settings.json"),
            serde_json::to_vec_pretty(&gemini_config)
                .map_err(|e| format!("serialize Gemini MCP config: {e}"))?,
        )
        .map_err(|e| format!("write Gemini MCP config: {e}"))?;

        Ok(Self {
            _config_dir: config_dir,
            working_dir,
            endpoint,
            secret,
        })
    }

    fn resolve_bridge_script() -> Result<String, String> {
        // Look for the bridge script in common locations
        let candidates = [
            // Relative to binary
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("../../scripts/mcp_stdio_bridge.py"))),
            // AGENTHALO_HOME
            std::env::var("AGENTHALO_HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join("scripts/mcp_stdio_bridge.py")),
            // Common dev location
            Some(std::path::PathBuf::from(
                "/home/abraxas/Work/agenthalo/scripts/mcp_stdio_bridge.py",
            )),
        ];
        for candidate in candidates.iter().flatten() {
            let resolved = candidate
                .canonicalize()
                .unwrap_or_else(|_| candidate.clone());
            if resolved.exists() {
                return Ok(resolved.display().to_string());
            }
        }
        Err("mcp_stdio_bridge.py not found".to_string())
    }

    fn extract_port(endpoint: &str) -> String {
        // Extract port from "http://127.0.0.1:PORT" URL
        endpoint
            .rsplit(':')
            .next()
            .unwrap_or("8390")
            .trim_end_matches('/')
            .to_string()
    }

    fn env_vars(&self) -> [(String, String); 4] {
        let rpc_endpoint = format!("{}/mcp", self.endpoint.trim_end_matches('/'));
        [
            ("AGENTHALO_MCP_ENDPOINT".to_string(), rpc_endpoint.clone()),
            (
                "AGENTHALO_ORCHESTRATOR_MCP_ENDPOINT".to_string(),
                rpc_endpoint,
            ),
            ("AGENTHALO_MCP_SECRET".to_string(), self.secret.clone()),
            (
                "AGENTHALO_ORCHESTRATOR_PROXY_VIA_MCP".to_string(),
                "1".to_string(),
            ),
        ]
    }
}

impl HookupTrace {
    fn new(trace_db_path: &Path) -> Result<Self, String> {
        Ok(Self {
            trace_db_path: trace_db_path.to_path_buf(),
            lock_path: config::agent_lock_path(),
            writer: Mutex::new(TraceWriter::new(trace_db_path)?),
            runtime: Mutex::new(HookupRuntimeState::default()),
        })
    }

    fn runtime(&self) -> HookupRuntimeState {
        self.runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn trace_session_id(&self) -> Option<String> {
        let runtime = self.runtime();
        runtime.trace_session_id.or(runtime.last_trace_session_id)
    }

    fn trace_db_path(&self) -> &Path {
        &self.trace_db_path
    }

    fn activate(
        &self,
        lock: &mut ContainerAgentLock,
        agent_label: &str,
        kind: AgentHookupKind,
        agent_id: String,
        model: Option<String>,
    ) -> Result<String, String> {
        if self.runtime().active {
            return self
                .runtime()
                .agent_id
                .ok_or_else(|| "hookup runtime is active without an agent id".to_string());
        }

        let trace_session_id = format!(
            "container-{}-{}-{}",
            sanitize_label(agent_label),
            now_unix_secs(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let started_at = now_unix_secs();
        {
            let mut writer = self
                .writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            writer.start_session(SessionMetadata {
                session_id: trace_session_id.clone(),
                agent: agent_label.to_string(),
                model: model.clone(),
                started_at,
                ended_at: None,
                prompt: None,
                status: SessionStatus::Running,
                user_id: None,
                machine_id: None,
                puf_digest: None,
            })?;
        }

        lock.initialize(kind.clone(), agent_id.clone())?;
        lock.attach_trace_session(Some(trace_session_id.clone()))?;
        lock.save_to_path(&self.lock_path)?;

        self.write_initialized(&kind, &agent_id, &lock.container_id, model.as_deref())?;

        let mut runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        runtime.agent_id = Some(agent_id.clone());
        runtime.container_id = Some(lock.container_id.clone());
        runtime.trace_session_id = Some(trace_session_id);
        runtime.last_trace_session_id = runtime.trace_session_id.clone();
        runtime.started_at_unix = Some(started_at);
        runtime.total_cost_usd = 0.0;
        runtime.total_input_tokens = 0;
        runtime.total_output_tokens = 0;
        runtime.active = true;
        Ok(agent_id)
    }

    fn write_prompt_sent(&self, prompt: &str) -> Result<(), String> {
        let token_count = estimate_tokens(prompt);
        self.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::PromptSent,
            content: json!({
                "prompt_text": prompt,
                "token_count": token_count,
                "timestamp": now_unix_secs(),
            }),
            input_tokens: Some(token_count),
            output_tokens: None,
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
    }

    fn write_response_received(&self, response: &AgentResponse) -> Result<(), String> {
        {
            let mut runtime = self
                .runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.total_cost_usd += response.cost_usd;
            runtime.total_input_tokens = runtime
                .total_input_tokens
                .saturating_add(response.input_tokens);
            runtime.total_output_tokens = runtime
                .total_output_tokens
                .saturating_add(response.output_tokens);
        }

        self.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::ResponseReceived,
            content: json!({
                "response_text": response.content,
                "model": response.model,
                "tokens_in": response.input_tokens,
                "tokens_out": response.output_tokens,
                "cost_usd": response.cost_usd,
                "duration_ms": response.duration_ms,
            }),
            input_tokens: Some(response.input_tokens),
            output_tokens: Some(response.output_tokens),
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })?;

        for tool_call in &response.tool_calls {
            self.write_event(TraceEvent {
                seq: 0,
                timestamp: now_unix_secs(),
                event_type: EventType::ToolCall,
                content: json!({
                    "tool_name": tool_call.name,
                    "tool_input": tool_call.input,
                    "tool_output": tool_call.output,
                    "duration_ms": tool_call.duration_ms,
                }),
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                tool_name: Some(tool_call.name.clone()),
                tool_input: Some(tool_call.input.clone()),
                tool_output: tool_call.output.clone(),
                file_path: None,
                content_hash: String::new(),
            })?;
        }
        Ok(())
    }

    fn write_error(
        &self,
        error_type: &str,
        message: &str,
        recoverable: bool,
    ) -> Result<(), String> {
        self.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::Error,
            content: json!({
                "error_type": error_type,
                "message": message,
                "recoverable": recoverable,
            }),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
    }

    fn deactivate(&self, reason: &str) -> Result<(), String> {
        let runtime = self.runtime();
        let mut lock = ContainerAgentLock::load_or_create_at(
            &self.lock_path,
            runtime
                .container_id
                .as_deref()
                .unwrap_or(&crate::container::current_container_id()),
        )?;
        lock.begin_deinitialize()?;
        lock.save_to_path(&self.lock_path)?;

        let duration_secs = runtime
            .started_at_unix
            .map(|started| now_unix_secs().saturating_sub(started))
            .unwrap_or(0);
        self.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::AgentDeinitialized,
            content: json!({
                "reason": reason,
                "total_cost_usd": runtime.total_cost_usd,
                "total_tokens": runtime.total_input_tokens.saturating_add(runtime.total_output_tokens),
                "session_duration_s": duration_secs,
            }),
            input_tokens: Some(runtime.total_input_tokens),
            output_tokens: Some(runtime.total_output_tokens),
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })?;

        {
            let mut writer = self
                .writer
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = writer.end_session(SessionStatus::Completed)?;
        }

        lock.complete_deinitialize()?;
        lock.save_to_path(&self.lock_path)?;

        let mut runtime_state = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let last_trace_session_id = runtime_state
            .trace_session_id
            .clone()
            .or(runtime_state.last_trace_session_id.clone());
        *runtime_state = HookupRuntimeState {
            last_trace_session_id,
            ..HookupRuntimeState::default()
        };
        Ok(())
    }

    fn write_initialized(
        &self,
        kind: &AgentHookupKind,
        agent_id: &str,
        container_id: &str,
        model: Option<&str>,
    ) -> Result<(), String> {
        self.write_event(TraceEvent {
            seq: 0,
            timestamp: now_unix_secs(),
            event_type: EventType::AgentInitialized,
            content: json!({
                "kind": serde_json::to_value(kind).unwrap_or(Value::Null),
                "model": model,
                "agent_id": agent_id,
                "container_id": container_id,
            }),
            input_tokens: None,
            output_tokens: None,
            cache_read_tokens: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            file_path: None,
            content_hash: String::new(),
        })
    }

    fn write_event(&self, event: TraceEvent) -> Result<(), String> {
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        writer.write_event(event)
    }
}

pub struct CliAgentHookup {
    cli_name: String,
    model: Option<String>,
    agent_pool: Arc<AgentPool>,
    trace: Arc<HookupTrace>,
    timeout_secs: u64,
    mcp: Mutex<Option<McpInjection>>,
}

impl CliAgentHookup {
    pub fn new(
        cli_name: impl Into<String>,
        pty_manager: Arc<PtyManager>,
        model: Option<String>,
    ) -> Result<Self, String> {
        Self::with_trace_path(cli_name, pty_manager, model, &config::db_path())
    }

    pub fn with_trace_path(
        cli_name: impl Into<String>,
        pty_manager: Arc<PtyManager>,
        model: Option<String>,
        trace_db_path: &Path,
    ) -> Result<Self, String> {
        Ok(Self {
            cli_name: cli_name.into(),
            model,
            agent_pool: Arc::new(AgentPool::new(pty_manager, None)),
            trace: Arc::new(HookupTrace::new(trace_db_path)?),
            timeout_secs: 120,
            mcp: Mutex::new(None),
        })
    }

    pub fn trace_session_id(&self) -> Option<String> {
        self.trace.trace_session_id()
    }

    pub fn trace_db_path(&self) -> &Path {
        self.trace.trace_db_path()
    }

    #[cfg(test)]
    fn mcp_working_dir(&self) -> Option<PathBuf> {
        self.mcp
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|mcp| mcp.working_dir.clone())
    }
}

#[async_trait]
impl AgentHookup for CliAgentHookup {
    fn name(&self) -> &str {
        &self.cli_name
    }

    fn kind(&self) -> AgentHookupKind {
        AgentHookupKind::Cli {
            cli_name: self.cli_name.clone(),
        }
    }

    async fn start(&self, lock: &mut ContainerAgentLock) -> Result<String, String> {
        if !lock.can_initialize() {
            return Err(format!("container `{}` is not empty", lock.container_id));
        }
        let mcp = McpInjection::prepare()?;
        let mut env = std::collections::BTreeMap::new();
        for (key, value) in mcp.env_vars() {
            env.insert(key, value);
        }
        let managed = self
            .agent_pool
            .launch(LaunchSpec {
                agent: self.cli_name.clone(),
                agent_name: format!("{}-container-hookup", self.cli_name),
                working_dir: Some(mcp.working_dir.display().to_string()),
                env,
                timeout_secs: self.timeout_secs,
                model: self.model.clone(),
                trace: false,
                capabilities: vec!["*".to_string()],
            })
            .await?;
        *self
            .mcp
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(mcp);
        self.trace.activate(
            lock,
            &format!("{}-cli", self.cli_name),
            self.kind(),
            managed.agent_id,
            self.model.clone(),
        )
    }

    async fn send_prompt(&self, prompt: &str) -> Result<AgentResponse, String> {
        self.trace.write_prompt_sent(prompt)?;
        let started = Instant::now();
        let runtime = self.trace.runtime();
        let agent_id = runtime
            .agent_id
            .ok_or_else(|| "CLI hookup not started".to_string())?;
        let task_id = format!(
            "task-{}-{}",
            now_unix_secs(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let execution = match self
            .agent_pool
            .start_task(&agent_id, &task_id, prompt, Some(self.timeout_secs))
            .await
        {
            Ok(execution) => execution,
            Err(error) => {
                self.trace.write_error("task_start", &error, true)?;
                return Err(error);
            }
        };
        let session = self
            .agent_pool
            .pty_session_by_id(&execution.session_id)
            .ok_or_else(|| format!("missing PTY session {}", execution.session_id))?;
        let outcome = collect_task_output(
            session,
            &self.cli_name,
            self.trace.trace_db_path(),
            runtime
                .trace_session_id
                .as_deref()
                .unwrap_or("container-hookup-trace"),
            prompt,
            false,
            execution.timeout_secs,
        )
        .await;
        let _ = self.agent_pool.destroy_pty_session(&execution.session_id);
        let mut cost = 0.0;
        let result = match outcome {
            Ok(outcome) => {
                cost = outcome.estimated_cost_usd;
                self.agent_pool.complete_task(&agent_id, cost).await;
                if outcome.exit_code != 0 {
                    let message = outcome.output.trim().to_string();
                    let message = if message.is_empty() {
                        format!("CLI agent exited with {}", outcome.exit_code)
                    } else {
                        message
                    };
                    self.trace.write_error("agent_exit", &message, false)?;
                    Err(message)
                } else {
                    let file_answer = execution.answer_path.as_ref().and_then(|path| {
                        std::fs::read_to_string(path).ok().and_then(|text| {
                            let trimmed = text.trim();
                            if trimmed.is_empty() {
                                None
                            } else {
                                Some(trimmed.to_string())
                            }
                        })
                    });
                    Ok(AgentResponse {
                        content: file_answer
                            .or_else(|| outcome.answer.filter(|value| !value.trim().is_empty()))
                            .unwrap_or_else(|| outcome.output.trim().to_string()),
                        model: self.model.clone().unwrap_or_else(|| self.cli_name.clone()),
                        input_tokens: outcome.input_tokens,
                        output_tokens: outcome.output_tokens,
                        cost_usd: cost,
                        tool_calls: Vec::new(),
                        duration_ms: started.elapsed().as_millis() as u64,
                    })
                }
            }
            Err(error) => {
                self.agent_pool.complete_task(&agent_id, cost).await;
                self.trace.write_error("task_execution", &error, true)?;
                Err(error)
            }
        };
        if let Some(path) = execution.answer_path.as_ref() {
            let _ = std::fs::remove_file(path);
        }
        let result = result?;
        self.trace.write_response_received(&result)?;
        Ok(result)
    }

    async fn stop(&self) -> Result<(), String> {
        if let Some(agent_id) = self.trace.runtime().agent_id {
            let _ = self.agent_pool.stop(&agent_id, true).await;
        }
        let result = self.trace.deactivate("shutdown");
        self.mcp
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        result
    }

    async fn health(&self) -> AgentHealth {
        AgentHealth {
            ready: which_path(&self.cli_name).is_some() || self.cli_name == "shell",
            status: if self.trace.runtime().active {
                "running".to_string()
            } else {
                "ready".to_string()
            },
            detail: None,
            agent_id: self.trace.runtime().agent_id,
            model: self.model.clone(),
        }
    }
}

pub struct ApiAgentHookup {
    provider: String,
    model: String,
    api_key_source: String,
    base_url_override: Option<String>,
    trace: Arc<HookupTrace>,
}

impl ApiAgentHookup {
    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        api_key_source: impl Into<String>,
    ) -> Result<Self, String> {
        Self::with_base_url(provider, model, api_key_source, None, &config::db_path())
    }

    pub fn with_base_url(
        provider: impl Into<String>,
        model: impl Into<String>,
        api_key_source: impl Into<String>,
        base_url_override: Option<String>,
        trace_db_path: &Path,
    ) -> Result<Self, String> {
        Ok(Self {
            provider: provider.into(),
            model: model.into(),
            api_key_source: api_key_source.into(),
            base_url_override,
            trace: Arc::new(HookupTrace::new(trace_db_path)?),
        })
    }

    pub fn trace_session_id(&self) -> Option<String> {
        self.trace.trace_session_id()
    }

    pub fn trace_db_path(&self) -> &Path {
        self.trace.trace_db_path()
    }
}

#[async_trait]
impl AgentHookup for ApiAgentHookup {
    fn name(&self) -> &str {
        &self.provider
    }

    fn kind(&self) -> AgentHookupKind {
        AgentHookupKind::Api {
            provider: self.provider.clone(),
        }
    }

    async fn start(&self, lock: &mut ContainerAgentLock) -> Result<String, String> {
        if !lock.can_initialize() {
            return Err(format!("container `{}` is not empty", lock.container_id));
        }
        let agent_id = format!(
            "api-{}-{}",
            sanitize_label(&self.provider),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        self.trace.activate(
            lock,
            &format!("{}-api", self.provider),
            self.kind(),
            agent_id,
            Some(self.model.clone()),
        )
    }

    async fn send_prompt(&self, prompt: &str) -> Result<AgentResponse, String> {
        self.trace.write_prompt_sent(prompt)?;
        let started = Instant::now();
        let endpoint = api_endpoint(&self.provider, self.base_url_override.as_deref())?;
        let headers = api_headers(
            &self.provider,
            &self.api_key_source,
            self.base_url_override.is_some(),
        )?;
        let response = match complete_with_optional_tools(
            &endpoint,
            &headers,
            &self.model,
            prompt,
            started,
            true,
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                self.trace.write_error("api_request", &error, true)?;
                return Err(error);
            }
        };
        self.trace.write_response_received(&response)?;
        Ok(response)
    }

    async fn stop(&self) -> Result<(), String> {
        self.trace.deactivate("shutdown")
    }

    async fn health(&self) -> AgentHealth {
        let detail = if self.base_url_override.is_none() && self.provider != "openrouter" {
            Some("only OpenRouter is supported without a direct base URL override".to_string())
        } else {
            None
        };
        AgentHealth {
            ready: detail.is_none()
                && api_headers(
                    &self.provider,
                    &self.api_key_source,
                    self.base_url_override.is_some(),
                )
                .is_ok(),
            status: if self.trace.runtime().active {
                "running".to_string()
            } else {
                "ready".to_string()
            },
            detail,
            agent_id: self.trace.runtime().agent_id,
            model: Some(self.model.clone()),
        }
    }
}

pub struct LocalModelHookup {
    model_id: String,
    vllm_port: u16,
    base_url_override: Option<String>,
    trace: Arc<HookupTrace>,
    runtime: Mutex<LocalRuntime>,
}

#[derive(Clone, Debug, Default)]
struct LocalRuntime {
    pid: Option<u32>,
    base_url: Option<String>,
    owns_backend: bool,
}

impl LocalModelHookup {
    pub fn new(model_id: impl Into<String>, vllm_port: u16) -> Result<Self, String> {
        Self::with_base_url(model_id, vllm_port, None, &config::db_path())
    }

    pub fn with_base_url(
        model_id: impl Into<String>,
        vllm_port: u16,
        base_url_override: Option<String>,
        trace_db_path: &Path,
    ) -> Result<Self, String> {
        Ok(Self {
            model_id: model_id.into(),
            vllm_port,
            base_url_override,
            trace: Arc::new(HookupTrace::new(trace_db_path)?),
            runtime: Mutex::new(LocalRuntime::default()),
        })
    }

    pub fn trace_session_id(&self) -> Option<String> {
        self.trace.trace_session_id()
    }

    pub fn trace_db_path(&self) -> &Path {
        self.trace.trace_db_path()
    }
}

#[async_trait]
impl AgentHookup for LocalModelHookup {
    fn name(&self) -> &str {
        "vllm-local"
    }

    fn kind(&self) -> AgentHookupKind {
        AgentHookupKind::LocalModel {
            model_id: self.model_id.clone(),
        }
    }

    async fn start(&self, lock: &mut ContainerAgentLock) -> Result<String, String> {
        if !lock.can_initialize() {
            return Err(format!("container `{}` is not empty", lock.container_id));
        }
        let mut local_runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.base_url_override.is_none() && local_models::detect_gpu().is_none() {
            return Err("vLLM local model backend requires a detected GPU".to_string());
        }
        if let Some(base_url) = &self.base_url_override {
            local_runtime.base_url = Some(base_url.trim_end_matches('/').to_string());
        } else {
            let serve = local_models::serve_backend(ServeRequest {
                backend: LocalBackendType::Vllm,
                port: Some(self.vllm_port),
                model: Some(self.model_id.clone()),
            })?;
            local_runtime.pid = serve.pid;
            local_runtime.base_url = Some(serve.base_url);
            local_runtime.owns_backend = !serve.already_running;
        }
        drop(local_runtime);
        let agent_id = format!(
            "local-{}-{}",
            sanitize_label(&self.model_id),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        self.trace.activate(
            lock,
            "vllm-local",
            self.kind(),
            agent_id,
            Some(self.model_id.clone()),
        )
    }

    async fn send_prompt(&self, prompt: &str) -> Result<AgentResponse, String> {
        self.trace.write_prompt_sent(prompt)?;
        let started = Instant::now();
        let base_url = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .base_url
            .clone()
            .ok_or_else(|| "local model hookup not started".to_string())?;
        let request = ChatCompletionRequest {
            model: self.model_id.clone(),
            messages: vec![user_message(prompt)],
            temperature: None,
            max_tokens: None,
            stream: Some(false),
            top_p: None,
            tools: None,
        };
        let endpoint = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
        let body = match openai_chat_completion(&endpoint, &[], &request) {
            Ok(body) => body,
            Err(error) => {
                self.trace
                    .write_error("local_model_request", &error, true)?;
                return Err(error);
            }
        };
        let response = response_from_body(
            &body,
            &self.model_id,
            started.elapsed().as_millis() as u64,
            0.0,
        );
        self.trace.write_response_received(&response)?;
        Ok(response)
    }

    async fn stop(&self) -> Result<(), String> {
        let runtime = self
            .runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if runtime.owns_backend && self.base_url_override.is_none() {
            let _ = local_models::stop_backend(LocalBackendType::Vllm);
        }
        self.trace.deactivate("shutdown")
    }

    async fn health(&self) -> AgentHealth {
        let status = local_models::detect_status();
        AgentHealth {
            ready: if self.base_url_override.is_some() {
                true
            } else {
                status.backend.healthy
            },
            status: if self.trace.runtime().active {
                "running".to_string()
            } else if status.backend.healthy {
                "ready".to_string()
            } else {
                "unhealthy".to_string()
            },
            detail: status.backend.error,
            agent_id: self.trace.runtime().agent_id,
            model: Some(self.model_id.clone()),
        }
    }
}

fn sanitize_label(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn estimate_tokens(text: &str) -> u64 {
    ((text.trim().len() as u64) / 4).max(1)
}

fn which_path(binary: &str) -> Option<String> {
    let output = std::process::Command::new("which")
        .arg(binary)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn api_endpoint(provider: &str, base_url_override: Option<&str>) -> Result<String, String> {
    if let Some(base_url) = base_url_override {
        return Ok(format!(
            "{}/v1/chat/completions",
            base_url.trim_end_matches('/')
        ));
    }
    match provider.trim().to_ascii_lowercase().as_str() {
        "openrouter" => Ok("https://openrouter.ai/api/v1/chat/completions".to_string()),
        other => Err(format!(
            "provider `{other}` requires a direct base URL override in Phase 2"
        )),
    }
}

fn api_headers(
    provider: &str,
    api_key_source: &str,
    allow_without_auth: bool,
) -> Result<Vec<(String, String)>, String> {
    if allow_without_auth {
        return Ok(Vec::new());
    }
    let token = if let Some(provider_name) = api_key_source.strip_prefix("vault:") {
        open_vault()?
            .ok_or_else(|| "vault unavailable".to_string())?
            .get_key(provider_name)?
    } else {
        api_key_source.to_string()
    };
    let mut headers = vec![("Authorization".to_string(), format!("Bearer {token}"))];
    if provider.eq_ignore_ascii_case("openrouter") {
        headers.push((
            "HTTP-Referer".to_string(),
            "https://agenthalo.local".to_string(),
        ));
        headers.push((
            "X-Title".to_string(),
            "AgentHALO Container Hookup".to_string(),
        ));
    }
    Ok(headers)
}

fn open_vault() -> Result<Option<Vault>, String> {
    let wallet_path = config::pq_wallet_path();
    let vault_path = config::vault_path();
    if !wallet_path.exists() || !vault_path.exists() {
        return Ok(None);
    }
    Vault::open(&wallet_path, &vault_path)
        .map(Some)
        .map_err(|e| format!("open vault: {e}"))
}

fn openai_chat_completion(
    url: &str,
    headers: &[(String, String)],
    request: &ChatCompletionRequest,
) -> Result<Value, String> {
    let mut builder = http_client::post_with_timeout(url, std::time::Duration::from_secs(30))?;
    for (name, value) in headers {
        builder = builder.header(name, value);
    }
    builder = builder.header("Content-Type", "application/json");
    let body = serde_json::to_vec(request)
        .map_err(|e| format!("serialize chat completion request: {e}"))?;
    let mut response = builder.send(body).map_err(|e| format!("POST {url}: {e}"))?;
    response
        .body_mut()
        .read_json::<Value>()
        .map_err(|e| format!("parse completion response from {url}: {e}"))
}

async fn complete_with_optional_tools(
    endpoint: &str,
    headers: &[(String, String)],
    model: &str,
    prompt: &str,
    started: Instant,
    enable_tools: bool,
) -> Result<AgentResponse, String> {
    let tools = if enable_tools {
        Some(api_tool_definitions().await?)
    } else {
        None
    };
    let mut messages = vec![user_message(prompt)];
    let mut total_input_tokens = 0u64;
    let mut total_output_tokens = 0u64;
    let mut total_cost_usd = 0.0;
    let mut all_tool_calls = Vec::new();
    let mut rounds = 0usize;

    loop {
        rounds += 1;
        if rounds > 10 {
            return Err("tool calling loop exceeded max rounds".to_string());
        }

        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages: messages.clone(),
            temperature: None,
            max_tokens: None,
            stream: Some(false),
            top_p: None,
            tools: tools.clone().filter(|items| !items.is_empty()),
        };
        let body = openai_chat_completion(endpoint, headers, &request)?;
        let content = extract_content(&body);
        let usage = usage_from_body(&body, model, &content);
        total_input_tokens = total_input_tokens.saturating_add(usage.prompt_tokens);
        total_output_tokens = total_output_tokens.saturating_add(usage.completion_tokens);
        total_cost_usd += api_cost_usd(model, &body);

        let tool_calls = extract_tool_calls(&body);
        if tool_calls.is_empty() {
            return Ok(AgentResponse {
                content,
                model: body
                    .get("model")
                    .and_then(|value| value.as_str())
                    .unwrap_or(model)
                    .to_string(),
                input_tokens: total_input_tokens,
                output_tokens: total_output_tokens,
                cost_usd: total_cost_usd,
                tool_calls: all_tool_calls,
                duration_ms: started.elapsed().as_millis() as u64,
            });
        }

        messages.push(assistant_tool_call_message(&body, &tool_calls));
        for tool_call in tool_calls {
            let tool_started = Instant::now();
            let output = execute_tool_call(&tool_call).await?;
            let duration_ms = tool_started.elapsed().as_millis() as u64;
            messages.push(tool_result_message(tool_call.call_id.as_deref(), &output));
            all_tool_calls.push(ToolCallRecord {
                output: Some(output),
                duration_ms,
                ..tool_call
            });
        }
    }
}

async fn api_tool_definitions() -> Result<Vec<Value>, String> {
    let tools = mcp_bridge::tool_catalog().await?;
    Ok(tools
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description.unwrap_or_default(),
                    "parameters": tool.input_schema,
                }
            })
        })
        .collect())
}

async fn execute_tool_call(tool_call: &ToolCallRecord) -> Result<Value, String> {
    let args = normalize_tool_arguments(&tool_call.input)?;
    let result = mcp_bridge::invoke_tool(&tool_call.name, args).await?;
    Ok(json!({
        "tool": result.tool,
        "is_error": result.is_error,
        "structured_content": result.structured_content,
        "content": result.content,
    }))
}

fn user_message(prompt: &str) -> Message {
    Message {
        role: "user".to_string(),
        content: Value::String(prompt.to_string()),
        name: None,
        tool_call_id: None,
        tool_calls: None,
    }
}

fn assistant_tool_call_message(body: &Value, tool_calls: &[ToolCallRecord]) -> Message {
    let raw_tool_calls = body
        .get("choices")
        .and_then(|value| value.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .cloned()
        .unwrap_or_else(|| {
            Value::Array(
                tool_calls
                    .iter()
                    .map(|tool_call| {
                        json!({
                            "id": tool_call.call_id,
                            "type": "function",
                            "function": {
                                "name": tool_call.name,
                                "arguments": tool_call.input,
                            }
                        })
                    })
                    .collect(),
            )
        });
    Message {
        role: "assistant".to_string(),
        content: body
            .get("choices")
            .and_then(|value| value.as_array())
            .and_then(|choices| choices.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|message| message.get("content"))
            .cloned()
            .unwrap_or(Value::Null),
        name: None,
        tool_call_id: None,
        tool_calls: Some(raw_tool_calls),
    }
}

fn tool_result_message(call_id: Option<&str>, result: &Value) -> Message {
    Message {
        role: "tool".to_string(),
        content: Value::String(result.to_string()),
        name: None,
        tool_call_id: call_id.map(str::to_string),
        tool_calls: None,
    }
}

fn normalize_tool_arguments(value: &Value) -> Result<Value, String> {
    match value {
        Value::Null => Ok(json!({})),
        Value::Object(_) => Ok(value.clone()),
        Value::String(raw) => {
            if raw.trim().is_empty() {
                Ok(json!({}))
            } else {
                serde_json::from_str::<Value>(raw)
                    .map_err(|e| format!("parse tool arguments for MCP execution: {e}"))
            }
        }
        other => Err(format!(
            "tool arguments must be an object or JSON string, got {}",
            other
        )),
    }
}

fn response_from_body(
    body: &Value,
    default_model: &str,
    duration_ms: u64,
    cost_usd: f64,
) -> AgentResponse {
    let content = extract_content(body);
    let tool_calls = extract_tool_calls(body);
    let usage = usage_from_body(body, default_model, &content);
    AgentResponse {
        content,
        model: body
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or(default_model)
            .to_string(),
        input_tokens: usage.prompt_tokens,
        output_tokens: usage.completion_tokens,
        cost_usd,
        tool_calls,
        duration_ms,
    }
}

fn usage_from_body(body: &Value, default_model: &str, content: &str) -> UsageSummary {
    UsageSummary {
        prompt_tokens: body
            .get("usage")
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(|value| value.as_u64())
            .unwrap_or_else(|| estimate_tokens(default_model)),
        completion_tokens: body
            .get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(|value| value.as_u64())
            .unwrap_or_else(|| estimate_tokens(content)),
    }
}

fn extract_content(body: &Value) -> String {
    body.get("choices")
        .and_then(|value| value.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .map(|content| match content {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

fn extract_tool_calls(body: &Value) -> Vec<ToolCallRecord> {
    body.get("choices")
        .and_then(|value| value.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("tool_calls"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let function = item.get("function")?;
                    Some(ToolCallRecord {
                        call_id: item
                            .get("id")
                            .and_then(|value| value.as_str())
                            .map(str::to_string),
                        name: function.get("name")?.as_str()?.to_string(),
                        input: function
                            .get("arguments")
                            .map(parse_tool_argument_value)
                            .unwrap_or(Value::Null),
                        output: None,
                        duration_ms: 0,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn parse_tool_argument_value(value: &Value) -> Value {
    match value {
        Value::String(raw) => {
            serde_json::from_str::<Value>(raw).unwrap_or_else(|_| Value::String(raw.clone()))
        }
        other => other.clone(),
    }
}

fn api_cost_usd(model: &str, body: &Value) -> f64 {
    let pricing_table =
        pricing::load_or_default(&config::pricing_path()).unwrap_or_else(|_| HashMap::new());
    let normalized_model = model.split('/').next_back().unwrap_or(model);
    pricing::calculate_cost(
        normalized_model,
        body.get("usage")
            .and_then(|usage| usage.get("prompt_tokens"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        body.get("usage")
            .and_then(|usage| usage.get("completion_tokens"))
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        0,
        &pricing_table,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::halo::trace::session_events;
    use crate::test_support::{lock_env, EnvVarGuard, MockOpenAiServer};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    #[derive(Clone, Debug, Default)]
    struct MockMcpState {
        tool_calls: usize,
        last_tool: Option<String>,
        last_arguments: Option<Value>,
    }

    struct MockMcpServer {
        base_url: String,
        state: Arc<Mutex<MockMcpState>>,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl MockMcpServer {
        fn spawn() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock mcp server");
            listener.set_nonblocking(true).expect("set nonblocking");
            let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
            let state = Arc::new(Mutex::new(MockMcpState::default()));
            let state_clone = state.clone();
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_flag = shutdown.clone();
            let handle = thread::spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut buffer = [0u8; 16384];
                            let bytes = stream.read(&mut buffer).unwrap_or(0);
                            let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
                            let body = request
                                .split("\r\n\r\n")
                                .nth(1)
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let response = if request.starts_with("GET /health") {
                                json!({"ok": true})
                            } else {
                                let rpc: Value =
                                    serde_json::from_str(&body).expect("parse mock MCP rpc");
                                let method = rpc
                                    .get("method")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default();
                                let result = match method {
                                    "initialize" => json!({"protocolVersion": "2025-03-01"}),
                                    "tools/list" => json!({
                                        "tools": [{
                                            "name": "nucleusdb_help",
                                            "description": "Show NucleusDB help",
                                            "inputSchema": {
                                                "type": "object",
                                                "properties": {
                                                    "topic": { "type": "string" }
                                                }
                                            }
                                        }]
                                    }),
                                    "tools/call" => {
                                        let params =
                                            rpc.get("params").cloned().unwrap_or_else(|| json!({}));
                                        let tool_name = params
                                            .get("name")
                                            .and_then(|value| value.as_str())
                                            .unwrap_or_default()
                                            .to_string();
                                        let arguments = params
                                            .get("arguments")
                                            .cloned()
                                            .unwrap_or_else(|| json!({}));
                                        let mut state = state_clone
                                            .lock()
                                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                                        state.tool_calls += 1;
                                        state.last_tool = Some(tool_name.clone());
                                        state.last_arguments = Some(arguments.clone());
                                        json!({
                                            "isError": false,
                                            "structuredContent": {
                                                "message": format!("executed {tool_name}"),
                                                "echo": arguments,
                                            },
                                            "content": [{
                                                "type": "text",
                                                "text": format!("executed {tool_name}"),
                                            }]
                                        })
                                    }
                                    other => json!({"unsupported": other}),
                                };
                                json!({
                                    "jsonrpc": "2.0",
                                    "id": rpc.get("id").cloned().unwrap_or(json!(1)),
                                    "result": result,
                                })
                            };
                            let raw = response.to_string();
                            let http = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                raw.len(),
                                raw
                            );
                            let _ = stream.write_all(http.as_bytes());
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                base_url,
                state,
                shutdown,
                handle: Some(handle),
            }
        }

        fn state(&self) -> MockMcpState {
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl Drop for MockMcpServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            let _ = std::net::TcpStream::connect(self.base_url.trim_start_matches("http://"));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    struct ToolLoopOpenAiServer {
        base_url: String,
        requests: Arc<Mutex<Vec<Value>>>,
        shutdown: Arc<AtomicBool>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl ToolLoopOpenAiServer {
        fn spawn(model: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock openai server");
            listener.set_nonblocking(true).expect("set nonblocking");
            let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let requests_clone = requests.clone();
            let shutdown = Arc::new(AtomicBool::new(false));
            let shutdown_flag = shutdown.clone();
            let model = model.to_string();
            let handle = thread::spawn(move || {
                while !shutdown_flag.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let mut buffer = [0u8; 16384];
                            let bytes = stream.read(&mut buffer).unwrap_or(0);
                            let request = String::from_utf8_lossy(&buffer[..bytes]).to_string();
                            let body = request
                                .split("\r\n\r\n")
                                .nth(1)
                                .unwrap_or("")
                                .trim()
                                .to_string();
                            let parsed: Value =
                                serde_json::from_str(&body).expect("parse mock openai request");
                            let mut requests = requests_clone
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            requests.push(parsed);
                            let round = requests.len();
                            drop(requests);
                            let response = if round == 1 {
                                json!({
                                    "id": "chatcmpl-tool-1",
                                    "object": "chat.completion",
                                    "model": model,
                                    "choices": [{
                                        "index": 0,
                                        "message": {
                                            "role": "assistant",
                                            "content": Value::Null,
                                            "tool_calls": [{
                                                "id": "call_1",
                                                "type": "function",
                                                "function": {
                                                    "name": "nucleusdb_help",
                                                    "arguments": "{\"topic\":\"status\"}"
                                                }
                                            }]
                                        },
                                        "finish_reason": "tool_calls"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 11,
                                        "completion_tokens": 5,
                                        "total_tokens": 16
                                    }
                                })
                            } else {
                                json!({
                                    "id": "chatcmpl-tool-2",
                                    "object": "chat.completion",
                                    "model": model,
                                    "choices": [{
                                        "index": 0,
                                        "message": {
                                            "role": "assistant",
                                            "content": "tool loop complete"
                                        },
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 7,
                                        "completion_tokens": 4,
                                        "total_tokens": 11
                                    }
                                })
                            };
                            let raw = response.to_string();
                            let http = format!(
                                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                                raw.len(),
                                raw
                            );
                            let _ = stream.write_all(http.as_bytes());
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Self {
                base_url,
                requests,
                shutdown,
                handle: Some(handle),
            }
        }

        fn requests(&self) -> Vec<Value> {
            self.requests
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl Drop for ToolLoopOpenAiServer {
        fn drop(&mut self) {
            self.shutdown.store(true, Ordering::Relaxed);
            let _ = std::net::TcpStream::connect(self.base_url.trim_start_matches("http://"));
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn set_mock_mcp_env(mcp: &MockMcpServer) -> (EnvVarGuard, EnvVarGuard) {
        let endpoint = format!("{}/mcp", mcp.base_url);
        (
            EnvVarGuard::set("AGENTHALO_MCP_SECRET", Some("bridge-secret")),
            EnvVarGuard::set("AGENTHALO_ORCHESTRATOR_MCP_ENDPOINT", Some(&endpoint)),
        )
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cli_hookup_lifecycle_and_trace() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());
        let mcp = MockMcpServer::spawn();
        let (_mcp_secret, _mcp_endpoint) = set_mock_mcp_env(&mcp);
        let output_path = dir.path().join("cli_hookup_output.txt");
        let prompt = format!(
            "printf 'hello from shell\\n' > '{}' && cat '{}'",
            output_path.display(),
            output_path.display()
        );

        let pty_manager = Arc::new(PtyManager::new(4));
        let hookup =
            CliAgentHookup::with_trace_path("shell", pty_manager, None, &config::db_path())
                .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");
        let _response = hookup.send_prompt(&prompt).await.expect("response");
        let output = std::fs::read_to_string(&output_path).expect("cli output file");
        assert!(output.contains("hello from shell"));
        hookup.stop().await.expect("stop");

        let trace_id = hookup.trace_session_id().expect("trace session id");
        let events = session_events(hookup.trace_db_path(), &trace_id).expect("events");
        let event_types = events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                EventType::AgentInitialized,
                EventType::PromptSent,
                EventType::ResponseReceived,
                EventType::AgentDeinitialized
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn api_hookup_lifecycle_and_trace() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());
        let mcp = MockMcpServer::spawn();
        let (_mcp_secret, _mcp_endpoint) = set_mock_mcp_env(&mcp);

        let server = MockOpenAiServer::spawn("openrouter/test-model", "api response");
        let hookup = ApiAgentHookup::with_base_url(
            "openrouter",
            "openrouter/test-model",
            "literal-test-key",
            Some(server.base_url.clone()),
            &config::db_path(),
        )
        .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");
        let response = hookup.send_prompt("hello api").await.expect("response");
        assert_eq!(response.content, "api response");
        hookup.stop().await.expect("stop");

        let trace_id = hookup.trace_session_id().expect("trace session id");
        let events = session_events(hookup.trace_db_path(), &trace_id).expect("events");
        let event_types = events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                EventType::AgentInitialized,
                EventType::PromptSent,
                EventType::ResponseReceived,
                EventType::AgentDeinitialized
            ]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cli_hookup_mcp_config_injected() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());
        let mcp = MockMcpServer::spawn();
        let (_mcp_secret, _mcp_endpoint) = set_mock_mcp_env(&mcp);

        let pty_manager = Arc::new(PtyManager::new(2));
        let hookup =
            CliAgentHookup::with_trace_path("shell", pty_manager, None, &config::db_path())
                .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");

        let working_dir = hookup.mcp_working_dir().expect("mcp working dir");
        let endpoint = format!("{}/mcp", mcp.base_url);
        let claude = std::fs::read_to_string(working_dir.join(".mcp.json")).expect("claude config");
        let codex =
            std::fs::read_to_string(working_dir.join(".codex/config.toml")).expect("codex config");
        let gemini = std::fs::read_to_string(working_dir.join(".gemini/settings.json"))
            .expect("gemini config");
        assert!(claude.contains(&endpoint));
        assert!(claude.contains("bridge-secret"));
        assert!(codex.contains(&endpoint));
        assert!(gemini.contains(&endpoint));

        hookup.stop().await.expect("stop");
        assert!(
            !working_dir.exists(),
            "temp MCP config dir should be removed"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cli_hookup_mcp_tools_discoverable() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());
        let mcp = MockMcpServer::spawn();
        let (_mcp_secret, _mcp_endpoint) = set_mock_mcp_env(&mcp);

        let pty_manager = Arc::new(PtyManager::new(2));
        let hookup =
            CliAgentHookup::with_trace_path("shell", pty_manager, None, &config::db_path())
                .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");
        let response = hookup
            .send_prompt("printf '%s' \"$AGENTHALO_MCP_ENDPOINT\"")
            .await
            .expect("response");
        assert_eq!(response.content, format!("{}/mcp", mcp.base_url));
        hookup.stop().await.expect("stop");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn api_hookup_tool_calling_loop() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());
        let mcp = MockMcpServer::spawn();
        let (_mcp_secret, _mcp_endpoint) = set_mock_mcp_env(&mcp);
        let server = ToolLoopOpenAiServer::spawn("openrouter/test-model");

        let hookup = ApiAgentHookup::with_base_url(
            "openrouter",
            "openrouter/test-model",
            "literal-test-key",
            Some(server.base_url.clone()),
            &config::db_path(),
        )
        .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");
        let response = hookup.send_prompt("hello api").await.expect("response");
        assert_eq!(response.content, "tool loop complete");
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].name, "nucleusdb_help");
        assert_eq!(
            response.tool_calls[0]
                .output
                .as_ref()
                .and_then(|value| value.get("structured_content"))
                .and_then(|value| value.get("message"))
                .and_then(|value| value.as_str()),
            Some("executed nucleusdb_help")
        );
        let mcp_state = mcp.state();
        assert_eq!(mcp_state.tool_calls, 1);
        assert_eq!(mcp_state.last_tool.as_deref(), Some("nucleusdb_help"));
        assert_eq!(
            mcp_state
                .last_arguments
                .as_ref()
                .and_then(|value| value.get("topic"))
                .and_then(|value| value.as_str()),
            Some("status")
        );
        let requests = server.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests[0]["tools"].is_array());
        assert_eq!(requests[1]["messages"][2]["role"], "tool");
        hookup.stop().await.expect("stop");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn api_hookup_tool_calls_in_trace() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());
        let mcp = MockMcpServer::spawn();
        let (_mcp_secret, _mcp_endpoint) = set_mock_mcp_env(&mcp);
        let server = ToolLoopOpenAiServer::spawn("openrouter/test-model");

        let hookup = ApiAgentHookup::with_base_url(
            "openrouter",
            "openrouter/test-model",
            "literal-test-key",
            Some(server.base_url.clone()),
            &config::db_path(),
        )
        .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");
        let _response = hookup.send_prompt("hello api").await.expect("response");
        hookup.stop().await.expect("stop");

        let trace_id = hookup.trace_session_id().expect("trace session id");
        let events = session_events(hookup.trace_db_path(), &trace_id).expect("events");
        assert!(events
            .iter()
            .any(|event| event.event_type == EventType::ToolCall
                && event.tool_name.as_deref() == Some("nucleusdb_help")));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn local_model_hookup_lifecycle_and_trace() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", dir.path().to_str());

        let server = MockOpenAiServer::spawn("test/local-model", "local response");
        let hookup = LocalModelHookup::with_base_url(
            "test/local-model",
            8000,
            Some(server.base_url.clone()),
            &config::db_path(),
        )
        .expect("hookup");
        let mut lock = ContainerAgentLock::load_or_create("container-a").expect("lock");
        hookup.start(&mut lock).await.expect("start");
        let response = hookup.send_prompt("hello local").await.expect("response");
        assert_eq!(response.content, "local response");
        hookup.stop().await.expect("stop");

        let trace_id = hookup.trace_session_id().expect("trace session id");
        let events = session_events(hookup.trace_db_path(), &trace_id).expect("events");
        let event_types = events
            .iter()
            .map(|event| event.event_type.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            event_types,
            vec![
                EventType::AgentInitialized,
                EventType::PromptSent,
                EventType::ResponseReceived,
                EventType::AgentDeinitialized
            ]
        );
    }
}
