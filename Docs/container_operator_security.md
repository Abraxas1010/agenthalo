# AgentHALO Native Operator Security Notes

AgentHALO's cockpit and native orchestration stack support operator -> subsidiary
session management. That remains an active product surface, and it has two
security-sensitive deployment concerns:

## Native Process Control

If an operator account can launch or signal local AgentHALO processes through
the dashboard, MCP bridge, or local CLI surfaces, it can start, stop, or
inspect subsidiary sessions. Treat operator-capable access as host-equivalent
power.

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
