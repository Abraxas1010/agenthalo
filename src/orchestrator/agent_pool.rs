use crate::cockpit::pty_manager::PtyManager;
use crate::halo::vault::Vault;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

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
    pub model: Option<String>,
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
    #[serde(skip)]
    pub env_remove: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LaunchSpec {
    pub agent: String,
    pub agent_name: String,
    pub working_dir: Option<String>,
    pub env: BTreeMap<String, String>,
    pub timeout_secs: u64,
    pub model: Option<String>,
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

const MAX_MANAGED_AGENTS: usize = 64;

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
        {
            let agents = self.agents.lock().await;
            if agents.len() >= MAX_MANAGED_AGENTS {
                return Err(format!(
                    "maximum managed agents reached ({MAX_MANAGED_AGENTS})"
                ));
            }
        }
        let agent_id = format!(
            "orch-{}-{}",
            crate::pod::now_unix(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        let env = resolve_env_vars(&spec.env, self.vault.as_ref())?;
        let (command, mut static_args, mut env_remove) = command_for_kind(&kind);
        if let Some(model) = spec
            .model
            .as_ref()
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
        {
            static_args.extend(model_args_for_kind(&kind, model));
        }
        let explicit_env_keys = BTreeSet::from_iter(env.iter().map(|(k, _)| k.as_str()));
        env_remove.retain(|key| !explicit_env_keys.contains(key.as_str()));
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
            model: spec.model.filter(|m| !m.trim().is_empty()),
            working_dir: spec.working_dir.filter(|v| !v.trim().is_empty()),
            tasks_completed: 0,
            total_cost_usd: 0.0,
            trace_enabled: spec.trace,
            command,
            static_args,
            env,
            env_remove,
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
        let session_id = {
            let agents = self.agents.lock().await;
            let agent = agents
                .get(agent_id)
                .ok_or_else(|| format!("unknown agent_id {agent_id}"))?;
            agent.pty_session_id.clone()
        };

        if let Some(session_id) = session_id {
            if let Some(session) = self.pty_manager.get_session(&session_id) {
                if !force {
                    let _ = session.write_input(&[3]); // SIGINT (^C)
                    for _ in 0..20 {
                        if session.poll_exit_status().is_some() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
                let _ = session.terminate();
            }
            let _ = self.pty_manager.destroy_session(&session_id);
        }

        let mut agents = self.agents.lock().await;
        let agent = agents
            .get_mut(agent_id)
            .ok_or_else(|| format!("unknown agent_id {agent_id}"))?;
        agent.pty_session_id = None;
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
            // Shell: prompt is the command string after `sh -c`
            "shell" => args.push(prompt.to_string()),
            // Claude CLI: prompt is a positional argument (not a flag)
            // Codex CLI: prompt is a positional argument after `exec`
            "claude" | "codex" => args.push(prompt.to_string()),
            // Gemini CLI uses -p/--prompt flag
            "gemini" => {
                args.push("--prompt".to_string());
                args.push(prompt.to_string());
            }
            // OpenClaw uses --message flag (per `openclaw agent --message`)
            "openclaw" => {
                args.push("--message".to_string());
                args.push(prompt.to_string());
            }
            // Unknown kinds: positional arg as safest default
            _ => args.push(prompt.to_string()),
        }
        let session_id = self
            .pty_manager
            .create_session_with_env_control(
                &agent.command,
                &args,
                agent.env.clone(),
                &agent.env_remove,
                agent.working_dir.as_deref(),
                120,
                24,
                Some(agent.agent_type.clone()),
            )
            .map_err(|e| format!("start task PTY session failed: {e}"))?;
        agent.pty_session_id = Some(session_id.clone());
        agent.status = AgentStatus::Busy {
            task_id: task_id.to_string(),
        };
        Ok(TaskExecution {
            agent_id: agent.agent_id.clone(),
            agent_type: agent.agent_type.clone(),
            task_id: task_id.to_string(),
            session_id,
            timeout_secs: timeout_secs.unwrap_or(agent.timeout_secs).clamp(1, 3600),
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

fn command_for_kind(kind: &str) -> (String, Vec<String>, Vec<String>) {
    match kind {
        "claude" => (
            "claude".to_string(),
            vec![
                "--print".to_string(),
                "--output-format".to_string(),
                "json".to_string(),
                "--verbose".to_string(),
                // Non-interactive orchestrator lanes cannot satisfy permission prompts.
                // Keep this explicit because it materially elevates tool authority.
                "--dangerously-skip-permissions".to_string(),
            ],
            vec!["CLAUDECODE".to_string()],
        ),
        "codex" => (
            "codex".to_string(),
            vec![
                "exec".to_string(),
                "--full-auto".to_string(),
                "--json".to_string(),
                "--skip-git-repo-check".to_string(),
            ],
            vec!["CODEX_CLI".to_string()],
        ),
        "gemini" => ("gemini".to_string(), vec!["--yolo".to_string()], Vec::new()),
        "openclaw" => (
            "openclaw".to_string(),
            vec!["run".to_string(), "--non-interactive".to_string()],
            Vec::new(),
        ),
        _ => (
            "sh".to_string(),
            vec!["-c".to_string()],
            vec![
                "ENV".to_string(),
                "BASH_ENV".to_string(),
                "PROMPT_COMMAND".to_string(),
            ],
        ),
    }
}

fn model_args_for_kind(kind: &str, model: &str) -> Vec<String> {
    match kind {
        "claude" | "codex" | "gemini" => vec!["--model".to_string(), model.to_string()],
        _ => Vec::new(),
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
    use std::path::{Path, PathBuf};

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "orch_agent_pool_test_{}_{}_{}",
            tag,
            std::process::id(),
            crate::pod::now_unix()
        ))
    }

    fn make_wallet(path: &Path, key_id: &str, seed_hex: &str) {
        let wallet = serde_json::json!({
            "version": 1,
            "algorithm": "ml_dsa65",
            "key_id": key_id,
            "public_key_hex": "00",
            "secret_seed_hex": seed_hex,
            "created_at": crate::pod::now_unix(),
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&wallet).expect("serialize wallet"),
        )
        .expect("write wallet");
    }

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

    #[test]
    fn command_for_kind_defines_env_removals_for_nested_clis() {
        let (_, _, claude_remove) = command_for_kind("claude");
        assert!(claude_remove.contains(&"CLAUDECODE".to_string()));
        let (_, _, codex_remove) = command_for_kind("codex");
        assert!(codex_remove.contains(&"CODEX_CLI".to_string()));
    }

    #[test]
    fn codex_command_uses_exec_noninteractive_mode() {
        let (cmd, args, _) = command_for_kind("codex");
        assert_eq!(cmd, "codex");
        assert!(args.iter().any(|a| a == "exec"));
        assert!(args.iter().any(|a| a == "--full-auto"));
        assert!(args.iter().any(|a| a == "--json"));
        assert!(args.iter().any(|a| a == "--skip-git-repo-check"));
    }

    #[test]
    fn shell_command_uses_non_login_mode() {
        let (_, shell_args, shell_remove) = command_for_kind("shell");
        assert_eq!(shell_args, vec!["-c".to_string()]);
        assert!(shell_remove.contains(&"ENV".to_string()));
        assert!(shell_remove.contains(&"BASH_ENV".to_string()));
        assert!(shell_remove.contains(&"PROMPT_COMMAND".to_string()));
    }

    #[test]
    fn model_args_only_emitted_for_supported_clis() {
        assert_eq!(
            model_args_for_kind("claude", "claude-3-7"),
            vec!["--model".to_string(), "claude-3-7".to_string()]
        );
        assert_eq!(
            model_args_for_kind("gemini", "gemini-2.5-pro"),
            vec!["--model".to_string(), "gemini-2.5-pro".to_string()]
        );
        assert_eq!(
            model_args_for_kind("shell", "irrelevant"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn resolve_env_vars_rejects_vault_provider_without_vault() {
        let mut env = BTreeMap::new();
        env.insert("OPENAI_API_KEY".to_string(), "vault:openai".to_string());
        let err = resolve_env_vars(&env, None).expect_err("vault provider without vault must fail");
        assert!(err.contains("vault unavailable"));
    }

    #[test]
    fn resolve_env_vars_uses_vault_provider_value() {
        let wallet_path = temp_path("wallet.json");
        let vault_path = temp_path("vault.enc");
        make_wallet(
            &wallet_path,
            "kid-orch-test",
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        );

        let vault = Arc::new(
            Vault::open(&wallet_path, &vault_path).expect("open vault for orchestrator env test"),
        );
        vault
            .set_key("openai", "OPENAI_API_KEY", "sk-test-xyz")
            .expect("set test key");

        let mut env = BTreeMap::new();
        env.insert("OPENAI_API_KEY".to_string(), "vault:openai".to_string());
        let out = resolve_env_vars(&env, Some(&vault)).expect("resolve env vars via vault");
        assert_eq!(
            out,
            vec![("OPENAI_API_KEY".to_string(), "sk-test-xyz".to_string())]
        );

        let _ = std::fs::remove_file(wallet_path);
        let _ = std::fs::remove_file(vault_path);
    }

    #[tokio::test]
    async fn launch_respects_max_managed_agents_limit() {
        let pool = AgentPool::new(Arc::new(PtyManager::new(64)), None);
        for idx in 0..MAX_MANAGED_AGENTS {
            pool.launch(LaunchSpec {
                agent: "shell".to_string(),
                agent_name: format!("a{idx}"),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 10,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
            })
            .await
            .expect("launch within limit");
        }
        let err = pool
            .launch(LaunchSpec {
                agent: "shell".to_string(),
                agent_name: "overflow".to_string(),
                working_dir: None,
                env: BTreeMap::new(),
                timeout_secs: 10,
                model: None,
                trace: false,
                capabilities: vec!["memory_read".to_string()],
            })
            .await
            .expect_err("must reject overflow");
        assert!(err.contains("maximum managed agents"));
    }
}
