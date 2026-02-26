use crate::cockpit::session::{SessionInfo, SessionStatus};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub struct PtyManager {
    sessions: Mutex<HashMap<String, Arc<PtySession>>>,
    max_sessions: usize,
}

#[derive(Clone, Debug)]
pub enum SessionEvent {
    Output(Vec<u8>),
    Status(SessionStatus),
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
        writer
            .write_all(bytes)
            .and_then(|_| writer.flush())
            .map_err(|e| format!("write PTY input: {e}"))
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
        let _ = self.output_tx.send(SessionEvent::Output(bytes));
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            id: self.id.clone(),
            agent_type: self.agent_type.clone(),
            status: self.status(),
            created_at: self.created_at,
            cols: *self.cols.lock().unwrap_or_else(|e| e.into_inner()),
            rows: *self.rows.lock().unwrap_or_else(|e| e.into_inner()),
            command: self.command.clone(),
            args: self.args.clone(),
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
}

impl PtyManager {
    pub fn new(max: usize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            max_sessions: max,
        }
    }

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
        });

        spawn_reader_thread(session.clone())?;
        sessions.insert(id.clone(), session);
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
        Ok(())
    }

    pub fn resize_session(&self, id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let session = self
            .get_session(id)
            .ok_or_else(|| format!("session `{id}` not found"))?;
        session.resize(cols, rows)
    }

    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions
            .lock()
            .map(|map| map.values().map(|s| s.info()).collect())
            .unwrap_or_default()
    }

    pub fn session_count(&self) -> usize {
        self.sessions.lock().map(|m| m.len()).unwrap_or(0)
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

        manager.destroy_session(&id).expect("destroy session");
        assert_eq!(manager.session_count(), 0);
    }
}
