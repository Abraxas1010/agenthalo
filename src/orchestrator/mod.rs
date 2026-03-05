pub mod a2a;
pub mod agent_pool;
pub mod task;
pub mod task_graph;
pub mod trace_bridge;

use crate::cockpit::pty_manager::PtyManager;
use crate::halo::vault::Vault;
use crate::orchestrator::agent_pool::{AgentPool, LaunchSpec, ManagedAgent};
use crate::orchestrator::task::{Task, TaskStatus, TaskUsage};
use crate::orchestrator::task_graph::{PipeTransform, TaskEdge, TaskGraph};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaunchAgentRequest {
    pub agent: String,
    pub agent_name: String,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub timeout_secs: u64,
    pub trace: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
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

#[derive(Debug, Clone)]
pub struct Orchestrator {
    inner: Arc<OrchestratorInner>,
}

#[derive(Debug)]
struct OrchestratorInner {
    pool: AgentPool,
    tasks: tokio::sync::Mutex<BTreeMap<String, Task>>,
    graph: tokio::sync::Mutex<TaskGraph>,
    trace_db_path: PathBuf,
    background_runs: tokio::sync::Mutex<BTreeMap<String, tokio::task::JoinHandle<()>>>,
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
        Self {
            inner: Arc::new(OrchestratorInner {
                pool: AgentPool::new(pty_manager, vault),
                tasks: tokio::sync::Mutex::new(BTreeMap::new()),
                graph: tokio::sync::Mutex::new(TaskGraph::default()),
                trace_db_path,
                background_runs: tokio::sync::Mutex::new(BTreeMap::new()),
            }),
        }
    }

    pub async fn launch_agent(&self, req: LaunchAgentRequest) -> Result<ManagedAgent, String> {
        self.inner
            .pool
            .launch(LaunchSpec {
                agent: req.agent,
                agent_name: req.agent_name,
                working_dir: req.working_dir,
                env: req.env,
                timeout_secs: req.timeout_secs.clamp(5, MAX_TASK_TIMEOUT_SECS),
                trace: req.trace,
                capabilities: req.capabilities,
            })
            .await
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
                } else {
                    updated.mark_failed(
                        format!("task exited with code {}", outcome.exit_code),
                        Some(outcome.exit_code),
                    );
                    updated.result = Some(outcome.output.clone());
                }
                {
                    let mut tasks = self.inner.tasks.lock().await;
                    tasks.insert(task_id.clone(), updated.clone());
                }
                {
                    let mut graph = self.inner.graph.lock().await;
                    graph.upsert_node(&updated.task_id, &updated.agent_id, updated.status.clone());
                }
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
        let edges = {
            let graph = self.inner.graph.lock().await;
            graph.outgoing_for(&task.task_id)
        };
        let mut out = Vec::new();
        for edge in edges {
            let transformed = edge.transform.apply(&result);
            out.push((
                task.task_id.clone(),
                edge.target_agent_id.clone(),
                transformed,
            ));
        }
        out
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
            let input = transform.apply(source.result.as_deref().unwrap_or_default());
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
        self.inner.pool.list().await
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
    use std::path::PathBuf;

    fn test_orchestrator() -> Orchestrator {
        let pty = Arc::new(PtyManager::new(8));
        Orchestrator::new(pty, None, PathBuf::from("/tmp/orch_trace_test.ndb"))
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
                trace: false,
                capabilities: vec!["memory_read".to_string()],
            })
            .await
            .expect("launch");
        assert_eq!(agent.agent_name, "tester");
        let all = orchestrator.list_agents().await;
        assert_eq!(all.len(), 1);
    }

    #[tokio::test]
    async fn task_state_machine_completes_for_shell() {
        let orchestrator = test_orchestrator();
        let agent = orchestrator
            .launch_agent(LaunchAgentRequest {
                agent: "shell".to_string(),
                agent_name: "shell".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 30,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
            })
            .await
            .expect("launch");
        let task = orchestrator
            .send_task(SendTaskRequest {
                agent_id: agent.agent_id,
                task: "printf 'hello orchestrator'".to_string(),
                timeout_secs: Some(30),
                wait: true,
            })
            .await
            .expect("task");
        assert!(matches!(task.status, TaskStatus::Complete));
        assert!(task
            .result
            .unwrap_or_default()
            .contains("hello orchestrator"));
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
                trace: false,
                capabilities: vec!["memory_read".to_string()],
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
                trace: false,
                capabilities: vec!["memory_read".to_string()],
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
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        let handle_count = orchestrator.inner.background_runs.lock().await.len();
        assert_eq!(handle_count, 0);
        let task = orchestrator
            .get_task(&submitted.task_id)
            .await
            .expect("task exists");
        assert!(matches!(task.status, TaskStatus::Complete));
    }
}
