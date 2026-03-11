use crate::container::{current_container_id, AgentHookupKind, ReusePolicy};
use crate::halo::config;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

const REGISTRY_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const REGISTRY_LOCK_RETRY: Duration = Duration::from_millis(25);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubsidiaryRegistry {
    pub operator_container_id: String,
    pub operator_agent_id: String,
    #[serde(default)]
    pub subsidiaries: Vec<SubsidiaryRecord>,
    #[serde(default)]
    pub tasks: Vec<SubsidiaryTaskRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubsidiaryRecord {
    pub session_id: String,
    pub container_id: String,
    pub peer_agent_id: String,
    pub agent_lock_state: String,
    pub agent_kind: Option<AgentHookupKind>,
    pub provisioned_at_unix: u64,
    pub initialized_at_unix: Option<u64>,
    pub initialized_agent_id: Option<String>,
    pub trace_session_id: Option<String>,
    pub reuse_policy: Option<ReusePolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubsidiaryTaskRecord {
    pub task_id: String,
    pub session_id: String,
    pub operator_agent_id: String,
    pub prompt: String,
    pub status: String,
    pub model: Option<String>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub trace_session_id: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub created_at_unix: u64,
    pub completed_at_unix: Option<u64>,
}

impl SubsidiaryRegistry {
    pub fn load_or_create(operator_agent_id: &str) -> Result<Self, String> {
        let path = config::subsidiaries_registry_path();
        Self::load_or_create_at(&path, operator_agent_id)
    }

    pub fn load_or_create_locked(
        operator_agent_id: &str,
    ) -> Result<(SubsidiaryRegistryLock, Self), String> {
        let path = config::subsidiaries_registry_path();
        Self::load_or_create_locked_at(&path, operator_agent_id)
    }

    pub fn load_or_create_at(path: &Path, operator_agent_id: &str) -> Result<Self, String> {
        let operator_container_id = current_container_id();
        if path.exists() {
            let raw = std::fs::read(path)
                .map_err(|e| format!("read subsidiary registry {}: {e}", path.display()))?;
            let registry: Self = serde_json::from_slice(&raw)
                .map_err(|e| format!("parse subsidiary registry {}: {e}", path.display()))?;
            if registry.operator_agent_id != operator_agent_id {
                return Err(format!(
                    "subsidiary registry belongs to operator `{}` not `{}`",
                    registry.operator_agent_id, operator_agent_id
                ));
            }
            if registry.operator_container_id != operator_container_id {
                return Err(format!(
                    "subsidiary registry belongs to container `{}` not `{}`",
                    registry.operator_container_id, operator_container_id
                ));
            }
            return Ok(registry);
        }
        let registry = Self {
            operator_container_id,
            operator_agent_id: operator_agent_id.to_string(),
            subsidiaries: Vec::new(),
            tasks: Vec::new(),
        };
        registry.save_to_path(path)?;
        Ok(registry)
    }

    pub fn load_or_create_locked_at(
        path: &Path,
        operator_agent_id: &str,
    ) -> Result<(SubsidiaryRegistryLock, Self), String> {
        let lock = SubsidiaryRegistryLock::acquire(path)?;
        let registry = Self::load_or_create_at(path, operator_agent_id)?;
        Ok((lock, registry))
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to_path(&config::subsidiaries_registry_path())
    }

    pub fn save_to_path(&self, path: &Path) -> Result<(), String> {
        ensure_parent_dir(path)?;
        let raw = serde_json::to_vec_pretty(self)
            .map_err(|e| format!("serialize subsidiary registry: {e}"))?;
        path.parent()
            .ok_or_else(|| format!("subsidiary registry path {} has no parent", path.display()))?;
        let tmp = unique_temp_path(path);
        {
            let mut file = File::create(&tmp)
                .map_err(|e| format!("create temp subsidiary registry {}: {e}", tmp.display()))?;
            file.write_all(&raw)
                .map_err(|e| format!("write temp subsidiary registry {}: {e}", tmp.display()))?;
            file.flush()
                .map_err(|e| format!("flush temp subsidiary registry {}: {e}", tmp.display()))?;
        }
        std::fs::rename(&tmp, path).map_err(|e| {
            format!(
                "commit subsidiary registry {} -> {}: {e}",
                tmp.display(),
                path.display()
            )
        })?;
        Ok(())
    }

    pub fn subsidiary(&self, session_id: &str) -> Option<&SubsidiaryRecord> {
        self.subsidiaries
            .iter()
            .find(|item| item.session_id == session_id)
    }

    pub fn subsidiary_mut(&mut self, session_id: &str) -> Option<&mut SubsidiaryRecord> {
        self.subsidiaries
            .iter_mut()
            .find(|item| item.session_id == session_id)
    }

    pub fn assert_owned(&self, session_id: &str) -> Result<&SubsidiaryRecord, String> {
        self.subsidiary(session_id).ok_or_else(|| {
            format!(
                "operator `{}` does not own subsidiary session `{session_id}`",
                self.operator_agent_id
            )
        })
    }

    pub fn register_provision(
        &mut self,
        session_id: String,
        container_id: String,
        peer_agent_id: String,
    ) {
        let record = SubsidiaryRecord {
            session_id: session_id.clone(),
            container_id,
            peer_agent_id,
            agent_lock_state: "empty".to_string(),
            agent_kind: None,
            provisioned_at_unix: now_unix_secs(),
            initialized_at_unix: None,
            initialized_agent_id: None,
            trace_session_id: None,
            reuse_policy: None,
        };
        match self.subsidiary_mut(&session_id) {
            Some(existing) => *existing = record,
            None => self.subsidiaries.push(record),
        }
    }

    pub fn register_initialize(
        &mut self,
        session_id: &str,
        kind: AgentHookupKind,
        initialized_agent_id: String,
        trace_session_id: Option<String>,
        reuse_policy: ReusePolicy,
    ) -> Result<(), String> {
        let record = self
            .subsidiary_mut(session_id)
            .ok_or_else(|| format!("unknown subsidiary session `{session_id}`"))?;
        record.agent_lock_state = "locked".to_string();
        record.agent_kind = Some(kind);
        record.initialized_at_unix = Some(now_unix_secs());
        record.initialized_agent_id = Some(initialized_agent_id);
        record.trace_session_id = trace_session_id;
        record.reuse_policy = Some(reuse_policy);
        Ok(())
    }

    pub fn register_deinitialize(
        &mut self,
        session_id: &str,
        reuse_policy: ReusePolicy,
    ) -> Result<(), String> {
        let record = self
            .subsidiary_mut(session_id)
            .ok_or_else(|| format!("unknown subsidiary session `{session_id}`"))?;
        record.agent_lock_state = "empty".to_string();
        record.agent_kind = None;
        record.initialized_at_unix = None;
        record.initialized_agent_id = None;
        record.trace_session_id = None;
        record.reuse_policy = Some(reuse_policy);
        Ok(())
    }

    pub fn remove_subsidiary(&mut self, session_id: &str) -> Result<SubsidiaryRecord, String> {
        let index = self
            .subsidiaries
            .iter()
            .position(|item| item.session_id == session_id)
            .ok_or_else(|| format!("unknown subsidiary session `{session_id}`"))?;
        Ok(self.subsidiaries.remove(index))
    }

    pub fn record_task(
        &mut self,
        session_id: &str,
        prompt: String,
        status: String,
        model: Option<String>,
        result: Option<String>,
        error: Option<String>,
        trace_session_id: Option<String>,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        cost_usd: Option<f64>,
    ) -> Result<SubsidiaryTaskRecord, String> {
        self.assert_owned(session_id)?;
        let task = SubsidiaryTaskRecord {
            task_id: format!(
                "subtask-{}-{}",
                now_unix_secs(),
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ),
            session_id: session_id.to_string(),
            operator_agent_id: self.operator_agent_id.clone(),
            prompt,
            status,
            model,
            result,
            error,
            trace_session_id,
            input_tokens,
            output_tokens,
            cost_usd,
            created_at_unix: now_unix_secs(),
            completed_at_unix: Some(now_unix_secs()),
        };
        self.tasks.push(task.clone());
        Ok(task)
    }

    pub fn task(&self, task_id: &str) -> Option<&SubsidiaryTaskRecord> {
        self.tasks.iter().find(|item| item.task_id == task_id)
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Debug)]
pub struct SubsidiaryRegistryLock {
    path: PathBuf,
    _file: File,
}

impl SubsidiaryRegistryLock {
    fn acquire(path: &Path) -> Result<Self, String> {
        let lock_path = lock_path(path);
        ensure_parent_dir(&lock_path)?;
        let deadline = std::time::Instant::now() + REGISTRY_LOCK_TIMEOUT;
        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "pid={}", std::process::id());
                    return Ok(Self {
                        path: lock_path,
                        _file: file,
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if std::time::Instant::now() >= deadline {
                        return Err(format!(
                            "timed out acquiring subsidiary registry lock {}",
                            lock_path.display()
                        ));
                    }
                    std::thread::sleep(REGISTRY_LOCK_RETRY);
                }
                Err(err) => {
                    return Err(format!(
                        "open subsidiary registry lock {}: {err}",
                        lock_path.display()
                    ));
                }
            }
        }
    }
}

impl Drop for SubsidiaryRegistryLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn lock_path(path: &Path) -> PathBuf {
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    PathBuf::from(lock)
}

fn unique_temp_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("subsidiaries.json");
    parent.join(format!(".{}.{}.tmp", stem, uuid::Uuid::new_v4().simple()))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("subsidiary registry path {} has no parent", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("create subsidiary registry dir {}: {e}", parent.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};

    #[test]
    fn registry_roundtrip_and_ownership() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
        let _container = EnvVarGuard::set("NUCLEUSDB_MESH_AGENT_ID", Some("operator-container"));

        let path = config::subsidiaries_registry_path();
        let mut registry = SubsidiaryRegistry::load_or_create("operator-1").expect("registry");
        registry.register_provision(
            "sess-1".to_string(),
            "ctr-1".to_string(),
            "peer-1".to_string(),
        );
        registry
            .register_initialize(
                "sess-1",
                AgentHookupKind::Cli {
                    cli_name: "shell".to_string(),
                },
                "agent-1".to_string(),
                Some("trace-1".to_string()),
                ReusePolicy::SingleUse,
            )
            .expect("init");
        registry.save().expect("save");

        let loaded = SubsidiaryRegistry::load_or_create_at(&path, "operator-1").expect("load");
        assert_eq!(loaded.subsidiaries.len(), 1);
        assert_eq!(
            loaded
                .subsidiary("sess-1")
                .expect("subsidiary")
                .reuse_policy,
            Some(ReusePolicy::SingleUse)
        );
        let err = SubsidiaryRegistry::load_or_create_at(&path, "operator-2")
            .expect_err("ownership mismatch");
        assert!(err.contains("belongs to operator"));
    }

    #[test]
    fn task_recording_requires_owned_subsidiary() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
        let _container = EnvVarGuard::set("NUCLEUSDB_MESH_AGENT_ID", Some("operator-container"));

        let mut registry = SubsidiaryRegistry::load_or_create("operator-1").expect("registry");
        let err = registry
            .record_task(
                "sess-missing",
                "prompt".to_string(),
                "failed".to_string(),
                None,
                None,
                Some("missing".to_string()),
                None,
                None,
                None,
                None,
            )
            .expect_err("must require ownership");
        assert!(err.contains("does not own"));
    }
}
