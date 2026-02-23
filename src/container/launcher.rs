use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Channel {
    Everything,
    Chat,
    Payments,
    Tools,
    State,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub channels: Vec<Channel>,
    pub agent_id: String,
    pub max_nesting_depth: u32,
}

impl MonitorConfig {
    pub fn channels_csv(&self) -> String {
        self.channels
            .iter()
            .map(|c| match c {
                Channel::Everything => "everything",
                Channel::Chat => "chat",
                Channel::Payments => "payments",
                Channel::Tools => "tools",
                Channel::State => "state",
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunConfig {
    pub image: String,
    pub agent_id: String,
    pub command: Vec<String>,
    pub use_gvisor: bool,
    pub host_sock: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub container_id: String,
    pub image: String,
    pub agent_id: String,
    pub host_sock: PathBuf,
    pub started_at_unix: u64,
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn make_session_id() -> String {
    let pid = std::process::id();
    let ts = now_unix_secs();
    format!("sess-{ts}-{pid}")
}

fn run_dir() -> PathBuf {
    std::env::temp_dir().join("nucleusdb-container")
}

pub fn launch_container(cfg: RunConfig) -> Result<SessionInfo, String> {
    let session_id = make_session_id();
    let run_dir = run_dir();
    std::fs::create_dir_all(&run_dir)
        .map_err(|e| format!("failed to create run dir {}: {e}", run_dir.display()))?;
    let host_sock = cfg
        .host_sock
        .unwrap_or_else(|| run_dir.join(format!("{session_id}.sock")));
    if host_sock.exists() {
        let _ = std::fs::remove_file(&host_sock);
    }

    let mut cmd = Command::new("docker");
    cmd.arg("run")
        .arg("-d")
        .arg("--name")
        .arg(&session_id)
        .arg("-v")
        .arg(format!("{}:/run/nucleusdb.sock", host_sock.display()));
    if cfg.use_gvisor {
        cmd.arg("--runtime").arg("runsc");
    }
    cmd.arg(&cfg.image);
    for arg in &cfg.command {
        cmd.arg(arg);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run docker: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker run failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let container_id = String::from_utf8(out.stdout)
        .map_err(|e| format!("invalid docker output: {e}"))?
        .trim()
        .to_string();
    let info = SessionInfo {
        session_id: session_id.clone(),
        container_id,
        image: cfg.image,
        agent_id: cfg.agent_id,
        host_sock,
        started_at_unix: now_unix_secs(),
    };
    let meta = run_dir.join(format!("{session_id}.json"));
    std::fs::write(
        &meta,
        serde_json::to_vec_pretty(&info).map_err(|e| format!("failed to encode session: {e}"))?,
    )
    .map_err(|e| format!("failed to persist {}: {e}", meta.display()))?;
    Ok(info)
}

pub fn container_status(session_id: &str) -> Result<String, String> {
    let out = Command::new("docker")
        .arg("inspect")
        .arg("--format")
        .arg("{{.State.Status}}")
        .arg(session_id)
        .output()
        .map_err(|e| format!("failed to run docker inspect: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker inspect failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn stop_container(session_id: &str) -> Result<(), String> {
    let out = Command::new("docker")
        .arg("stop")
        .arg(session_id)
        .output()
        .map_err(|e| format!("failed to run docker stop: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker stop failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

pub fn container_logs(session_id: &str, follow: bool) -> Result<String, String> {
    let mut cmd = Command::new("docker");
    cmd.arg("logs");
    if follow {
        cmd.arg("-f");
    }
    cmd.arg(session_id);
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run docker logs: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "docker logs failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn load_session(session_id: &str) -> Result<SessionInfo, String> {
    let path = run_dir().join(format!("{session_id}.json"));
    let data =
        std::fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    serde_json::from_slice(&data).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

pub fn list_sessions() -> Result<Vec<SessionInfo>, String> {
    let mut sessions = Vec::new();
    let dir = run_dir();
    if !dir.exists() {
        return Ok(sessions);
    }
    let entries = std::fs::read_dir(&dir)
        .map_err(|e| format!("failed to read run dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let data = match std::fs::read(&path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Ok(info) = serde_json::from_slice::<SessionInfo>(&data) {
            sessions.push(info);
        }
    }
    Ok(sessions)
}

pub fn ensure_sidecar_binary(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    Err(format!(
        "sidecar binary missing at {} (build it before container build)",
        path.display()
    ))
}
