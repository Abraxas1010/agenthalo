use crate::halo::compile_workflow::{
    self, CompileOutputType, CompileResult, CompileStatus, CompileTask,
};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const DEFAULT_POLL_INTERVAL_MS: u64 = 2_000;

#[derive(Debug)]
pub enum DispatchError {
    Workflow(String),
    Local(String),
}

impl std::fmt::Display for DispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Workflow(err) | Self::Local(err) => f.write_str(err),
        }
    }
}

impl std::error::Error for DispatchError {}

pub async fn dispatch_compilation(task: &CompileTask) -> Result<CompileResult, DispatchError> {
    let task_id =
        compile_workflow::submit_compile_task(task.clone()).map_err(DispatchError::Workflow)?;
    let timeout_ms = dispatch_timeout_ms(task.timeout_ms);
    let poll_interval = Duration::from_millis(poll_interval_ms());
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        if let Some(result) =
            compile_workflow::get_compile_status(&task_id).map_err(DispatchError::Workflow)?
        {
            return Ok(result);
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(poll_interval).await;
    }

    compile_locally(task).await
}

async fn compile_locally(task: &CompileTask) -> Result<CompileResult, DispatchError> {
    let task = task.clone();
    tokio::task::spawn_blocking(move || compile_locally_blocking(&task))
        .await
        .map_err(|e| DispatchError::Local(format!("local compile join error: {e}")))?
}

fn compile_locally_blocking(task: &CompileTask) -> Result<CompileResult, DispatchError> {
    let workspace = WorkspaceGuard::new()?;
    let module_name = sanitized_module_name(&task.artifact_id);
    let module_file = workspace.path().join(format!("{module_name}.lean"));
    std::fs::write(workspace.path().join("lakefile.lean"), &task.lakefile)
        .map_err(|e| DispatchError::Local(format!("write lakefile: {e}")))?;
    std::fs::write(&module_file, &task.lean_source)
        .map_err(|e| DispatchError::Local(format!("write lean source: {e}")))?;
    hydrate_stripped_cache(workspace.path())?;

    let started = Instant::now();
    let build = run_lake(
        workspace.path(),
        &["build", &module_name],
        Duration::from_millis(task.timeout_ms.max(1_000)),
    )?;
    let mut build_log = format!(
        "Building {module_name}\n{}{}",
        build.stdout,
        if build.stdout.contains("Build completed") {
            build.stderr
        } else {
            format!("{}Build completed\n", build.stderr)
        }
    );

    let mut lambda_ir = None;
    let mut c_source = None;
    let mut acsl_annotations = None;

    if build.exit_code == 0 {
        match task.output_type {
            CompileOutputType::LeanBuild => {}
            CompileOutputType::LambdaExtract => {
                let out_path = workspace.path().join("lambda_ir.json");
                let extra = run_lake(
                    workspace.path(),
                    &[
                        "exe",
                        "lean_kernel_export",
                        "--module",
                        &module_name,
                        "--out",
                        out_path.to_str().unwrap_or("lambda_ir.json"),
                    ],
                    Duration::from_millis(task.timeout_ms.max(1_000)),
                )?;
                build_log.push_str(&extra.stdout);
                build_log.push_str(&extra.stderr);
                if extra.exit_code == 0 && out_path.exists() {
                    let raw = std::fs::read_to_string(&out_path)
                        .map_err(|e| DispatchError::Local(format!("read lambda ir: {e}")))?;
                    lambda_ir = serde_json::from_str(&raw).ok();
                }
            }
            CompileOutputType::FullCab => {
                let cab_dir = workspace.path().join("cab_out");
                let extra = run_lake(
                    workspace.path(),
                    &[
                        "exe",
                        "lens_export",
                        "--lens",
                        "omega",
                        "--out",
                        cab_dir.to_str().unwrap_or("cab_out"),
                    ],
                    Duration::from_millis(task.timeout_ms.max(1_000)),
                )?;
                build_log.push_str(&extra.stdout);
                build_log.push_str(&extra.stderr);
                if extra.exit_code == 0 {
                    let cert_path = cab_dir.join("certificate.json");
                    let verify = run_lake(
                        workspace.path(),
                        &[
                            "exe",
                            "cab_verify_export",
                            cert_path.to_str().unwrap_or("cab_out/certificate.json"),
                        ],
                        Duration::from_millis(task.timeout_ms.max(1_000)),
                    )?;
                    build_log.push_str(&verify.stdout);
                    build_log.push_str(&verify.stderr);
                    let source_path = cab_dir.join("generated.c");
                    let acsl_path = cab_dir.join("generated.acsl");
                    if source_path.exists() {
                        c_source =
                            Some(std::fs::read_to_string(&source_path).map_err(|e| {
                                DispatchError::Local(format!("read c source: {e}"))
                            })?);
                    }
                    if acsl_path.exists() {
                        acsl_annotations =
                            Some(std::fs::read_to_string(&acsl_path).map_err(|e| {
                                DispatchError::Local(format!("read acsl annotations: {e}"))
                            })?);
                    }
                }
            }
        }
    }

    let lean_hash = hash_olean_or_source(workspace.path(), &task.lean_source)?;
    let result = CompileResult {
        task_id: task.task_id.clone(),
        status: if build.exit_code == 0 {
            CompileStatus::Success
        } else {
            CompileStatus::BuildFailed
        },
        build_log,
        lean_hash,
        lambda_ir,
        c_source,
        acsl_annotations,
        timing_ms: started.elapsed().as_secs_f64() * 1000.0,
        worker_did: "local-fallback".to_string(),
    };
    compile_workflow::submit_compile_result(&result).map_err(DispatchError::Workflow)?;
    Ok(result)
}

struct CommandOutcome {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

struct WorkspaceGuard {
    path: PathBuf,
}

impl WorkspaceGuard {
    fn new() -> Result<Self, DispatchError> {
        let path = std::env::temp_dir().join(format!(
            "agenthalo-compile-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&path).map_err(|e| {
            DispatchError::Local(format!("create compile workspace {}: {e}", path.display()))
        })?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for WorkspaceGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn run_lake(cwd: &Path, args: &[&str], timeout: Duration) -> Result<CommandOutcome, DispatchError> {
    let mut cmd = Command::new("lake");
    cmd.current_dir(cwd).args(args);
    let child = cmd
        .spawn()
        .map_err(|e| DispatchError::Local(format!("spawn lake {:?}: {e}", args)))?;
    wait_with_timeout(child, args, timeout)
}

fn hydrate_stripped_cache(workspace: &Path) -> Result<(), DispatchError> {
    let cache_dir = crate::halo::config::heytinglean_cache_dir();
    if !cache_dir.exists() {
        return Ok(());
    }
    let source_lake = cache_dir.join(".lake");
    if !source_lake.exists() {
        return Ok(());
    }
    let workspace_lake = workspace.join(".lake");
    std::fs::create_dir_all(&workspace_lake).map_err(|e| {
        DispatchError::Local(format!(
            "create stripped-cache workspace {}: {e}",
            workspace_lake.display()
        ))
    })?;
    copy_tree(&source_lake, &workspace_lake)?;
    for entry in ["lake-manifest.json", "lean-toolchain"] {
        let src = cache_dir.join(entry);
        if src.exists() {
            let dst = workspace.join(entry);
            std::fs::copy(&src, &dst).map_err(|e| {
                DispatchError::Local(format!(
                    "copy stripped-cache file {} -> {}: {e}",
                    src.display(),
                    dst.display()
                ))
            })?;
        }
    }
    Ok(())
}

fn wait_with_timeout(
    mut child: std::process::Child,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutcome, DispatchError> {
    let start = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| DispatchError::Local(format!("poll lake {:?}: {e}", args)))?
        {
            let output = child
                .wait_with_output()
                .map_err(|e| DispatchError::Local(format!("collect lake {:?}: {e}", args)))?;
            return Ok(CommandOutcome {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                exit_code: status.code().unwrap_or(1),
            });
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let output = child.wait_with_output().map_err(|e| {
                DispatchError::Local(format!("collect timed out lake {:?}: {e}", args))
            })?;
            return Ok(CommandOutcome {
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: format!(
                    "{}timed out after {} ms\n",
                    String::from_utf8_lossy(&output.stderr),
                    timeout.as_millis()
                ),
                exit_code: 124,
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn hash_olean_or_source(workspace: &Path, lean_source: &str) -> Result<String, DispatchError> {
    if let Some(path) = first_olean_path(workspace)? {
        return hash_file(&path);
    }
    Ok(hash_bytes(lean_source.as_bytes()))
}

fn first_olean_path(workspace: &Path) -> Result<Option<PathBuf>, DispatchError> {
    let build_lib = workspace.join(".lake").join("build").join("lib");
    if !build_lib.exists() {
        return Ok(None);
    }
    let mut stack = vec![build_lib];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path)
            .map_err(|e| DispatchError::Local(format!("read build dir {}: {e}", path.display())))?
        {
            let entry = entry.map_err(|e| {
                DispatchError::Local(format!("iterate build dir {}: {e}", path.display()))
            })?;
            let entry_path = entry.path();
            if entry_path.is_dir() {
                stack.push(entry_path);
            } else if matches!(
                entry_path.extension().and_then(|ext| ext.to_str()),
                Some("olean")
            ) {
                return Ok(Some(entry_path));
            }
        }
    }
    Ok(None)
}

fn hash_file(path: &Path) -> Result<String, DispatchError> {
    let bytes = std::fs::read(path)
        .map_err(|e| DispatchError::Local(format!("read {}: {e}", path.display())))?;
    Ok(hash_bytes(&bytes))
}

fn copy_tree(src: &Path, dst: &Path) -> Result<(), DispatchError> {
    for entry in std::fs::read_dir(src)
        .map_err(|e| DispatchError::Local(format!("read cache dir {}: {e}", src.display())))?
    {
        let entry = entry.map_err(|e| {
            DispatchError::Local(format!("iterate cache dir {}: {e}", src.display()))
        })?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let file_type = entry.file_type().map_err(|e| {
            DispatchError::Local(format!("inspect cache entry {}: {e}", src_path.display()))
        })?;
        if file_type.is_dir() {
            std::fs::create_dir_all(&dst_path).map_err(|e| {
                DispatchError::Local(format!(
                    "create cache target dir {}: {e}",
                    dst_path.display()
                ))
            })?;
            copy_tree(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&src_path, &dst_path).map_err(|e| {
                DispatchError::Local(format!(
                    "copy cache file {} -> {}: {e}",
                    src_path.display(),
                    dst_path.display()
                ))
            })?;
        }
    }
    Ok(())
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sanitized_module_name(artifact_id: &str) -> String {
    let filtered: String = artifact_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect();
    if filtered.is_empty() {
        "GeneratedTarget".to_string()
    } else {
        filtered
    }
}

fn poll_interval_ms() -> u64 {
    std::env::var("AGENTHALO_COMPILE_POLL_INTERVAL_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_POLL_INTERVAL_MS)
}

fn dispatch_timeout_ms(task_timeout_ms: u64) -> u64 {
    std::env::var("AGENTHALO_COMPILE_DISPATCH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(task_timeout_ms.max(1_000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{lock_env, EnvVarGuard};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn write_fake_lake(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("lake");
        let script = r#"#!/bin/sh
set -eu
cmd="$1"
if [ "$cmd" = "build" ]; then
  mkdir -p .lake/build/lib
  echo "compiled" > .lake/build/lib/GeneratedTarget.olean
  echo "Compiling $2"
  exit 0
fi
if [ "$cmd" = "exe" ] && [ "$2" = "lean_kernel_export" ]; then
  out="$5"
  echo '{"kind":"lambda_ir"}' > "$out"
  echo "built successfully"
  exit 0
fi
if [ "$cmd" = "exe" ] && [ "$2" = "lens_export" ]; then
  outdir="$5"
  mkdir -p "$outdir"
  echo "int main(void) { return 0; }" > "$outdir/generated.c"
  echo "/*@ ensures \\true; */" > "$outdir/generated.acsl"
  echo '{}' > "$outdir/certificate.json"
  echo "built successfully"
  exit 0
fi
if [ "$cmd" = "exe" ] && [ "$2" = "cab_verify_export" ]; then
  echo "built successfully"
  exit 0
fi
exit 1
"#;
        fs::write(&path, script).expect("write fake lake");
        let mut perms = fs::metadata(&path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("chmod fake lake");
        path
    }

    fn sample_task(task_id: &str) -> CompileTask {
        CompileTask {
            task_id: task_id.to_string(),
            artifact_id: "GeneratedTarget".to_string(),
            lean_source: "def x := 1\n".to_string(),
            lakefile: "package demo\n".to_string(),
            dependencies: vec![],
            output_type: CompileOutputType::LeanBuild,
            requester_did: None,
            submitted_at: 0,
            timeout_ms: 100,
        }
    }

    #[tokio::test]
    async fn test_compile_dispatch_prefers_worker_result() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
        let _poll = EnvVarGuard::set("AGENTHALO_COMPILE_POLL_INTERVAL_MS", Some("5"));
        let _timeout = EnvVarGuard::set("AGENTHALO_COMPILE_DISPATCH_TIMEOUT_MS", Some("50"));

        let task = sample_task("dispatch-worker");
        let task_id = task.task_id.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(15));
            let result = CompileResult {
                task_id,
                status: CompileStatus::Success,
                build_log: "Building GeneratedTarget\nBuild completed\n".to_string(),
                lean_hash: "b".repeat(64),
                lambda_ir: None,
                c_source: None,
                acsl_annotations: None,
                timing_ms: 4.0,
                worker_did: "did:worker:test".to_string(),
            };
            let _ = compile_workflow::submit_compile_result(&result);
        });

        let result = dispatch_compilation(&task).await.expect("dispatch result");
        assert_eq!(result.worker_did, "did:worker:test");
        assert_eq!(result.status, CompileStatus::Success);
    }

    #[tokio::test]
    async fn test_compile_dispatch_falls_back_to_local_compile() {
        let _guard = lock_env();
        let home = tempfile::tempdir().expect("tempdir");
        let bin = tempfile::tempdir().expect("tempdir");
        let _home = EnvVarGuard::set("AGENTHALO_HOME", home.path().to_str());
        let _poll = EnvVarGuard::set("AGENTHALO_COMPILE_POLL_INTERVAL_MS", Some("5"));
        let _timeout = EnvVarGuard::set("AGENTHALO_COMPILE_DISPATCH_TIMEOUT_MS", Some("15"));
        let fake_lake = write_fake_lake(bin.path());
        let old_path = std::env::var("PATH").unwrap_or_default();
        let joined = format!("{}:{}", fake_lake.parent().unwrap().display(), old_path);
        let _path = EnvVarGuard::set("PATH", Some(&joined));

        let result = dispatch_compilation(&sample_task("dispatch-local"))
            .await
            .expect("local fallback");
        assert_eq!(result.worker_did, "local-fallback");
        assert_eq!(result.status, CompileStatus::Success);
    }
}
