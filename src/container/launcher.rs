use crate::container::coordination::{
    mesh_auth_token, prepare_bind_mount_dir, prepare_named_volume, registry_volume_is_named,
    DEFAULT_MESH_REGISTRY_VOLUME,
};
use crate::container::mesh;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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

pub fn parse_channel_list(input: &str) -> Result<Vec<Channel>, String> {
    let mut out = Vec::new();
    for raw in input.split(',') {
        let normalized = raw.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        let channel = match normalized.as_str() {
            "everything" => Channel::Everything,
            "chat" => Channel::Chat,
            "payments" => Channel::Payments,
            "tools" => Channel::Tools,
            "state" => Channel::State,
            other => return Err(format!("unknown monitor channel `{other}`")),
        };
        out.push(channel);
    }
    if out.is_empty() {
        return Err("at least one monitor channel is required".to_string());
    }
    Ok(out)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    pub enabled: bool,
    pub mcp_port: u16,
    pub registry_volume: PathBuf,
    pub agent_did: Option<String>,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mcp_port: std::env::var("AGENTHALO_CONTAINER_MCP_PORT")
                .ok()
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(mesh::DEFAULT_MCP_PORT),
            registry_volume: std::env::var("AGENTHALO_CONTAINER_REGISTRY_VOLUME")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_MESH_REGISTRY_VOLUME)),
            agent_did: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunConfig {
    pub image: String,
    pub agent_id: String,
    pub command: Vec<String>,
    pub host_sock: Option<PathBuf>,
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
    #[serde(default)]
    pub mesh: Option<MeshConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub container_id: String,
    pub image: String,
    pub agent_id: String,
    pub host_sock: PathBuf,
    pub started_at_unix: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_home: Option<PathBuf>,
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
    std::env::temp_dir().join("agenthalo-native")
}

fn env_contains(env_vars: &[(String, String)], key: &str) -> bool {
    env_vars.iter().any(|(existing, _)| existing == key)
}

fn direct_mcp_server_command(command: &[String]) -> bool {
    command
        .first()
        .map(|value| {
            value == "agenthalo-mcp-server"
                || value.ends_with("/agenthalo-mcp-server")
                || value.ends_with("\\agenthalo-mcp-server")
        })
        .unwrap_or(false)
}

/// Headless cockpit agent lanes run `agenthalo-mcp-server` under an isolated
/// `AGENTHALO_HOME` so each persistent agent keeps its own lock/state material.
/// Interactive PTY sessions intentionally inherit the host shell environment.
fn apply_direct_mcp_defaults(
    session_id: &str,
    agent_id: &str,
    agent_home: &Path,
    env_vars: &mut Vec<(String, String)>,
) {
    if !env_contains(env_vars, "AGENTHALO_HOME") {
        env_vars.push((
            "AGENTHALO_HOME".to_string(),
            agent_home.display().to_string(),
        ));
    }
    if !env_contains(env_vars, "AGENTHALO_SESSION_ID") {
        env_vars.push(("AGENTHALO_SESSION_ID".to_string(), session_id.to_string()));
    }
    if !env_contains(env_vars, "AGENTHALO_AGENT_ID") {
        env_vars.push(("AGENTHALO_AGENT_ID".to_string(), agent_id.to_string()));
    }
    if !env_contains(env_vars, "AGENTHALO_MCP_HOST") {
        env_vars.push(("AGENTHALO_MCP_HOST".to_string(), "127.0.0.1".to_string()));
    }
    if let Some(secret) = mesh_auth_token() {
        if !env_contains(env_vars, "AGENTHALO_MCP_SECRET") {
            env_vars.push(("AGENTHALO_MCP_SECRET".to_string(), secret.clone()));
        }
        if !env_contains(env_vars, "NUCLEUSDB_MESH_AUTH_TOKEN") {
            env_vars.push(("NUCLEUSDB_MESH_AUTH_TOKEN".to_string(), secret));
        }
    }
}

fn session_dir(session_id: &str) -> PathBuf {
    run_dir().join(session_id)
}

fn session_home_dir(session_id: &str) -> PathBuf {
    session_dir(session_id).join("home")
}

fn pid_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid as i32, 0) };
        if rc == 0 {
            return true;
        }
        return std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn resolve_command(raw: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() || raw.contains('/') || raw.contains('\\') {
        return Ok(candidate);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join(raw);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
    }
    Ok(candidate)
}

fn metadata_path(session_id: &str) -> PathBuf {
    session_dir(session_id).join("session.json")
}

pub fn launch_container(cfg: RunConfig) -> Result<SessionInfo, String> {
    let session_id = make_session_id();
    let dir = session_dir(&session_id);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create run dir {}: {e}", dir.display()))?;
    let host_sock = cfg
        .host_sock
        .unwrap_or_else(|| dir.join("agenthalo.sock"));
    let log_path = dir.join("process.log");

    let mut env_vars = cfg.env_vars.clone();
    let agent_home = session_home_dir(&session_id);
    std::fs::create_dir_all(&agent_home)
        .map_err(|e| format!("failed to create agent home {}: {e}", agent_home.display()))?;
    let mut mesh_port_out: Option<u16> = None;
    if let Some(mesh_cfg) = &cfg.mesh {
        if mesh_cfg.enabled {
            mesh::ensure_mesh_network()?;
            let registry_dir = if registry_volume_is_named(&mesh_cfg.registry_volume) {
                prepare_named_volume(&mesh_cfg.registry_volume, &cfg.image, "mesh registry dir")?;
                crate::container::coordination::resolve_registry_dir(&mesh_cfg.registry_volume)
            } else {
                prepare_bind_mount_dir(&mesh_cfg.registry_volume, "mesh registry dir")?;
                mesh_cfg.registry_volume.clone()
            };
            let registry_path = registry_dir.join("peers.json");
            env_vars.push((
                "NUCLEUSDB_MESH_REGISTRY".to_string(),
                registry_path.display().to_string(),
            ));
            env_vars.push((
                "NUCLEUSDB_MESH_PORT".to_string(),
                mesh_cfg.mcp_port.to_string(),
            ));
            env_vars.push((
                "NUCLEUSDB_MESH_AGENT_ID".to_string(),
                cfg.agent_id.clone(),
            ));
            env_vars.push((
                "AGENTHALO_MCP_PORT".to_string(),
                mesh_cfg.mcp_port.to_string(),
            ));
            env_vars.push((
                "NUCLEUSDB_MCP_PORT".to_string(),
                mesh_cfg.mcp_port.to_string(),
            ));
            if let Some(did) = &mesh_cfg.agent_did {
                env_vars.push(("NUCLEUSDB_MESH_DID".to_string(), did.clone()));
            }
            mesh_port_out = Some(mesh_cfg.mcp_port);
        }
    }

    if direct_mcp_server_command(&cfg.command) {
        apply_direct_mcp_defaults(&session_id, &cfg.agent_id, &agent_home, &mut env_vars);
    }

    let (entrypoint, args) = cfg
        .command
        .split_first()
        .ok_or_else(|| "native launch requires a non-empty command".to_string())?;
    let program = resolve_command(entrypoint)?;
    let log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .map_err(|e| format!("open session log {}: {e}", log_path.display()))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|e| format!("clone session log {}: {e}", log_path.display()))?;

    let mut cmd = Command::new(&program);
    cmd.args(args)
        .current_dir(std::env::current_dir().map_err(|e| format!("cwd: {e}"))?)
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));
    for (key, value) in &env_vars {
        cmd.env(key, value);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }

    let child = cmd
        .spawn()
        .map_err(|e| format!("failed to start native session `{}`: {e}", program.display()))?;

    let info = SessionInfo {
        session_id: session_id.clone(),
        container_id: format!("native-{}", child.id()),
        image: if cfg.image.trim().is_empty() {
            "native-process".to_string()
        } else {
            cfg.image
        },
        agent_id: cfg.agent_id,
        host_sock,
        started_at_unix: now_unix_secs(),
        mesh_port: mesh_port_out,
        pid: Some(child.id()),
        log_path: Some(log_path),
        agent_home: Some(agent_home),
    };
    std::fs::write(
        metadata_path(&session_id),
        serde_json::to_vec_pretty(&info).map_err(|e| format!("failed to encode session: {e}"))?,
    )
    .map_err(|e| format!("failed to persist session metadata: {e}"))?;
    Ok(info)
}

pub fn container_status(session_id: &str) -> Result<String, String> {
    let info = load_session(session_id)?;
    match info.pid {
        Some(pid) if pid_is_alive(pid) => Ok("running".to_string()),
        Some(_) => Ok("stopped".to_string()),
        None => Ok("unknown".to_string()),
    }
}

pub fn stop_container(session_id: &str) -> Result<(), String> {
    let info = load_session(session_id)?;
    if let Some(pid) = info.pid {
        #[cfg(unix)]
        {
            let rc = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            if rc != 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(format!("stop native session `{session_id}`: {err}"));
                }
            }
        }
    }
    Ok(())
}

pub fn destroy_container(session_id: &str) -> Result<(), String> {
    let _ = stop_container(session_id);
    let dir = session_dir(session_id);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("remove session dir {}: {e}", dir.display()))?;
    }
    Ok(())
}

pub fn container_logs(session_id: &str, _follow: bool) -> Result<String, String> {
    let info = load_session(session_id)?;
    let log_path = info
        .log_path
        .ok_or_else(|| format!("session `{session_id}` has no log path"))?;
    std::fs::read_to_string(&log_path)
        .map_err(|e| format!("read native session log {}: {e}", log_path.display()))
}

pub fn load_session(session_id: &str) -> Result<SessionInfo, String> {
    let path = metadata_path(session_id);
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
        let path = entry.path().join("session.json");
        if !path.exists() {
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
        "native sidecar binary missing at {}",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_direct_mcp_defaults_sets_isolated_home_and_identity_env() {
        let mut env_vars = Vec::new();
        let home = std::env::temp_dir().join("agenthalo-test-home");
        apply_direct_mcp_defaults("sess-123", "agent-456", &home, &mut env_vars);
        let as_map = env_vars.into_iter().collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            as_map.get("AGENTHALO_HOME").map(String::as_str),
            Some(home.to_string_lossy().as_ref())
        );
        assert_eq!(
            as_map.get("AGENTHALO_SESSION_ID").map(String::as_str),
            Some("sess-123")
        );
        assert_eq!(
            as_map.get("AGENTHALO_AGENT_ID").map(String::as_str),
            Some("agent-456")
        );
        assert_eq!(
            as_map.get("AGENTHALO_MCP_HOST").map(String::as_str),
            Some("127.0.0.1")
        );
    }

    #[test]
    fn apply_direct_mcp_defaults_preserves_existing_values() {
        let home = std::env::temp_dir().join("agenthalo-test-home");
        let mut env_vars = vec![
            ("AGENTHALO_HOME".to_string(), "/tmp/existing-home".to_string()),
            (
                "AGENTHALO_SESSION_ID".to_string(),
                "existing-session".to_string(),
            ),
            ("AGENTHALO_AGENT_ID".to_string(), "existing-agent".to_string()),
            ("AGENTHALO_MCP_HOST".to_string(), "10.0.0.9".to_string()),
        ];
        apply_direct_mcp_defaults("sess-123", "agent-456", &home, &mut env_vars);
        let as_map = env_vars.into_iter().collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            as_map.get("AGENTHALO_HOME").map(String::as_str),
            Some("/tmp/existing-home")
        );
        assert_eq!(
            as_map.get("AGENTHALO_SESSION_ID").map(String::as_str),
            Some("existing-session")
        );
        assert_eq!(
            as_map.get("AGENTHALO_AGENT_ID").map(String::as_str),
            Some("existing-agent")
        );
        assert_eq!(
            as_map.get("AGENTHALO_MCP_HOST").map(String::as_str),
            Some("10.0.0.9")
        );
    }
}
