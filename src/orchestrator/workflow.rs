//! Workflow definitions, storage, and runner for multi-agent orchestration.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ── Schema ────────────────────────────────────────────────────────────

/// A saved workflow definition: litegraph JSON envelope + HALO metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    #[serde(default)]
    pub workflow_id: String,
    pub name: String,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    /// Raw `graph.serialize()` from litegraph — opaque to Rust.
    #[serde(default)]
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

// ── Graph validation ──────────────────────────────────────────────────

/// Validate a workflow's litegraph blob for structural issues.
/// Returns a list of warnings (empty = valid).
pub fn validate_graph(wf: &WorkflowDefinition) -> Vec<String> {
    let mut warnings = Vec::new();
    let lg = &wf.litegraph;

    // Check nodes exist and are from known types
    if let Some(nodes) = lg.get("nodes").and_then(|v| v.as_array()) {
        let known_types = [
            "halo/agent", "halo/decision", "halo/transform", "halo/phase",
            "halo/tool", "halo/skill", "halo/lean_verifier",
        ];
        for node in nodes {
            if let Some(ntype) = node.get("type").and_then(|v| v.as_str()) {
                if !known_types.contains(&ntype) {
                    warnings.push(format!("unknown node type: {ntype}"));
                }
            }
            // Decision nodes must have max_iterations ≤ 50
            if node.get("type").and_then(|v| v.as_str()) == Some("halo/decision") {
                if let Some(props) = node.get("properties") {
                    let max_iter = props
                        .get("max_iterations")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(10);
                    if max_iter > 50 {
                        let nid = node.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                        warnings.push(format!(
                            "decision node {nid}: max_iterations={max_iter} exceeds limit of 50"
                        ));
                    }
                }
            }
        }
    } else if !lg.is_null() {
        warnings.push("litegraph blob missing 'nodes' array".to_string());
    }

    warnings
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

    #[test]
    fn path_traversal_sanitized_to_safe_id() {
        // F1: verify that path traversal attempts are sanitized
        let malicious_ids = [
            "../../../etc/passwd",
            "..\\..\\windows\\system32",
            "normal-id/../escape",
            "good_id/../../bad",
            "../../../../tmp/evil",
            "id\x00null",
        ];
        for id in &malicious_ids {
            let safe: String = id
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            assert!(!safe.contains('/'), "sanitized ID still contains /: {safe}");
            assert!(!safe.contains('\\'), "sanitized ID still contains \\: {safe}");
            assert!(!safe.contains(".."), "sanitized ID still contains ..: {safe}");
            assert!(!safe.contains('\0'), "sanitized ID still contains null byte: {safe}");
        }
        // Verify the specific traversal attack collapses to harmless string
        let attack = "../../../tmp/evil";
        let safe: String = attack
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        assert_eq!(safe, "tmpevil");
    }

    #[test]
    fn validate_graph_catches_bad_decision_node() {
        let wf = WorkflowDefinition {
            workflow_id: "test".to_string(),
            name: "test".to_string(),
            version: 1,
            created_at: 0,
            updated_at: 0,
            litegraph: serde_json::json!({
                "nodes": [
                    {"id": 1, "type": "halo/agent", "properties": {}},
                    {"id": 2, "type": "halo/decision", "properties": {"max_iterations": 999}},
                    {"id": 3, "type": "unknown/type", "properties": {}},
                ],
                "links": []
            }),
            halo_meta: WorkflowMeta::default(),
        };
        let warnings = validate_graph(&wf);
        assert!(warnings.iter().any(|w| w.contains("max_iterations=999")));
        assert!(warnings.iter().any(|w| w.contains("unknown node type")));
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn validate_graph_passes_clean_graph() {
        let wf = WorkflowDefinition {
            workflow_id: "test".to_string(),
            name: "test".to_string(),
            version: 1,
            created_at: 0,
            updated_at: 0,
            litegraph: serde_json::json!({
                "nodes": [
                    {"id": 1, "type": "halo/agent", "properties": {}},
                    {"id": 2, "type": "halo/decision", "properties": {"max_iterations": 10}},
                    {"id": 3, "type": "halo/transform", "properties": {}},
                    {"id": 4, "type": "halo/phase", "properties": {}},
                ],
                "links": []
            }),
            halo_meta: WorkflowMeta::default(),
        };
        let warnings = validate_graph(&wf);
        assert!(warnings.is_empty(), "expected no warnings, got: {warnings:?}");
    }

    #[test]
    fn minimal_create_request_deserializes() {
        // F4: verify that minimal JSON (just name) deserializes with defaults
        let json = r#"{"name":"test workflow"}"#;
        let wf: WorkflowDefinition = serde_json::from_str(json).expect("should deserialize");
        assert_eq!(wf.name, "test workflow");
        assert!(wf.workflow_id.is_empty());
        assert_eq!(wf.created_at, 0);
        assert_eq!(wf.updated_at, 0);
        assert_eq!(wf.version, 1);
        assert!(wf.litegraph.is_null());
    }
}
