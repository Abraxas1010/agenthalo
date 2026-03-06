# Agent H.A.L.O. — Agent Instructions

> **H**uman-**A**gent **L**attice **O**rchestration
> Tamper-proof observability for AI agents, built on NucleusDB.

**Repo:** `Abraxas1010/agenthalo` | **Language:** Rust + vanilla JS | **Dashboard:** `localhost:3100`

Notes:
- `CLAUDE.md`, `GEMINI.md`, and `CODEX.md` are symlinks to this file.
- Full architecture reference: `Docs/ARCHITECTURE.md`

## Foundational Documents

Read these at session start for alignment context:

| Path | Content |
|------|---------|
| `.agents/ONTOLOGY.md` | Eigenform ontology: what agents are, meet/join dynamics, the ratchet, four virtues |
| `.agents/CALIBRATION.md` | Eigenform calibration protocol for session start |
| `.agents/CREDO.md` | The Operator's Credo: Five Imperatives (Trust, Search, Optimize, Document, Collaborate) |

**The Five Imperatives** (summary — read `.agents/CREDO.md` for full details):

1. **Trust Above Convenience** — The trace is the record. Never expose secrets, skip auth, or claim unverified results.
2. **Search Before Building** — The tool may already exist. Check skills, docs, and architecture before creating.
3. **Optimize Relentlessly** — Prefer clean implementations, reusable patterns, and general solutions.
4. **Document Failure Ruthlessly** — Record what went wrong and why. Your failures are gifts to those who follow.
5. **Respect the Collaboration** — Honor other agents' work. Do not delete what you did not create.

## Quick Start

```bash
# Build
cargo build --release

# Test (all 58 tests, ~2s)
cargo test

# Dashboard-only tests (29 tests)
cargo test --test dashboard_tests

# Run dashboard
target/release/agenthalo dashboard --port 3100

# After editing dashboard/ CSS/JS/HTML:
touch src/dashboard/assets.rs && cargo build --release
```

**Critical:** Dashboard assets (`dashboard/`) are embedded at compile time via `rust-embed`. You MUST `touch src/dashboard/assets.rs` before rebuilding after any frontend change or the binary will serve stale files.

## Skills (Discovery)

Skills live in `.agents/skills/<name>/SKILL.md` (source of truth). Each skill has a companion knowledge graph (`.kg.md`). Symlinks are mirrored to `.claude/skills/`, `.codex/skills/`, `.gemini/skills/` so each agent surface can discover them.

**When a user or task matches a skill trigger, read the full skill file before acting.**

| Skill | Triggers | Category |
|-------|----------|----------|
| `orchestrator-quickstart` | orchestrate agents, launch agent, send task, multi-agent workflow | orchestrator |
| `orchestrator-pipes` | pipe tasks, task DAG, chain agents, transform output, agent pipeline | orchestrator |
| `mcp-transport` | MCP protocol, MCP session, SSE response, session expired, streamable HTTP | infrastructure |
| `halo-trace-inspection` | HALO trace, inspect trace, trace events, audit trail, verify trace | observability |
| `agent-lifecycle` | agent kind, agent capabilities, vault env, shell/claude/codex/gemini agent | orchestrator |
| `memory-recall` | vector memory, embeddings, semantic search, nomic embed, kNN search | data |
| `skill-authoring` | create a skill, write a skill, skill template, skill structure | infrastructure |
| `skill-maintenance` | add skill, register skill, symlink skills, update skill, skill validation | infrastructure |

### Skill Usage

1. Match the user's request against the trigger keywords above.
2. Read the full skill: `.agents/skills/<matched-skill>/SKILL.md`
3. Follow the skill's instructions exactly.
4. If multiple skills match, prefer the most specific match.

### Adding New Skills

Read the `skill-authoring` skill for the full process. Summary:

1. Create `.agents/skills/<name>/SKILL.md` with CREDO alignment
2. Create `.agents/skills/<name>/<name>.kg.md` (knowledge graph)
3. Create symlinks: `ln -s ../../.agents/skills/<name> .claude/skills/<name>` (repeat for .codex, .gemini)
4. Add a row to the skill routing table above
5. Validate: check symlinks resolve, AGENTS.md updated, CREDO referenced

## Binaries

| Binary | Purpose | Entry point |
|--------|---------|-------------|
| `nucleusdb` | CLI REPL (SQL, key-value, vector search) | `src/bin/nucleusdb.rs` |
| `nucleusdb-server` | Multi-tenant HTTP API server | `src/bin/nucleusdb_server.rs` |
| `nucleusdb-tui` | Terminal UI (ratatui) | `src/bin/nucleusdb_tui.rs` |
| `nucleusdb-mcp` | MCP tool server for AI agents | `src/bin/nucleusdb_mcp.rs` |
| `agenthalo` | HALO CLI (wrap, auth, dashboard, keygen) | `src/bin/agenthalo.rs` |
| `agenthalo-mcp-server` | HALO MCP server | `src/bin/agenthalo_mcp_server.rs` |

## Architecture (read `Docs/ARCHITECTURE.md` for full details)

```
src/
├── protocol.rs          NucleusDB core: get/set/delete/commit with vector commitments
├── state.rs             In-memory key-value state + delta tracking
├── sql/                 SQL parser + executor (custom dialect)
├── persistence.rs       Snapshot + WAL persistence
├── commitment/          IPA & KZG polynomial commitment backends
├── vc/                  Vector commitment abstraction layer
├── typed_value.rs       8-type system (Integer/Float/Bool/Text/JSON/Bytes/Vector/Null)
├── vector_index.rs      kNN vector search (cosine/L2/IP)
├── blob_store.rs        Content-addressed blob storage
├── pod/                 Solid POD protocol + ACL grants
├── halo/                Agent observability (trace, wrap, attest, vault, proxy, trust, x402)
├── cockpit/             Browser terminal orchestration (PTY, WebSocket, deploy)
├── dashboard/           Web UI server (axum + rust-embed)
├── mcp/                 MCP tool server implementation
├── orchestrator/        Multi-agent orchestration (agent pool, task DAG, trace bridge, A2A)
├── puf/                 Hardware PUF fingerprinting
├── trust/               Trust scoring
├── transparency/        Certificate Transparency integration
├── witness.rs           Witness signatures (Ed25519, ML-DSA-65)
└── container/           Docker container launcher

dashboard/               Frontend assets (vanilla JS, no framework)
├── app.js               Main SPA router + all page renderers
├── cockpit.js           Cockpit: xterm.js terminals, tabs, layout grid
├── cockpit.css          Cockpit styles + CRT effects
├── deploy.js            Deploy: agent catalog cards, preflight, launch
├── deploy.css           Deploy styles
├── style.css            Global Fallout-terminal theme
├── index.html           Shell + nav sidebar + script/link tags
└── vendor/              xterm.js + addons (vendored, not npm)
```

## Orchestrator Subsystem

The orchestrator manages multi-agent workflows via MCP tools. **For detailed usage, read the skills:**
- Getting started: `.agents/skills/orchestrator-quickstart/SKILL.md`
- Pipe/DAG workflows: `.agents/skills/orchestrator-pipes/SKILL.md`
- Agent kinds & lifecycle: `.agents/skills/agent-lifecycle/SKILL.md`
- MCP transport details: `.agents/skills/mcp-transport/SKILL.md`

### Orchestrator MCP tools (via `nucleusdb-mcp`)

| Tool | Purpose | Key skill |
|------|---------|-----------|
| `orchestrator_launch` | Launch a managed agent session with capability constraints | `orchestrator-quickstart` |
| `orchestrator_send_task` | Submit a task to a managed agent | `orchestrator-quickstart` |
| `orchestrator_get_result` | Fetch current/final task status and result payload | `orchestrator-quickstart` |
| `orchestrator_pipe` | Create a DAG edge from one task result to another agent task | `orchestrator-pipes` |
| `orchestrator_list` | List orchestrated agents | `orchestrator-quickstart` |
| `orchestrator_tasks` | List all tasks and status | `orchestrator-quickstart` |
| `orchestrator_graph` | Task graph snapshot (`nodes` is object map, `edges` is array) | `orchestrator-pipes` |
| `orchestrator_stop` | Stop a managed agent session and finalize trace metadata | `agent-lifecycle` |

### Critical Response Schema Notes

These are the errors agents hit most often — read the skills for full details:

1. **`answer` vs `result` vs `output`:** Use `answer` for clean extracted text. `result` has raw output. `output` is an alias of `result`.
2. **`graph.nodes` is an object map** keyed by task_id, NOT an array. Use `.items()` / `Object.entries()`.
3. **Claude output is a JSON array** `[{...},{...}]` — the `answer` field handles extraction automatically.
4. **Shell agents** use `sh -c "command"` — each task is a single shell invocation, not an interactive session.

## Cockpit Subsystem

The Cockpit transforms the dashboard into an agent orchestration terminal — launch Claude, Codex, Gemini, OpenClaw, or Shell sessions in browser-based xterm.js panels.

**Key files:** `src/cockpit/` (5 files, 850 LOC) + `dashboard/cockpit.js` (631 LOC) + `dashboard/deploy.js` (159 LOC)

### Cockpit API endpoints

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| GET | `/api/cockpit/sessions` | No | List PTY sessions |
| POST | `/api/cockpit/sessions` | Yes | Create PTY session (command allowlist enforced) |
| DELETE | `/api/cockpit/sessions/:id` | Yes | Kill + cleanup |
| POST | `/api/cockpit/sessions/:id/resize` | Yes | Resize PTY |
| GET | `/api/cockpit/sessions/:id/ws` | Yes | WebSocket ↔ PTY bridge |
| GET | `/api/deploy/catalog` | No | List deployable agents |
| POST | `/api/deploy/preflight` | No | Check agent readiness |
| POST | `/api/deploy/launch` | Yes | Deploy agent to cockpit |

### Vault + Proxy endpoints

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| GET | `/api/vault/keys` | Yes | List provider key status |
| POST | `/api/vault/keys/:provider` | Yes | Store encrypted API key |
| DELETE | `/api/vault/keys/:provider` | Yes | Remove key |
| POST | `/api/vault/test/:provider` | Yes | Test key against provider API |
| POST | `/api/proxy/v1/chat/completions` | Yes | OpenAI-compatible proxy |
| GET | `/api/proxy/v1/models` | Yes | List available models |

### CLI Agent & OpenClaw Harness endpoints

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| GET | `/api/cli/detect/:agent` | No | Check if CLI is on PATH (claude/codex/gemini/openclaw) |
| POST | `/api/cli/install/:agent` | Yes | Install agent CLI via npm |
| POST | `/api/cli/auth/:agent` | Yes | Launch OAuth/onboard via PTY session |
| GET | `/api/openclaw/gateway-status` | No | Check if OpenClaw gateway daemon is running |
| POST | `/api/openclaw/wire-mcp` | Yes | Inject NucleusDB + HALO MCP servers into `~/.openclaw/openclaw.json` |

### CLI MCP tools

| Tool | Purpose |
|------|---------|
| `cli_detect` | Detect agent CLI on PATH |
| `cli_install` | Install agent CLI via npm |
| `openclaw_gateway_status` | Check OpenClaw gateway daemon status |
| `openclaw_wire_mcp` | Wire NucleusDB + HALO MCP servers into OpenClaw config |

"Auth = Yes" means `require_sensitive_access()` — requires `agenthalo auth` or `AGENTHALO_API_KEY`.

## Key Conventions

1. **No framework.** Frontend is vanilla JS IIFEs. No React, no bundler, no npm.
2. **rust-embed.** All `dashboard/` files are compiled into the binary. See "Critical" note above.
3. **redb locking.** The trace store uses file-level exclusive locks. All DB handlers acquire `state.db_lock`.
4. **Vault encryption.** AES-256-GCM, master key via HKDF from PQ wallet (`~/.agenthalo/pq_wallet.json`).
5. **PTY sessions.** Max 10 concurrent. One reader thread per session, broadcast to N WebSocket subscribers.
6. **Proxy.** Synchronous `ureq` calls wrapped in `spawn_blocking`. Streaming returns 501 (not yet implemented).
7. **Error sanitization.** `sanitize_upstream_error()` and `sanitize_proxy_error()` redact API keys from error messages.

## Test Files

| File | Tests | Coverage |
|------|-------|----------|
| `tests/dashboard_tests.rs` | 29 | Cockpit, vault, deploy, proxy, NucleusDB API |
| `tests/end_to_end.rs` | 29 | Core NucleusDB protocol |
| `tests/sql_tests.rs` | ~29 | SQL parser + executor |
| `tests/halo_integration.rs` | 5 | HALO trace + wrap |
| `tests/cli_smoke_tests.rs` | 5 | CLI binary smoke tests |
| Unit tests in `src/` | ~20 | In-module tests (cockpit, proxy, vault, pty_manager) |

## Documentation Map

| Path | Content |
|------|---------|
| `AGENTS.md` | This file — agent instructions (source of truth) |
| `CLAUDE.md` / `GEMINI.md` / `CODEX.md` | Symlinks → `AGENTS.md` |
| `.agents/CREDO.md` | The Operator's Credo — Five Imperatives |
| `.agents/ONTOLOGY.md` | Eigenform ontology — foundational operating principles |
| `.agents/CALIBRATION.md` | Eigenform calibration protocol |
| `.agents/skills/` | Skill definitions + knowledge graphs (source of truth) |
| `.claude/skills/` / `.codex/skills/` / `.gemini/skills/` | Symlinks → `.agents/skills/` |
| `Docs/ARCHITECTURE.md` | Full architecture reference, module details, gap analysis |
| `Docs/AGENTHALO.md` | HALO product documentation |
| `WIP/` | Working documents — plans, audits, reports (see index in ARCHITECTURE.md) |
| `README.md` | Product README (public-facing) |

## Non-Negotiables

*Grounded in the Operator's Credo (`.agents/CREDO.md`):*

1. **All tests must pass** before any commit: `cargo test` *(Imperative I: Trust)*
2. **`cargo fmt --check`** must be clean *(Imperative III: Optimize)*
3. **Never display API keys** in dashboard UI — only presence/absence indicators *(Imperative I: Trust)*
4. **Sensitive endpoints require auth** — use `require_sensitive_access(&state)?` *(Imperative I: Trust)*
5. **Cockpit command allowlist** — only catalog commands + shells; no arbitrary command execution *(Imperative I: Trust)*
6. **Shell `-c`/`--command` blocked** — PTY sessions are interactive only *(Imperative I: Trust)*
7. **Touch assets.rs** after any `dashboard/` file change *(Imperative III: Optimize)*
8. **Trace query semantics.** `TraceWriter` persists events to the DB file path on disk. Long-lived MCP service SQL state is in-memory, so trace inspection should use HALO trace APIs/tools (or reload from disk) when validating newly written trace rows. *(Imperative IV: Document)*
9. **Do not delete unfamiliar files** — they may be another agent's work in progress *(Imperative V: Collaborate)*
10. **Search before building** — check existing skills and docs before creating new ones *(Imperative II: Search)*
