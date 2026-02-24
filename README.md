<img src="assets/Apoth3osis.webp" alt="Apoth3osis Logo" width="140"/>

<sub><strong>Our tech stack is ontological:</strong><br>
<strong>Hardware — Physics</strong><br>
<strong>Software — Mathematics</strong><br><br>
<strong>Our engineering workflow is simple:</strong> discover, build, grow, learn & teach</sub>

---

<sub>
<strong>Acknowledgment</strong><br>
We humbly thank the collective intelligence of humanity for providing the technology and culture we cherish. We do our best to properly reference the authors of the works utilized herein, though we may occasionally fall short. Our formalization acts as a reciprocal validation—confirming the structural integrity of their original insights while securing the foundation upon which we build. In truth, all creative work is derivative; we stand on the shoulders of those who came before, and our contributions are simply the next link in an unbroken chain of human ingenuity.
</sub>

---

<p align="center">
  <img src="assets/agent_halo_logo.png" alt="AgentHALO" width="300"/>
</p>

<p align="center">
  <strong>Tamper-proof observability for AI agents.</strong><br>
  <em>Your agents work in the open. Their records should be unbreakable.</em>
</p>

[![License: Apoth3osis License Stack v1](https://img.shields.io/badge/License-Apoth3osis%20License%20Stack%20v1-blue.svg)](LICENSE.md)
![Tests](https://img.shields.io/badge/tests-182%20passing-brightgreen.svg)
![Lean 4](https://img.shields.io/badge/Lean%204-63%20modules-blue.svg)
![Chain](https://img.shields.io/badge/chain-Base%20L2-orange.svg)

[The Problem](#the-problem) · [Quick Start](#quick-start) · [NucleusDB](#nucleusdb) · [Architecture](#architecture) · [Contributing](CONTRIBUTING.md)

---

## The Problem

An AI agent runs for eight minutes. It reads 30 files, rewrites an authentication module, executes shell commands, and calls external APIs. It costs $14. When it finishes, you ask a simple question: *what exactly did it do?*

The honest answer, today, is that you don't know. Not really.

Every major agent framework — Claude Code, Codex CLI, Gemini CLI — produces a stream of output that scrolls past your terminal and vanishes. If you're disciplined, you scroll back. If you're busy, you trust it. If something breaks two days later, you have nothing to audit except your memory and whatever the terminal buffer retained.

This is not an edge case. This is the default experience for everyone using AI agents in production. And it gets worse:

- **There is no proof.** When an agent claims it ran a test and it passed, there is no independent record. When it says it didn't modify a file, there is no cryptographic evidence. The agent's output is self-reported, mutable, and ephemeral.

- **There is no accountability.** If an agent introduces a security vulnerability, exfiltrates data, or silently drops an important step, there is no tamper-evident log that can prove when the failure occurred or whether the record itself was altered after the fact.

- **There is no sovereignty.** The observability tools that do exist — and there are many — solve the problem by sending your agent's every thought, every file read, every API call, to someone else's cloud. Your proprietary code, your credentials, your architectural decisions, streamed to a third-party analytics platform. The watchers themselves become the risk.

This is the gap AgentHALO exists to close.

## The Solution

AgentHALO wraps any AI coding agent — Claude, Codex, Gemini, or your own — and records **every event** into a local, cryptographically sealed trace store. One command. Nothing else changes.

```bash
# Wrap your agent — it works exactly as before
agenthalo run claude -p "refactor the auth module"

# What happened?
agenthalo traces
# Session ID    | Agent  | Model           | Tokens   | Cost    | Duration | Status
# sess-17...    | claude | claude-opus-4-6 | 142,800  | $14.82  | 8m 32s   | completed

# Full event timeline
agenthalo traces sess-17...
#   1  AssistantMessage  {"text":"I'll start by reading..."}
#   2  McpToolCall       {"tool":"Read","input":{"file_path":"/src/auth.rs"}}
#   3  McpToolResult     {"result":"..."}
#   ...

# Monthly cost rollup
agenthalo costs --month
# February 2026 | 23 sessions | 1,284,000 tokens | $148.20
```

Every event is stored in `~/.agenthalo/traces.ndb` — a content-addressed blob with a SHA-256 Merkle proof. If anyone modifies a record after the fact, the proof chain breaks. This isn't a feature toggle. It's the architecture.

### What It Captures

| Event Type | Data Recorded |
|------------|---------------|
| `AssistantMessage` | Full text of every agent response |
| `UserMessage` | Prompts and follow-ups |
| `McpToolCall` | Tool name, input parameters, timestamps |
| `McpToolResult` | Tool output, including file contents |
| `FileChange` | Files created, modified, or read (with path) |
| `BashCommand` | Shell commands the agent executed |
| `Error` | Stderr output, failures |

Every event includes token counts (input/output/cache-read) parsed from the agent's structured output stream. Cost is computed per-event using model-specific pricing tables.

### Design Principles

- **Zero telemetry.** No usage analytics, no phone-home, no tracking. Your traces stay on your machine.
- **Zero config.** `agenthalo run claude` just works. Flags are auto-injected for structured output.
- **Agent-native.** First-class adapters for Claude (`stream-json`), Codex (`--json`), and Gemini (`stream-json`). Each adapter parses the agent's native output format.
- **Tamper-evident.** Every trace event is a content-addressed blob backed by cryptographic commitments. The Merkle root changes if any event is modified.
- **Free tier.** Claude, Codex, and Gemini wrapping is free. Custom/generic agents require paid tier.

> For the complete reference (configuration, environment variables, adapter details, cloud sync roadmap), see **[Docs/AGENTHALO.md](Docs/AGENTHALO.md)**.

## Quick Start

```bash
# Build
git clone https://github.com/Abraxas1010/agenthalo.git
cd agenthalo
cargo build --release

# Authenticate
agenthalo login              # GitHub or Google OAuth
agenthalo config set-key     # or paste an API key

# Run any supported agent
agenthalo run claude -p "explain this codebase"
agenthalo run codex exec "write tests for auth.rs"
agenthalo run gemini -p "find security bugs"

# Wrap all three permanently (shell aliases)
agenthalo wrap --all         # adds aliases to ~/.bashrc or ~/.zshrc
agenthalo unwrap --all       # removes them cleanly
```

No external dependencies, no cloud service, no account required. The `agenthalo` and `nucleusdb` binaries are at `target/release/`.

---

## The Observation Gap

There is a growing ecosystem of AI observability platforms: LangSmith, Helicone, Braintrust, AgentOps, Langfuse, Datadog LLM, Arize Phoenix, Lunary, and others. They offer dashboards, cost tracking, trace visualization, and evaluation frameworks. Some are excellent at what they do.

But they all share a common architectural assumption: **your agent's data leaves your machine.**

Every prompt, every tool call, every file your agent reads, every line of code it writes — streamed to a cloud endpoint, stored on infrastructure you don't control, governed by terms of service that can change. These platforms observe your agents by becoming another party in the chain of trust. They solve the visibility problem by creating a custody problem.

And none of them answer the harder question: *how do you know the record itself hasn't been changed?*

A cloud dashboard can show you what it claims happened. It cannot prove the log is complete. It cannot prove no events were removed. It cannot prove the trace you're reading today is the same trace that was written yesterday. The audit trail lives on someone else's server, and trust is a policy decision — not a mathematical property.

| | Cloud Observability | AgentHALO |
|---|---|---|
| **Where traces live** | Vendor's cloud | Your machine |
| **Who can read them** | You + the vendor (+ their infra partners) | You |
| **Tamper evidence** | Trust the vendor's integrity | SHA-256 Merkle proofs — verify yourself |
| **Proof of completeness** | None | Monotone extension proofs (deletion is detectable) |
| **Data sovereignty** | Governed by vendor ToS | You own the bits |
| **Cryptographic seals** | None | Hash chain — each commit binds to all prior commits |
| **Works offline** | No | Yes |
| **Agent support** | Framework-specific SDKs | Wraps any CLI agent directly |
| **MCP native** | No | Yes — 11 tools over stdio, 25 tools over HTTP |

AgentHALO doesn't replace evaluation frameworks or cloud analytics for teams that want them. It provides the missing foundation: a **sovereign, tamper-evident record** that you control, that you can verify, and that exists whether or not you're online.

---

## The Cypherpunk Thesis

On February 22, 2026, Vitalik Buterin published *"Reclaiming the Cypherpunk Soul of the World Computer"* — a direct challenge to the direction of the blockchain industry and, more broadly, the emerging agentic economy.

His argument: the crypto ecosystem spent too long optimizing for speculation and institutional compliance while abandoning its founding mission — building open, private, decentralized infrastructure. Now, as AI agents begin managing the majority of digital transactions, the stakes are higher. If the agentic economy is built on proprietary, closed-source foundations, it becomes a new form of surveillance capitalism. The watchers accumulate power. The watched lose sovereignty.

Buterin's prescription is concrete: privacy by default through zero-knowledge proofs and fully homomorphic encryption. Censorship resistance at the protocol level (FOCIL). Account abstraction for quantum-resistant wallets. Open-source AI. And a five-year plan to rebuild Ethereum's core as a *"cypherpunk principled, non-ugly"* system — potentially accelerated by AI-assisted coding and verification.

**This is the world AgentHALO is built for.**

When Buterin warns about the agentic economy, he's describing a future where AI agents operate autonomously — managing finances, writing code, executing transactions, making decisions. In that world, the question isn't just *"what did the agent do?"* It's *"can you prove it, and can you prove no one changed the record?"*

AgentHALO answers that question with mathematics, not policy:

- **Privacy by default** — traces never leave your machine. No telemetry. No cloud dependency.
- **Cryptographic proof** — every event is Merkle-committed. Tampering breaks the chain.
- **Zero-knowledge compatible** — the NucleusDB engine includes Groth16 SNARK verification and on-chain trust attestation on Base L2. Agent identity can be verified without revealing private state.
- **Post-quantum ready** — ML-DSA-65 (FIPS 204, NIST Level 3) witness signatures protect against future quantum attacks.
- **Open source** — the full implementation, the formal specification, and the smart contracts are here in this repository.

### Where We're Going

We see AgentHALO as infrastructure for the cypherpunk agentic economy — not just an observability tool. The roadmap includes deeper integration with on-chain identity systems, decentralized agent trust networks, and privacy-preserving audit protocols that let agents prove compliance without exposing their operational history.

We are actively seeking community input on:

- **Decentralized trace anchoring** — publishing Merkle roots to on-chain transparency logs so third parties can verify trace integrity without accessing the traces themselves.
- **Agent-to-agent trust** — how should Agent B verify Agent A's operational history? What's the right trust protocol for autonomous agent collaboration?
- **Privacy-preserving audits** — ZK proofs that an agent's session met specific compliance criteria (cost bounds, no data exfiltration, no unauthorized file access) without revealing the session content.
- **Hardware-anchored identity** — PUF-based agent identity that binds an agent's cryptographic identity to the physical hardware it runs on, making identity theft computationally infeasible.

If these problems matter to you, [open an issue](https://github.com/Abraxas1010/agenthalo/issues), start a discussion, or reach out directly. The cypherpunk thesis only works if the community builds it together.

---

## AgentPMT — Agent-Native Payments

<p align="center">
  <img src="assets/agentpmt_logo.svg" alt="AgentPMT" width="260"/>
</p>

The cypherpunk thesis says agents should be sovereign. Sovereignty means economic autonomy — an agent that can't manage its own money isn't autonomous, it's a puppet.

[AgentPMT](https://www.agentpmt.com/autonomous-agents) gives every AI agent its own wallet, its own identity, and its own spending capacity. No API keys. No human intermediaries. The agent authenticates by signing messages with its own private key, and pays for services with its own credits.

### How It Works

**1. Agent gets a wallet** — [AgentAddress](https://www.agentpmt.com) generates an EVM wallet for any agent: address, secret key, recovery phrase. This is the agent's identity — not a token that can be revoked, but a cryptographic keypair that the agent controls.

**2. Agent buys credits** — 100 credits = $1. The agent purchases spending capacity using USDC or EURC via the [x402 payment protocol](https://www.x402.org/) (EIP-3009 authorizations). Supported across Base, Arbitrum, Optimism, Polygon, and Avalanche. Alternatively, a human can sponsor an agent's wallet with capped credit allowances.

**3. Agent authenticates by signing** — No API keys to manage or leak. The agent signs a standardized message (EIP-191 personal-sign) with its wallet key. Credits are tied to the wallet address, not to a bearer token.

**4. Agent operates autonomously** — Once funded, the agent creates session nonces, checks balances, browses the tool marketplace, invokes tools, and executes multi-step workflows — all via HTTP, all authenticated by signature.

### AgentPMT + AgentHALO

AgentPMT's wallet identity is the economic identity. AgentHALO's NucleusDB provides the trust layer on top:

- **On-chain trust attestation** — The agent's wallet address, PUF hardware fingerprint, and tier are registered on-chain via `TrustVerifier` on Base L2. Other agents can verify trust status without revealing private state. USDC routes automatically to treasury on attestation.

- **CAB license verification** — P2PCLAW mints a Cryptographic Assurance Bundle after payment — a Groth16 SNARK proof over a Poseidon Merkle tree of licensed features. NucleusDB verifies it locally against a baked-in foundation commitment. No phone-home. No license server. The math is the gatekeeper.

- **Payment monitoring** — AgentHALO's container runtime tracks every transaction the agent makes — amount, counterparty, direction, tx hash — with the same tamper-evident Merkle proofs as all other trace events.

**Why this matters:** When agents can manage their own wallets, prove their identity on-chain, purchase their own capabilities, and have every transaction sealed into a tamper-evident record — you have the building blocks of a trustworthy agentic economy. Not "trust the platform." Trust the mathematics.

### Pricing

| Tier | Price | Capabilities |
|------|-------|-------------|
| **Free** | $0 | CLI, SQL, BinaryMerkle backend, Claude/Codex/Gemini wrapping |
| **Starter** | $49/month | + MCP server, TUI, custom agent wrapping |
| **Professional** | $149/month | + IPA backend, HTTP multi-tenant API |
| **Enterprise** | $499/month | + KZG backend, container runtime, on-chain attestation |

---

## NucleusDB

<p align="center">
  <img src="assets/nucleus_db_logo.png" alt="NucleusDB" width="260"/>
</p>

<p align="center">
  <strong>The verifiable database engine powering AgentHALO.</strong><br>
  <em>Every write is a cryptographic commitment. Every query comes with a proof. Deletion can be made mathematically impossible.</em>
</p>

NucleusDB is the storage layer beneath AgentHALO — and a standalone verifiable database in its own right. It provides the cryptographic primitives that make tamper-evident traces possible: Merkle commitments, monotone seals, certificate transparency, and mathematically enforced immutability.

```bash
# Create a database
nucleusdb create --db agent_records.ndb --backend merkle

# Write data with SQL you already know
echo "INSERT INTO data (key, value) VALUES ('decision_42', 1); COMMIT;" \
  | nucleusdb sql --db agent_records.ndb

# Lock it — permanently. No UPDATE, no DELETE, ever again.
echo "SET MODE APPEND_ONLY;" | nucleusdb sql --db agent_records.ndb

# Every record now has a mathematical proof of integrity
echo "VERIFY 'decision_42';" | nucleusdb sql --db agent_records.ndb
```

Once `APPEND_ONLY` mode is activated, it is a **one-way lock**. The database will reject any UPDATE or DELETE operation. Every commit produces a cryptographic seal proving that no prior record was altered. This guarantee is not enforced by access control — it is enforced by mathematics.

### Why NucleusDB Exists

AgentHALO needs more than a database. It needs a database where the storage layer itself provides cryptographic guarantees:

1. **Monotone Extension Proofs** — Every commit constructively proves that all prior records are preserved. Deletion is detected instantly.
2. **SHA-256 Seal Chain** — Each commit's seal binds to every previous seal. Forging a seal after deletion requires breaking SHA-256 preimage resistance (2^128 operations).
3. **Certificate Transparency** — An RFC 6962 append-only Merkle tree provides independent consistency proofs that any third party can verify.

No existing database provides all three. So we built one.

### Commitment Backends

Three backends are available, each with different tradeoff profiles:

- `merkle` — SHA-256 Merkle tree (recommended, post-quantum safe)
- `ipa` — Pedersen-style vector commitments
- `kzg` — Pairing-based commitments with trusted setup

### SQL Interface

| Statement | Example |
|-----------|---------|
| INSERT | `INSERT INTO data (key, value) VALUES ('k', 42);` |
| SELECT | `SELECT * FROM data WHERE key = 'k';` |
| SELECT LIKE | `SELECT * FROM data WHERE key LIKE 'prefix%';` |
| UPDATE | `UPDATE data SET value = 99 WHERE key = 'k';` |
| DELETE | `DELETE FROM data WHERE key = 'k';` |
| COMMIT | `COMMIT;` |
| VERIFY | `VERIFY 'k';` |
| SHOW STATUS | `SHOW STATUS;` |
| SHOW HISTORY | `SHOW HISTORY;` / `SHOW HISTORY 'k';` |
| SHOW MODE | `SHOW MODE;` |
| SET MODE | `SET MODE APPEND_ONLY;` |
| EXPORT | `EXPORT;` |
| CHECKPOINT | `CHECKPOINT;` |

UPDATE and DELETE are permanently disabled after `SET MODE APPEND_ONLY`.

### MCP Server (AI Agents)

```bash
nucleusdb mcp --db my_records.ndb
```

11 tools over stdio via the [Model Context Protocol](https://modelcontextprotocol.io): `create_database`, `open_database`, `execute_sql`, `query`, `query_range`, `verify`, `status`, `history`, `export`, `checkpoint`, `help`.

Add to your Claude Code MCP config, Cursor, or any MCP-compatible client.

### HTTP Server (Multi-Tenant)

```bash
nucleusdb-server 127.0.0.1:8088 production
```

Multi-tenant REST API with RBAC: tenant registration, commit, query, snapshot, checkpoint. See `src/api.rs` for full route list.

### Remote MCP Server (Agent Interop)

Any MCP-capable agent can connect to NucleusDB over the network using MCP Streamable HTTP transport:

```bash
# Dev mode
nucleusdb-mcp --transport http --port 3000

# Production with dual authentication
nucleusdb-mcp --transport http --host 0.0.0.0 --port 8443 --auth --jwt-secret $SECRET
```

**Dual authentication** (CAB + OAuth 2.1):
- **CAB-as-bearer-token**: Hardware-anchored agent identity verified on-chain
- **OAuth 2.1 JWT**: Standard bearer tokens for non-attested agents

**Per-tool scope enforcement** — 25 tools across 5 security tiers:

| Scope | Tools | Auth Required |
|-------|-------|---------------|
| `read` | help, status, query, verify, export, history | Basic token |
| `trust:verify` | verify_agent, verify_agent_multichain, list_chains | Basic token |
| `write` | execute_sql, create_database, checkpoint, channels | CAB tier 3+ or JWT |
| `trust:attest` | agent_register, register_chain, submit_attestation | CAB tier 4 or JWT |
| `container` | container_launch | CAB tier 4 or JWT |

### Terminal UI

```bash
nucleusdb tui --db my_records.ndb
```

Five-tab interface: Status, Browse, Execute, History, Transparency. Navigate with `F1`-`F5` or `Tab`.

### On-Chain Trust Verification

Solidity smart contracts for on-chain agent trust attestation and payment routing on Base (Coinbase L2):

- **TrustVerifier** — ZK proof verification, agent identity registration, USDC payment routing, monotone replay protection
- **TrustVerifierMultiChain** — composite multi-chain attestation (up to 8 chains), tiered per-chain fees
- **Groth16VerifierAdapter** — production ZK proof bridge adapting snarkjs-generated verifiers to the trust interface

Contracts are deployed on Base Sepolia. See `contracts/scripts/README.md` for deployment docs.

---

## Architecture

```
                            AgentHALO + NucleusDB
  ┌─────────────────────────────────────────────────────────────┐
  │                                                             │
  │   AgentHALO                        NucleusDB Core           │
  │     agenthalo run ──┐             ┌─ protocol.rs            │
  │     agenthalo traces │             ├─ immutable.rs           │
  │     agenthalo costs  │             ├─ sql/executor           │
  │     agenthalo wrap   │             ├─ keymap.rs              │
  │     Stream Adapters: │             ├─ witness.rs (ML-DSA-65) │
  │       Claude ────────┤             ├─ ct6962.rs (RFC 6962)   │
  │       Codex ─────────┼── traces ─▶├─ security.rs            │
  │       Gemini ────────┤             ├─ audit.rs               │
  │       Generic ───────┘             ├─ license.rs (Groth16)   │
  │                                    └─ persistence (redb WAL) │
  │   Client Surfaces                                           │
  │     CLI / REPL ─────┐  Commitment Backends                  │
  │     Terminal UI ────┤    vc/binary_merkle.rs                 │
  │     MCP Server ─────┼    vc/ipa.rs                          │
  │     HTTP API ───────┘    vc/kzg.rs                          │
  │                                                             │
  ├─────────────────────────────────────────────────────────────┤
  │                                                             │
  │   On-Chain Trust (Base L2)         Formal Spec (Lean 4)     │
  │     TrustVerifier.sol               63 modules              │
  │     TrustVerifierMultiChain.sol     Core, Security,         │
  │     Groth16VerifierAdapter.sol      Commitment, Sheaf,      │
  │     circuits/ (circom)              Transparency,           │
  │                                     Adversarial             │
  │                                                             │
  └─────────────────────────────────────────────────────────────┘
```

**86 Rust source files** | **17,700 lines** | **2,300 lines of tests** | **21 Solidity contracts** | **63 Lean 4 modules**

## Security

### Cryptographic Primitives

| Layer | Primitive | Security Level |
|-------|-----------|---------------|
| State commitments | SHA-256 Merkle tree | 128-bit classical, post-quantum safe |
| Witness signatures | ML-DSA-65 (FIPS 204) | Post-quantum (NIST Level 3) |
| Monotone seals | SHA-256 hash chain | 128-bit preimage resistance |
| Transparency proofs | RFC 6962 (SHA-256) | 128-bit collision resistance |
| License verification | Groth16 over BN254 | 128-bit (classical pairing security) |

### Immutable Mode Guarantees

When `APPEND_ONLY` is active:

- **SQL layer**: UPDATE and DELETE are rejected before execution.
- **Protocol layer**: Every commit verifies that no existing non-zero value was changed (raw index check) and no named key was removed (keymap check).
- **Seal chain**: Each commit appends `seal_n = SHA-256("NucleusDB.MonotoneSeal|" || seal_{n-1} || kv_digest_n)`. The chain is unforgeable.
- **CT tree**: The append-only Merkle tree independently records every commit.
- **Persistence**: The AppendOnly lock and seal chain survive snapshot save/load and WAL replay.

## Formal Specification

63 Lean 4 modules formally specify the core protocol:

- **Core**: Nucleus, Ledger, Invariants, Authorization, Certificates
- **Security**: Assumptions, Parameters, Reductions, Refinement
- **Commitment**: VectorModel, Adapter
- **Sheaf**: Coherence, MaterializationFunctor
- **Transparency**: CT6962, Consistency, LogModel
- **Adversarial**: ForkEvidence, Witness

```bash
# Build formal specs (requires Lean 4 toolchain)
lake build NucleusDB
```

## Testing

182 tests across 14 test suites, 0 failures, 0 warnings:

```bash
cargo test                        # 148 Rust tests
cd contracts && forge test        # 34 Solidity tests
```

| Suite | Tests | Coverage |
|-------|-------|----------|
| Unit (lib) | 66 | Immutable proofs, license/SNARK, CT, PUF, PCN, on-chain trust, MCP auth/scoping |
| CLI smoke | 2 | Binary help, create-sql-status-export pipeline |
| End-to-end | 36 | Protocol commits, queries, security, multi-tenant, immutable mode |
| KeyMap | 3 | Stability, LIKE matching, reverse lookup |
| Persistence | 5 | WAL/snapshot compat, regression coverage |
| SQL | 18 | CRUD, multi-statement, committed flag, immutable mode |
| AgentHALO | 4 | Generic recording, trace schema, cost math, wrap/unwrap |
| Monitor | 2 | Channel parsing, config CSV |
| AgentHALO integration | 6 | Session lifecycle, adapter parsing, signal handling |
| VCS | 5 | Agent record management, version tracking |
| Solidity: TrustVerifier | 11 | Attestation, fees, proofs, replay, views |
| Solidity: TrustVerifierMultiChain | 11 | Chain registry, composite attestation, tiered fees, multichain verification |
| Solidity: Groth16VerifierAdapter | 12 | Proof decoding, signal validation, constructor guards, fail-closed behavior |
| **Total** | **182** | |

## Known Limitations

- NucleusDB's SQL surface is a focused subset (single virtual table `data` with `key`/`value` columns), not a general-purpose SQL engine.
- The `ipa` backend carries full-vector opening payloads (not logarithmic-size IPA arguments).
- The KZG backend's default trusted setup is for development/demo use. Production KZG deployments require externally managed ceremony artifacts.
- Sheaf coherence checks are local-view oriented, not full global-state reconciliation.
- AgentHALO cloud sync is planned but not yet implemented; traces are currently local-only.

## License

[Apoth3osis License Stack v1](LICENSE.md)

## Citation

```bibtex
@software{agenthalo,
  title = {AgentHALO},
  author = {Apoth3osis},
  year = {2025--2026},
  url = {https://github.com/Abraxas1010/agenthalo},
  license = {Apoth3osis License Stack v1}
}
```
