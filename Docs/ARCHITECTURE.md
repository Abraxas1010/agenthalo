# Agent H.A.L.O. / NucleusDB — Architecture Reference

**Last updated:** 2026-03-04
**Test count:** 731+ passing

---

## 1. Project Identity

Agent H.A.L.O. (Human-Agent Lattice Orchestration) is a sovereign agent platform
providing post-quantum cryptographic identity, DIDComm-based communication, and
tamper-proof observability for AI agents. It wraps any agent CLI (Claude, Codex,
Gemini, OpenClaw) and records every event into a local, cryptographically sealed
trace store backed by NucleusDB. All agent-controlled cryptographic surfaces are
PQ-hardened: hybrid KEM (X25519 + ML-KEM-768) for DIDComm, dual signatures
(Ed25519 + ML-DSA-65) for identity, SHA-512 for integrity, and PQ-gated EVM signing.

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

The HALO subsystem provides sovereign agent identity, PQ-hardened communication, and observability.

### 4.0 Identity & Post-Quantum Cryptography

| Module | File | Purpose |
|--------|------|---------|
| `did` | `did.rs` | DID derivation from genesis seed, Ed25519 + ML-DSA-65 dual sign/verify, `DIDDocument` |
| `genesis_seed` | `genesis_seed.rs` | Genesis seed ceremony, BIP-39 mnemonic derivation, entropy mixing |
| `genesis_entropy` | `genesis_entropy.rs` | Entropy source management for genesis ceremonies |
| `identity` | `identity.rs` | Identity category state (device/network/social/super-secure) |
| `identity_ledger` | `identity_ledger.rs` | Append-only hash-chained identity ledger (SHA-512 for new entries) |
| `pq` | `pq.rs` | ML-DSA-65 PQ wallet management (keygen, signing, envelopes) |
| `hash` | `hash.rs` | `HashAlgorithm` dispatch: SHA-256 (legacy) / SHA-512 (current), `hash_bytes()`/`hash_hex()` |
| `hybrid_kem` | `hybrid_kem.rs` | X25519 + ML-KEM-768 hybrid KEM (IETF Composite, HKDF-SHA-512, salt v2) |
| `didcomm` | `didcomm.rs` | DIDComm v2 authcrypt/anoncrypt with hybrid KEM paths |
| `didcomm_handler` | `didcomm_handler.rs` | Inbound DIDComm message handling, hybrid KEM detection |
| `evm_wallet` | `evm_wallet.rs` | BIP-32 secp256k1 wallet derivation; `sign_with_evm_key` is `pub(crate)` (gate-enforced) |
| `evm_gate` | `evm_gate.rs` | PQ-gated EVM signing: dual Ed25519 + ML-DSA-65 authorization before secp256k1 |
| `twine_anchor` | `twine_anchor.rs` | CURBy-Q Twine identity attestation, triple-signed binding proofs |

### 4.0a P2P Mesh & Communication

| Module | File | Purpose |
|--------|------|---------|
| `p2p_node` | `p2p_node.rs` | libp2p swarm: Noise XX transport, gossipsub, Kademlia DHT, relay, AutoNAT |
| `p2p_discovery` | `p2p_discovery.rs` | Agent discovery, `GossipPrivacy` metadata minimization, DHT address publish |
| `a2a_bridge` | `a2a_bridge.rs` | HTTP bridge for agent-to-agent DIDComm (hybrid KEM) |
| `startup` | `startup.rs` | Full stack orchestration: P2P + Nym + DIDComm bootstrap |
| `nym` | `nym.rs` | Nym SOCKS5 proxy integration |
| `nym_native` | `nym_native.rs` | Native Sphinx packet construction, SURB replies, cover traffic |

### 4.0b Observability & Adapters

| Module | File | Purpose |
|--------|------|---------|
| `schema` | `schema.rs` | `TraceEvent`, `SessionMetadata`, `EventType` — the data model |
| `trace` | `trace.rs` | `TraceWriter` / `TraceReader` — redb-backed trace store |
| `wrap` | `wrap.rs` | Agent wrapper — intercepts stdin/stdout, logs events |
| `runner` | `runner.rs` | Process runner for wrapped agents |
| `detect` | `detect.rs` | Auto-detect agent type from command line |
| `viewer` | `viewer.rs` | Session export (JSON format) |
| `adapters/` | `adapters/` | Provider-specific adapters (Claude, Codex, Gemini, Generic) |

### 4.0c Trust, Attestation & ZK

| Module | File | Purpose |
|--------|------|---------|
| `attest` | `attest.rs` | Session attestation (Merkle root SHA-512, anonymous membership proofs) |
| `trust` | `trust.rs` | Trust score computation (SHA-512 digest) |
| `circuit` | `circuit.rs` | Groth16 proving/verifying (BN254, arkworks) |
| `circuit_policy` | `circuit_policy.rs` | Dev vs production circuit key policy |
| `public_input_schema` | `public_input_schema.rs` | Groth16 public input layout versioning |
| `audit` | `audit.rs` | Solidity static analysis engine |
| `zk_compute` | `zk_compute.rs` | ZK compute receipts |
| `zk_credential` | `zk_credential.rs` | ZK credential proofs and anonymous membership |

### 4.0d Auth, Config & Integrations

| Module | File | Purpose |
|--------|------|---------|
| `auth` | `auth.rs` | Credentials management (`agenthalo auth`) |
| `vault` | `vault.rs` | AES-256-GCM encrypted API key vault |
| `config` | `config.rs` | Path helpers: `db_path()`, `vault_path()`, `pq_wallet_path()` |
| `crypto_scope` | `crypto_scope.rs` | Scoped cryptographic key management |
| `proxy` | `proxy.rs` | OpenAI-compatible multi-provider API proxy |
| `pricing` | `pricing.rs` | Token-based cost calculation per provider/model |
| `x402` | `x402.rs` | HTTP 402 payment protocol integration |
| `onchain` | `onchain.rs` | On-chain configuration (Base L2) |
| `addons` | `addons.rs` | Plugin/addon system |
| `agentpmt` | `agentpmt.rs` | Agent PMT (Product Market Testing) hooks |

### 4.0e Mesh DIDComm (`src/comms/`)

| Module | File | Purpose |
|--------|------|---------|
| `didcomm` | `comms/didcomm.rs` | DIDComm v2 mesh envelope: hybrid KEM encrypt/decrypt (X25519 + ML-KEM-768) |
| `envelope` | `comms/envelope.rs` | Envelope serialization |
| `session` | `comms/session.rs` | Communication session state |

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

### 5.5 Orchestrator Subsystem (`src/orchestrator/`)

The orchestrator is an in-process multi-agent coordinator built on top of
`PtyManager` and HALO tracing. It provides explicit agent lifecycle, task
execution, piped task DAGs, and mesh delegation helpers.

| Module | File | Purpose |
|--------|------|---------|
| `orchestrator` | `orchestrator/mod.rs` | Public orchestration API and shared state (`launch`, `task`, `pipe`, `stop`, list/snapshot) |
| `agent_pool` | `orchestrator/agent_pool.rs` | Managed agent sessions with allowlisted CLIs + capability checks |
| `task` | `orchestrator/task.rs` | Task model and status transitions (`pending/running/complete/failed/timeout`) |
| `task_graph` | `orchestrator/task_graph.rs` | DAG edges, transform parsing/apply, cycle rejection |
| `trace_bridge` | `orchestrator/trace_bridge.rs` | PTY output stream to HALO trace events + telemetry/cost accumulation |
| `a2a` | `orchestrator/a2a.rs` | Remote mesh delegation wrapper for orchestrator tasks |

Orchestrator MCP tools (`src/mcp/tools.rs`):
- `orchestrator_launch`
- `orchestrator_send_task`
- `orchestrator_get_result`
- `orchestrator_pipe`
- `orchestrator_list`
- `orchestrator_tasks`
- `orchestrator_graph`
- `orchestrator_stop`

Dashboard routes (`src/dashboard/api.rs`):
- `GET /api/orchestrator/agents`
- `GET /api/orchestrator/tasks`
- `GET /api/orchestrator/graph`
- `POST /api/orchestrator/launch`
- `POST /api/orchestrator/task`
- `POST /api/orchestrator/pipe`
- `POST /api/orchestrator/stop`
- `GET /api/orchestrator/agents/{id}/ws`

Proxy-mode env vars (shared aliases; both dashboard and NucleusDB MCP honor both):
- `AGENTHALO_ORCHESTRATOR_PROXY_VIA_MCP`
- `NUCLEUSDB_ORCHESTRATOR_PROXY_VIA_AGENTHALO`
- `AGENTHALO_ORCHESTRATOR_MCP_ENDPOINT`
- `NUCLEUSDB_ORCHESTRATOR_PROXY_ENDPOINT`

When proxy mode is enabled, orchestrator websocket output is provided via MCP task-status polling
rather than direct PTY stream subscription.

### 5.5.1 Trace Persistence and Visibility Semantics

Orchestrator trace writes are append-only and durable, but there are two distinct views:

1. **Runtime in-memory DB view** (service-local):
   - The running service keeps an in-memory `NucleusDb` instance for MCP/SQL reads.
   - This view is not automatically refreshed from disk after out-of-band writes.
2. **Persisted trace store view** (`traces.ndb` / configured trace path):
   - `TraceWriter` commits events and persists snapshot/WAL to disk.
   - Trace integrity and post-run audits should read from this persisted store.

Operational implication:
- A task can expose `trace_session_id` immediately after completion while SQL queries
  against a long-lived in-memory DB handle do not yet reflect those new rows.
- For authoritative trace verification, query the persisted trace DB (or reload state)
  instead of assuming live in-memory SQL visibility.

Reference:
- `Docs/ops/orchestrator_debugging_playbook.md`

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

### CLI Agent & OpenClaw Harness API (new, 2026-03-04)

```
GET    /api/cli/detect/{agent}          # Check if CLI is on PATH (claude/codex/gemini/openclaw)
POST   /api/cli/install/{agent}         # npm install -g <package>
POST   /api/cli/auth/{agent}            # Launch OAuth/onboard via PTY session
GET    /api/openclaw/gateway-status     # Check if OpenClaw gateway daemon is running
POST   /api/openclaw/wire-mcp           # Inject NucleusDB + HALO MCP servers into ~/.openclaw/openclaw.json
```

## 8. Key Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` | HTTP framework (dashboard + API) |
| `redb` | Embedded database (HALO trace store) |
| `ureq` | Sync HTTP client (proxy, key testing) |
| `portable-pty` | Cross-platform PTY management |
| `aes-gcm` | Vault encryption + DIDComm AEAD |
| `hkdf` | Key derivation (HKDF-SHA-512 for hybrid KEM, HKDF-SHA-256 for vault) |
| `sha2` | SHA-256 (legacy) + SHA-512 (current) hash dispatch |
| `ml-kem` | ML-KEM-768 (FIPS 203) post-quantum key encapsulation |
| `ml-dsa` | ML-DSA-65 (FIPS 204) post-quantum digital signatures |
| `ed25519-dalek` | Ed25519 classical signatures |
| `x25519-dalek` | X25519 ECDH key exchange |
| `k256` | secp256k1 ECDSA (EVM wallet) |
| `bip32` / `bip39` | Hierarchical deterministic wallet derivation |
| `libp2p` | P2P mesh: gossipsub, Kademlia DHT, Noise XX, relay, AutoNAT |
| `rust-embed` | Compile dashboard files into binary |
| `serde` / `serde_json` | Serialization |
| `tokio` | Async runtime |
| `uuid` | Session ID generation |
| `ratatui` | TUI framework |
| `webbrowser` | Auto-open dashboard URL |
| `zeroize` | Secure memory wiping for key material |

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

### PQ Hardening (completed, 2026-03-04)

| File | Description |
|------|-------------|
| `WIP/pq_hardening_preproject_plan_2026-03-03.md` | Pre-project analysis |
| `WIP/pq_hardening_closure_report_2026-03-04.md` | Closure report (8/14 pass, 6 Lean deferred) |
| `WIP/pq_defense_in_depth_partner_instructions_2026-03-04.md` | Defense-in-depth implementation plan |
| `Docs/ops/pq_mesh_hardening.md` | P2P mesh audit (no DIDComm bypass) |
| `Docs/ops/pq_nym_assessment.md` | Nym mixnet PQ assessment |
| `Docs/ops/pq_evm_assessment.md` | EVM wallet PQ assessment |

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
