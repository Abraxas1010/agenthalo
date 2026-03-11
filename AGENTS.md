# NucleusDB — Agent Instructions

**Repo:** `Abraxas1010/nucleusdb`  
**Scope:** standalone verifiable database core, Discord recorder, MCP server, dashboard

Notes:
- `CLAUDE.md`, `CODEX.md`, and `GEMINI.md` are symlinks to this file.
- Keep edits additive and product-focused. This repo is not AgentHALO.

## Foundations

Read these at session start:

- `.agents/ONTOLOGY.md`
- `.agents/CALIBRATION.md`
- `.agents/CREDO.md`

Operating commitments:

1. Preparation before production
2. Verification before delivery
3. Depth over speed when the tradeoff matters
4. Immediate disclosure of context loss or unverified claims
5. Respect for other agents and shared workspaces

## Project Boundaries

Build and maintain only these surfaces unless the user explicitly expands scope:

- `src/protocol.rs`, `src/sql/`, `src/persistence.rs`, `src/blob_store.rs`, `src/vector_index.rs`
- `src/mcp/`
- `src/dashboard/`
- `src/discord/`
- `src/genesis.rs`, `src/identity.rs`, `src/encrypted_file.rs`, `src/password.rs`, `src/vault.rs`, `src/did.rs`
- `deploy/`, `Dockerfile`, `docker-compose.yml`
- `lean/NucleusDB/`

Do not reintroduce HALO orchestration, wallet-routing, mesh, cockpit, or agent-wrapper features into this repo.

## Build Targets

```bash
cargo build --release \
  --bin nucleusdb \
  --bin nucleusdb-server \
  --bin nucleusdb-mcp \
  --bin nucleusdb-tui \
  --bin nucleusdb-discord
```

## Verification

Minimum verification after code changes:

```bash
cargo check --bin nucleusdb --bin nucleusdb-mcp --bin nucleusdb-discord --bin nucleusdb-server --bin nucleusdb-tui
cargo test
```

If dashboard assets change, rebuild the binary before claiming the frontend changed.

## Product Expectations

- Discord recording is append-only by default
- edits and deletes are logged as new records, never overwrites
- stdio and HTTP MCP transports both stay working
- dashboard keeps the CRT aesthetic but only for NucleusDB/identity/security surfaces
- credentials stay in environment files or encrypted local storage, never hardcoded

## Handoff

When you finish a non-trivial change, report:

- what changed
- what was verified
- what remains incomplete or intentionally deferred
