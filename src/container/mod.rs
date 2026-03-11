pub mod agent_hookup;
pub mod agent_lock;
pub mod builder;
pub mod launcher;
pub mod mesh;
pub mod mesh_init;
pub mod shim;
pub mod sidecar;

pub use agent_hookup::{
    AgentHealth, AgentHookup, AgentResponse, ApiAgentHookup, CliAgentHookup, LocalModelHookup,
    ToolCallRecord,
};
pub use agent_lock::{
    current_container_id, AgentHookupKind, ContainerAgentLock, ContainerAgentState, DeinitContext,
    ReusePolicy, StateTransition,
};
pub use builder::{build_container_image, BuildConfig};
pub use launcher::{
    container_logs, container_status, destroy_container, launch_container, stop_container, Channel,
    MeshConfig, MonitorConfig, RunConfig, SessionInfo,
};
pub use mesh::{
    call_remote_tool, call_remote_tool_with_timeout, discover_peer, ensure_mesh_network,
    exchange_envelope, mesh_registry_path, ping_peer, ping_peer_with_latency, PeerInfo,
    PeerRegistry, MESH_NETWORK_NAME, MESH_REGISTRY_PATH,
};
pub use mesh_init::{deregister_self_from_mesh, mesh_enabled, register_self_in_mesh};
pub use sidecar::SidecarEvent;
