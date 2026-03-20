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
python3 scripts/check_theory_boundary.py
./scripts/validate_formal_provenance.sh
./scripts/generate_proof_certificates.sh   # when theorem surfaces or gate requirements change
(cd lean && lake build NucleusDB)
cargo test --test formal_integration_tests
```

Use the Heyting repo only as the read-only canonical theorem source. Do not import Heyting Lean modules into this repo.

## Key Subsystems for External Agents

Agents loading this repo (via worktree injection or MCP) should know:

### Persistent Library (`src/halo/library.rs`, `src/halo/library_mcp.rs`)
System-wide NucleusDB at `~/.agenthalo/library/`. Accumulates knowledge from all sessions.
- **Query tools** (read-only, available in MCP): `library_search`, `library_browse`, `library_session_lookup`, `library_sessions`, `library_status`
- **Push protocol**: auto-push on session end, 24h heartbeat for long-running sessions, manual push via dashboard
- **Key namespace**: `lib:session:`, `lib:summary:`, `lib:evt:`, `lib:idx:agent/date/model:`, `lib:push:watermark:`
- Disable auto-push: `AGENTHALO_LIBRARY_AUTO_PUSH=false`

### Worktree Isolation (`src/halo/workspace_profile.rs`, `src/container/worktree.rs`)
Per-agent git worktrees with host path injection.
- **Profiles** at `~/.agenthalo/workspace_profiles/<name>.json`
- **Three injection modes**: `readonly` (copy+chmod, OS enforced), `approved_write` (symlink+hook), `copy` (agent-owned)
- **Dashboard API**: `/api/worktree/profiles`, `/api/worktree/active-profile`, `/api/worktree/profile/{name}`, `/api/worktree/list`, `/api/worktree/session/{id}/skills|mcp-tools|instructions|verify`
- **Formal proof**: `lean/NucleusDB/Core/WorktreeIsolation.lean` (7 theorems, 0 sorry)

### MCP Tool Surface
- `agenthalo-mcp-server` exposes 125 tools including 5 Library tools
- Dashboard also exposes Library tools via the rmcp-based `NucleusDbMcpService`
- Library tools are always read-only; write attempts return helpful error explaining the push protocol

## Product Expectations

- Discord recording is append-only by default
- edits and deletes are logged as new records, never overwrites
- stdio and HTTP MCP transports both stay working
- dashboard keeps the CRT aesthetic across cockpit, setup, verification, and operator surfaces
- credentials stay in environment files or encrypted local storage, never hardcoded
- cockpit, mesh, wallet-routing, proxy telemetry, and native session orchestration are first-class product surfaces, not legacy exclusions
- Library auto-push must not block session completion — failures are logged, not fatal

## System Architecture Diagram (Pre-Push Gate)

The file `dashboard/agenthalo-system-diagram.html` contains 15 Mermaid diagrams documenting the full system architecture. A pre-push hook (`scripts/check_diagram_freshness.sh`) blocks pushes if the diagram hasn't been reviewed within 14 days.

**Before pushing**, if the diagram is stale or you've made structural changes:
1. Compare each diagram section against the current codebase
2. Update any Mermaid diagrams that no longer match
3. Update `DIAGRAM_REVIEW_DATE` in the HTML comment header to today's date
4. Update `DIAGRAM_REVIEWER` to your identity
5. Update the "Reviewed:" badge in the `<header>` element

Key sections to check: Binary Targets (vs Cargo.toml), Module Architecture (vs src/), Dashboard Frontend (vs dashboard/*.js), MCP Tool Surface (vs tool registrations), Complete File Map (vs file tree).

## Handoff

When you finish a non-trivial change, report:

- what changed
- what was verified
- what remains incomplete or intentionally deferred
