use crate::halo::config;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ContainerAgentState {
    Empty,
    Locked {
        agent_kind: AgentHookupKind,
        agent_id: String,
        initialized_at_unix: u64,
        trace_session_id: Option<String>,
    },
    Deinitializing {
        agent_kind: AgentHookupKind,
        agent_id: String,
        deinit_started_at_unix: u64,
        trace_session_id: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "hookup", rename_all = "snake_case")]
pub enum AgentHookupKind {
    Cli { cli_name: String },
    Api { provider: String },
    LocalModel { model_id: String },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReusePolicy {
    Reusable,
    SingleUse,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    pub at_unix: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContainerAgentLock {
    pub state: ContainerAgentState,
    pub container_id: String,
    pub reuse_policy: ReusePolicy,
    #[serde(default)]
    pub state_history: Vec<StateTransition>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeinitContext {
    pub agent_kind: AgentHookupKind,
    pub agent_id: String,
    pub trace_session_id: Option<String>,
}

impl Default for ReusePolicy {
    fn default() -> Self {
        Self::Reusable
    }
}

impl ContainerAgentLock {
    pub fn load_or_create(container_id: &str) -> Result<Self, String> {
        let path = config::agent_lock_path();
        Self::load_or_create_at(&path, container_id)
    }

    pub fn load_or_create_at(path: &Path, container_id: &str) -> Result<Self, String> {
        if path.exists() {
            let raw = std::fs::read(path)
                .map_err(|e| format!("read agent lock {}: {e}", path.display()))?;
            let lock: Self = serde_json::from_slice(&raw)
                .map_err(|e| format!("parse agent lock {}: {e}", path.display()))?;
            return Ok(lock);
        }
        let lock = Self {
            state: ContainerAgentState::Empty,
            container_id: container_id.to_string(),
            reuse_policy: ReusePolicy::Reusable,
            state_history: Vec::new(),
        };
        lock.save_to_path(path)?;
        Ok(lock)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config::agent_lock_path();
        self.save_to_path(&path)
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        ensure_lock_parent_dir(path)?;
        let tmp = temp_path(&path);
        let raw =
            serde_json::to_vec_pretty(self).map_err(|e| format!("serialize agent lock: {e}"))?;
        write_private_file(&tmp, &raw)?;
        std::fs::rename(&tmp, &path)
            .map_err(|e| format!("commit agent lock {}: {e}", path.display()))?;
        Ok(())
    }

    pub fn initialize(&mut self, kind: AgentHookupKind, agent_id: String) -> Result<(), String> {
        if !self.can_initialize() {
            return Err(lock_busy_message(&self.state));
        }
        let from = self.state_label();
        self.state = ContainerAgentState::Locked {
            agent_kind: kind,
            agent_id,
            initialized_at_unix: now_unix_secs(),
            trace_session_id: None,
        };
        self.push_transition(from, self.state_label(), "agent_initialized");
        Ok(())
    }

    pub fn attach_trace_session(&mut self, trace_session_id: Option<String>) -> Result<(), String> {
        match &mut self.state {
            ContainerAgentState::Locked {
                trace_session_id: current,
                ..
            } => {
                *current = trace_session_id;
                Ok(())
            }
            _ => Err("cannot attach trace session unless container is locked".to_string()),
        }
    }

    pub fn begin_deinitialize(&mut self) -> Result<DeinitContext, String> {
        let (agent_kind, agent_id, trace_session_id) = match &self.state {
            ContainerAgentState::Locked {
                agent_kind,
                agent_id,
                trace_session_id,
                ..
            } => (
                agent_kind.clone(),
                agent_id.clone(),
                trace_session_id.clone(),
            ),
            _ => {
                return Err("cannot begin deinitialize unless container is locked".to_string());
            }
        };
        let from = self.state_label();
        self.state = ContainerAgentState::Deinitializing {
            agent_kind: agent_kind.clone(),
            agent_id: agent_id.clone(),
            deinit_started_at_unix: now_unix_secs(),
            trace_session_id: trace_session_id.clone(),
        };
        self.push_transition(from, self.state_label(), "agent_deinitializing");
        Ok(DeinitContext {
            agent_kind,
            agent_id,
            trace_session_id,
        })
    }

    pub fn complete_deinitialize(&mut self) -> Result<ReusePolicy, String> {
        if !matches!(self.state, ContainerAgentState::Deinitializing { .. }) {
            return Err(
                "cannot complete deinitialize unless container is deinitializing".to_string(),
            );
        }
        let from = self.state_label();
        self.state = ContainerAgentState::Empty;
        self.push_transition(from, self.state_label(), "agent_deinitialized");
        Ok(self.reuse_policy)
    }

    pub fn can_initialize(&self) -> bool {
        matches!(self.state, ContainerAgentState::Empty)
    }

    pub fn locked_agent(&self) -> Option<(&AgentHookupKind, &str)> {
        match &self.state {
            ContainerAgentState::Locked {
                agent_kind,
                agent_id,
                ..
            }
            | ContainerAgentState::Deinitializing {
                agent_kind,
                agent_id,
                ..
            } => Some((agent_kind, agent_id.as_str())),
            ContainerAgentState::Empty => None,
        }
    }

    pub fn state_label(&self) -> &'static str {
        match self.state {
            ContainerAgentState::Empty => "empty",
            ContainerAgentState::Locked { .. } => "locked",
            ContainerAgentState::Deinitializing { .. } => "deinitializing",
        }
    }

    fn push_transition(&mut self, from: &str, to: &str, reason: &str) {
        self.state_history.push(StateTransition {
            from: from.to_string(),
            to: to.to_string(),
            at_unix: now_unix_secs(),
            reason: reason.to_string(),
        });
    }
}

pub fn current_container_id() -> String {
    std::env::var("NUCLEUSDB_MESH_AGENT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("HOSTNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "local-container".to_string())
}

fn lock_busy_message(state: &ContainerAgentState) -> String {
    match state {
        ContainerAgentState::Empty => "container is empty".to_string(),
        ContainerAgentState::Locked { agent_id, .. } => {
            format!("container already locked by agent {agent_id}")
        }
        ContainerAgentState::Deinitializing { agent_id, .. } => {
            format!("container is deinitializing agent {agent_id}")
        }
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn temp_path(path: &Path) -> PathBuf {
    path.with_extension("json.tmp")
}

fn ensure_lock_parent_dir(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("agent lock path {} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("set agent lock dir permissions {}: {e}", parent.display()))?;
    }
    Ok(())
}

fn write_private_file(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create dir {}: {e}", parent.display()))?;
    }
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let mut file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| format!("open {}: {e}", path.display()))?;
        file.write_all(bytes)
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        file.flush()
            .map_err(|e| format!("flush {}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};

    #[test]
    fn lock_roundtrip_and_transitions() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("agent_lock_roundtrip");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", home.to_str());
        let mut lock = ContainerAgentLock::load_or_create("ctr-1").expect("create lock");
        assert!(matches!(lock.state, ContainerAgentState::Empty));
        lock.initialize(
            AgentHookupKind::Cli {
                cli_name: "shell".to_string(),
            },
            "agent-shell".to_string(),
        )
        .expect("initialize");
        lock.attach_trace_session(Some("trace-1".to_string()))
            .expect("attach trace");
        let ctx = lock.begin_deinitialize().expect("begin deinit");
        assert_eq!(ctx.agent_id, "agent-shell");
        assert_eq!(ctx.trace_session_id.as_deref(), Some("trace-1"));
        assert_eq!(
            lock.complete_deinitialize().expect("complete"),
            ReusePolicy::Reusable
        );
        lock.save().expect("save");

        let loaded = ContainerAgentLock::load_or_create("ctr-ignored").expect("reload");
        assert!(matches!(loaded.state, ContainerAgentState::Empty));
        assert_eq!(loaded.container_id, "ctr-1");
        assert_eq!(loaded.state_history.len(), 3);
    }

    #[test]
    fn invalid_transitions_are_rejected() {
        let _guard = lock_env();
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("agent_lock_invalid");
        let _env = EnvVarGuard::set("AGENTHALO_HOME", home.to_str());
        let mut lock = ContainerAgentLock::load_or_create("ctr-2").expect("create lock");
        assert!(lock.begin_deinitialize().is_err());
        assert!(lock.complete_deinitialize().is_err());
        lock.initialize(
            AgentHookupKind::Api {
                provider: "openrouter".to_string(),
            },
            "agent-api".to_string(),
        )
        .expect("initialize");
        let err = lock
            .initialize(
                AgentHookupKind::LocalModel {
                    model_id: "Qwen/Qwen2.5-Coder-7B".to_string(),
                },
                "agent-2".to_string(),
            )
            .expect_err("double init must fail");
        assert!(err.contains("already locked"));
    }
}
