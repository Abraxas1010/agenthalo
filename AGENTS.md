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

### Persistent Library (`src/halo/library.rs`, `src/halo/library_embeddings.rs`)
System-wide NucleusDB at `~/.agenthalo/library/`. Accumulates knowledge from all sessions.
- **Semantic search** (recommended): `library_semantic_search` — vector-based cross-session recall using embedded session summaries. Understands meaning, not just keywords.
- **Keyword search**: `library_search` — full-text term matching across Library records.
- **Other query tools** (read-only): `library_browse`, `library_session_lookup`, `library_sessions`, `library_status`
- **Push protocol**: auto-push on session end (also embeds summary into semantic sidecar), 24h heartbeat for long-running sessions
- **Embedding sidecar**: `library_embeddings.ndb` stores vector embeddings of session summaries. Regenerable via backfill.
- **Key namespace**: `lib:session:`, `lib:summary:`, `lib:evt:`, `lib:idx:agent/date/model:`, `lib:push:watermark:`
- Disable auto-push: `AGENTHALO_LIBRARY_AUTO_PUSH=false`

### Worktree Isolation (`src/halo/workspace_profile.rs`, `src/container/worktree.rs`)
Per-agent git worktrees with host path injection.
- **Profiles** at `~/.agenthalo/workspace_profiles/<name>.json`
- **Three injection modes**: `readonly` (copy+chmod, OS enforced), `approved_write` (symlink+hook), `copy` (agent-owned)
- **Dashboard API**: `/api/worktree/profiles`, `/api/worktree/active-profile`, `/api/worktree/profile/{name}`, `/api/worktree/list`, `/api/worktree/session/{id}/skills|mcp-tools|instructions|verify`
- **Formal proof**: `lean/NucleusDB/Core/WorktreeIsolation.lean` (7 theorems, 0 sorry)

### Semantic Memory (`src/memory.rs`, `src/embeddings.rs`, `src/vector_index.rs`)
Agents have persistent semantic memory via three MCP tools:
- **`agenthalo_memory_store`** — store text with auto-embedding (nomic-embed-text-v1.5, 768-dim). Accepts `session_id`, `agent_id`, `ttl_secs` for context enrichment and expiry.
- **`agenthalo_memory_recall`** — retrieve memories by natural-language query. Pipeline: HyDE query expansion → cosine search → fused reranking (base similarity + bi-encoder + lexical + negation).
- **`agenthalo_memory_ingest`** — chunk a document by headings and store each chunk as a memory fragment.

**When to use memory tools:**
- Store important findings, decisions, or context that should persist across sessions
- Recall previous decisions or context before starting related work
- Ingest documents (architecture docs, meeting notes, specs) for later retrieval
- Key namespace: `mem:chunk:*` (auto-generated from text hash)

### MCP Tool Surface
- `agenthalo-mcp-server` exposes 128+ tools including 3 memory tools, 5 Library tools
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

## HeytingLean Observatory

The cockpit includes the HeytingLean Observatory — a collapsible right-side drawer
with buttons that spawn independent, draggable, floating visualization windows for
codebase health (treemap, dependency graph, complexity, clusters, sorrys, frontier).

**Backend**: `lean-xray/` scans the Lean project in <1s (3,700+ files). API at `/api/observatory/*`.

**Persistent LSP (lean-lsp-mcp)**: When available, the Observatory enriches per-file
data with ground truth from the Lean LSP:

- Checking if code compiles: prefer `lean_diagnostics` over `lake build`
- Getting goal states: use `lean_goal` with file_path and line
- Testing tactics: use `lean_multi_attempt` to try several at once
- Finding theorems: use `lean_leansearch` or `lean_loogle`
- Getting type info: use `lean_hover` at any position

Only use `lake build` when you need to rebuild the entire project
(e.g., after modifying lakefile.lean or adding a new dependency).

**Sending visualizations to the cockpit Observatory panel:**

When running inside a cockpit agent panel, you can push rich visualizations to the
user's Observatory drawer by including fenced blocks in your output:

````
```observatory:goals
{"goals": [{"hyps": [{"name": "h", "type": "P"}], "target": "P → Q"}]}
```
````

Available viz types and their expected JSON shapes:

| Type | Description | JSON shape |
|------|-------------|------------|
| `goals` | Proof goal state with KaTeX math | `{"goals": [{"hyps": [{"name":"","type":""}], "target": ""}]}` |
| `prooftree` | Tactic trace / proof steps | `{"steps": [{"tactic":"","goal_before":"","goal_after":"","status":"success"}]}` |
| `depgraph` | D3 force-directed dependency graph | `{"nodes": [{"id":"","group":0}], "edges": [["from","to"]]}` |
| `treemap` | D3 squarified file health treemap | `{"files": [{"path":"","lines":0,"health_score":1,"health_status":"clean","sorry_count":0}]}` |
| `tactics` | Tactic suggestions with confidence | `{"tactics": [{"tactic":"","confidence":0.9,"source":"","description":""}]}` |
| `latex` | KaTeX math equations | `{"blocks": [{"label":"","latex":"","display":true}]}` |
| `flowchart` | Mermaid diagram | `{"mermaid": "graph TD\\n A-->B"}` |
| `table` | Sortable data table | `{"columns": ["A","B"], "rows": [["x","y"]]}` |

The Observatory drawer auto-unfurls when data arrives. The user clicks the lit button
to open a floating window. Multiple windows can be open simultaneously.

For sorry elimination, check `lean_observatory_frontier` first — it returns sorrys
whose dependencies are all proved, with actual goal states from the LSP.

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
