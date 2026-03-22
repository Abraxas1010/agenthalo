//! Pantograph subprocess manager for the Proof Explorer.
//!
//! Manages a long-running Pantograph (or lean-repl) process, communicating
//! via JSON over stdin/stdout. The process is spawned once and reused across
//! API requests through shared `PantographState`.

use serde_json::Value;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// Manages a long-running Pantograph subprocess.
/// Sends JSON commands via stdin, reads JSON responses from stdout.
pub struct PantographProcess {
    child: Child,
    stdin: tokio::process::ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
    /// Stderr reader for diagnostics (collected but not blocking).
    _stderr_task: tokio::task::JoinHandle<()>,
    /// Track whether the process has been confirmed alive.
    started: bool,
}

impl PantographProcess {
    /// Spawn Pantograph with the given binary path and Lean project path.
    ///
    /// The binary is expected to be either:
    /// - `vendor/pantograph/.lake/build/bin/pantograph` (Pantograph)
    /// - `vendor/lean-repl/.lake/build/bin/repl` (lean-repl fallback)
    pub async fn spawn(binary_path: &str, project_path: &str) -> Result<Self, String> {
        let mut child = Command::new(binary_path)
            .arg("--project")
            .arg(project_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn Pantograph at {binary_path}: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture Pantograph stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture Pantograph stdout".to_string())?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| "Failed to capture Pantograph stderr".to_string())?;

        // Spawn a task to drain stderr so it doesn't block the process.
        let stderr_task = tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                eprintln!("[Pantograph/stderr] {line}");
            }
        });

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            _stderr_task: stderr_task,
            started: true,
        })
    }

    /// Send a JSON command and read the JSON response (one line per exchange).
    pub async fn send_command(&mut self, cmd: Value) -> Result<Value, String> {
        let mut line = serde_json::to_string(&cmd)
            .map_err(|e| format!("JSON serialize error: {e}"))?;
        line.push('\n');

        self.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to Pantograph stdin: {e}"))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush Pantograph stdin: {e}"))?;

        let mut response_line = String::new();
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            self.stdout.read_line(&mut response_line),
        )
        .await;

        match read_result {
            Ok(Ok(0)) => Err("Pantograph process closed stdout (crashed?)".to_string()),
            Ok(Ok(_)) => {
                let trimmed = response_line.trim();
                serde_json::from_str(trimmed)
                    .map_err(|e| format!("Failed to parse Pantograph response: {e}: {trimmed}"))
            }
            Ok(Err(e)) => Err(format!("Failed to read from Pantograph stdout: {e}")),
            Err(_) => Err("Pantograph tactic timed out after 30s".to_string()),
        }
    }

    /// Start a new proof goal from a type expression.
    /// Returns `{ stateId: N, root: { goalId: 0, target: "...", vars: [...] } }`.
    pub async fn goal_start(&mut self, expr: &str) -> Result<Value, String> {
        let resp = self
            .send_command(serde_json::json!({
                "cmd": "goal.start",
                "expr": expr,
            }))
            .await?;
        if let Some(err) = resp.get("error") {
            return Err(format!("Pantograph goal.start error: {err}"));
        }
        Ok(resp)
    }

    /// Apply a tactic to a specific goal within a proof state.
    pub async fn goal_tactic(
        &mut self,
        state_id: u64,
        goal_id: u64,
        tactic: &str,
    ) -> Result<Value, String> {
        let resp = self
            .send_command(serde_json::json!({
                "cmd": "goal.tactic",
                "stateId": state_id,
                "goalId": goal_id,
                "tactic": tactic,
            }))
            .await?;
        if let Some(err) = resp.get("error") {
            return Err(format!("Tactic failed: {err}"));
        }
        Ok(resp)
    }

    /// Delete a proof state to free resources.
    pub async fn goal_delete(&mut self, state_id: u64) -> Result<(), String> {
        let _ = self
            .send_command(serde_json::json!({
                "cmd": "goal.delete",
                "stateIds": [state_id],
            }))
            .await?;
        Ok(())
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        if !self.started {
            return false;
        }
        // try_wait returns Ok(Some(status)) if exited, Ok(None) if still running
        match self.child.try_wait() {
            Ok(Some(_)) => {
                self.started = false;
                false
            }
            Ok(None) => true,
            Err(_) => false,
        }
    }
}

impl Drop for PantographProcess {
    fn drop(&mut self) {
        // kill_on_drop is set, but let's be explicit
        let _ = self.child.start_kill();
    }
}

/// Shared state for the Pantograph process, accessible from request handlers.
/// `None` means no Lean server is connected (simulation mode).
pub type PantographState = Arc<Mutex<Option<PantographProcess>>>;

/// Create empty (disconnected) Pantograph state.
pub fn empty_state() -> PantographState {
    Arc::new(Mutex::new(None))
}

/// Try to spawn Pantograph and return shared state.
/// Falls back to empty state if binary is not found.
pub async fn try_spawn(
    vendor_dir: &std::path::Path,
    lean_project: &str,
) -> PantographState {
    // Try Pantograph first, then lean-repl
    let candidates = [
        vendor_dir.join("pantograph/.lake/build/bin/pantograph"),
        vendor_dir.join("lean-repl/.lake/build/bin/repl"),
    ];

    for path in &candidates {
        if path.exists() {
            let path_str = path.to_string_lossy();
            match PantographProcess::spawn(&path_str, lean_project).await {
                Ok(proc) => {
                    eprintln!(
                        "[ProofBuilder] Pantograph connected: {}",
                        path.display()
                    );
                    return Arc::new(Mutex::new(Some(proc)));
                }
                Err(e) => {
                    eprintln!(
                        "[ProofBuilder] Failed to spawn {}: {e}",
                        path.display()
                    );
                }
            }
        }
    }

    eprintln!("[ProofBuilder] No Lean proof server found — simulation mode");
    empty_state()
}
