# Container Operator Security Notes

The unified AgentHALO container supports operator -> subsidiary orchestration.
That pattern has two security-sensitive deployment surfaces:

## Host Docker Socket

If `/var/run/docker.sock` is mounted into the operator container, code inside
that container can ask the host Docker daemon to create, stop, or inspect
containers. Treat this as host-equivalent power.

Guidance:

- Mount the socket only for trusted operator deployments.
- Do not expose operator-capable MCP or dashboard surfaces to untrusted users.
- Prefer a dedicated operator host or VM when running subsidiary automation.

## Mesh Registry Storage

Mesh peer state is shared between operator and subsidiary containers through a
Docker-managed volume by default (`agenthalo-mesh`). This avoids the previous
default of a world-writable host `/tmp` bind mount.

Guidance:

- Keep the default named-volume configuration unless you have a specific reason
  to use an absolute host bind mount.
- If you override `AGENTHALO_CONTAINER_REGISTRY_VOLUME` with an absolute host
  path, you are responsible for the host filesystem permissions on that path.
- Mesh RPC still requires the shared auth token (`NUCLEUSDB_MESH_AUTH_TOKEN` or
  `AGENTHALO_MCP_SECRET`) even when peer registry entries exist.
