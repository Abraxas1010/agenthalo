use crate::cockpit::pty_manager::PtyManager;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Busy { task_id: String },
    Stopped { exit_code: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedAgent {
    pub agent_id: String,
    pub agent_name: String,
    pub agent_type: String,
    pub pty_session_id: Option<String>,
    pub capabilities: Vec<String>,
    pub status: AgentStatus,
    pub launched_at: u64,
    pub timeout_secs: u64,
    pub working_dir: Option<String>,
    pub tasks_completed: u32,
    pub total_cost_usd: f64,
    pub trace_enabled: bool,
    #[serde(skip)]
    pub command: String,
    #[serde(skip)]
    pub static_args: Vec<String>,
    #[serde(skip)]
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub agent: String,
    pub agent_name: String,
    pub working_dir: Option<String>,
    pub env: BTreeMap<String, String>,
    pub timeout_secs: u64,
    pub trace: bool,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TaskExecution {
    pub agent_id: String,
    pub agent_type: String,
    pub task_id: String,
    pub session_id: String,
    pub timeout_secs: u64,
    pub trace_enabled: bool,
}

pub struct AgentPool {
    pty_manager: Arc<PtyManager>,
    vault: Option<Arc<Vault>>,
    agents: tokio::sync::Mutex<BTreeMap<String, ManagedAgent>>,
}

impl std::fmt::Debug for AgentPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentPool").finish_non_exhaustive()
    }
}

impl AgentPool {
    pub fn new(pty_manager: Arc<PtyManager>, vault: Option<Arc<Vault>>) -> Self {
        Self {
            pty_manager,
            vault,
            agents: tokio::sync::Mutex::new(BTreeMap::new()),
        }
    }

    pub async fn launch(&self, spec: LaunchSpec) -> Result<ManagedAgent, String> {
        let kind = normalize_agent_kind(&spec.agent)?;
        validate_capabilities(&spec.capabilities)?;
        let agent_id = format!(
            "orch-{}-{}",
            crate::pod::now_unix(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let env = resolve_env_vars(&spec.env, self.vault.as_ref())?;
        let (command, static_args) = command_for_kind(&kind);
        let managed = ManagedAgent {
            agent_id: agent_id.clone(),
            agent_name: if spec.agent_name.trim().is_empty() {
                kind.clone()
            } else {
                spec.agent_name.trim().to_string()
            },
            agent_type: kind,
            pty_session_id: None,
            capabilities: normalize_capabilities(spec.capabilities),
            status: AgentStatus::Idle,
            launched_at: crate::pod::now_unix(),
            timeout_secs: spec.timeout_secs.max(5),
            working_dir: spec.working_dir.filter(|v| !v.trim().is_empty()),
            tasks_completed: 0,
            total_cost_usd: 0.0,
            trace_enabled: spec.trace,
            command,
            static_args,
            env,
        };
        let mut agents = self.agents.lock().await;
        agents.insert(agent_id, managed.clone());
        Ok(managed)
    }

    pub async fn list(&self) -> Vec<ManagedAgent> {
        let agents = self.agents.lock().await;
        agents.values().cloned().collect()
    }

    pub async fn get(&self, agent_id: &str) -> Option<ManagedAgent> {
        let agents = self.agents.lock().await;
        agents.get(agent_id).cloned()
    }

    pub async fn stop(&self, agent_id: &str, force: bool) -> Result<ManagedAgent, String> {
        let mut agents = self.agents.lock().await;
        let agent = agents
            .get_mut(agent_id)
            .ok_or_else(|| format!("unknown agent_id {agent_id}"))?;
        if let Some(session_id) = agent.pty_session_id.clone() {
            if force {
                if let Some(session) = self.pty_manager.get_session(&session_id) {
                    let _ = session.terminate();
                }
            }
        }
        agent.status = AgentStatus::Stopped { exit_code: 0 };
        let stopped = agent.clone();
        Ok(stopped)
    }

    pub async fn start_task(
        &self,
        agent_id: &str,
        task_id: &str,
        prompt: &str,
        timeout_secs: Option<u64>,
    ) -> Result<TaskExecution, String> {
        let mut agents = self.agents.lock().await;
        let agent = agents
            .get_mut(agent_id)
            .ok_or_else(|| format!("unknown agent_id {agent_id}"))?;
        if matches!(agent.status, AgentStatus::Stopped { .. }) {
            return Err(format!("agent {agent_id} is stopped"));
        }
        if matches!(agent.status, AgentStatus::Busy { .. }) {
            return Err(format!("agent {agent_id} is busy"));
        }
        let mut args = agent.static_args.clone();
        match agent.agent_type.as_str() {
            "shell" => args.push(prompt.to_string()),
            _ => {
                args.push("--prompt".to_string());
                args.push(prompt.to_string());
            }
        }
        let session_id = self.pty_manager.create_session(
            &agent.command,
            &args,
            agent.env.clone(),
            agent.working_dir.as_deref(),
            120,
            24,
            Some(agent.agent_type.clone()),
        )?;
        agent.pty_session_id = Some(session_id.clone());
        agent.status = AgentStatus::Busy {
            task_id: task_id.to_string(),
        };
        Ok(TaskExecution {
            agent_id: agent.agent_id.clone(),
            agent_type: agent.agent_type.clone(),
            task_id: task_id.to_string(),
            session_id,
            timeout_secs: timeout_secs.unwrap_or(agent.timeout_secs).max(1),
            trace_enabled: agent.trace_enabled,
        })
    }

    pub async fn complete_task(&self, agent_id: &str, cost_usd: f64) {
        let mut agents = self.agents.lock().await;
        if let Some(agent) = agents.get_mut(agent_id) {
            if !matches!(agent.status, AgentStatus::Stopped { .. }) {
                agent.status = AgentStatus::Idle;
            }
            agent.tasks_completed = agent.tasks_completed.saturating_add(1);
            agent.total_cost_usd += cost_usd.max(0.0);
            agent.pty_session_id = None;
        }
    }

    pub async fn capability_check(&self, agent_id: &str, required: &str) -> Result<(), String> {
        let agents = self.agents.lock().await;
        let agent = agents
            .get(agent_id)
            .ok_or_else(|| format!("unknown agent_id {agent_id}"))?;
        let allowed = agent.capabilities.iter().any(|c| c == "*" || c == required);
        if allowed {
            Ok(())
        } else {
            Err(format!(
                "agent '{}' lacks capability '{}'",
                agent.agent_name, required
            ))
        }
    }

    pub fn pty_session_by_id(
        &self,
        session_id: &str,
    ) -> Option<Arc<crate::cockpit::pty_manager::PtySession>> {
        self.pty_manager.get_session(session_id)
    }

    pub async fn current_session_for_agent(
        &self,
        agent_id: &str,
    ) -> Option<Arc<crate::cockpit::pty_manager::PtySession>> {
        let agents = self.agents.lock().await;
        let agent = agents.get(agent_id)?;
        let session_id = agent.pty_session_id.as_deref()?;
        self.pty_manager.get_session(session_id)
    }
}

fn normalize_agent_kind(raw: &str) -> Result<String, String> {
    let kind = raw.trim().to_ascii_lowercase();
    match kind.as_str() {
        "claude" | "codex" | "gemini" | "openclaw" | "shell" => Ok(kind),
        _ => Err(format!("unsupported agent kind '{raw}'")),
    }
}

fn command_for_kind(kind: &str) -> (String, Vec<String>) {
    match kind {
        "claude" => (
            "claude".to_string(),
            vec![
                "--print".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "--verbose".to_string(),
                "--dangerously-skip-permissions".to_string(),
            ],
        ),
        "codex" => (
            "codex".to_string(),
            vec![
                "--quiet".to_string(),
                "--approval-mode".to_string(),
                "full-auto".to_string(),
            ],
        ),
        "gemini" => ("gemini".to_string(), vec!["--non-interactive".to_string()]),
        "openclaw" => (
            "openclaw".to_string(),
            vec!["run".to_string(), "--non-interactive".to_string()],
        ),
        _ => ("sh".to_string(), vec!["-lc".to_string()]),
    }
}

fn normalize_capabilities(mut capabilities: Vec<String>) -> Vec<String> {
    if capabilities.is_empty() {
        capabilities = vec!["memory_read".to_string(), "memory_write".to_string()];
    }
    let mut set = BTreeSet::new();
    for cap in capabilities {
        if !cap.trim().is_empty() {
            set.insert(cap.trim().to_ascii_lowercase());
        }
    }
    set.into_iter().collect()
}

fn validate_capabilities(capabilities: &[String]) -> Result<(), String> {
    let allowed = BTreeSet::from_iter(
        [
            "*",
            "memory_read",
            "memory_write",
            "sql_read",
            "sql_write",
            "container_launch",
            "orchestrator_pipe",
        ]
        .into_iter()
        .map(str::to_string),
    );
    for cap in capabilities {
        if !allowed.contains(&cap.to_ascii_lowercase()) {
            return Err(format!("unknown capability '{cap}'"));
        }
    }
    Ok(())
}

pub fn resolve_env_vars(
    env: &BTreeMap<String, String>,
    vault: Option<&Arc<Vault>>,
) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::with_capacity(env.len());
    for (k, v) in env {
        if let Some(provider) = v.strip_prefix("vault:") {
            let vault = vault.ok_or_else(|| format!("vault unavailable for key {k}"))?;
            let value = vault.get_key(provider.trim())?;
            out.push((k.clone(), value));
        } else {
            out.push((k.clone(), v.clone()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_env_vars_plain_works() {
        let mut env = BTreeMap::new();
        env.insert("A".to_string(), "B".to_string());
        let out = resolve_env_vars(&env, None).expect("resolve plain env");
        assert_eq!(out, vec![("A".to_string(), "B".to_string())]);
    }

    #[test]
    fn capability_validation_rejects_unknown() {
        let err = validate_capabilities(&["invalid_cap".to_string()]).expect_err("must reject");
        assert!(err.contains("unknown capability"));
    }
}
