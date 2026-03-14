use crate::container::mesh;
use crate::container::ContainerBackend;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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

/// Mesh network configuration for inter-container communication.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Enable mesh networking for this container.
    pub enabled: bool,
    /// MCP port to expose on the mesh network.
    pub mcp_port: u16,
    /// Path to shared peer registry volume on host.
    pub registry_volume: PathBuf,
    /// Agent DID URI (populated at launch from genesis seed).
    pub agent_did: Option<String>,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mcp_port: mesh::DEFAULT_MCP_PORT,
            registry_volume: PathBuf::from("/tmp/nucleusdb-mesh"),
            agent_did: None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunConfig {
    pub image: String,
    pub agent_id: String,
    pub command: Vec<String>,
    pub use_gvisor: bool,
    pub host_sock: Option<PathBuf>,
    #[serde(default)]
    pub env_vars: Vec<(String, String)>,
    /// Mesh network configuration. When set and enabled, the container
    /// joins the `halo-mesh` Docker network with MCP port exposed.
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
    /// MCP port on the mesh network (if mesh-enabled).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_port: Option<u16>,
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
    let engine = ContainerBackend::detect();
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

    let mut cmd = engine.command();
    cmd.arg("run")
        .arg("-d")
        .arg("--name")
        .arg(&session_id)
        .arg("-v")
        .arg(format!("{}:/run/nucleusdb.sock", host_sock.display()));
    if cfg.use_gvisor {
        cmd.arg("--runtime").arg("runsc");
    }

    // Mesh networking: shared Docker network + MCP port exposure
    let mut mesh_port_out: Option<u16> = None;
    if let Some(mesh_cfg) = &cfg.mesh {
        if mesh_cfg.enabled {
            mesh::ensure_mesh_network()?;

            cmd.arg("--network").arg(mesh::MESH_NETWORK_NAME);
            cmd.arg("--hostname").arg(&cfg.agent_id);
            cmd.arg("--expose").arg(mesh_cfg.mcp_port.to_string());

            cmd.arg("-e")
                .arg(format!("NUCLEUSDB_MESH_PORT={}", mesh_cfg.mcp_port));
            cmd.arg("-e")
                .arg(format!("NUCLEUSDB_MESH_AGENT_ID={}", cfg.agent_id));
            cmd.arg("-e")
                .arg(format!("AGENTHALO_MCP_PORT={}", mesh_cfg.mcp_port));
            cmd.arg("-e")
                .arg(format!("NUCLEUSDB_MCP_PORT={}", mesh_cfg.mcp_port));
            cmd.arg("-e")
                .arg("NUCLEUSDB_MESH_REGISTRY=/data/mesh/peers.json");
            if let Some(did) = &mesh_cfg.agent_did {
                cmd.arg("-e").arg(format!("NUCLEUSDB_MESH_DID={did}"));
            }

            std::fs::create_dir_all(&mesh_cfg.registry_volume).map_err(|e| {
                format!(
                    "create mesh registry dir {}: {e}",
                    mesh_cfg.registry_volume.display()
                )
            })?;
            cmd.arg("-v")
                .arg(format!("{}:/data/mesh", mesh_cfg.registry_volume.display()));

            mesh_port_out = Some(mesh_cfg.mcp_port);
        }
    }

    for (key, value) in &cfg.env_vars {
        cmd.arg("-e").arg(format!("{key}={value}"));
    }
    cmd.arg(&cfg.image);
    for arg in &cfg.command {
        cmd.arg(arg);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run {}: {e}", engine))?;
    if !out.status.success() {
        return Err(format!(
            "{} run failed: {}",
            engine,
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
        mesh_port: mesh_port_out,
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
    let engine = ContainerBackend::detect();
    let out = engine
        .command()
        .arg("inspect")
        .arg("--format")
        .arg("{{.State.Status}}")
        .arg(session_id)
        .output()
        .map_err(|e| format!("failed to run {} inspect: {e}", engine))?;
    if !out.status.success() {
        return Err(format!(
            "{} inspect failed: {}",
            engine,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn stop_container(session_id: &str) -> Result<(), String> {
    let engine = ContainerBackend::detect();
    let out = engine
        .command()
        .arg("stop")
        .arg(session_id)
        .output()
        .map_err(|e| format!("failed to run {} stop: {e}", engine))?;
    if !out.status.success() {
        return Err(format!(
            "{} stop failed: {}",
            engine,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

pub fn destroy_container(session_id: &str) -> Result<(), String> {
    let engine = ContainerBackend::detect();
    let out = engine
        .command()
        .arg("rm")
        .arg("-f")
        .arg(session_id)
        .output()
        .map_err(|e| format!("failed to run {} rm -f: {e}", engine))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let missing = stderr.contains("No such container") || stderr.contains("No such object");
        if !missing {
            return Err(format!("{} rm -f failed: {}", engine, stderr));
        }
    }
    let meta = run_dir().join(format!("{session_id}.json"));
    let _ = std::fs::remove_file(meta);
    Ok(())
}

pub fn container_logs(session_id: &str, follow: bool) -> Result<String, String> {
    let engine = ContainerBackend::detect();
    let mut cmd = engine.command();
    cmd.arg("logs");
    if follow {
        cmd.arg("-f");
    }
    cmd.arg(session_id);
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run {} logs: {e}", engine))?;
    if !out.status.success() {
        return Err(format!(
            "{} logs failed: {}",
            engine,
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
