use crate::comms::envelope::{unwrap_orchestrator_result, OrchestratorResultEnvelope};
use crate::container::mesh::{call_remote_tool, mesh_registry_path, PeerRegistry};
use crate::container::mesh_auth_token;
use crate::halo::did::DIDIdentity;
use crate::pod::capability::CapabilityToken;

/// Delegate a task to a remote mesh peer.
///
/// This uses the peer's `orchestrator_send_task` MCP tool surface. The DID identity
/// and capability tokens are accepted for forward compatibility with full DIDComm
/// delegation flows.
pub fn delegate_task_to_peer(
    _local_identity: &DIDIdentity,
    peer_agent_id: &str,
    target_agent_id: &str,
    prompt: &str,
    timeout_secs: u64,
    _capability_tokens: &[CapabilityToken],
) -> Result<OrchestratorResultEnvelope, String> {
    let registry = PeerRegistry::load(&mesh_registry_path())?;
    let peer = registry
        .find(peer_agent_id)
        .ok_or_else(|| format!("unknown mesh peer '{peer_agent_id}'"))?;
    let result = call_remote_tool(
        peer,
        "orchestrator_send_task",
        serde_json::json!({
            "agent_id": target_agent_id,
            "task": prompt,
            "wait": true,
            "timeout_secs": timeout_secs,
        }),
        mesh_auth_token().as_deref(),
    )?;
    unwrap_orchestrator_result(result)
}
