# Native Operator Security Notes

Native AgentHALO sessions support operator -> subsidiary orchestration.
That pattern has two security-sensitive deployment surfaces:

## Native Process Control

If an operator account can launch or signal local AgentHALO processes, it can
start, stop, or inspect subsidiary sessions. Treat the operator surface as
host-equivalent power.

Guidance:

- Run operator-capable surfaces only for trusted local users.
- Do not expose operator-capable MCP or dashboard surfaces to untrusted users.
- Prefer a dedicated operator host or VM when running subsidiary automation.

## Mesh Registry Storage

Mesh peer state is shared between operator and subsidiary sessions through the
native registry path configured by `AGENTHALO_CONTAINER_REGISTRY_VOLUME` or the
default AgentHALO home.

Guidance:

- Keep the default AgentHALO-managed registry directory unless you have a
  specific reason to use an absolute host bind path.
- If you override `AGENTHALO_CONTAINER_REGISTRY_VOLUME` with an absolute host
  path, you are responsible for the host filesystem permissions on that path.
- Mesh RPC still requires the shared auth token (`NUCLEUSDB_MESH_AUTH_TOKEN` or
  `AGENTHALO_MCP_SECRET`) even when peer registry entries exist.
