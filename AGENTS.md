# AgentHALO — Agent Instructions

**Repo:** `Abraxas1010/agenthalo`  
**Scope:** Human-AI Agent Lifecycle Orchestrator with verifiable database, agent cockpit, metered proxy, native session orchestration, and P2PCLAW marketplace integration

Notes:
- `CLAUDE.md`, `CODEX.md`, and `GEMINI.md` are symlinks to this file.
- Keep edits additive and product-focused. This repo is the live AgentHALO product surface.

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

Build and maintain these active product surfaces unless the user explicitly narrows scope:

- Core NucleusDB: `src/protocol.rs`, `src/sql/`, `src/persistence.rs`, `src/blob_store.rs`, `src/vector_index.rs`
- Agent lifecycle: `src/halo/`, `src/cockpit/`, `src/orchestrator/`
- Identity and crypto: `src/genesis.rs`, `src/identity.rs`, `src/encrypted_file.rs`, `src/password.rs`, `src/vault.rs`, `src/did.rs`, `src/puf/`
- Networking and native sessions: `src/container/`, `src/comms/`, `src/swarm/`, `src/pod/`
- Verification and trust: `src/verifier/`, `src/sheaf/`, `src/trust/`
- Operator and customer surfaces: `src/mcp/`, `src/dashboard/`, `src/discord/`, `src/tui/`
- Integration surfaces: `contracts/`, `deploy/`, `scripts/agenthalo-instances.sh`, `lean/NucleusDB/`

## Build Targets

```bash
cargo build --release \
  --bin agenthalo \
  --bin agenthalo-mcp-server \
  --bin nucleusdb \
  --bin nucleusdb-server \
  --bin nucleusdb-mcp \
  --bin nucleusdb-tui \
  --bin nucleusdb-discord
```

## Verification

Minimum verification after code changes:

```bash
cargo check --bin agenthalo --bin agenthalo-mcp-server --bin nucleusdb --bin nucleusdb-mcp --bin nucleusdb-discord --bin nucleusdb-server --bin nucleusdb-tui
cargo test
```

If dashboard assets change, rebuild the binary before claiming the frontend changed.

### Formal Verification Workflow

When editing `lean/NucleusDB/` or Rust provenance surfaces:

```bash
./scripts/validate_formal_provenance.sh
./scripts/generate_proof_certificates.sh   # when theorem surfaces or gate requirements change
(cd lean && lake build NucleusDB)
cargo test --test formal_integration_tests
```

Use the Heyting repo only as the read-only canonical theorem source. Do not import Heyting Lean modules into this repo.

## Product Expectations

- Discord recording is append-only by default
- edits and deletes are logged as new records, never overwrites
- stdio and HTTP MCP transports both stay working
- dashboard keeps the CRT aesthetic across cockpit, setup, verification, and operator surfaces
- credentials stay in environment files or encrypted local storage, never hardcoded
- cockpit, mesh, wallet-routing, proxy telemetry, and native session orchestration are first-class product surfaces, not legacy exclusions

## Handoff

When you finish a non-trivial change, report:

- what changed
- what was verified
- what remains incomplete or intentionally deferred
