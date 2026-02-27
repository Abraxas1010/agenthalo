# Agent H.A.L.O. / NucleusDB — Architecture Reference

**Last updated:** 2026-02-26
**Test count:** 58 passing (29 dashboard + 29 unit/integration)

---

## 1. Project Identity

Agent H.A.L.O. (Human-Agent Lattice Orchestration) provides tamper-proof observability
for AI coding agents. It wraps any agent (Claude, Codex, Gemini, OpenClaw) and records
every event into a local, cryptographically sealed trace store backed by NucleusDB.

NucleusDB is the verifiable database underneath — a key-value store with vector
commitments (IPA/KZG), post-quantum signatures (ML-DSA-65), typed values, SQL interface,
vector search, and content-addressed blob storage.

## 2. Binary Targets

| Binary | Entry | Description |
|--------|-------|-------------|
| `nucleusdb` | `src/bin/nucleusdb.rs` | CLI REPL: SQL, key-value ops, vector search, export |
| `nucleusdb-server` | `src/bin/nucleusdb_server.rs` | Multi-tenant HTTP API (axum) |
| `nucleusdb-tui` | `src/bin/nucleusdb_tui.rs` | Terminal UI (ratatui) |
| `nucleusdb-mcp` | `src/bin/nucleusdb_mcp.rs` | MCP tool server for AI agent tool-calling |
| `agenthalo` | `src/bin/agenthalo.rs` | HALO CLI: `run`, `auth`, `dashboard`, `keygen`, `attest` |
| `agenthalo-mcp-server` | `src/bin/agenthalo_mcp_server.rs` | HALO-specific MCP server |

## 3. Core NucleusDB Modules

### 3.1 Protocol Layer

| Module | File | Purpose |
|--------|------|---------|
| `protocol` | `src/protocol.rs` | `NucleusDb` trait — get/set/delete/commit with vector commitment proofs |
| `state` | `src/state.rs` | In-memory key-value `State` + `Delta` change tracking |
| `persistence` | `src/persistence.rs` | Snapshot + WAL persistence (atomic writes, compaction) |
| `keymap` | `src/keymap.rs` | Deterministic key-to-index mapping for commitment schemes |
| `immutable` | `src/immutable.rs` | Immutable (append-only) key constraints |

### 3.2 Commitment & Verification

| Module | File | Purpose |
|--------|------|---------|
| `commitment/` | `src/commitment/` | IPA and KZG polynomial commitment backends |
| `vc/` | `src/vc/` | Vector commitment abstraction (`VcBackend` trait) |
| `witness` | `src/witness.rs` | Witness signatures: Ed25519 + ML-DSA-65 (post-quantum) |
| `transparency/` | `src/transparency/` | Certificate Transparency log integration |
| `security` | `src/security.rs` | Security parameter sets, reduction contracts, VC profiles |

### 3.3 Data Layer

| Module | File | Purpose |
|--------|------|---------|
| `typed_value` | `src/typed_value.rs` | 8-type system: Integer, Float, Bool, Text, JSON, Bytes, Vector, Null |
| `type_map` | `src/type_map.rs` | Per-key type tracking |
| `vector_index` | `src/vector_index.rs` | kNN vector search (cosine, L2, inner-product) |
| `blob_store` | `src/blob_store.rs` | Content-addressed blob storage (SHA-256 keyed) |
| `sql/` | `src/sql/` | Custom SQL dialect: parser + executor with typed values |

### 3.4 Access & Multi-tenancy

| Module | File | Purpose |
|--------|------|---------|
| `pod/` | `src/pod/` | Solid POD protocol, ACL grants, permissions |
| `multitenant` | `src/multitenant.rs` | Multi-tenant NucleusDB with role-based access |
| `license` | `src/license.rs` | CAB license certificate verification |

## 4. HALO Modules (`src/halo/`)

The HALO subsystem provides agent observability.

| Module | File | Purpose |
|--------|------|---------|
| `schema` | `schema.rs` | `TraceEvent`, `SessionMetadata`, `EventType` — the data model |
| `trace` | `trace.rs` | `TraceWriter` / `TraceReader` — redb-backed trace store |
| `wrap` | `wrap.rs` | Agent wrapper — intercepts stdin/stdout, logs events |
| `runner` | `runner.rs` | Process runner for wrapped agents |
| `detect` | `detect.rs` | Auto-detect agent type from command line |
| `auth` | `auth.rs` | Credentials management (`agenthalo auth`) |
| `attest` | `attest.rs` | Cryptographic session attestations |
| `vault` | `vault.rs` | AES-256-GCM encrypted API key vault (461 LOC) |
| `identity` | `identity.rs` | Identity category state (profile/device/network/social/super-secure) |
| `identity_ledger` | `identity_ledger.rs` | Append-only hash-chained social/super-secure ledger |
| `proxy` | `proxy.rs` | OpenAI-compatible multi-provider API proxy (359 LOC) |
| `config` | `config.rs` | Path helpers: `db_path()`, `vault_path()`, `pq_wallet_path()` |
| `pq` | `pq.rs` | Post-quantum wallet management (ML-DSA-65 keypairs) |
| `trust` | `trust.rs` | Trust score computation |
| `pricing` | `pricing.rs` | Token-based cost calculation per provider/model |
| `viewer` | `viewer.rs` | Session export (JSON format) |
| `x402` | `x402.rs` | HTTP 402 payment protocol integration |
| `onchain` | `onchain.rs` | On-chain configuration (Base L2) |
| `circuit` | `circuit.rs` | ZK circuit for trace verification |
| `addons` | `addons.rs` | Plugin/addon system |
| `agentpmt` | `agentpmt.rs` | Agent PMT (Product Market Testing) hooks |
| `adapters/` | `adapters/` | Provider-specific adapters |

### 4.1 Vault Design

- **File:** `~/.agenthalo/vault.enc`
- **Encryption:** AES-256-GCM per-key + whole-file
- **Master key:** HKDF-SHA256 from PQ wallet's `secret_seed_hex` (salt: `"agenthalo-vault-v1"`, info: `"aes-master"`)
- **Atomic writes:** temp file + rename
- **Key rotation:** detected via `key_id` mismatch between wallet and vault

### 4.2 Proxy Design

- **Routing:** model name prefix → provider (claude→Anthropic, gpt/o1/o3/o4→OpenAI, gemini→Google)
- **Transform:** request/response translated between OpenAI format and provider-native formats
- **Blocking I/O:** `ureq` calls in `spawn_blocking` (proxy is sync, dashboard is async)
- **Error sanitization:** `sanitize_upstream_error()` redacts `key=` from error strings
- **Streaming:** returns 501 (not yet implemented)

## 5. Cockpit Subsystem (`src/cockpit/`)

Browser-based agent orchestration — VS Code-like terminal panels.

| Module | File | LOC | Purpose |
|--------|------|-----|---------|
| `mod` | `mod.rs` | 9 | Module declarations |
| `pty_manager` | `pty_manager.rs` | 348 | PTY session lifecycle (portable-pty), max 10 concurrent |
| `session` | `session.rs` | 30 | `SessionStatus` enum + `SessionInfo` struct |
| `ws_bridge` | `ws_bridge.rs` | 156 | axum WebSocket ↔ PTY bidirectional bridge |
| `deploy` | `deploy.rs` | 307 | Agent catalog (5 agents), preflight checks, launch orchestration |

### 5.1 PTY Architecture

```
Browser (xterm.js)  ←WebSocket→  ws_bridge.rs  ←broadcast→  PtySession  ←PTY→  /bin/bash
                                                    ↑
                                          spawn_reader_thread()
                                          (one per session, reads PTY → broadcasts)
```

- One `std::thread` reader per PTY session (blocking `read()` loop)
- Output broadcast via `tokio::sync::broadcast` channel (1024 buffer)
- N WebSocket subscribers can attach/detach without affecting the reader thread
- Reconnect = new subscriber, NOT new reader thread

### 5.2 WebSocket Protocol

| Direction | Frame type | Content |
|-----------|-----------|---------|
| Client→Server | Binary | Raw terminal input (keystrokes, paste) |
| Client→Server | Text (JSON) | `{"type":"resize","cols":N,"rows":N}` or `{"type":"ping"}` |
| Client→Server | Text (non-JSON) | Fallback: treated as terminal input |
| Server→Client | Binary | Raw terminal output |
| Server→Client | Text (JSON) | `{"type":"status","state":"active\|done\|error","session_id":"..."}` |

### 5.3 Deploy Agent Catalog

| Agent ID | CLI | Required Keys | `--cwd` handling |
|----------|-----|---------------|-----------------|
| `claude` | `claude` | anthropic | CLI `--cwd` flag |
| `codex` | `codex` | openai | ignored |
| `gemini` | `gemini` | google | ignored |
| `openclaw` | `openclaw` | openai | ignored |
| `shell` | `/bin/bash` | none | PTY process cwd |

### 5.4 Cockpit Gap Analysis (vs Master Plan)

**Implemented:**
- Phases 0-6 core: vault, PTY bridge, cockpit UI, deploy, proxy, polish
- 7 layout presets, tab state machine, keyboard shortcuts, session persistence
- Auth gates on all mutating endpoints, command allowlist, shell `-c` block

**Not yet implemented:**
- Drag resize between panels (only preset layouts)
- Docker container isolation (agents run as local processes)
- HALO trace integration for cockpit sessions
- Proxy streaming (SSE responses)
- Proxy → HALO trace logging
- Live cost ticker (shows $0.00 placeholder)
- Log stream and Metrics panel types

**Master plan reference:** `WIP/cockpit_master_plan_2026-02-25.md`

## 6. Dashboard (`src/dashboard/` + `dashboard/`)

### 6.1 Server Side

| File | Purpose |
|------|---------|
| `mod.rs` | `DashboardState`, `build_state()`, `build_router()`, `serve()` |
| `api.rs` | All API route handlers (~2200 LOC, largest file in the project) |
| `assets.rs` | `rust-embed` static file handler |

`DashboardState` fields:
- `db_path` — redb trace store location
- `credentials_path` — auth credentials JSON
- `db_lock` — `Arc<Mutex<()>>` serializing redb access
- `grant_store` — Solid POD ACL grants
- `vault` — `Option<Arc<Vault>>` (initialized from PQ wallet if present)
- `pty_manager` — `Arc<PtyManager>` (max 10 sessions)

### 6.2 Frontend

| File | LOC | Description |
|------|-----|-------------|
| `index.html` | ~200 | Shell: sidebar nav, content div, script/link tags |
| `app.js` | ~850 | SPA router, all page renderers (Overview, Sessions, Costs, Config, Trust, NucleusDB, Cockpit, Deploy) |
| `style.css` | ~800 | Fallout terminal theme (green-on-black, CRT effects) |
| `cockpit.js` | 631 | `CockpitPanel` + `CockpitManager` classes (IIFE) |
| `cockpit.css` | 239 | Cockpit layout, tabs, panels, CRT terminal effects |
| `deploy.js` | 159 | Deploy page: agent cards, preflight, launch (IIFE) |
| `deploy.css` | 82 | Deploy grid and card styles |
| `chart.min.js` | - | Chart.js (vendored) |
| `vendor/` | - | xterm.js core + fit + webgl addons (vendored) |

**Key pattern:** Each page module (cockpit.js, deploy.js) is a self-contained IIFE that exposes
a single global (`window.CockpitPage`, `window.DeployPage`). The main `app.js` calls these
globals when routing to the corresponding page. HTML escaping is shared via `window.__escapeHtml`
(defined in app.js) with inline fallbacks in each IIFE.

### 6.3 Pages

| Route | Function | Description |
|-------|----------|-------------|
| `#/overview` | `renderOverview()` | Status, recent sessions, cost summary |
| `#/sessions` | `renderSessions()` | Session list with filters |
| `#/sessions/:id` | Detail view | Events, cost breakdown, attestation |
| `#/costs` | `renderCosts()` | Cost charts (daily, by-agent, by-model) |
| `#/config` | `renderConfig()` | Wrap config, x402, API keys (vault UI) |
| `#/trust` | `renderTrust()` | Attestations, trust scores |
| `#/nucleusdb` | `renderNucleusDB()` | Database browser, SQL console, vector search |
| `#/cockpit` | `renderCockpit()` | Terminal orchestration (delegates to cockpit.js) |
| `#/deploy` | `renderDeploy()` | Agent catalog and launch (delegates to deploy.js) |

## 7. API Endpoint Summary

### NucleusDB API (existing)

```
GET    /api/nucleusdb/status
GET    /api/nucleusdb/browse
GET    /api/nucleusdb/stats
POST   /api/nucleusdb/sql
GET    /api/nucleusdb/history
POST   /api/nucleusdb/edit
GET    /api/nucleusdb/verify/:key
GET    /api/nucleusdb/key-history/:key
GET    /api/nucleusdb/export
POST   /api/nucleusdb/vector-search
GET    /api/nucleusdb/grants
POST   /api/nucleusdb/grants
POST   /api/nucleusdb/grants/:id/revoke
```

### HALO API (existing)

```
GET    /api/status
GET    /api/sessions
GET    /api/sessions/:id
GET    /api/sessions/:id/events
GET    /api/sessions/:id/export
POST   /api/sessions/:id/attest
GET    /api/costs
GET    /api/costs/daily
GET    /api/costs/by-agent
GET    /api/costs/by-model
GET    /api/costs/paid
GET    /api/config
POST   /api/config/wrap
POST   /api/config/x402
GET    /api/trust/:session_id
GET    /api/attestations
POST   /api/attestations/verify
GET    /api/capabilities
GET    /api/x402/summary
GET    /api/x402/balance
GET    /events                    (SSE stream)
```

### Cockpit/Vault/Proxy API (new, 2026-02-25)

See tables in CLAUDE.md § Cockpit API endpoints and § Vault + Proxy endpoints.

## 8. Key Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP framework (dashboard + API) |
| `redb` | Embedded database (HALO trace store) |
| `ureq` | Sync HTTP client (proxy, key testing) |
| `portable-pty` | Cross-platform PTY management |
| `aes-gcm` | Vault encryption |
| `hkdf` | Master key derivation |
| `rust-embed` | Compile dashboard files into binary |
| `serde` / `serde_json` | Serialization |
| `tokio` | Async runtime |
| `uuid` | Session ID generation |
| `ratatui` | TUI framework |
| `webbrowser` | Auto-open dashboard URL |

## 9. File Paths & Configuration

| Path | Description |
|------|-------------|
| `~/.agenthalo/` | HALO data directory |
| `~/.agenthalo/pq_wallet.json` | ML-DSA-65 keypair (PQ wallet) |
| `~/.agenthalo/vault.enc` | Encrypted API key vault |
| `~/.agenthalo/identity.json` | Identity category state |
| `~/.agenthalo/identity_social_ledger.jsonl` | Immutable social/super-secure event ledger |
| `~/.agenthalo/halo.db` | redb trace store |
| `~/.agenthalo/credentials.json` | Auth credentials |

## 10. WIP Document Index

### Active (Cockpit — in progress)

| File | Description |
|------|-------------|
| `WIP/cockpit_master_plan_2026-02-25.md` | Full cockpit implementation plan (6 phases) |
| `WIP/cockpit_adversarial_qa_remediation_2026-02-26.md` | QA audit fixes for phases 0-6 |
| `WIP/cockpit_pm_implementation_report_2026-02-26.md` | PM's implementation report |
| `WIP/cockpit_preproject_plan_2026-02-25.md` | Pre-project analysis |
| `WIP/cockpit_preproject_report_2026-02-25.md` | Pre-project findings |
| `WIP/cockpit_phase{0-6}_partner_instructions_2026-02-25.md` | Phase-by-phase instructions (7 files) |

### Historical (completed work)

| Category | Files | Description |
|----------|-------|-------------|
| Trust & On-chain | `phase{1-6}_completion_report_2026-02-21.md` | 13-phase trust layer |
| Dashboard UX | `dashboard_*.md`, `nucleusdb_tab_redesign_*.md` | UI/UX audits and redesigns |
| NucleusPod | `nucleuspod_*.md` | Solid POD + provenance DAG |
| Bootstrap | `pre_project_bootstrap_*.md` | Initial project setup |
| Typed Values | `typed_layer_*.md` | 8-type value system |
| User Audits | `agentic_user_*.md`, `standard_user_*.md` | UX audit reports |
| SQL Safety | `p0_sql_safety_*.md` | SQL injection and streaming fixes |
| Resume | `resume_next_steps.md` | License gate removal notes |
