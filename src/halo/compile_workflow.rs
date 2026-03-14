use crate::halo::capability_spec::CapabilityQuery;
use crate::halo::capability_task::{CapabilitySlot, ManifoldConstraints, TaskManifold};
use crate::halo::lambda_credits;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

pub const COMPILE_CAPABILITY: &str = "heytinglean.compile.v1";
pub const DEFAULT_TIMEOUT_MS: u64 = 300_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompileOutputType {
    LeanBuild,
    LambdaExtract,
    FullCab,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompileTask {
    pub task_id: String,
    pub artifact_id: String,
    pub lean_source: String,
    pub lakefile: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default = "default_output_type")]
    pub output_type: CompileOutputType,
    #[serde(default)]
    pub requester_did: Option<String>,
    #[serde(default)]
    pub submitted_at: u64,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompileStatus {
    Success,
    BuildFailed,
    Timeout,
    Error,
    Pending,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompileResult {
    pub task_id: String,
    pub status: CompileStatus,
    pub build_log: String,
    pub lean_hash: String,
    #[serde(default)]
    pub lambda_ir: Option<serde_json::Value>,
    #[serde(default)]
    pub c_source: Option<String>,
    #[serde(default)]
    pub acsl_annotations: Option<String>,
    pub timing_ms: f64,
    pub worker_did: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
enum StoredTaskStatus {
    Pending,
    Completed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredCompileTask {
    task: CompileTask,
    manifold: TaskManifold,
    status: StoredTaskStatus,
    assigned_worker: Option<String>,
    created_at_ms: u64,
}

fn workflow_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn default_output_type() -> CompileOutputType {
    CompileOutputType::LeanBuild
}

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

fn now_unix_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn workflow_tasks_dir() -> std::path::PathBuf {
    crate::halo::config::compile_workflow_dir().join("tasks")
}

fn workflow_results_dir() -> std::path::PathBuf {
    crate::halo::config::compile_workflow_dir().join("results")
}

fn task_record_path(task_id: &str) -> std::path::PathBuf {
    workflow_tasks_dir().join(format!("{task_id}.json"))
}

fn result_record_path(task_id: &str) -> std::path::PathBuf {
    workflow_results_dir().join(format!("{task_id}.json"))
}

fn ensure_workflow_dirs() -> Result<(), String> {
    std::fs::create_dir_all(workflow_tasks_dir())
        .map_err(|e| format!("create workflow tasks dir: {e}"))?;
    std::fs::create_dir_all(workflow_results_dir())
        .map_err(|e| format!("create workflow results dir: {e}"))?;
    Ok(())
}

fn build_compile_manifold(task: &CompileTask) -> TaskManifold {
    let now = now_unix_millis();
    TaskManifold {
        task_id: task.task_id.clone(),
        description: format!("HeytingLean compile: {}", task.artifact_id),
        slots: vec![CapabilitySlot {
            slot_id: "compiler".to_string(),
            query: CapabilityQuery {
                domain_prefix: COMPILE_CAPABILITY.to_string(),
                required_inputs: Vec::new(),
                required_outputs: Vec::new(),
                required_constraints: Vec::new(),
                min_success_rate: None,
                max_latency_p99_ms: None,
                max_cost_microdollars: None,
                min_attestations: None,
                min_onchain_reputation: None,
                count: 1,
                query_timeout_ms: task.timeout_ms,
            },
            redundancy: 1,
            optional: false,
        }],
        edges: vec![],
        constraints: ManifoldConstraints {
            max_total_latency_ms: Some(task.timeout_ms),
            ..Default::default()
        },
        originator_did: task
            .requester_did
            .clone()
            .unwrap_or_else(|| "heytinglean-cloud".to_string()),
        created_at: now / 1000,
        formation_timeout_ms: task.timeout_ms,
        expires_at: (now + task.timeout_ms + 60_000) / 1000,
    }
}

fn normalize_task(mut task: CompileTask) -> CompileTask {
    if task.task_id.trim().is_empty() {
        task.task_id = format!("compile-{}", uuid::Uuid::new_v4().simple());
    }
    if task.timeout_ms == 0 {
        task.timeout_ms = DEFAULT_TIMEOUT_MS;
    }
    if task.submitted_at == 0 {
        task.submitted_at = now_unix_millis();
    }
    task
}

pub fn prepare_compile_task(task: CompileTask) -> CompileTask {
    normalize_task(task)
}

pub fn submit_compile_task(task: CompileTask) -> Result<String, String> {
    let _guard = workflow_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_workflow_dirs()?;
    let task = normalize_task(task);
    let record = StoredCompileTask {
        manifold: build_compile_manifold(&task),
        status: StoredTaskStatus::Pending,
        assigned_worker: None,
        created_at_ms: now_unix_millis(),
        task: task.clone(),
    };
    persist_task_record(&record)?;
    Ok(task.task_id)
}

pub fn poll_compile_tasks(worker_did: &str) -> Result<Vec<CompileTask>, String> {
    let _guard = workflow_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_workflow_dirs()?;
    let mut out = Vec::new();
    for mut record in load_task_records()? {
        if record.status == StoredTaskStatus::Completed {
            continue;
        }
        if result_record_path(&record.task.task_id).exists() {
            record.status = StoredTaskStatus::Completed;
            persist_task_record(&record)?;
            continue;
        }
        if is_expired(&record.task) {
            persist_timeout_result_for(&record.task, worker_did)?;
            record.status = StoredTaskStatus::Completed;
            persist_task_record(&record)?;
            continue;
        }
        if record
            .assigned_worker
            .as_deref()
            .map(|assigned| assigned != worker_did)
            .unwrap_or(false)
        {
            continue;
        }
        if record.assigned_worker.is_none() {
            record.assigned_worker = Some(worker_did.to_string());
            persist_task_record(&record)?;
        }
        out.push(record.task.clone());
    }
    Ok(out)
}

pub fn submit_compile_result(result: &CompileResult) -> Result<(), String> {
    let _guard = workflow_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_workflow_dirs()?;
    if load_compile_result(&result.task_id)?.is_some() {
        return Ok(());
    }
    validate_compile_result(result)?;
    persist_compile_result(result)?;
    if let Some(mut record) = load_task_record(&result.task_id)? {
        record.status = StoredTaskStatus::Completed;
        persist_task_record(&record)?;
        if let Some(user_did) = record.task.requester_did.as_deref() {
            let _ = lambda_credits::award_compile_credits_for_result(
                user_did,
                &record.task.task_id,
                record.task.lean_source.len(),
                record.task.dependencies.len(),
                compile_status_str(&result.status),
            );
        }
    }
    Ok(())
}

pub fn get_compile_status(task_id: &str) -> Result<Option<CompileResult>, String> {
    let _guard = workflow_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    ensure_workflow_dirs()?;
    if let Some(result) = load_compile_result(task_id)? {
        return Ok(Some(result));
    }
    if let Some(task) = load_task_record(task_id)?.map(|record| record.task) {
        if is_expired(&task) {
            persist_timeout_result_for(&task, "workflow-timeout")?;
            return load_compile_result(task_id);
        }
    }
    Ok(None)
}

pub fn validate_compile_result(result: &CompileResult) -> Result<(), String> {
    if result.status == CompileStatus::Success {
        let log = &result.build_log;
        let has_building = log.contains("Building ") || log.contains("Compiling ");
        let has_completion = log.contains("Build completed") || log.contains("built successfully");
        if !has_building || !has_completion {
            return Err("build log missing expected lake build markers".to_string());
        }
        if result.lean_hash.len() != 64
            || !result.lean_hash.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err("invalid lean_hash format".to_string());
        }
    }
    Ok(())
}

fn compile_status_str(status: &CompileStatus) -> &'static str {
    match status {
        CompileStatus::Success => "success",
        CompileStatus::BuildFailed => "build_failed",
        CompileStatus::Timeout => "timeout",
        CompileStatus::Error => "error",
        CompileStatus::Pending => "pending",
    }
}

fn is_expired(task: &CompileTask) -> bool {
    now_unix_millis() > task.submitted_at.saturating_add(task.timeout_ms)
}

fn persist_timeout_result_for(task: &CompileTask, worker_did: &str) -> Result<(), String> {
    let result = CompileResult {
        task_id: task.task_id.clone(),
        status: CompileStatus::Timeout,
        build_log: "timed out waiting for compile worker".to_string(),
        lean_hash: "0".repeat(64),
        lambda_ir: None,
        c_source: None,
        acsl_annotations: None,
        timing_ms: task.timeout_ms as f64,
        worker_did: worker_did.to_string(),
    };
    persist_compile_result(&result)
}

fn persist_task_record(record: &StoredCompileTask) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(record)
        .map_err(|e| format!("encode workflow task {}: {e}", record.task.task_id))?;
    std::fs::write(task_record_path(&record.task.task_id), raw)
        .map_err(|e| format!("write workflow task {}: {e}", record.task.task_id))
}

fn persist_compile_result(result: &CompileResult) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(result)
        .map_err(|e| format!("encode workflow result {}: {e}", result.task_id))?;
    std::fs::write(result_record_path(&result.task_id), raw)
        .map_err(|e| format!("write workflow result {}: {e}", result.task_id))
}

fn load_task_record(task_id: &str) -> Result<Option<StoredCompileTask>, String> {
    let path = task_record_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read workflow task {}: {e}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .map_err(|e| format!("parse workflow task {}: {e}", path.display()))?;
    Ok(Some(record))
}

fn load_compile_result(task_id: &str) -> Result<Option<CompileResult>, String> {
    let path = result_record_path(task_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("read workflow result {}: {e}", path.display()))?;
    let result = serde_json::from_str(&raw)
        .map_err(|e| format!("parse workflow result {}: {e}", path.display()))?;
    Ok(Some(result))
}

fn load_task_records() -> Result<Vec<StoredCompileTask>, String> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(workflow_tasks_dir())
        .map_err(|e| format!("read workflow tasks dir: {e}"))?
    {
        let entry = entry.map_err(|e| format!("iterate workflow tasks dir: {e}"))?;
        let path = entry.path();
        if !matches!(path.extension().and_then(|ext| ext.to_str()), Some("json")) {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("read workflow task {}: {e}", path.display()))?;
        let record: StoredCompileTask = serde_json::from_str(&raw)
            .map_err(|e| format!("parse workflow task {}: {e}", path.display()))?;
        out.push(record);
    }
    out.sort_by(|left, right| left.created_at_ms.cmp(&right.created_at_ms));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};

    fn success_result(task_id: &str) -> CompileResult {
        CompileResult {
            task_id: task_id.to_string(),
            status: CompileStatus::Success,
            build_log: "Building GeneratedTarget\nBuild completed\n".to_string(),
            lean_hash: "a".repeat(64),
            lambda_ir: None,
            c_source: None,
            acsl_annotations: None,
            timing_ms: 12.0,
            worker_did: "did:worker:test".to_string(),
        }
    }

    fn sample_task(task_id: &str) -> CompileTask {
        CompileTask {
            task_id: task_id.to_string(),
            artifact_id: "GeneratedTarget".to_string(),
            lean_source: "def x := 1".to_string(),
            lakefile: "package demo\n".to_string(),
            dependencies: vec!["Mathlib".to_string()],
            output_type: CompileOutputType::LeanBuild,
            requester_did: Some("did:user:test".to_string()),
            submitted_at: 0,
            timeout_ms: 250,
        }
    }

    #[test]
    fn test_compile_workflow_roundtrip() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());

        let task = sample_task("workflow-roundtrip");
        let task_id = submit_compile_task(task).expect("submit task");
        let polled = poll_compile_tasks("did:worker:test").expect("poll tasks");
        assert_eq!(polled.len(), 1);
        assert_eq!(polled[0].task_id, task_id);

        submit_compile_result(&success_result(&task_id)).expect("submit result");
        let status = get_compile_status(&task_id)
            .expect("load status")
            .expect("status result");
        assert_eq!(status.status, CompileStatus::Success);
    }

    #[test]
    fn test_compile_workflow_rejects_fabricated_success_log() {
        let result = CompileResult {
            build_log: "totally legit".to_string(),
            ..success_result("bad-log")
        };
        let err = validate_compile_result(&result).expect_err("validation should fail");
        assert!(err.contains("build log"));
    }

    #[test]
    fn test_compile_workflow_timeout_marks_result() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());

        let mut task = sample_task("workflow-timeout");
        task.timeout_ms = 1;
        let task_id = submit_compile_task(task).expect("submit task");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let polled = poll_compile_tasks("did:worker:test").expect("poll tasks");
        assert!(polled.is_empty());
        let status = get_compile_status(&task_id)
            .expect("load status")
            .expect("status result");
        assert_eq!(status.status, CompileStatus::Timeout);
    }
}
