use crate::cockpit::session::{SessionInfo, SessionStatus};
use crate::halo::governor_registry::GovernorRegistry;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub struct PtyManager {
    sessions: Mutex<HashMap<String, Arc<PtySession>>>,
    max_sessions: usize,
    governor_registry: Option<Arc<GovernorRegistry>>,
    recommended_idle_timeout_secs: AtomicU64,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Output(Vec<u8>),
    Status(SessionStatus),
}

#[derive(Clone, Debug)]
pub struct SessionTelemetry {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub estimated_input_tokens: u64,
    pub estimated_output_tokens: u64,
    pub estimated_cost_usd: f64,
    pub runtime_secs: u64,
    pub trace_flushed: bool,
}

pub struct PtySession {
    pub id: String,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    status: Mutex<SessionStatus>,
    created_at: u64,
    agent_type: Option<String>,
    cols: Mutex<u16>,
    rows: Mutex<u16>,
    command: String,
    args: Vec<String>,
    output_tx: broadcast::Sender<SessionEvent>,
    captured_output: Mutex<Vec<u8>>,
    input_bytes: AtomicU64,
    output_bytes: AtomicU64,
    trace_flushed: AtomicBool,
    last_activity_unix: AtomicU64,
}

impl PtySession {
    fn clone_reader(&self) -> Result<Box<dyn Read + Send>, String> {
        let master = self
            .master
            .lock()
            .map_err(|e| format!("pty master lock poisoned: {e}"))?;
        master
            .try_clone_reader()
            .map_err(|e| format!("clone PTY reader: {e}"))
    }

    pub fn subscribe_output(&self) -> broadcast::Receiver<SessionEvent> {
        self.output_tx.subscribe()
    }

    pub fn write_input(&self, bytes: &[u8]) -> Result<(), String> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| format!("pty writer lock poisoned: {e}"))?;
        let result = writer
            .write_all(bytes)
            .and_then(|_| writer.flush())
            .map_err(|e| format!("write PTY input: {e}"));
        if result.is_ok() && !bytes.is_empty() {
            self.note_input(bytes.len() as u64);
        }
        result
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let master = self
            .master
            .lock()
            .map_err(|e| format!("pty master lock poisoned: {e}"))?;
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize PTY: {e}"))?;
        drop(master);

        if let Ok(mut c) = self.cols.lock() {
            *c = cols;
        }
        if let Ok(mut r) = self.rows.lock() {
            *r = rows;
        }
        Ok(())
    }

    pub fn status(&self) -> SessionStatus {
        self.status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_else(|_| SessionStatus::Error {
                message: "status lock poisoned".to_string(),
            })
    }

    pub fn set_status(&self, status: SessionStatus) {
        if let Ok(mut s) = self.status.lock() {
            *s = status.clone();
        }
        let _ = self.output_tx.send(SessionEvent::Status(status));
    }

    pub fn publish_output(&self, bytes: Vec<u8>) {
        if !bytes.is_empty() {
            self.note_output(bytes.len() as u64);
            if let Ok(mut captured) = self.captured_output.lock() {
                const MAX_CAPTURED_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
                captured.extend_from_slice(&bytes);
                if captured.len() > MAX_CAPTURED_OUTPUT_BYTES {
                    let overflow = captured.len() - MAX_CAPTURED_OUTPUT_BYTES;
                    captured.drain(..overflow);
                }
            }
        }
        let _ = self.output_tx.send(SessionEvent::Output(bytes));
    }

    pub fn snapshot_output(&self) -> Vec<u8> {
        self.captured_output
            .lock()
            .map(|buf| buf.clone())
            .unwrap_or_default()
    }

    pub fn note_input(&self, n: u64) {
        self.input_bytes.fetch_add(n, Ordering::Relaxed);
        self.touch_activity();
    }

    pub fn note_output(&self, n: u64) {
        self.output_bytes.fetch_add(n, Ordering::Relaxed);
        self.touch_activity();
    }

    pub fn telemetry_snapshot(&self) -> SessionTelemetry {
        let input_bytes = self.input_bytes.load(Ordering::Relaxed);
        let output_bytes = self.output_bytes.load(Ordering::Relaxed);
        // Conservative plain-text estimate: ~4 chars per token.
        let estimated_input_tokens = input_bytes.div_ceil(4);
        let estimated_output_tokens = output_bytes.div_ceil(4);
        let estimated_cost_usd = (estimated_input_tokens as f64 * 3.0 / 1_000_000.0)
            + (estimated_output_tokens as f64 * 15.0 / 1_000_000.0);
        let runtime_secs = now_unix().saturating_sub(self.created_at);

        SessionTelemetry {
            input_bytes,
            output_bytes,
            estimated_input_tokens,
            estimated_output_tokens,
            estimated_cost_usd,
            runtime_secs,
            trace_flushed: self.trace_flushed.load(Ordering::Relaxed),
        }
    }

    pub fn mark_trace_flushed(&self) -> bool {
        !self.trace_flushed.swap(true, Ordering::SeqCst)
    }

    pub fn is_trace_flushed(&self) -> bool {
        self.trace_flushed.load(Ordering::Relaxed)
    }

    pub fn info(&self) -> SessionInfo {
        let t = self.telemetry_snapshot();
        SessionInfo {
            id: self.id.clone(),
            agent_type: self.agent_type.clone(),
            status: self.status(),
            created_at: self.created_at,
            cols: *self.cols.lock().unwrap_or_else(|e| e.into_inner()),
            rows: *self.rows.lock().unwrap_or_else(|e| e.into_inner()),
            command: self.command.clone(),
            args: self.args.clone(),
            input_bytes: t.input_bytes,
            output_bytes: t.output_bytes,
            estimated_input_tokens: t.estimated_input_tokens,
            estimated_output_tokens: t.estimated_output_tokens,
            estimated_cost_usd: t.estimated_cost_usd,
            runtime_secs: t.runtime_secs,
            trace_flushed: t.trace_flushed,
            idle_secs: self.idle_secs(),
            recommended_idle_timeout_secs: 0,
        }
    }

    pub fn terminate(&self) -> Result<(), String> {
        let mut child = self
            .child
            .lock()
            .map_err(|e| format!("pty child lock poisoned: {e}"))?;
        child.kill().map_err(|e| format!("kill PTY child: {e}"))
    }

    pub fn poll_exit_status(&self) -> Option<i32> {
        let mut child = self.child.lock().ok()?;
        let maybe = child.try_wait().ok()?;
        maybe.map(|status| status.exit_code() as i32)
    }

    pub fn idle_secs(&self) -> u64 {
        now_unix().saturating_sub(self.last_activity_unix.load(Ordering::Relaxed))
    }

    fn touch_activity(&self) {
        self.last_activity_unix.store(now_unix(), Ordering::Relaxed);
    }
}

impl PtyManager {
    pub fn new(max: usize) -> Self {
        Self::with_governor_registry(max, None)
    }

    pub fn with_governor_registry(
        max: usize,
        governor_registry: Option<Arc<GovernorRegistry>>,
    ) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            max_sessions: max,
            governor_registry,
            recommended_idle_timeout_secs: AtomicU64::new(120),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_session(
        &self,
        command: &str,
        args: &[String],
        env: Vec<(String, String)>,
        working_dir: Option<&str>,
        cols: u16,
        rows: u16,
        agent_type: Option<String>,
    ) -> Result<String, String> {
        self.create_session_with_env_control(
            command,
            args,
            env,
            &[],
            working_dir,
            cols,
            rows,
            agent_type,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_session_with_env_control(
        &self,
        command: &str,
        args: &[String],
        env: Vec<(String, String)>,
        env_remove: &[String],
        working_dir: Option<&str>,
        cols: u16,
        rows: u16,
        agent_type: Option<String>,
    ) -> Result<String, String> {
        self.reap_idle_sessions_if_needed();
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|e| format!("session map lock poisoned: {e}"))?;
        if sessions.len() >= self.max_sessions {
            return Err(format!("maximum {} sessions reached", self.max_sessions));
        }

        let pty_system = native_pty_system();
        let pty_pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("open PTY: {e}"))?;

        let mut cmd = CommandBuilder::new(command);
        for arg in args {
            cmd.arg(arg);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        for key in env_remove {
            cmd.env_remove(key);
        }
        if let Some(dir) = working_dir.filter(|d| !d.trim().is_empty()) {
            cmd.cwd(dir);
        }

        let child = pty_pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("spawn PTY command `{command}`: {e}"))?;
        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|e| format!("open PTY writer: {e}"))?;

        let (output_tx, _) = broadcast::channel(1024);
        let id = format!(
            "pty-{}-{}",
            now_unix(),
            &uuid::Uuid::new_v4().as_simple().to_string()[..4]
        );
        let session = Arc::new(PtySession {
            id: id.clone(),
            master: Mutex::new(pty_pair.master),
            child: Mutex::new(child),
            writer: Mutex::new(writer),
            status: Mutex::new(SessionStatus::Active),
            created_at: now_unix(),
            agent_type,
            cols: Mutex::new(cols),
            rows: Mutex::new(rows),
            command: command.to_string(),
            args: args.to_vec(),
            output_tx,
            captured_output: Mutex::new(Vec::new()),
            input_bytes: AtomicU64::new(0),
            output_bytes: AtomicU64::new(0),
            trace_flushed: AtomicBool::new(false),
            last_activity_unix: AtomicU64::new(now_unix()),
        });

        spawn_reader_thread(session.clone())?;
        sessions.insert(id.clone(), session);
        let current_len = sessions.len();
        drop(sessions);
        self.observe_runtime(current_len);
        Ok(id)
    }

    pub fn get_session(&self, id: &str) -> Option<Arc<PtySession>> {
        self.sessions.lock().ok()?.get(id).cloned()
    }

    pub fn destroy_session(&self, id: &str) -> Result<(), String> {
        let removed = self
            .sessions
            .lock()
            .map_err(|e| format!("session map lock poisoned: {e}"))?
            .remove(id);
        let session = removed.ok_or_else(|| format!("session `{id}` not found"))?;
        let _ = session.terminate();
        session.set_status(SessionStatus::Done { exit_code: 0 });
        self.observe_runtime(self.session_count());
        Ok(())
    }

    pub fn resize_session(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self
            .get_session(id)
            .ok_or_else(|| format!("session `{id}` not found"))?;
        session.resize(cols, rows)
    }

    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        let recommended_idle_timeout_secs = self.recommended_idle_timeout_secs();
        let list: Vec<SessionInfo> = self
            .sessions
            .lock()
            .map(|map| map.values().map(|s| s.info()).collect())
            .unwrap_or_default();
        let mut list = list;
        for item in &mut list {
            item.recommended_idle_timeout_secs = recommended_idle_timeout_secs;
        }
        self.observe_runtime(list.len());
        list
    }

    pub fn list_session_handles(&self) -> Vec<Arc<PtySession>> {
        self.sessions
            .lock()
            .map(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().map(|m| m.len()).unwrap_or(0)
    }

    pub fn recommended_idle_timeout_secs(&self) -> u64 {
        self.recommended_idle_timeout_secs
            .load(Ordering::Relaxed)
            .max(30)
    }

    pub fn soft_reset_quiescent_governors(&self) {
        let Some(registry) = &self.governor_registry else {
            return;
        };
        if self.session_count() == 0 {
            let _ = registry.soft_reset("gov-compute");
            let _ = registry.soft_reset("gov-pty");
        }
    }

    fn observe_runtime(&self, current_len: usize) {
        let Some(registry) = &self.governor_registry else {
            return;
        };
        let _ = registry.observe("gov-compute", current_len as f64);
        let average_idle_secs = self
            .sessions
            .lock()
            .map(|map| {
                if map.is_empty() {
                    0.0
                } else {
                    map.values()
                        .map(|session| session.idle_secs() as f64)
                        .sum::<f64>()
                        / map.len() as f64
                }
            })
            .unwrap_or(0.0);
        let _ = registry.observe("gov-pty", average_idle_secs);
        if let Ok(snapshot) = registry.snapshot_one("gov-pty") {
            self.recommended_idle_timeout_secs
                .store(snapshot.epsilon.ceil().max(30.0) as u64, Ordering::Relaxed);
        }
    }

    fn reap_idle_sessions_if_needed(&self) {
        let Some(registry) = &self.governor_registry else {
            return;
        };
        let timeout = registry
            .snapshot_one("gov-pty")
            .ok()
            .map(|snapshot| snapshot.epsilon.ceil().max(30.0) as u64)
            .unwrap_or_else(|| self.recommended_idle_timeout_secs());
        let ids_to_reap = self
            .sessions
            .lock()
            .map(|map| {
                map.iter()
                    .filter(|(_, session)| {
                        let idle = session.idle_secs();
                        let status = session.status();
                        idle > timeout
                            && match status {
                                SessionStatus::Done { .. } | SessionStatus::Error { .. } => true,
                                SessionStatus::Active => map.len() >= self.max_sessions,
                                SessionStatus::Starting => false,
                            }
                    })
                    .map(|(id, _)| id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for id in ids_to_reap {
            let _ = self.destroy_session(&id);
        }
    }
}

fn spawn_reader_thread(session: Arc<PtySession>) -> Result<(), String> {
    let mut reader = session.clone_reader()?;
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    let exit_code = session.poll_exit_status().unwrap_or(0);
                    session.set_status(SessionStatus::Done { exit_code });
                    break;
                }
                Ok(n) => {
                    session.publish_output(buf[..n].to_vec());
                }
                Err(e) => {
                    session.set_status(SessionStatus::Error {
                        message: format!("PTY read error: {e}"),
                    });
                    break;
                }
            }
        }
    });
    Ok(())
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_sessions_enforced() {
        let manager = PtyManager::new(1);
        let id1 = manager
            .create_session(
                "/bin/sh",
                &["-c".to_string(), "sleep 2".to_string()],
                vec![],
                None,
                80,
                24,
                Some("shell".to_string()),
            )
            .expect("first session should be created");

        let err = manager
            .create_session(
                "/bin/sh",
                &["-c".to_string(), "sleep 2".to_string()],
                vec![],
                None,
                80,
                24,
                Some("shell".to_string()),
            )
            .expect_err("second session should fail");
        assert!(err.contains("maximum"));

        manager
            .destroy_session(&id1)
            .expect("destroy first session");
    }

    #[test]
    fn governor_registry_does_not_clamp_nominal_session_limit() {
        let registry = crate::halo::governor_registry::build_default_registry();
        let manager = PtyManager::with_governor_registry(3, Some(registry));
        let id1 = manager
            .create_session(
                "/bin/sh",
                &["-c".to_string(), "sleep 2".to_string()],
                vec![],
                None,
                80,
                24,
                Some("shell".to_string()),
            )
            .expect("first session");
        let id2 = manager
            .create_session(
                "/bin/sh",
                &["-c".to_string(), "sleep 2".to_string()],
                vec![],
                None,
                80,
                24,
                Some("shell".to_string()),
            )
            .expect("second session");
        assert_eq!(manager.session_count(), 2);
        manager.destroy_session(&id1).expect("destroy first");
        manager.destroy_session(&id2).expect("destroy second");
    }

    #[test]
    fn list_and_resize_session() {
        let manager = PtyManager::new(2);
        let id = manager
            .create_session(
                "/bin/sh",
                &["-c".to_string(), "sleep 2".to_string()],
                vec![],
                None,
                80,
                24,
                None,
            )
            .expect("create session");

        manager
            .resize_session(&id, 120, 40)
            .expect("resize should succeed");

        let list = manager.list_sessions();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);
        assert_eq!(list[0].cols, 120);
        assert_eq!(list[0].rows, 40);
        assert!(list[0].recommended_idle_timeout_secs >= 30);

        manager.destroy_session(&id).expect("destroy session");
        assert_eq!(manager.session_count(), 0);
    }

    #[test]
    fn telemetry_estimates_tokens_and_cost() {
        let manager = PtyManager::new(1);
        let id = manager
            .create_session(
                "/bin/sh",
                &["-c".to_string(), "sleep 1".to_string()],
                vec![],
                None,
                80,
                24,
                Some("shell".to_string()),
            )
            .expect("create session");
        let session = manager.get_session(&id).expect("session exists");
        session.note_input(40);
        session.note_output(80);
        let t = session.telemetry_snapshot();
        assert_eq!(t.estimated_input_tokens, 10);
        assert_eq!(t.estimated_output_tokens, 20);
        assert!(t.estimated_cost_usd > 0.0);
        manager.destroy_session(&id).expect("destroy session");
    }
}
