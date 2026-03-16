use crate::container::agent_lock::ReusePolicy;
use crate::container::launcher::{destroy_container, launch_container, MeshConfig, RunConfig};
use crate::container::mesh::{call_remote_tool_with_timeout, mesh_registry_path, PeerRegistry};
use crate::container::AgentResponse;
use crate::container::{mesh_auth_token, DEFAULT_MESH_REGISTRY_VOLUME};
use crate::orchestrator::dispatch::ContainerHookupRequest;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ContainerProvisionSpec {
    pub image: String,
    pub peer_agent_id: String,
    pub mcp_port: u16,
    pub registry_volume: PathBuf,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ContainerProvisionDefaults {
    pub image: String,
    pub registry_volume: PathBuf,
    pub mcp_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvisionedContainer {
    pub session_id: String,
    pub container_id: String,
    pub image: String,
    pub peer_agent_id: String,
    pub host_sock: String,
    pub started_at_unix: u64,
    pub mesh_port: Option<u16>,
    pub agent_home: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ContainerInitializeSpec {
    pub peer_agent_id: String,
    pub reuse_policy: ReusePolicy,
    pub hookup: ContainerHookupRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializedContainerAgent {
    pub container_id: String,
    pub agent_id: String,
    pub state: String,
    pub trace_session_id: Option<String>,
    pub reuse_policy: ReusePolicy,
}

#[derive(Debug, Clone)]
pub struct ContainerPromptSpec {
    pub peer_agent_id: String,
    pub prompt: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ContainerDeinitializeSpec {
    pub peer_agent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerDeinitializeResult {
    pub container_id: String,
    pub state: String,
    pub trace_session_id: Option<String>,
    pub reuse_policy: ReusePolicy,
}

#[async_trait]
pub trait ContainerDispatch: Send + Sync {
    fn provision_defaults(&self) -> ContainerProvisionDefaults;

    async fn provision(&self, spec: ContainerProvisionSpec)
        -> Result<ProvisionedContainer, String>;
    async fn initialize(
        &self,
        spec: ContainerInitializeSpec,
    ) -> Result<InitializedContainerAgent, String>;
    async fn send_prompt(&self, spec: ContainerPromptSpec) -> Result<AgentResponse, String>;
    async fn deinitialize(
        &self,
        spec: ContainerDeinitializeSpec,
    ) -> Result<ContainerDeinitializeResult, String>;
    async fn destroy(&self, session_id: &str) -> Result<(), String>;
}

#[derive(Debug, Clone)]
pub struct MeshContainerDispatch {
    default_image: String,
    default_registry_volume: PathBuf,
    default_mcp_port: u16,
}

impl Default for MeshContainerDispatch {
    fn default() -> Self {
        Self {
            default_image: std::env::var("AGENTHALO_CONTAINER_IMAGE")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "nucleusdb-agent:latest".to_string()),
            default_registry_volume: std::env::var("AGENTHALO_CONTAINER_REGISTRY_VOLUME")
                .ok()
                .filter(|v| !v.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(DEFAULT_MESH_REGISTRY_VOLUME)),
            default_mcp_port: std::env::var("AGENTHALO_CONTAINER_MCP_PORT")
                .ok()
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(crate::container::mesh::DEFAULT_MCP_PORT),
        }
    }
}

impl MeshContainerDispatch {
    pub fn default_image(&self) -> &str {
        &self.default_image
    }

    pub fn default_registry_volume(&self) -> &std::path::Path {
        &self.default_registry_volume
    }

    pub fn default_mcp_port(&self) -> u16 {
        self.default_mcp_port
    }

    async fn find_peer(&self, peer_agent_id: &str) -> Result<crate::container::PeerInfo, String> {
        let target = peer_agent_id.to_string();
        tokio::task::spawn_blocking(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                let registry = PeerRegistry::load(&mesh_registry_path())?;
                if let Some(peer) = registry.find(&target) {
                    return Ok(peer.clone());
                }
                if std::time::Instant::now() >= deadline {
                    return Err(format!("mesh peer `{target}` did not register within 30s"));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .await
        .map_err(|e| format!("mesh peer discovery join failure: {e}"))?
    }

    async fn call_remote(
        &self,
        peer_agent_id: &str,
        tool_name: &'static str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.call_remote_with_timeout(
            peer_agent_id,
            tool_name,
            arguments,
            Duration::from_secs(30),
        )
        .await
    }

    async fn call_remote_with_timeout(
        &self,
        peer_agent_id: &str,
        tool_name: &'static str,
        arguments: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let peer = self.find_peer(peer_agent_id).await?;
        let auth_token = mesh_auth_token();
        tokio::task::spawn_blocking(move || {
            call_remote_tool_with_timeout(&peer, tool_name, arguments, auth_token.as_deref(), timeout)
        })
        .await
        .map_err(|e| format!("mesh remote call join failure: {e}"))?
    }
}

fn allocate_local_mcp_port(preferred: u16) -> Result<u16, String> {
    if preferred != 0 && std::net::TcpListener::bind(("127.0.0.1", preferred)).is_ok() {
        return Ok(preferred);
    }
    std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("bind fallback MCP port: {e}"))?
        .local_addr()
        .map_err(|e| format!("read fallback MCP port: {e}"))
        .map(|addr| addr.port())
}

#[async_trait]
impl ContainerDispatch for MeshContainerDispatch {
    fn provision_defaults(&self) -> ContainerProvisionDefaults {
        ContainerProvisionDefaults {
            image: self.default_image.clone(),
            registry_volume: self.default_registry_volume.clone(),
            mcp_port: self.default_mcp_port,
        }
    }

    async fn provision(
        &self,
        spec: ContainerProvisionSpec,
    ) -> Result<ProvisionedContainer, String> {
        let image = if spec.image.trim().is_empty() {
            self.default_image.clone()
        } else {
            spec.image
        };
        let registry_volume = if spec.registry_volume.as_os_str().is_empty() {
            self.default_registry_volume.clone()
        } else {
            spec.registry_volume
        };
        let requested_mcp_port = if spec.mcp_port == 0 {
            self.default_mcp_port
        } else {
            spec.mcp_port
        };
        let mcp_port = allocate_local_mcp_port(requested_mcp_port)?;
        let peer_agent_id = spec.peer_agent_id.clone();
        let env_vars = spec.env.into_iter().collect::<Vec<_>>();
        let info = tokio::task::spawn_blocking(move || {
            launch_container(RunConfig {
                image,
                agent_id: peer_agent_id.clone(),
                command: vec!["agenthalo-mcp-server".to_string()],
                host_sock: None,
                env_vars,
                mesh: Some(MeshConfig {
                    enabled: true,
                    mcp_port,
                    registry_volume,
                    agent_did: None,
                }),
            })
        })
        .await
        .map_err(|e| format!("container provision join failure: {e}"))??;
        Ok(ProvisionedContainer {
            session_id: info.session_id,
            container_id: info.container_id,
            image: info.image,
            peer_agent_id: info.agent_id,
            host_sock: info.host_sock.display().to_string(),
            started_at_unix: info.started_at_unix,
            mesh_port: info.mesh_port,
            agent_home: info.agent_home.map(|path| path.display().to_string()),
        })
    }

    async fn initialize(
        &self,
        spec: ContainerInitializeSpec,
    ) -> Result<InitializedContainerAgent, String> {
        let payload = json!({
            "hookup": spec.hookup,
            "reuse_policy": spec.reuse_policy,
        });
        let value = self
            .call_remote(
                &spec.peer_agent_id,
                "nucleusdb_container_initialize",
                payload,
            )
            .await?;
        serde_json::from_value(value)
            .map_err(|e| format!("decode container initialize response: {e}"))
    }

    async fn send_prompt(&self, spec: ContainerPromptSpec) -> Result<AgentResponse, String> {
        let value = self
            .call_remote_with_timeout(
                &spec.peer_agent_id,
                "nucleusdb_container_agent_prompt",
                json!({ "prompt": spec.prompt }),
                Duration::from_secs(spec.timeout_secs.clamp(5, 3600)),
            )
            .await?;
        serde_json::from_value(value).map_err(|e| format!("decode container prompt response: {e}"))
    }

    async fn deinitialize(
        &self,
        spec: ContainerDeinitializeSpec,
    ) -> Result<ContainerDeinitializeResult, String> {
        let value = self
            .call_remote(
                &spec.peer_agent_id,
                "nucleusdb_container_deinitialize",
                json!({}),
            )
            .await?;
        serde_json::from_value(value)
            .map_err(|e| format!("decode container deinitialize response: {e}"))
    }

    async fn destroy(&self, session_id: &str) -> Result<(), String> {
        let owned = session_id.to_string();
        tokio::task::spawn_blocking(move || destroy_container(&owned))
            .await
            .map_err(|e| format!("container destroy join failure: {e}"))?
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct InMemoryContainerDispatch {
    inner: Mutex<InMemoryContainerState>,
}

#[cfg(test)]
#[derive(Default)]
struct InMemoryContainerState {
    next: u64,
    sessions: BTreeMap<String, FakeContainer>,
}

#[cfg(test)]
struct FakeContainer {
    container_id: String,
    peer_agent_id: String,
    hookup: Option<ContainerHookupRequest>,
    reuse_policy: ReusePolicy,
}

#[cfg(test)]
#[async_trait]
impl ContainerDispatch for InMemoryContainerDispatch {
    fn provision_defaults(&self) -> ContainerProvisionDefaults {
        ContainerProvisionDefaults {
            image: "nucleusdb-agent:test".to_string(),
            registry_volume: PathBuf::from("/tmp/in-memory-registry"),
            mcp_port: 7331,
        }
    }

    async fn provision(
        &self,
        spec: ContainerProvisionSpec,
    ) -> Result<ProvisionedContainer, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.next += 1;
        let session_id = format!("sess-test-{}", inner.next);
        let container_id = format!("ctr-test-{}", inner.next);
        inner.sessions.insert(
            session_id.clone(),
            FakeContainer {
                container_id: container_id.clone(),
                peer_agent_id: spec.peer_agent_id.clone(),
                hookup: None,
                reuse_policy: ReusePolicy::Reusable,
            },
        );
        Ok(ProvisionedContainer {
            session_id: session_id.clone(),
            container_id,
            image: spec.image,
            peer_agent_id: spec.peer_agent_id,
            host_sock: "/tmp/in-memory.sock".to_string(),
            started_at_unix: crate::pod::now_unix(),
            mesh_port: Some(spec.mcp_port),
            agent_home: Some(format!("/tmp/{session_id}/home")),
        })
    }

    async fn initialize(
        &self,
        spec: ContainerInitializeSpec,
    ) -> Result<InitializedContainerAgent, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let (session_id, fake) = inner
            .sessions
            .iter_mut()
            .find(|(_, fake)| fake.peer_agent_id == spec.peer_agent_id)
            .ok_or_else(|| format!("unknown in-memory peer `{}`", spec.peer_agent_id))?;
        fake.hookup = Some(spec.hookup.clone());
        fake.reuse_policy = spec.reuse_policy;
        Ok(InitializedContainerAgent {
            container_id: fake.container_id.clone(),
            agent_id: format!("{}-agent", session_id),
            state: "locked".to_string(),
            trace_session_id: Some(format!("trace-{session_id}")),
            reuse_policy: fake.reuse_policy,
        })
    }

    async fn send_prompt(&self, spec: ContainerPromptSpec) -> Result<AgentResponse, String> {
        let inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let fake = inner
            .sessions
            .values()
            .find(|fake| fake.peer_agent_id == spec.peer_agent_id)
            .ok_or_else(|| format!("unknown in-memory peer `{}`", spec.peer_agent_id))?;
        let hookup = fake
            .hookup
            .as_ref()
            .ok_or_else(|| "container agent not initialized".to_string())?;
        let (content, model) = match hookup {
            ContainerHookupRequest::Cli { cli_name, model } => (
                format!("cli:{}:{}", cli_name, spec.prompt),
                model.clone().unwrap_or_else(|| cli_name.clone()),
            ),
            ContainerHookupRequest::Api {
                provider, model, ..
            } => (format!("api:{}:{}", provider, spec.prompt), model.clone()),
            ContainerHookupRequest::LocalModel { model_id, .. } => (
                format!("local:{}:{}", model_id, spec.prompt),
                model_id.clone(),
            ),
        };
        Ok(AgentResponse {
            content,
            model,
            input_tokens: 1,
            output_tokens: 1,
            cost_usd: 0.0,
            tool_calls: Vec::new(),
            duration_ms: 1,
        })
    }

    async fn deinitialize(
        &self,
        spec: ContainerDeinitializeSpec,
    ) -> Result<ContainerDeinitializeResult, String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let fake = inner
            .sessions
            .values_mut()
            .find(|fake| fake.peer_agent_id == spec.peer_agent_id)
            .ok_or_else(|| format!("unknown in-memory peer `{}`", spec.peer_agent_id))?;
        fake.hookup = None;
        Ok(ContainerDeinitializeResult {
            container_id: fake.container_id.clone(),
            state: "empty".to_string(),
            trace_session_id: None,
            reuse_policy: fake.reuse_policy,
        })
    }

    async fn destroy(&self, session_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        inner.sessions.remove(session_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_provision_is_separate_from_initialize() {
        let dispatch = InMemoryContainerDispatch::default();
        let provisioned = dispatch
            .provision(ContainerProvisionSpec {
                image: "nucleusdb-agent:test".to_string(),
                peer_agent_id: "peer-test".to_string(),
                mcp_port: 7331,
                registry_volume: PathBuf::from("/tmp/in-memory-registry"),
                env: BTreeMap::new(),
            })
            .await
            .expect("provision");
        {
            let inner = dispatch.inner.lock().unwrap_or_else(|p| p.into_inner());
            let fake = inner
                .sessions
                .get(&provisioned.session_id)
                .expect("session exists after provision");
            assert!(
                fake.hookup.is_none(),
                "provision must not initialize agent hookup"
            );
            assert_eq!(fake.reuse_policy, ReusePolicy::Reusable);
        }

        let initialized = dispatch
            .initialize(ContainerInitializeSpec {
                peer_agent_id: "peer-test".to_string(),
                reuse_policy: ReusePolicy::SingleUse,
                hookup: ContainerHookupRequest::Cli {
                    cli_name: "shell".to_string(),
                    model: None,
                },
            })
            .await
            .expect("initialize");
        assert_eq!(initialized.state, "locked");
        assert_eq!(initialized.reuse_policy, ReusePolicy::SingleUse);
    }

    #[test]
    fn allocate_local_mcp_port_falls_back_when_preferred_is_busy() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind busy port");
        let busy_port = listener.local_addr().expect("local addr").port();
        let fallback = allocate_local_mcp_port(busy_port).expect("fallback port");
        assert_ne!(fallback, busy_port);
        assert!(fallback > 0);
    }

    #[tokio::test]
    async fn in_memory_destroy_removes_session() {
        let dispatch = InMemoryContainerDispatch::default();
        let provisioned = dispatch
            .provision(ContainerProvisionSpec {
                image: "nucleusdb-agent:test".to_string(),
                peer_agent_id: "peer-destroy".to_string(),
                mcp_port: 7331,
                registry_volume: PathBuf::from("/tmp/in-memory-registry"),
                env: BTreeMap::new(),
            })
            .await
            .expect("provision");
        dispatch
            .destroy(&provisioned.session_id)
            .await
            .expect("destroy");
        let inner = dispatch.inner.lock().unwrap_or_else(|p| p.into_inner());
        assert!(
            !inner.sessions.contains_key(&provisioned.session_id),
            "destroy must remove the in-memory session"
        );
    }
}
