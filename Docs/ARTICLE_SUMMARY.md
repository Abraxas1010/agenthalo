# Agent H.A.L.O. and NucleusDB — Project Summary for Press

*Last updated: 2026-02-24. Repository: [github.com/Abraxas1010/agenthalo](https://github.com/Abraxas1010/agenthalo)*

---

## What It Is

**Agent H.A.L.O.** (Human-AI Agent Lifecycle Orchestrator) is an open-source system that gives AI agents — Claude, Codex, Gemini, or any custom agent — a tamper-proof audit trail. It wraps agent sessions transparently, records every event into a local cryptographic store, and produces Merkle proofs that make post-hoc tampering mathematically detectable. No data ever leaves the user's machine.

**NucleusDB** is the verifiable database engine underneath. It is also a standalone product: a content-addressed key-value store where every write is a cryptographic commitment, every query can come with a proof, and immutability — once activated — is enforced by mathematics, not access control.

Both are built by **Apoth3osis** and ship from a single Rust codebase: ~77,000 lines of Rust, 131 Lean 4 formal proof modules, 20 Solidity smart contracts, and 875 tests. The entire system — CLI, web dashboard, embedded assets — compiles to a single statically-linked binary with zero runtime dependencies.

---

## The Problem It Solves

An AI coding agent runs for eight minutes. It reads thirty files, rewrites an authentication module, executes shell commands, and calls external APIs. It costs fourteen dollars. The question everyone using AI agents in production eventually asks: *what exactly did it do?*

Today, the honest answer is: you don't know. Agent output scrolls past and vanishes. There is no independent record. When an agent says it ran a test and it passed, there is no cryptographic evidence. When something breaks two days later, there is nothing to audit.

The observability platforms that exist — LangSmith, Helicone, Braintrust, Arize, and others — solve this by sending the agent's data to the cloud. Every prompt, tool call, file read, and line of code written, streamed to a third-party service. They solve the visibility problem by creating a custody problem. And none of them can prove that the log they show you hasn't been altered.

H.A.L.O. takes the opposite approach: **local-first, zero telemetry, cryptographically verifiable.** Traces stay on your machine. Every event gets a SHA-256 Merkle proof. If anyone modifies a record after the fact, the proof chain breaks. This guarantee comes from mathematics, not policy.

---

## How It Works

### Getting Started

```bash
# One-line install (Linux/macOS)
curl -fsSL https://raw.githubusercontent.com/Abraxas1010/agenthalo/master/install.sh | bash

# Or build from source
cargo install --git https://github.com/Abraxas1010/agenthalo --bin agenthalo

# Interactive first-run wizard
agenthalo setup

# Check everything is working
agenthalo doctor
```

The `setup` wizard detects the user's environment and configures the optimal workflow — web dashboard for visual users, CLI for terminal power users, or MCP integration for tool-calling agents.

### Wrapping

One command. Nothing else changes about how the agent runs:

```bash
agenthalo run claude -p "refactor the auth module"
```

H.A.L.O. detects the agent type, injects the right flags for structured output, spawns the agent as a subprocess, tees stdout and stderr so the user sees everything in real time, parses the structured output stream into discrete events, and writes each event to a local NucleusDB trace store at `~/.agenthalo/traces.ndb`.

For permanent wrapping, a single `agenthalo wrap --all` command adds aliases to the user's shell RC file so that `claude`, `codex`, and `gemini` transparently route through H.A.L.O.

### Web Dashboard

```bash
agenthalo dashboard
```

Opens a real-time web dashboard at `localhost:3100` with six pages: Overview (live KPIs), Sessions (drill-down into any session's event timeline), Costs (Chart.js analytics with daily/agent/model views), Configuration (toggle agent wrapping and x402 from the browser), Trust (attestation management), and NucleusDB (browse the underlying verifiable store). Dark and light themes. Server-Sent Events for live updates. All assets embedded at compile time — the dashboard is the binary.

### What Gets Recorded

Every assistant message (full text), user prompt, MCP tool call (name + parameters + result), file change (path + operation), shell command, error, and system event. Each event includes token counts (input, output, cache-read) parsed from the agent's native output stream. Cost is computed per-event using model-specific pricing tables.

### Inspecting Traces

```bash
agenthalo traces
# Session ID    | Agent  | Model           | Tokens   | Cost    | Duration
# sess-17...    | claude | claude-opus-4-6 | 142,800  | $14.82  | 8m 32s

agenthalo costs --month
# February 2026 | 47 sessions | 2,184,000 tokens | $248.30
```

---

## The H.A.L.O. Model

The acronym maps to four concentric layers of the system:

| Layer | Meaning | What It Captures |
|-------|---------|-----------------|
| **H** — Hardware | The physical host | PUF (Physical Unclonable Function) fingerprint, hardware entropy. Traces are anchored to the machine that produced them. |
| **A** — Agent | The identity | Wallet, cryptographic keypair, session metadata. Which agent, which model, which credentials. |
| **L** — Logic | The reasoning | Every tool call, file edit, shell command, and LLM output. The full causal lightcone of the agent's decisions. |
| **O** — Orbit | The boundary | The Merkle root, the seal chain, the monotone extension proofs. Everything outside the orbit is the verifiable, tamper-evident record. |

---

## NucleusDB — The Verifiable Database

NucleusDB is the storage engine that makes all of this possible, and it works as a standalone database for any use case that requires verifiable data integrity.

### Core Properties

- **Content-addressed storage.** Every key-value pair is committed via a cryptographic backend. Three are available: SHA-256 Merkle tree (post-quantum safe, recommended), Pedersen IPA vector commitments, and KZG pairing-based commitments.
- **Monotone extension proofs.** Every commit constructively proves that all prior records are preserved. Deletion is detected instantly.
- **SHA-256 seal chain.** Each commit's seal binds to every previous seal. Forging a seal after deletion requires breaking SHA-256 preimage resistance (2^128 operations).
- **Certificate Transparency.** An RFC 6962 append-only Merkle tree provides independent consistency proofs that any third party can verify.
- **Irreversible immutability.** `SET MODE APPEND_ONLY` is a one-way lock. After activation, UPDATE and DELETE are rejected at both the SQL layer and the protocol layer. This is enforced by the data structure, not by access control.

### Interfaces

NucleusDB ships five client interfaces:

1. **CLI / REPL** — SQL-based interactive shell (`nucleusdb sql`)
2. **Terminal UI** — Five-tab ratatui interface with Status, Browse, Execute, History, and Transparency views (`nucleusdb tui`)
3. **MCP Server (stdio)** — 11 tools for AI agents via Model Context Protocol (`nucleusdb mcp`)
4. **MCP Server (HTTP)** — 25 tools across 5 security tiers with dual authentication (CAB + OAuth 2.1) over MCP Streamable HTTP transport (`nucleusdb-mcp`)
5. **HTTP API** — Multi-tenant REST server with RBAC for production deployments (`nucleusdb-server`)

### SQL Surface

NucleusDB uses a focused SQL dialect over a single virtual table with `key`/`value` columns: INSERT, SELECT, SELECT LIKE, UPDATE, DELETE, COMMIT, VERIFY, SHOW STATUS, SHOW HISTORY, SHOW MODE, SET MODE APPEND_ONLY, EXPORT, and CHECKPOINT.

---

## Cryptographic Stack

| Layer | Primitive | Security Level |
|-------|-----------|---------------|
| State commitments | SHA-256 Merkle tree | 128-bit classical, post-quantum safe |
| Witness signatures | ML-DSA-65 (FIPS 204, Dilithium) | NIST Level 3 post-quantum |
| Monotone seals | SHA-256 hash chain | 128-bit preimage resistance |
| Transparency proofs | RFC 6962 (SHA-256) | 128-bit collision resistance |
| On-chain attestation | Groth16 over BN254 (arkworks) | 128-bit classical pairing security |
| License verification | SHA-256 Merkle CAB certificates | Offline, no phone-home |

---

## Agent Capabilities (MCP Tools)

H.A.L.O. exposes 18 native MCP tools with full JSON Schema parameter discovery, plus proxied third-party tools via AgentPMT integration. Key tool groups:

### Observability
- **halo_traces** — List sessions with agent/model filters, or get full session detail
- **halo_costs** — Cost bucketed by day or month, with optional paid operation breakdown
- **halo_status** — System status: session count, total cost, auth state
- **halo_export** — Full session export as standalone JSON
- **halo_capabilities** — Discover which features and add-ons are enabled

### Attestation and Trust
- **attest** — Create tamper-evident attestation (local Merkle or on-chain Groth16 proof)
- **trust_query** — Computed trust score based on attestation integrity and behavioral signals
- **audit_contract** — Solidity static analysis (small/medium/large tiers)
- **sign_pq** — Post-quantum detached signing with ML-DSA-65

### Payments (x402direct)
- **x402_check** — Parse and validate an HTTP 402 payment request without transacting
- **x402_pay** — Execute a USDC payment on Base (with idempotency protection against double-pay)
- **x402_balance** — Check wallet USDC balance
- **x402_summary** — Unified spending dashboard: budget, total spent, remaining

### Governance and Network (Intent Recording)
- **vote**, **sync**, **privacy_pool_create**, **privacy_pool_withdraw**, **pq_bridge_transfer** — Record intents locally with cryptographic digests. On-chain execution for these operations is planned for future releases.

---

## x402direct — Stablecoin Payments for Agents

H.A.L.O. natively integrates the [x402direct](https://www.x402direct.org) stablecoin payment protocol. When an AI agent encounters an HTTP 402 Payment Required response, H.A.L.O. can handle the entire payment flow:

1. Parse and validate the x402direct JSON payment request
2. Auto-select a payment option on a known network (Base mainnet or Base Sepolia)
3. Enforce budget limits (configurable max auto-approve, default 5 USDC)
4. Check USDC wallet balance
5. Execute ERC-20 transfer via UPC (Unified Payment Contract)
6. Wait for on-chain receipt
7. Return a payment proof with submission instructions for the agent to re-access the resource

Every payment flows through H.A.L.O.'s trace store. Duplicate payments are rejected via protocol nonce tracking. The wallet private key is isolated from other cryptographic keys.

---

## On-Chain Trust Verification (Base L2)

Three Solidity smart contracts deployed on Base (Coinbase L2):

- **TrustVerifier** — ZK proof verification, agent identity registration, USDC payment routing, monotone replay protection
- **TrustVerifierMultiChain** — Composite multi-chain attestation (up to 8 chains), tiered per-chain fees
- **Groth16VerifierAdapter** — Production ZK proof bridge adapting snarkjs-generated verifiers to the trust interface

An agent can attest a session on-chain with a single command (`agenthalo attest --onchain`), producing a Groth16 proof that is posted to the smart contract. Any third party can verify the attestation without accessing the underlying session data.

---

## Formal Verification (Lean 4)

63 Lean 4 modules prove the mathematical properties that NucleusDB relies on. This is not testing — it is machine-checked proof, verified by the Lean 4 kernel.

### What's Proved

- **Sheaf coherence** — Local views of state compose into a globally consistent picture. If local sections agree on overlaps, they assemble into a unique global section. If they don't, someone's view is inconsistent — tamper detection built from pure mathematics.
- **Chain transport** — Round-trip contracts between different chain representations. Forward-backward transport composes to the identity — proved, not assumed.
- **Gluing conditions** — Two chain-local sections can be glued when their projections to the shared space agree.
- **Materialization functors** — The abstract protocol-to-vector relationship satisfies naturality.
- **Multi-chain compliance presheaves** — Per-chain compliance modeled as a presheaf over chain topology.
- **Fork evidence** — Adversarial models formalized. Fork symmetry is a theorem: fork detection is order-independent.

The formal spec covers core protocol invariants, security reductions (if you break NucleusDB, you break SHA-256), commitment correctness, transparency proofs (RFC 6962), and adversarial witness validity.

```bash
# Verify the proofs yourself
lake build NucleusDB
```

---

## AgentPMT Integration

[AgentPMT](https://www.agentpmt.com) is an MCP-native tool infrastructure platform providing budget-controlled access to 100+ third-party tools — Gmail, Stripe, Google Workspace, blockchain scanners, and more.

H.A.L.O. integrates AgentPMT as a **tool proxy**: when enabled, AgentPMT's tools appear alongside H.A.L.O.'s native tools in a single MCP `tools/list` response with an `agentpmt/` prefix. The agent sees one unified tool surface. Budget controls and credentials live on the AgentPMT side. H.A.L.O. records every proxied call in its tamper-evident trace.

---

## Licensing and Pricing

H.A.L.O. uses a **freemium model** gated by SHA-256 CAB (Cryptographic Assurance Bundle) certificates — offline verification, no license server, no phone-home.

| Tier | Price | What You Get |
|------|-------|-------------|
| **Free** | $0 | CLI, SQL, BinaryMerkle backend, Claude/Codex/Gemini wrapping |
| **Starter** | $49/month | + MCP server, Terminal UI, custom agent wrapping |
| **Professional** | $149/month | + IPA backend, HTTP multi-tenant API |
| **Enterprise** | $499/month | + KZG backend, container runtime, on-chain attestation |

---

## Codebase at a Glance

| Component | Count |
|-----------|-------|
| Rust source files | 99 |
| Rust lines of code | ~28,000 |
| Lean 4 formal proof modules | 63 |
| Solidity smart contracts | 10 (7 source + 3 test) |
| Automated tests | 202 Rust + 39 Solidity = 241 total |
| Build warnings | 0 |
| MCP tools (H.A.L.O.) | 18 native + proxied |
| MCP tools (NucleusDB stdio) | 11 |
| MCP tools (NucleusDB HTTP) | 25 |
| Commitment backends | 3 (Merkle, IPA, KZG) |
| Agent adapters | 4 (Claude, Codex, Gemini, Generic) |
| Client interfaces | 6 (CLI, TUI, Web Dashboard, MCP stdio, MCP HTTP, REST API) |
| Release binary size | 9.5 MB (all assets embedded) |

---

## Design Philosophy

The project operates on a specific thesis: **the agentic economy requires sovereign, verifiable infrastructure.** As AI agents begin managing code, finances, and business decisions autonomously, the question isn't just "what did the agent do?" — it's "can you prove it, and can you prove no one changed the record?"

The existing answers — cloud observability dashboards — solve visibility by creating custody. Your agent's entire operational history lives on someone else's servers. H.A.L.O. provides the alternative: local-first, zero-telemetry, mathematically verifiable. The security properties come from SHA-256, Merkle trees, and Lean 4 proofs — not from trust in a vendor.

The project draws explicit inspiration from Vitalik Buterin's February 2026 essay "Reclaiming the Cypherpunk Soul of the World Computer" — specifically the argument that the agentic economy must be built on open, private, decentralized infrastructure rather than proprietary surveillance platforms.

---

## Open Research Directions

The team is actively seeking community input on:

- **Decentralized trace anchoring** — Publishing Merkle roots to on-chain transparency logs so third parties can verify trace integrity without accessing the traces themselves.
- **Agent-to-agent trust** — Protocols for Agent B to verify Agent A's operational history before collaboration.
- **Privacy-preserving audits** — ZK proofs that an agent session met compliance criteria (cost bounds, no data exfiltration, no unauthorized access) without revealing session content.
- **Hardware-anchored identity** — PUF-based agent identity binding cryptographic keys to physical hardware.

---

## Links

- **Repository:** [github.com/Abraxas1010/agenthalo](https://github.com/Abraxas1010/agenthalo)
- **x402direct protocol:** [x402direct.org](https://www.x402direct.org)
- **AgentPMT:** [agentpmt.com](https://www.agentpmt.com)
- **License:** Apoth3osis License Stack v1

---

*Built by [Apoth3osis](https://github.com/Abraxas1010). 54 commits. Zero telemetry.*
