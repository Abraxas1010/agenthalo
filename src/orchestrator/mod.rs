pub mod a2a;
pub mod agent_pool;
pub mod container_dispatch;
pub mod dispatch;
pub mod subsidiary_registry;
pub mod task;
pub mod task_graph;
pub mod trace_bridge;

pub use dispatch::{ContainerHookupRequest, DispatchMode};

use crate::cockpit::pty_manager::PtyManager;
use crate::container::agent_lock::ReusePolicy;
use crate::halo::vault::Vault;
use crate::orchestrator::agent_pool::{
    normalize_capabilities, validate_capabilities, AgentPool, ContainerBudget, LaunchSpec,
    ManagedAgent,
};
use crate::orchestrator::container_dispatch::{
    ContainerDeinitializeSpec, ContainerDispatch, ContainerInitializeSpec, ContainerPromptSpec,
    ContainerProvisionSpec, MeshContainerDispatch,
};
use crate::orchestrator::task::{Task, TaskStatus, TaskUsage};
use crate::orchestrator::task_graph::{PipeTransform, TaskEdge, TaskGraph};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchAgentRequest {
    pub agent: String,
    pub agent_name: String,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub timeout_secs: u64,
    #[serde(default)]
    pub model: Option<String>,
    pub trace: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub dispatch_mode: Option<DispatchMode>,
    #[serde(default)]
    pub container_hookup: Option<ContainerHookupRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendTaskRequest {
    pub agent_id: String,
    pub task: String,
    pub timeout_secs: Option<u64>,
    pub wait: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipeRequest {
    pub source_task_id: String,
    pub target_agent_id: String,
    pub transform: Option<String>,
    pub task_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopRequest {
    pub agent_id: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopResult {
    pub agent_id: String,
    pub status: String,
    pub trace_session_id: Option<String>,
    pub attestation_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshPeerStatus {
    pub agent_id: String,
    pub container_name: String,
    pub did_uri: Option<String>,
    pub mcp_endpoint: String,
    pub reachable: bool,
    pub latency_ms: Option<u64>,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MeshStatusResponse {
    pub enabled: bool,
    pub self_agent_id: Option<String>,
    pub peers: Vec<MeshPeerStatus>,
    pub network_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorStatus {
    pub agents: Vec<ManagedAgent>,
    pub budget: ContainerBudget,
    pub agents_total: usize,
    pub agents_busy: usize,
    pub agents_idle: usize,
    pub agents_stopped: usize,
}

#[derive(Debug, Clone)]
pub struct Orchestrator {
    inner: Arc<OrchestratorInner>,
}

struct OrchestratorInner {
    pool: AgentPool,
    container_dispatch: Arc<dyn ContainerDispatch>,
    container_agents: tokio::sync::Mutex<BTreeMap<String, ManagedAgent>>,
    container_sessions: tokio::sync::Mutex<BTreeMap<String, ContainerAgentSession>>,
    tasks: tokio::sync::Mutex<BTreeMap<String, Task>>,
    graph: tokio::sync::Mutex<TaskGraph>,
    trace_db_path: PathBuf,
    background_runs: tokio::sync::Mutex<BTreeMap<String, tokio::task::JoinHandle<()>>>,
}

impl std::fmt::Debug for OrchestratorInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrchestratorInner").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
struct ContainerAgentSession {
    session_id: String,
    peer_agent_id: String,
    reuse_policy: ReusePolicy,
}

const MAX_TASK_TIMEOUT_SECS: u64 = 3600;
const TASK_RETENTION_SECS: u64 = 86_400;
const MAX_TASKS_RETAINED: usize = 2_000;

impl Orchestrator {
    pub fn new(
        pty_manager: Arc<PtyManager>,
        vault: Option<Arc<Vault>>,
        trace_db_path: PathBuf,
    ) -> Self {
        Self::with_budget(
            pty_manager,
            vault,
            trace_db_path,
            ContainerBudget::default(),
        )
    }

    pub fn with_budget(
        pty_manager: Arc<PtyManager>,
        vault: Option<Arc<Vault>>,
        trace_db_path: PathBuf,
        budget: ContainerBudget,
    ) -> Self {
        Self::with_budget_and_container_dispatch(
            pty_manager,
            vault,
            trace_db_path,
            budget,
            Arc::new(MeshContainerDispatch::default()),
        )
    }

    pub fn with_budget_and_container_dispatch(
        pty_manager: Arc<PtyManager>,
        vault: Option<Arc<Vault>>,
        trace_db_path: PathBuf,
        budget: ContainerBudget,
        container_dispatch: Arc<dyn ContainerDispatch>,
    ) -> Self {
        Self {
            inner: Arc::new(OrchestratorInner {
                pool: AgentPool::with_budget(pty_manager, vault, budget),
                container_dispatch,
                container_agents: tokio::sync::Mutex::new(BTreeMap::new()),
                container_sessions: tokio::sync::Mutex::new(BTreeMap::new()),
                tasks: tokio::sync::Mutex::new(BTreeMap::new()),
                graph: tokio::sync::Mutex::new(TaskGraph::default()),
                trace_db_path,
                background_runs: tokio::sync::Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub async fn launch_agent(&self, req: LaunchAgentRequest) -> Result<ManagedAgent, String> {
        match req.dispatch_mode.unwrap_or_else(DispatchMode::from_env) {
            DispatchMode::Pty => {
                self.inner
                    .pool
                    .launch(LaunchSpec {
                        agent: req.agent,
                        agent_name: req.agent_name,
                        working_dir: req.working_dir,
                        env: req.env,
                        timeout_secs: req.timeout_secs.clamp(5, MAX_TASK_TIMEOUT_SECS),
                        model: req.model,
                        trace: req.trace,
                        capabilities: req.capabilities,
                    })
                    .await
            }
            DispatchMode::Container => self.launch_container_agent(req).await,
        }
    }

    pub async fn send_task(&self, req: SendTaskRequest) -> Result<Task, String> {
        let prompt = req.task.trim().to_string();
        if prompt.is_empty() {
            return Err("task must be non-empty".to_string());
        }
        if let Some((peer_agent_id, target_agent_id)) = parse_remote_agent_ref(&req.agent_id) {
            return self
                .send_remote_peer_task(
                    req.agent_id,
                    peer_agent_id,
                    target_agent_id,
                    prompt,
                    req.timeout_secs,
                )
                .await;
        }
        let task = self.create_task(req.agent_id.clone(), prompt).await;
        let task_id = task.task_id.clone();

        if req.wait {
            self.run_task(task_id.clone(), req.timeout_secs).await?;
            self.get_task(&task_id)
                .await
                .ok_or_else(|| format!("task {task_id} missing after execution"))
        } else {
            let this = self.clone();
            let run_id = task_id.clone();
            let handle = tokio::spawn(async move {
                let _ = this.run_task(run_id, req.timeout_secs).await;
            });
            {
                let mut bg = self.inner.background_runs.lock().await;
                bg.insert(task_id.clone(), handle);
                if bg
                    .get(&task_id)
                    .map(tokio::task::JoinHandle::is_finished)
                    .unwrap_or(false)
                {
                    bg.remove(&task_id);
                }
            }
            Ok(task)
        }
    }

    async fn send_remote_peer_task(
        &self,
        agent_id: String,
        peer_agent_id: String,
        target_agent_id: String,
        prompt: String,
        timeout_secs: Option<u64>,
    ) -> Result<Task, String> {
        let task = self.create_task(agent_id, prompt.clone()).await;
        let task_id = task.task_id.clone();
        let timeout = timeout_secs.unwrap_or(600).clamp(1, MAX_TASK_TIMEOUT_SECS);
        let local_identity = load_local_identity_for_a2a()?;
        let prompt_for_call = prompt.clone();
        let a2a_result = tokio::task::spawn_blocking(move || {
            crate::orchestrator::a2a::delegate_task_to_peer(
                &local_identity,
                &peer_agent_id,
                &target_agent_id,
                &prompt_for_call,
                timeout,
                &[],
            )
        })
        .await
        .map_err(|e| format!("remote A2A task join failure: {e}"))?;

        let mut updated = task;
        match a2a_result {
            Ok(envelope) => {
                let status = envelope.status.trim().to_ascii_lowercase();
                if status == "complete" {
                    updated.mark_complete(
                        envelope.result.unwrap_or_default(),
                        envelope.exit_code.unwrap_or(0),
                        TaskUsage::default(),
                        None,
                    );
                } else if status == "timeout" {
                    updated.mark_timeout(
                        envelope
                            .error
                            .unwrap_or_else(|| "remote A2A task timed out".to_string()),
                    );
                } else {
                    updated.mark_failed(
                        envelope
                            .error
                            .unwrap_or_else(|| "remote A2A task failed".to_string()),
                        envelope.exit_code,
                    );
                }
            }
            Err(err) => {
                updated.mark_failed(err, None);
            }
        }
        {
            let mut tasks = self.inner.tasks.lock().await;
            tasks.insert(task_id.clone(), updated.clone());
        }
        {
            let mut graph = self.inner.graph.lock().await;
            graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
        }
        self.prune_tasks().await;
        self.get_task(&task_id)
            .await
            .ok_or_else(|| format!("task {task_id} missing after remote execution"))
    }

    async fn create_task(&self, agent_id: String, prompt: String) -> Task {
        let task_id = format!(
            "task-{}-{}",
            crate::pod::now_unix(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let mut task = Task::new(task_id.clone(), agent_id, prompt);
        task.mark_running();
        let mut tasks = self.inner.tasks.lock().await;
        tasks.insert(task_id, task.clone());
        drop(tasks);
        let mut graph = self.inner.graph.lock().await;
        graph.upsert_node(&task.task_id, &task.agent_id, task.status.clone());
        task
    }

    async fn run_task(&self, task_id: String, timeout_override: Option<u64>) -> Result<(), String> {
        let task = self
            .get_task(&task_id)
            .await
            .ok_or_else(|| format!("unknown task {task_id}"))?;
        if self.is_container_agent(&task.agent_id).await {
            let result = self
                .run_container_task(task_id.clone(), task, timeout_override)
                .await;
            self.clear_background_run(&task_id).await;
            self.prune_tasks().await;
            return result;
        }
        let execution = self
            .inner
            .pool
            .start_task(
                &task.agent_id,
                &task.task_id,
                &task.prompt,
                timeout_override.map(|t| t.clamp(1, MAX_TASK_TIMEOUT_SECS)),
            )
            .await?;
        let session = self
            .inner
            .pool
            .pty_session_by_id(&execution.session_id)
            .ok_or_else(|| format!("missing PTY session {}", execution.session_id))?;
        let trace_session_id = format!("orch-trace-{}", task_id);
        let outcome = trace_bridge::collect_task_output(
            session,
            &execution.agent_type,
            &self.inner.trace_db_path,
            &trace_session_id,
            &task.prompt,
            execution.trace_enabled,
            execution.timeout_secs,
        )
        .await;

        let result = match outcome {
            Ok(outcome) => {
                let mut updated = task.clone();
                if outcome.exit_code == 0 {
                    updated.mark_complete(
                        outcome.output.clone(),
                        outcome.exit_code,
                        TaskUsage {
                            input_tokens: outcome.input_tokens,
                            output_tokens: outcome.output_tokens,
                            estimated_cost_usd: outcome.estimated_cost_usd,
                        },
                        outcome.trace_session_id.clone(),
                    );
                    updated.answer = outcome.answer.clone();
                } else {
                    updated.mark_failed(
                        format!("task exited with code {}", outcome.exit_code),
                        Some(outcome.exit_code),
                    );
                    updated.result = Some(outcome.output.clone());
                    updated.answer = outcome.answer.clone();
                }
                {
                    let mut tasks = self.inner.tasks.lock().await;
                    tasks.insert(task_id.clone(), updated.clone());
                }
                {
                    let mut graph = self.inner.graph.lock().await;
                    graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
                }
                let _ = self.inner.pool.destroy_pty_session(&execution.session_id);
                self.inner
                    .pool
                    .complete_task(&execution.agent_id, outcome.estimated_cost_usd)
                    .await;
                let followups = self.collect_followups(&updated).await;
                for (source_task_id, target_agent_id, transformed) in followups {
                    let new_task = self.create_task(target_agent_id.clone(), transformed).await;
                    let new_task_id = new_task.task_id.clone();
                    {
                        let mut graph = self.inner.graph.lock().await;
                        graph.set_generated_task(
                            &source_task_id,
                            &target_agent_id,
                            new_task_id.clone(),
                        );
                    }
                    let _ = Box::pin(self.run_task(new_task_id, None)).await;
                }
                Ok(())
            }
            Err(err) => {
                let mut updated = task;
                if err.to_ascii_lowercase().contains("timeout") {
                    updated.mark_timeout(err.clone());
                } else {
                    updated.mark_failed(err.clone(), None);
                }
                {
                    let mut tasks = self.inner.tasks.lock().await;
                    tasks.insert(task_id.clone(), updated.clone());
                }
                {
                    let mut graph = self.inner.graph.lock().await;
                    graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
                }
                let _ = self.inner.pool.destroy_pty_session(&execution.session_id);
                self.inner
                    .pool
                    .complete_task(&execution.agent_id, 0.0)
                    .await;
                Err(err)
            }
        };
        self.clear_background_run(&task_id).await;
        self.prune_tasks().await;
        result
    }

    async fn collect_followups(&self, task: &Task) -> Vec<(String, String, String)> {
        if task.status != TaskStatus::Complete {
            return Vec::new();
        }
        let result = task.result.clone().unwrap_or_default();
        let answer = task.answer.as_deref();
        let edges = {
            let graph = self.inner.graph.lock().await;
            graph.outgoing_for(&task.task_id)
        };
        let mut out = Vec::new();
        for edge in edges {
            let transformed = edge.transform.apply_with_answer(&result, answer);
            out.push((
                task.task_id.clone(),
                edge.target_agent_id.clone(),
                transformed,
            ));
        }
        out
    }

    async fn launch_container_agent(
        &self,
        req: LaunchAgentRequest,
    ) -> Result<ManagedAgent, String> {
        let hookup = match req.container_hookup.clone() {
            Some(hookup) => hookup,
            None => ContainerHookupRequest::infer_cli(&req.agent, req.model.clone())?,
        };
        validate_capabilities(&req.capabilities)?;
        let kind = hookup.agent_type();
        let budget = self.inner.pool.budget().clone();
        if !budget.allowed_kinds.is_empty()
            && !budget.allowed_kinds.iter().any(|allowed| allowed == &kind)
        {
            return Err(format!(
                "agent kind '{}' not allowed by container budget (allowed: {:?})",
                kind, budget.allowed_kinds
            ));
        }
        let total_agents =
            self.inner.pool.list().await.len() + self.inner.container_agents.lock().await.len();
        if total_agents >= budget.max_agents {
            return Err(format!(
                "container budget exceeded: max {} agents",
                budget.max_agents
            ));
        }
        let defaults = self.inner.container_dispatch.provision_defaults();
        let peer_agent_id = format!(
            "container-{}-{}",
            crate::pod::now_unix(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let provisioned = self
            .inner
            .container_dispatch
            .provision(ContainerProvisionSpec {
                image: defaults.image,
                peer_agent_id: peer_agent_id.clone(),
                mcp_port: defaults.mcp_port,
                registry_volume: defaults.registry_volume,
                env: BTreeMap::new(),
            })
            .await?;
        let initialized = self
            .inner
            .container_dispatch
            .initialize(ContainerInitializeSpec {
                peer_agent_id: peer_agent_id.clone(),
                reuse_policy: ReusePolicy::Reusable,
                hookup: hookup.clone(),
            })
            .await?;
        let agent_id = format!(
            "ctr-{}-{}",
            crate::pod::now_unix(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let managed = ManagedAgent {
            agent_id: agent_id.clone(),
            agent_name: if req.agent_name.trim().is_empty() {
                hookup.agent_type()
            } else {
                req.agent_name.trim().to_string()
            },
            agent_type: hookup.agent_type(),
            pty_session_id: None,
            capabilities: normalize_capabilities(req.capabilities),
            status: crate::orchestrator::agent_pool::AgentStatus::Idle,
            launched_at: crate::pod::now_unix(),
            timeout_secs: req.timeout_secs.clamp(5, MAX_TASK_TIMEOUT_SECS),
            model: hookup.model(),
            working_dir: req.working_dir,
            tasks_completed: 0,
            total_cost_usd: 0.0,
            trace_enabled: req.trace,
            command: String::new(),
            static_args: Vec::new(),
            env: Vec::new(),
            env_remove: Vec::new(),
        };
        self.inner
            .container_agents
            .lock()
            .await
            .insert(agent_id.clone(), managed.clone());
        self.inner.container_sessions.lock().await.insert(
            agent_id,
            ContainerAgentSession {
                session_id: provisioned.session_id,
                peer_agent_id,
                reuse_policy: initialized.reuse_policy,
            },
        );
        Ok(managed)
    }

    async fn is_container_agent(&self, agent_id: &str) -> bool {
        self.inner
            .container_agents
            .lock()
            .await
            .contains_key(agent_id)
    }

    async fn run_container_task(
        &self,
        task_id: String,
        task: Task,
        timeout_override: Option<u64>,
    ) -> Result<(), String> {
        let session = self
            .inner
            .container_sessions
            .lock()
            .await
            .get(&task.agent_id)
            .cloned()
            .ok_or_else(|| format!("missing container session metadata for {}", task.agent_id))?;
        let pty_busy_count = self
            .inner
            .pool
            .list()
            .await
            .into_iter()
            .filter(|agent| {
                matches!(
                    agent.status,
                    crate::orchestrator::agent_pool::AgentStatus::Busy { .. }
                )
            })
            .count();
        let timeout_secs = {
            let mut agents = self.inner.container_agents.lock().await;
            let busy_count = pty_busy_count
                + agents
                    .values()
                    .filter(|agent| {
                        matches!(
                            agent.status,
                            crate::orchestrator::agent_pool::AgentStatus::Busy { .. }
                        )
                    })
                    .count();
            if busy_count >= self.inner.pool.budget().max_concurrent_busy {
                return Err(format!(
                    "concurrent busy limit reached ({}/{})",
                    busy_count,
                    self.inner.pool.budget().max_concurrent_busy
                ));
            }
            let agent = agents
                .get_mut(&task.agent_id)
                .ok_or_else(|| format!("unknown agent_id {}", task.agent_id))?;
            if matches!(
                agent.status,
                crate::orchestrator::agent_pool::AgentStatus::Stopped { .. }
            ) {
                return Err(format!("agent {} is stopped", task.agent_id));
            }
            if matches!(
                agent.status,
                crate::orchestrator::agent_pool::AgentStatus::Busy { .. }
            ) {
                return Err(format!("agent {} is busy", task.agent_id));
            }
            let timeout_secs = timeout_override
                .unwrap_or(agent.timeout_secs)
                .clamp(1, MAX_TASK_TIMEOUT_SECS);
            agent.status = crate::orchestrator::agent_pool::AgentStatus::Busy {
                task_id: task.task_id.clone(),
            };
            timeout_secs
        };
        let outcome = tokio::time::timeout(
            Duration::from_secs(timeout_secs),
            self.inner
                .container_dispatch
                .send_prompt(ContainerPromptSpec {
                    peer_agent_id: session.peer_agent_id.clone(),
                    prompt: task.prompt.clone(),
                }),
        )
        .await;
        let result = match outcome {
            Ok(Ok(response)) => {
                let mut updated = task.clone();
                updated.mark_complete(
                    response.content.clone(),
                    0,
                    TaskUsage {
                        input_tokens: response.input_tokens,
                        output_tokens: response.output_tokens,
                        estimated_cost_usd: response.cost_usd,
                    },
                    None,
                );
                updated.answer = Some(response.content.clone());
                {
                    let mut tasks = self.inner.tasks.lock().await;
                    tasks.insert(task_id.clone(), updated.clone());
                }
                {
                    let mut graph = self.inner.graph.lock().await;
                    graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
                }
                {
                    let mut agents = self.inner.container_agents.lock().await;
                    if let Some(agent) = agents.get_mut(&task.agent_id) {
                        agent.status = crate::orchestrator::agent_pool::AgentStatus::Idle;
                        agent.tasks_completed = agent.tasks_completed.saturating_add(1);
                        agent.total_cost_usd += response.cost_usd.max(0.0);
                    }
                }
                let followups = self.collect_followups(&updated).await;
                for (source_task_id, target_agent_id, transformed) in followups {
                    let new_task = self.create_task(target_agent_id.clone(), transformed).await;
                    let new_task_id = new_task.task_id.clone();
                    {
                        let mut graph = self.inner.graph.lock().await;
                        graph.set_generated_task(
                            &source_task_id,
                            &target_agent_id,
                            new_task_id.clone(),
                        );
                    }
                    let _ = Box::pin(self.run_task(new_task_id, None)).await;
                }
                Ok(())
            }
            Ok(Err(err)) => {
                let mut updated = task;
                updated.mark_failed(err.clone(), None);
                {
                    let mut tasks = self.inner.tasks.lock().await;
                    tasks.insert(task_id.clone(), updated.clone());
                }
                {
                    let mut graph = self.inner.graph.lock().await;
                    graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
                }
                {
                    let mut agents = self.inner.container_agents.lock().await;
                    if let Some(agent) = agents.get_mut(&updated.agent_id) {
                        agent.status = crate::orchestrator::agent_pool::AgentStatus::Idle;
                    }
                }
                Err(err)
            }
            Err(_) => {
                let err = format!("container task timed out after {timeout_secs}s");
                let mut updated = task;
                updated.mark_timeout(err.clone());
                {
                    let mut tasks = self.inner.tasks.lock().await;
                    tasks.insert(task_id.clone(), updated.clone());
                }
                {
                    let mut graph = self.inner.graph.lock().await;
                    graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
                }
                {
                    let mut agents = self.inner.container_agents.lock().await;
                    if let Some(agent) = agents.get_mut(&updated.agent_id) {
                        agent.status = crate::orchestrator::agent_pool::AgentStatus::Idle;
                    }
                }
                Err(err)
            }
        };
        result
    }

    pub async fn pipe(&self, req: PipeRequest) -> Result<Option<Task>, String> {
        let transform = PipeTransform::parse(req.transform.as_deref(), req.task_prefix.as_deref())?;
        let source = self
            .get_task(&req.source_task_id)
            .await
            .ok_or_else(|| format!("unknown source_task_id {}", req.source_task_id))?;
        let edge = TaskEdge {
            source_task_id: req.source_task_id.clone(),
            target_agent_id: req.target_agent_id.clone(),
            transform: transform.clone(),
            generated_task_id: None,
        };
        {
            let mut graph = self.inner.graph.lock().await;
            graph.add_edge(edge)?;
        }

        if source.status == TaskStatus::Complete {
            let input = transform.apply_with_answer(
                source.result.as_deref().unwrap_or_default(),
                source.answer.as_deref(),
            );
            let submitted = self.create_task(req.target_agent_id.clone(), input).await;
            let task_id = submitted.task_id.clone();
            let _ = self.run_task(task_id, None).await;
            let final_task = self.get_task(&submitted.task_id).await.unwrap_or(submitted);
            {
                let mut graph = self.inner.graph.lock().await;
                graph.set_generated_task(
                    &req.source_task_id,
                    &req.target_agent_id,
                    final_task.task_id.clone(),
                );
            }
            return Ok(Some(final_task));
        }
        Ok(None)
    }

    pub async fn list_agents(&self) -> Vec<ManagedAgent> {
        let mut agents = self.inner.pool.list().await;
        let mut containers = self
            .inner
            .container_agents
            .lock()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        agents.append(&mut containers);
        agents
    }

    pub async fn status(&self) -> OrchestratorStatus {
        let agents = self.list_agents().await;
        let agents_busy = agents
            .iter()
            .filter(|agent| {
                matches!(
                    agent.status,
                    crate::orchestrator::agent_pool::AgentStatus::Busy { .. }
                )
            })
            .count();
        let agents_idle = agents
            .iter()
            .filter(|agent| {
                matches!(
                    agent.status,
                    crate::orchestrator::agent_pool::AgentStatus::Idle
                )
            })
            .count();
        let agents_stopped = agents
            .iter()
            .filter(|agent| {
                matches!(
                    agent.status,
                    crate::orchestrator::agent_pool::AgentStatus::Stopped { .. }
                )
            })
            .count();
        OrchestratorStatus {
            agents_total: agents.len(),
            agents_busy,
            agents_idle,
            agents_stopped,
            budget: self.inner.pool.budget().clone(),
            agents,
        }
    }

    /// Query mesh peer status. Returns an empty disabled response when mesh
    /// is not configured.
    pub fn mesh_status(&self) -> MeshStatusResponse {
        if !crate::container::mesh_enabled() {
            return MeshStatusResponse {
                enabled: false,
                self_agent_id: None,
                peers: Vec::new(),
                network_name: None,
            };
        }

        let self_agent_id = std::env::var("NUCLEUSDB_MESH_AGENT_ID")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        let self_id = self_agent_id.as_deref().unwrap_or_default();
        let registry =
            crate::container::PeerRegistry::load(crate::container::mesh_registry_path().as_path())
                .unwrap_or_default();
        let peers = registry
            .peers_except(self_id)
            .into_iter()
            .map(|peer| {
                let (reachable, latency_ms) = crate::container::ping_peer_with_latency(peer);
                MeshPeerStatus {
                    agent_id: peer.agent_id.clone(),
                    container_name: peer.container_name.clone(),
                    did_uri: peer.did_uri.clone(),
                    mcp_endpoint: peer.mcp_endpoint.clone(),
                    reachable,
                    latency_ms: reachable.then_some(latency_ms),
                    last_seen: peer.last_seen,
                }
            })
            .collect();

        MeshStatusResponse {
            enabled: true,
            self_agent_id,
            peers,
            network_name: Some(crate::container::MESH_NETWORK_NAME.to_string()),
        }
    }

    /// Async wrapper for mesh status probing so network pings do not block
    /// Tokio worker threads serving API/MCP handlers.
    pub async fn mesh_status_async(&self) -> MeshStatusResponse {
        let orchestrator = self.clone();
        tokio::task::spawn_blocking(move || orchestrator.mesh_status())
            .await
            .unwrap_or_else(|_| MeshStatusResponse {
                enabled: false,
                self_agent_id: None,
                peers: Vec::new(),
                network_name: None,
            })
    }

    pub async fn get_task(&self, task_id: &str) -> Option<Task> {
        let tasks = self.inner.tasks.lock().await;
        tasks.get(task_id).cloned()
    }

    pub async fn list_tasks(&self) -> Vec<Task> {
        self.prune_tasks().await;
        let tasks = self.inner.tasks.lock().await;
        tasks.values().cloned().collect()
    }

    pub async fn stop_agent(&self, req: StopRequest) -> Result<StopResult, String> {
        if self.is_container_agent(&req.agent_id).await {
            self.cancel_inflight_for_agent(&req.agent_id).await;
            let session = self
                .inner
                .container_sessions
                .lock()
                .await
                .get(&req.agent_id)
                .cloned()
                .ok_or_else(|| {
                    format!("missing container session metadata for {}", req.agent_id)
                })?;
            let deinit = self
                .inner
                .container_dispatch
                .deinitialize(ContainerDeinitializeSpec {
                    peer_agent_id: session.peer_agent_id.clone(),
                })
                .await?;
            if req.force || session.reuse_policy == ReusePolicy::SingleUse {
                self.inner
                    .container_dispatch
                    .destroy(&session.session_id)
                    .await?;
            }
            let stopped = {
                let mut agents = self.inner.container_agents.lock().await;
                let agent = agents
                    .get_mut(&req.agent_id)
                    .ok_or_else(|| format!("unknown agent_id {}", req.agent_id))?;
                agent.status =
                    crate::orchestrator::agent_pool::AgentStatus::Stopped { exit_code: 0 };
                agent.clone()
            };
            return Ok(StopResult {
                agent_id: stopped.agent_id,
                status: "stopped".to_string(),
                trace_session_id: deinit.trace_session_id,
                attestation_ready: true,
            });
        }
        self.cancel_inflight_for_agent(&req.agent_id).await;
        let agent = self.inner.pool.stop(&req.agent_id, req.force).await?;
        Ok(StopResult {
            agent_id: agent.agent_id,
            status: "stopped".to_string(),
            trace_session_id: None,
            attestation_ready: true,
        })
    }

    pub async fn graph_snapshot(&self) -> TaskGraph {
        self.inner.graph.lock().await.clone()
    }

    pub async fn current_agent_session(
        &self,
        agent_id: &str,
    ) -> Option<Arc<crate::cockpit::pty_manager::PtySession>> {
        self.inner.pool.current_session_for_agent(agent_id).await
    }

    pub async fn require_capability(&self, agent_id: &str, capability: &str) -> Result<(), String> {
        if let Some(agent) = self
            .inner
            .container_agents
            .lock()
            .await
            .get(agent_id)
            .cloned()
        {
            let allowed = agent
                .capabilities
                .iter()
                .any(|c| c == "*" || c == capability);
            if allowed {
                return Ok(());
            }
            return Err(format!(
                "agent '{}' lacks capability '{}'",
                agent.agent_name, capability
            ));
        }
        self.inner.pool.capability_check(agent_id, capability).await
    }

    async fn clear_background_run(&self, task_id: &str) {
        let mut bg = self.inner.background_runs.lock().await;
        bg.remove(task_id);
    }

    async fn cancel_inflight_for_agent(&self, agent_id: &str) {
        let inflight_task_ids: Vec<String> = {
            let tasks = self.inner.tasks.lock().await;
            tasks
                .values()
                .filter(|task| {
                    task.agent_id == agent_id
                        && matches!(task.status, TaskStatus::Running | TaskStatus::Pending)
                })
                .map(|task| task.task_id.clone())
                .collect()
        };
        if inflight_task_ids.is_empty() {
            return;
        }

        {
            let mut bg = self.inner.background_runs.lock().await;
            for task_id in &inflight_task_ids {
                if let Some(handle) = bg.remove(task_id) {
                    handle.abort();
                }
            }
        }
        {
            let mut tasks = self.inner.tasks.lock().await;
            for task_id in &inflight_task_ids {
                if let Some(task) = tasks.get_mut(task_id) {
                    task.mark_failed("agent stopped".to_string(), None);
                }
            }
        }
    }

    async fn prune_tasks(&self) {
        let now = crate::pod::now_unix();
        let mut tasks = self.inner.tasks.lock().await;
        tasks.retain(|_, task| {
            if let Some(done_at) = task.completed_at {
                now.saturating_sub(done_at) <= TASK_RETENTION_SECS
            } else {
                true
            }
        });

        if tasks.len() <= MAX_TASKS_RETAINED {
            return;
        }

        let mut ids_by_age: Vec<(String, u64)> = tasks
            .iter()
            .map(|(id, task)| {
                let ts = task.completed_at.or(task.started_at).unwrap_or_default();
                (id.clone(), ts)
            })
            .collect();
        ids_by_age.sort_by_key(|(_, ts)| *ts);

        let mut excess = tasks.len().saturating_sub(MAX_TASKS_RETAINED);
        for (id, _) in ids_by_age {
            if excess == 0 {
                break;
            }
            if tasks.remove(&id).is_some() {
                excess -= 1;
            }
        }
    }
}

fn parse_remote_agent_ref(agent_id: &str) -> Option<(String, String)> {
    let raw = agent_id.trim();
    let rest = raw.strip_prefix("peer:")?;
    let (peer_agent_id, target_agent_id) = rest.split_once('/')?;
    if peer_agent_id.trim().is_empty() || target_agent_id.trim().is_empty() {
        return None;
    }
    Some((
        peer_agent_id.trim().to_string(),
        target_agent_id.trim().to_string(),
    ))
}

fn load_local_identity_for_a2a() -> Result<crate::halo::did::DIDIdentity, String> {
    let seed_hex = std::env::var("NUCLEUSDB_AGENT_PRIVATE_KEY").map_err(|_| {
        "NUCLEUSDB_AGENT_PRIVATE_KEY is required for remote A2A delegation".to_string()
    })?;
    let seed = hex::decode(seed_hex.trim())
        .map_err(|e| format!("decode NUCLEUSDB_AGENT_PRIVATE_KEY: {e}"))?;
    if seed.len() != 64 {
        return Err("NUCLEUSDB_AGENT_PRIVATE_KEY must decode to 64 bytes".to_string());
    }
    let mut seed_arr = [0u8; 64];
    seed_arr.copy_from_slice(&seed);
    crate::halo::did::did_from_genesis_seed(&seed_arr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container::{AgentResponse, PeerInfo, PeerRegistry};
    use crate::orchestrator::container_dispatch::{
        ContainerDeinitializeResult, ContainerDeinitializeSpec, ContainerDispatch,
        ContainerInitializeSpec, ContainerPromptSpec, ContainerProvisionDefaults,
        ContainerProvisionSpec, InMemoryContainerDispatch, InitializedContainerAgent,
        ProvisionedContainer,
    };
    use crate::test_support::env_lock;
    use std::path::PathBuf;
    use std::sync::Arc;

    struct EnvVarGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let prev = std::env::var(key).ok();
            match value {
                Some(v) => {
                    // SAFETY: serialized by env_lock in every test here.
                    unsafe { std::env::set_var(key, v) };
                }
                None => {
                    // SAFETY: serialized by env_lock in every test here.
                    unsafe { std::env::remove_var(key) };
                }
            }
            Self { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(v) = self.prev.take() {
                // SAFETY: serialized by env_lock in every test here.
                unsafe { std::env::set_var(self.key, v) };
            } else {
                // SAFETY: serialized by env_lock in every test here.
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    fn test_orchestrator() -> Orchestrator {
        let pty = Arc::new(PtyManager::new(8));
        Orchestrator::new(pty, None, PathBuf::from("/tmp/orch_trace_test.ndb"))
    }

    fn test_container_orchestrator() -> Orchestrator {
        let pty = Arc::new(PtyManager::new(8));
        Orchestrator::with_budget_and_container_dispatch(
            pty,
            None,
            PathBuf::from("/tmp/orch_container_trace_test.ndb"),
            ContainerBudget::default(),
            Arc::new(InMemoryContainerDispatch::default()),
        )
    }

    struct SlowContainerDispatch;

    #[async_trait::async_trait]
    impl ContainerDispatch for SlowContainerDispatch {
        fn provision_defaults(&self) -> ContainerProvisionDefaults {
            ContainerProvisionDefaults {
                image: "nucleusdb-agent:test".to_string(),
                registry_volume: PathBuf::from("/tmp/slow-dispatch"),
                mcp_port: 7331,
            }
        }

        async fn provision(
            &self,
            spec: ContainerProvisionSpec,
        ) -> Result<ProvisionedContainer, String> {
            Ok(ProvisionedContainer {
                session_id: "sess-slow".to_string(),
                container_id: "ctr-slow".to_string(),
                image: spec.image,
                peer_agent_id: spec.peer_agent_id,
                host_sock: "/tmp/slow.sock".to_string(),
                started_at_unix: crate::pod::now_unix(),
                mesh_port: Some(spec.mcp_port),
            })
        }

        async fn initialize(
            &self,
            spec: ContainerInitializeSpec,
        ) -> Result<InitializedContainerAgent, String> {
            Ok(InitializedContainerAgent {
                container_id: "ctr-slow".to_string(),
                agent_id: format!("{}-agent", spec.peer_agent_id),
                state: "locked".to_string(),
                trace_session_id: Some("trace-slow".to_string()),
                reuse_policy: spec.reuse_policy,
            })
        }

        async fn send_prompt(&self, _spec: ContainerPromptSpec) -> Result<AgentResponse, String> {
            tokio::time::sleep(Duration::from_secs(2)).await;
            Ok(AgentResponse {
                content: "late response".to_string(),
                model: "slow-model".to_string(),
                input_tokens: 1,
                output_tokens: 1,
                cost_usd: 0.0,
                tool_calls: Vec::new(),
                duration_ms: 2000,
            })
        }

        async fn deinitialize(
            &self,
            _spec: ContainerDeinitializeSpec,
        ) -> Result<ContainerDeinitializeResult, String> {
            Ok(ContainerDeinitializeResult {
                container_id: "ctr-slow".to_string(),
                state: "empty".to_string(),
                trace_session_id: None,
                reuse_policy: ReusePolicy::Reusable,
            })
        }

        async fn destroy(&self, _session_id: &str) -> Result<(), String> {
            Ok(())
        }
    }

    fn test_slow_container_orchestrator() -> Orchestrator {
        let pty = Arc::new(PtyManager::new(8));
        Orchestrator::with_budget_and_container_dispatch(
            pty,
            None,
            PathBuf::from("/tmp/orch_container_timeout_test.ndb"),
            ContainerBudget::default(),
            Arc::new(SlowContainerDispatch),
        )
    }

    #[tokio::test]
    async fn launch_and_list_agent() {
        let orchestrator = test_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "tester".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch");
        assert_eq!(agent.agent_name, "tester");
        let all = orchestrator.list_agents().await;
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn container_launch_send_stop_roundtrip_cli() {
        let orchestrator = test_container_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "container-shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: Some("shell-model".to_string()),
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: Some(DispatchMode::Container),
                container_hookup: None,
            })
            .await
            .expect("launch container cli");
        assert_eq!(agent.agent_type, "shell");
        assert_eq!(agent.model.as_deref(), Some("shell-model"));

        let task = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id.clone(),
                task: "review this diff".to_string(),
                timeout_secs: Some(30),
                wait: true,
            })
            .await
            .expect("run container cli task");
        assert!(matches!(task.status, TaskStatus::Complete));
        assert_eq!(task.result.as_deref(), Some("cli:shell:review this diff"));

        let stopped = orchestrator
            .stop_agent(StopRequest {
                agent_id: agent.agent_id.clone(),
                force: false,
            })
            .await
            .expect("stop container cli");
        assert_eq!(stopped.status, "stopped");

        let err = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id,
                task: "should fail".to_string(),
                timeout_secs: Some(30),
                wait: true,
            })
            .await
            .expect_err("stopped container agent must reject tasks");
        assert!(err.contains("stopped"));
    }

    #[tokio::test]
    async fn container_launch_send_stop_roundtrip_api() {
        let orchestrator = test_container_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "container-api".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: true,
                capabilities: vec!["memory_read".to_string(), "memory_write".to_string()],
                dispatch_mode: Some(DispatchMode::Container),
                container_hookup: Some(ContainerHookupRequest::Api {
                    provider: "openrouter".to_string(),
                    model: "openrouter/test-model".to_string(),
                    api_key_source: "test-key".to_string(),
                    base_url_override: Some("http://127.0.0.1:18080".to_string()),
                }),
            })
            .await
            .expect("launch container api");
        assert_eq!(agent.agent_type, "openrouter");
        assert_eq!(agent.model.as_deref(), Some("openrouter/test-model"));

        let task = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id.clone(),
                task: "summarize".to_string(),
                timeout_secs: Some(30),
                wait: true,
            })
            .await
            .expect("run container api task");
        assert!(matches!(task.status, TaskStatus::Complete));
        assert_eq!(task.result.as_deref(), Some("api:openrouter:summarize"));

        let stopped = orchestrator
            .stop_agent(StopRequest {
                agent_id: agent.agent_id,
                force: false,
            })
            .await
            .expect("stop container api");
        assert_eq!(stopped.status, "stopped");
    }

    #[tokio::test]
    async fn container_launch_send_stop_roundtrip_local_model() {
        let orchestrator = test_container_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "container-local".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: true,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: Some(DispatchMode::Container),
                container_hookup: Some(ContainerHookupRequest::LocalModel {
                    model_id: "meta-llama/Llama-3.1-8B-Instruct".to_string(),
                    vllm_port: Some(8000),
                    base_url_override: Some("http://127.0.0.1:18081".to_string()),
                }),
            })
            .await
            .expect("launch container local model");
        assert_eq!(agent.agent_type, "local_model");
        assert_eq!(
            agent.model.as_deref(),
            Some("meta-llama/Llama-3.1-8B-Instruct")
        );

        let task = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id.clone(),
                task: "write tests".to_string(),
                timeout_secs: Some(30),
                wait: true,
            })
            .await
            .expect("run container local task");
        assert!(matches!(task.status, TaskStatus::Complete));
        assert_eq!(
            task.result.as_deref(),
            Some("local:meta-llama/Llama-3.1-8B-Instruct:write tests")
        );

        let stopped = orchestrator
            .stop_agent(StopRequest {
                agent_id: agent.agent_id,
                force: true,
            })
            .await
            .expect("force stop container local");
        assert_eq!(stopped.status, "stopped");
    }

    #[tokio::test]
    async fn container_task_timeout_marks_timeout_and_recovers_agent() {
        let orchestrator = test_slow_container_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "container-timeout".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 1,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: Some(DispatchMode::Container),
                container_hookup: Some(ContainerHookupRequest::Cli {
                    cli_name: "shell".to_string(),
                    model: None,
                }),
            })
            .await
            .expect("launch slow container agent");

        let err = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id.clone(),
                task: "timeout me".to_string(),
                timeout_secs: Some(1),
                wait: true,
            })
            .await
            .err()
            .expect("container task should time out");
        assert!(err.contains("timed out"));

        let tasks = orchestrator.list_tasks().await;
        assert!(
            tasks.iter().any(|task| {
                task.agent_id == agent.agent_id && task.status == TaskStatus::Timeout
            }),
            "expected timeout task for {}",
            agent.agent_id
        );

        let managed = orchestrator
            .list_agents()
            .await
            .into_iter()
            .find(|candidate| candidate.agent_id == agent.agent_id)
            .expect("agent still listed after timeout");
        assert!(
            matches!(
                managed.status,
                crate::orchestrator::agent_pool::AgentStatus::Idle
            ),
            "expected idle agent after timeout, got {:?}",
            managed.status
        );
    }

    #[tokio::test]
    async fn task_state_machine_completes_for_shell() {
        let orchestrator = test_orchestrator();
        let output_dir = tempfile::tempdir().expect("tempdir");
        let output_path = output_dir.path().join("orchestrator_shell_output.txt");
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch");
        let task = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id,
                task: format!(
                    "printf 'hello orchestrator' > '{}' && cat '{}'",
                    output_path.display(),
                    output_path.display()
                ),
                timeout_secs: Some(30),
                wait: true,
            })
            .await
            .expect("task");
        assert!(matches!(task.status, TaskStatus::Complete));
        let output = std::fs::read_to_string(&output_path).expect("orchestrator shell output");
        assert!(output.contains("hello orchestrator"));
    }

    #[tokio::test]
    async fn stop_agent_cancels_background_task() {
        let orchestrator = test_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch");
        let submitted = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id.clone(),
                task: "sleep 5".to_string(),
                timeout_secs: Some(30),
                wait: false,
            })
            .await
            .expect("submit task");
        let stopped = orchestrator
            .stop_agent(StopRequest {
                agent_id: agent.agent_id,
                force: false,
            })
            .await
            .expect("stop agent");
        assert_eq!(stopped.status, "stopped");

        let task = orchestrator
            .get_task(&submitted.task_id)
            .await
            .expect("task exists");
        assert!(matches!(
            task.status,
            TaskStatus::Failed | TaskStatus::Timeout | TaskStatus::Complete
        ));
    }

    #[tokio::test]
    async fn background_handle_is_cleaned_up_after_fast_task() {
        let orchestrator = test_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch");
        let submitted = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id,
                task: "printf 'done'".to_string(),
                timeout_secs: Some(30),
                wait: false,
            })
            .await
            .expect("submit task");
        // Poll until the background handle is cleaned up (up to 5s under load)
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let handle_count = orchestrator.inner.background_runs.lock().await.len();
            if handle_count == 0 {
                break;
            }
        }
        let handle_count = orchestrator.inner.background_runs.lock().await.len();
        assert_eq!(
            handle_count, 0,
            "background handle not cleaned up within 5s"
        );
        let task = orchestrator
            .get_task(&submitted.task_id)
            .await
            .expect("task exists");
        assert!(matches!(task.status, TaskStatus::Complete));
    }

    #[test]
    fn mesh_status_returns_empty_when_not_enabled() {
        let _env_lock = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _agent_id = EnvVarGuard::set("NUCLEUSDB_MESH_AGENT_ID", None);
        let _mesh_path = EnvVarGuard::set("NUCLEUSDB_MESH_REGISTRY", None);
        let orchestrator = test_orchestrator();
        let status = orchestrator.mesh_status();
        assert!(!status.enabled);
        assert!(status.peers.is_empty());
        assert!(status.network_name.is_none());
    }

    #[tokio::test]
    async fn mesh_status_reads_registry_when_enabled() {
        let _env_lock = env_lock().lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("tempdir");
        let registry_path = dir.path().join("mesh-peers.json");
        let _agent_id = EnvVarGuard::set("NUCLEUSDB_MESH_AGENT_ID", Some("agent-self"));
        let _mesh_path = EnvVarGuard::set(
            "NUCLEUSDB_MESH_REGISTRY",
            Some(registry_path.to_string_lossy().as_ref()),
        );

        let mut registry = PeerRegistry::new();
        registry.register(PeerInfo {
            agent_id: "agent-peer".to_string(),
            container_name: "peer-node".to_string(),
            did_uri: Some("did:key:z6MkPeer".to_string()),
            mcp_endpoint: "http://127.0.0.1:1/mcp".to_string(),
            discovery_endpoint: "http://127.0.0.1:1/pod/.well-known/nucleus-pod".to_string(),
            registered_at: crate::pod::now_unix(),
            last_seen: crate::pod::now_unix(),
        });
        registry
            .save(registry_path.as_path())
            .expect("save registry");

        let orchestrator = test_orchestrator();
        let status = orchestrator.mesh_status();
        assert!(status.enabled);
        assert_eq!(status.self_agent_id.as_deref(), Some("agent-self"));
        assert_eq!(
            status.network_name.as_deref(),
            Some(crate::container::MESH_NETWORK_NAME)
        );
        assert_eq!(status.peers.len(), 1);
        assert_eq!(status.peers[0].agent_id, "agent-peer");
        assert!(!status.peers[0].reachable);
    }

    #[tokio::test]
    async fn orchestrator_with_budget_propagates_to_pool() {
        let budget = ContainerBudget {
            max_agents: 3,
            max_concurrent_busy: 2,
            allowed_kinds: vec!["shell".to_string(), "codex".to_string()],
        };
        let orchestrator = Orchestrator::with_budget(
            Arc::new(PtyManager::new(8)),
            None,
            PathBuf::from("/tmp/orch_budget_status.ndb"),
            budget.clone(),
        );
        let status = orchestrator.status().await;
        assert_eq!(status.budget, budget);
    }

    #[tokio::test]
    async fn orchestrator_send_task_enforces_busy_budget() {
        let budget = ContainerBudget {
            max_agents: 4,
            max_concurrent_busy: 1,
            allowed_kinds: Vec::new(),
        };
        let orchestrator = Orchestrator::with_budget(
            Arc::new(PtyManager::new(8)),
            None,
            PathBuf::from("/tmp/orch_busy_budget.ndb"),
            budget,
        );
        let first = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "first".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch first");
        let second = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "second".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch second");

        let _first_task = orchestrator
            .send_task(SendTaskRequest {
                agent_id: first.agent_id.clone(),
                task: "sleep 2".to_string(),
                timeout_secs: Some(10),
                wait: false,
            })
            .await
            .expect("first task submit");

        for _ in 0..20 {
            if orchestrator.status().await.agents_busy >= 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }

        let err = orchestrator
            .send_task(SendTaskRequest {
                agent_id: second.agent_id.clone(),
                task: "printf hi".to_string(),
                timeout_secs: Some(10),
                wait: true,
            })
            .await
            .expect_err("busy budget should reject second task");
        assert!(err.contains("concurrent busy limit reached"));

        let _ = orchestrator
            .stop_agent(StopRequest {
                agent_id: first.agent_id,
                force: true,
            })
            .await;
        let _ = orchestrator
            .stop_agent(StopRequest {
                agent_id: second.agent_id,
                force: true,
            })
            .await;
    }

    #[tokio::test]
    async fn orchestrator_status_includes_counts() {
        let orchestrator = test_orchestrator();
        let busy_agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "busy".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch busy");
        let idle_agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "idle".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch idle");
        let stopped_agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "stopped".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
                dispatch_mode: None,
                container_hookup: None,
            })
            .await
            .expect("launch stopped");

        let _ = orchestrator
            .inner
            .pool
            .start_task(&busy_agent.agent_id, "task-busy", "sleep 5", Some(30))
            .await
            .expect("start busy task");
        let _ = orchestrator
            .stop_agent(StopRequest {
                agent_id: stopped_agent.agent_id.clone(),
                force: true,
            })
            .await
            .expect("stop one agent");

        let status = orchestrator.status().await;
        assert_eq!(status.agents_total, 3);
        assert!(status.agents_busy >= 1);
        assert!(status.agents_idle >= 1);
        assert!(status.agents_stopped >= 1);

        let _ = orchestrator
            .stop_agent(StopRequest {
                agent_id: busy_agent.agent_id,
                force: true,
            })
            .await;
        let _ = orchestrator
            .stop_agent(StopRequest {
                agent_id: idle_agent.agent_id,
                force: true,
            })
            .await;
    }
}
