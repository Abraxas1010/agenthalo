# Contributing to AgentHALO

## Scope

This repository is the live AgentHALO platform. Contributions should strengthen one of these surfaces:

- AgentHALO CLI, dashboard, cockpit, and MCP operator surfaces
- NucleusDB core, SQL execution, append-only records, and verification
- agent lifecycle orchestration, native session management, and mesh/comms
- identity, wallet, attestation, trust, and post-quantum crypto flows
- Discord recording, AgentPMT, P2PCLAW, and deployment surfaces
- local Lean mirror proofs under `lean/NucleusDB/`

Do not narrow the repository back to a standalone NucleusDB-only product unless there is an explicit project decision to do so.

## Development

```bash
cargo check --bin agenthalo --bin agenthalo-mcp-server --bin nucleusdb --bin nucleusdb-mcp --bin nucleusdb-discord --bin nucleusdb-server --bin nucleusdb-tui
cargo test
```

If you change dashboard assets, rebuild `agenthalo` before claiming the frontend changed.

## Pull Requests

Please include:

- the problem being solved
- the affected modules
- the commands you used to verify the change
- any intentionally deferred work

## Security

Treat Discord tokens, environment files, and encrypted local storage as sensitive. Never commit live credentials. See [SECURITY.md](SECURITY.md).
