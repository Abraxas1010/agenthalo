# Agent H.A.L.O. — Agent Instructions

> **H**uman-**A**gent **L**attice **O**rchestration
> Tamper-proof observability for AI agents, built on NucleusDB.

**Repo:** `Abraxas1010/agenthalo` | **Language:** Rust + vanilla JS | **Dashboard:** `localhost:3100`

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

## Cockpit Subsystem (Active Development)

The Cockpit transforms the dashboard into an agent orchestration terminal — launch Claude, Codex, Gemini, OpenClaw, or Shell sessions in browser-based xterm.js panels.

**Key files:** `src/cockpit/` (5 files, 850 LOC) + `dashboard/cockpit.js` (631 LOC) + `dashboard/deploy.js` (159 LOC)

**Master plan:** `WIP/cockpit_master_plan_2026-02-25.md`
**QA remediation:** `WIP/cockpit_adversarial_qa_remediation_2026-02-26.md`
**Gap analysis:** See `Docs/ARCHITECTURE.md` § Cockpit Gap Analysis

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

### CLI commands (`agenthalo harness`)

| Subcommand | Purpose |
|------------|---------|
| `detect` | Detect all 4 agent CLIs on PATH |
| `install <agent>` | Install via npm (claude/codex/gemini/openclaw) |
| `wire-mcp` | Wire NucleusDB + HALO MCP into OpenClaw config |
| `gateway-status` | Check OpenClaw gateway daemon |

### MCP tools (via `nucleusdb-mcp`)

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
| `Docs/ARCHITECTURE.md` | Full architecture reference, module details, gap analysis |
| `Docs/AGENTHALO.md` | HALO product documentation |
| `WIP/cockpit_master_plan_2026-02-25.md` | Cockpit implementation plan (active) |
| `WIP/` | Working documents — plans, audits, reports (see index in ARCHITECTURE.md) |
| `README.md` | Product README (public-facing) |

## Non-Negotiables

1. **All tests must pass** before any commit: `cargo test`
2. **`cargo fmt --check`** must be clean
3. **Never display API keys** in dashboard UI — only presence/absence indicators
4. **Sensitive endpoints require auth** — use `require_sensitive_access(&state)?`
5. **Cockpit command allowlist** — only catalog commands + shells; no arbitrary command execution
6. **Shell `-c`/`--command` blocked** — PTY sessions are interactive only
7. **Touch assets.rs** after any `dashboard/` file change
