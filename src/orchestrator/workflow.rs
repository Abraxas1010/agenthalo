//! Workflow definitions, storage, and runner for multi-agent orchestration.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ── Schema ────────────────────────────────────────────────────────────

/// A saved workflow definition: litegraph JSON envelope + HALO metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub name: String,
    #[serde(default = "default_version")]
    pub version: u32,
    pub created_at: u64,
    pub updated_at: u64,
    /// Raw `graph.serialize()` from litegraph — opaque to Rust.
    pub litegraph: serde_json::Value,
    #[serde(default)]
    pub halo_meta: WorkflowMeta,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowMeta {
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
    #[serde(default)]
    pub role_definitions: BTreeMap<String, RoleDefinition>,
}

fn default_max_iterations() -> u32 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoleDefinition {
    pub role_name: String,
    #[serde(default)]
    pub agent_type: String,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub skill_ref: Option<String>,
    #[serde(default)]
    pub prompt_template: String,
}

// ── Workflow instance (runtime) ───────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub instance_id: String,
    pub workflow_id: String,
    pub status: WorkflowStatus,
    /// role_name → agent_id
    pub role_bindings: BTreeMap<String, String>,
    /// decision_node_id → iteration count
    pub iteration_counts: BTreeMap<String, u32>,
    pub current_node: Option<String>,
    pub events: Vec<WorkflowEvent>,
    pub started_at: u64,
    pub completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Stopped,
    MaxIterationsExceeded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEvent {
    pub timestamp: u64,
    pub node_id: String,
    pub event_type: WorkflowEventType,
    pub message: String,
    #[serde(default)]
    pub agent_letter: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowEventType {
    NodeStarted,
    NodeCompleted,
    NodeFailed,
    DecisionEvaluated {
        passed: bool,
        iteration: u32,
    },
    WorkflowCompleted,
    WorkflowFailed,
    MaxIterationsExceeded,
}

// ── Filesystem storage ────────────────────────────────────────────────

/// Directory for workflow JSON files.
pub fn workflows_dir() -> PathBuf {
    crate::halo::config::halo_dir().join("workflows")
}

pub fn workflow_runs_dir() -> PathBuf {
    crate::halo::config::halo_dir().join("workflow-runs")
}

fn ensure_dir(path: &Path) {
    if !path.exists() {
        let _ = std::fs::create_dir_all(path);
    }
}

pub fn list_workflows() -> Vec<WorkflowDefinition> {
    let dir = workflows_dir();
    ensure_dir(&dir);
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(wf) = serde_json::from_str::<WorkflowDefinition>(&data) {
                    results.push(wf);
                }
            }
        }
    }
    results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    results
}

pub fn get_workflow(id: &str) -> Option<WorkflowDefinition> {
    let path = workflows_dir().join(format!("{id}.json"));
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn save_workflow(mut wf: WorkflowDefinition) -> Result<WorkflowDefinition, String> {
    let dir = workflows_dir();
    ensure_dir(&dir);

    if wf.workflow_id.is_empty() {
        wf.workflow_id = format!(
            "wf-{}-{}",
            crate::pod::now_unix(),
            &uuid::Uuid::new_v4().simple().to_string()[..8]
        );
        wf.created_at = crate::pod::now_unix();
    }
    wf.updated_at = crate::pod::now_unix();

    // Sanitize ID for filesystem safety
    let safe_id: String = wf
        .workflow_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    if safe_id.is_empty() {
        return Err("invalid workflow_id".to_string());
    }
    wf.workflow_id = safe_id.clone();

    let path = dir.join(format!("{safe_id}.json"));
    let json = serde_json::to_string_pretty(&wf).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(wf)
}

pub fn delete_workflow(id: &str) -> Result<(), String> {
    let safe_id: String = id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let path = workflows_dir().join(format!("{safe_id}.json"));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("delete: {e}"))?;
    }
    Ok(())
}

// ── Workflow instances (run history) ──────────────────────────────────

pub fn save_workflow_instance(inst: &WorkflowInstance) -> Result<(), String> {
    let dir = workflow_runs_dir();
    ensure_dir(&dir);
    let safe_id: String = inst
        .instance_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let path = dir.join(format!("{safe_id}.json"));
    let json = serde_json::to_string_pretty(inst).map_err(|e| format!("serialize: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("write: {e}"))?;
    Ok(())
}

pub fn get_workflow_instance(instance_id: &str) -> Option<WorkflowInstance> {
    let safe_id: String = instance_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let path = workflow_runs_dir().join(format!("{safe_id}.json"));
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn list_workflow_instances() -> Vec<WorkflowInstance> {
    let dir = workflow_runs_dir();
    ensure_dir(&dir);
    let mut results = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return results,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(inst) = serde_json::from_str::<WorkflowInstance>(&data) {
                    results.push(inst);
                }
            }
        }
    }
    results.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_definition_round_trip() {
        let wf = WorkflowDefinition {
            workflow_id: "test-wf-1".to_string(),
            name: "Test Workflow".to_string(),
            version: 1,
            created_at: 1000,
            updated_at: 1000,
            litegraph: serde_json::json!({"nodes":[], "links":[]}),
            halo_meta: WorkflowMeta::default(),
        };
        let json = serde_json::to_string(&wf).expect("serialize");
        let parsed: WorkflowDefinition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.workflow_id, "test-wf-1");
        assert_eq!(parsed.name, "Test Workflow");
    }

    #[test]
    fn workflow_meta_defaults() {
        let meta: WorkflowMeta = serde_json::from_str("{}").expect("deserialize empty");
        assert_eq!(meta.max_iterations, 10);
        assert!(meta.role_definitions.is_empty());
    }

    #[test]
    fn workflow_status_serializes_snake_case() {
        let json = serde_json::to_string(&WorkflowStatus::MaxIterationsExceeded).unwrap();
        assert_eq!(json, "\"max_iterations_exceeded\"");
    }
}
