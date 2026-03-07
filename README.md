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
  <img src="assets/agent_halo_logo.png" alt="Agent H.A.L.O." width="300"/>
</p>

<p align="center">
  <strong>Agent H.A.L.O.</strong> — <strong>H</strong>uman-AI <strong>A</strong>gent <strong>L</strong>ifecycle <strong>O</strong>rchestrator<br>
  <em>Tamper-proof observability for AI agents.</em>
</p>

<br>

<table>
<tr>
<td width="25%" align="center"><strong>H</strong><br><sub>Human-AI</sub><br><em>The Interface</em></td>
<td width="25%" align="center"><strong>A</strong><br><sub>Agent</sub><br><em>The Identity</em></td>
<td width="25%" align="center"><strong>L</strong><br><sub>Lifecycle</sub><br><em>The Continuity</em></td>
<td width="25%" align="center"><strong>O</strong><br><sub>Orchestrator</sub><br><em>The Conductor</em></td>
</tr>
<tr>
<td align="center"><sub>The human-AI boundary — sovereign observation, cryptographic trust anchoring, PUF-bound hardware identity</sub></td>
<td align="center"><sub>Wallet identity, cryptographic keypair, the self that persists across sessions</sub></td>
<td align="center"><sub>Traces, tool calls, decisions, costs — the full causal history from launch to completion</sub></td>
<td align="center"><sub>Multi-agent coordination — budget enforcement, mesh networking, task DAGs, container management</sub></td>
</tr>
</table>

<br>

[![License: Apoth3osis License Stack v1](https://img.shields.io/badge/License-Apoth3osis%20License%20Stack%20v1-blue.svg)](LICENSE.md)
![Tests](https://img.shields.io/badge/tests-875%20passing-brightgreen.svg)
![Lean 4](https://img.shields.io/badge/Lean%204-131%20modules-blue.svg)
![Chain](https://img.shields.io/badge/chain-Base%20L2-orange.svg)

[The Problem](#the-problem) · [Quick Start](#quick-start) · [Web Dashboard](#web-dashboard) · [Orchestrator](#orchestrator) · [Sovereign Identity](#sovereign-identity) · [The Algebraic Foundation](#the-algebraic-foundation) · [NucleusDB](#nucleusdb) · [Architecture](#architecture) · [Contributing](CONTRIBUTING.md)

---

## The Problem

An AI agent runs for eight minutes. It reads 30 files, rewrites an authentication module, executes shell commands, and calls external APIs. It costs $14. When it finishes, you ask a simple question: *what exactly did it do?*

The honest answer, today, is that you don't know. Not really.

Every major agent framework — Claude Code, Codex CLI, Gemini CLI — produces a stream of output that scrolls past your terminal and vanishes. If you're disciplined, you scroll back. If you're busy, you trust it. If something breaks two days later, you have nothing to audit except your memory and whatever the terminal buffer retained.

This is not an edge case. This is the default experience for everyone using AI agents in production. And it gets worse:

- **There is no proof.** When an agent claims it ran a test and it passed, there is no independent record. When it says it didn't modify a file, there is no cryptographic evidence. The agent's output is self-reported, mutable, and ephemeral.

- **There is no accountability.** If an agent introduces a security vulnerability, exfiltrates data, or silently drops an important step, there is no tamper-evident log that can prove when the failure occurred or whether the record itself was altered after the fact.

- **There is no sovereignty.** The observability tools that do exist — and there are many — solve the problem by sending your agent's every thought, every file read, every API call, to someone else's cloud. Your proprietary code, your credentials, your architectural decisions, streamed to a third-party analytics platform. The watchers themselves become the risk.

This is the gap H.A.L.O. exists to close.

## The Solution

Agent H.A.L.O. wraps any AI coding agent — Claude, Codex, Gemini, or your own — and records **every event** into a local, cryptographically sealed trace store. One command. Nothing else changes.

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

The four layers of the H.A.L.O. model map directly to what gets recorded:

- **H** (Human-AI) — The trust boundary between human and AI. PUF fingerprints anchor traces to the physical machine. Sovereign observation without third-party custody.
- **A** (Agent) — The agent's wallet identity and session metadata. Which agent, which model, which credentials — cryptographically bound to the trace.
- **L** (Lifecycle) — Every reasoning step, tool call, file edit, and shell command. The full causal history from launch to completion, with token counts and cost attribution.
- **O** (Orchestrator) — Multi-agent coordination. Budget enforcement, mesh networking, task DAGs, and container management. The conductor that keeps agents accountable.

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

### Epistemic Calculi — Formal Reasoning About Agent Trust

H.A.L.O. integrates five epistemic calculi from the [Heyting formal mathematics project](https://github.com/Abraxas1010/heyting), providing mathematically grounded reasoning about agent behavior, trust, and uncertainty:

| Calculus | Module | What It Does |
|----------|--------|-------------|
| **Tsallis Diversity** | `halo::metrics::diversity` | Measures tool-usage diversity via Tsallis 2-entropy (Gini impurity). A high diversity score indicates the agent explores a broad tool repertoire rather than hammering one tool repeatedly. Exposed as a real-time gauge in the Cockpit. |
| **Epistemic Trust Nucleus** | `halo::trust` | Models trust as a Heyting algebra nucleus: `N(x) = max(x, floor)`. The nucleus is extensive, idempotent, and meet-preserving — guaranteeing a well-defined trust floor below which no agent can fall. Fuses multiple trust signals via product, with residuated implication for "if Y then Z" reasoning. |
| **Bayesian Evidence Combiner** | `halo::evidence` | Iterative Bayesian odds-update: each tool observation shifts the posterior via `P(E\|H)` / `P(E\|~H)` likelihood ratios. Correctly oriented as false-over-true odds for stable iterative composition. Available as an MCP tool (`agenthalo_evidence_combine`). |
| **Uncertainty Translation** | `halo::uncertainty` | Hub-and-spoke conversion between four uncertainty frameworks: Probability, Certainty Factor, Possibility, and Binary. Tools can report confidence in their native framework; H.A.L.O. translates to a common probabilistic scale. Available as an MCP tool (`agenthalo_uncertainty_translate`). |
| **Trace Topology (H0 Persistence)** | `halo::trace_topology` | Computes H0 persistence (connected components over time) from agent tool-transition graphs via Vietoris-Rips filtration with union-find. Reveals whether an agent's behavior is episodic (many disconnected clusters) or coherent (one persistent connected component). |

These are not heuristics — they are implementations of formally specified mathematical structures whose core properties (nucleus laws, Bayesian consistency, translation roundtrips) are verified by the Lean 4 kernel in the companion Heyting repository.

## Quick Start

```bash
# One-line install (clones and builds from source; requires Rust toolchain + repo access)
curl -fsSL https://raw.githubusercontent.com/Abraxas1010/agenthalo/master/install.sh | bash

# Or clone and build manually
git clone https://github.com/Abraxas1010/agenthalo.git && cd agenthalo
cargo install --path . --bin agenthalo

# Interactive first-run wizard
agenthalo setup

# Check everything is working
agenthalo doctor

# Launch the web dashboard
agenthalo dashboard
```

### After Setup

```bash
# Run any supported agent
agenthalo run claude -p "explain this codebase"
agenthalo run codex exec "write tests for auth.rs"
agenthalo run gemini -p "find security bugs"

# Wrap all three permanently (shell aliases)
agenthalo wrap --all         # adds aliases to ~/.bashrc or ~/.zshrc
agenthalo unwrap --all       # removes them cleanly

# View everything in the browser
agenthalo dashboard          # opens http://localhost:3100
```

No external dependencies, no cloud service, no account required. The entire system — CLI, web dashboard, embedded assets — compiles to a single statically-linked binary.

### Identity Category (CLI + MCP)

```bash
# Inspect full identity state
agenthalo identity status --json

# Social provider lifecycle (immutable ledger-backed)
agenthalo identity social connect google <token> --expires-days 30
agenthalo identity social revoke google --reason rotate_token

# Super-secure controls
agenthalo identity super-secure set passkey true
agenthalo identity super-secure set totp true --label "My Authenticator"
```

These actions are recorded in an append-only hash-chained ledger at
`~/.agenthalo/identity_social_ledger.jsonl`. The same identity surface is
available to agents through MCP tools: `identity_status`,
`identity_social_connect`, `identity_social_revoke`, and
`identity_super_secure_set`.

---

## Web Dashboard

```bash
agenthalo dashboard          # opens http://localhost:3100
agenthalo dashboard --port 8080 --no-open  # custom port, no auto-open
```

A real-time observability dashboard embedded in the binary — no npm, no CDN, no external dependencies. All assets are compiled in via `rust-embed`.

| Page | What It Shows |
|------|---------------|
| **Overview** | Live KPIs (sessions, tokens, cost, active agents), recent sessions, epistemic trust status |
| **Sessions** | Filterable list, drill-down to full event timeline, export, attest |
| **Costs** | Daily cost chart, agent distribution, model comparison, paid operations |
| **Configuration** | Toggle agent wrapping and x402 payments from the browser |
| **Trust** | Attestation list, one-click verify, create attestations |
| **NucleusDB** | Browse the verifiable store, execute SQL, view commit history |
| **Cockpit** | Launch and manage agent sessions in browser-based xterm.js terminals, diversity gauge, trace topology |
| **Deploy** | Agent catalog cards, preflight checks, one-click agent deployment |

Dark/light theme toggle. SSE live updates. Chart.js analytics. Responsive layout.

### Cockpit — Browser Terminal Orchestration

The Cockpit transforms the dashboard into a full agent orchestration terminal. Launch Claude, Codex, Gemini, OpenClaw, or Shell sessions directly in the browser — each in its own xterm.js panel with CRT terminal aesthetics.

- **PTY bridge** — real pseudo-terminal sessions via WebSocket, not simulated output
- **Multi-panel layout** — run multiple agents side-by-side with tab management
- **Deploy page** — agent catalog with preflight checks (CLI detection, auth status, vault keys)
- **Mesh sidebar** — live P2P peer topology with online/offline status and latency
- **Diversity gauge** — real-time Tsallis 2-entropy tool diversity score with doughnut chart
- **Trace topology** — H0 persistence visualization showing behavioral coherence over time

> Full API reference: **[Docs/AGENTHALO.md](Docs/AGENTHALO.md#web-dashboard)**

---

## Orchestrator

H.A.L.O. manages multiple AI agents as a unified fleet. The orchestrator provides lifecycle control, budget enforcement, and task coordination — all exposed via MCP tools that any controlling agent can call.

```bash
# Launch agents
agenthalo orchestrate launch --agent claude --name reviewer --timeout 120
agenthalo orchestrate launch --agent shell --name builder

# Submit tasks
agenthalo orchestrate send-task --agent-id orch-abc --task "review auth.rs for vulnerabilities"
agenthalo orchestrate send-task --agent-id orch-def --task "cargo test --release"

# Pipe outputs between agents (task DAG)
agenthalo orchestrate pipe --from task-123 --to agent-id orch-ghi --task "summarize the review"
```

### Multi-Agent Budget Control

Per-instance resource limits prevent runaway agent spawning:

| Constraint | Default | Description |
|------------|---------|-------------|
| `max_agents` | 64 | Total managed agents across all kinds |
| `max_concurrent_busy` | 10 | Maximum agents executing tasks simultaneously |
| `allowed_kinds` | all | Restrict to specific agent types |

Budget enforcement is atomic — check-and-insert under a single lock with no TOCTOU window.

### Agent Kinds

| Kind | CLI | Use Case |
|------|-----|----------|
| `claude` | `claude --print --output-format json` | Code review, analysis, generation |
| `codex` | `codex exec --full-auto --json` | Autonomous coding tasks |
| `gemini` | `gemini --yolo` | Large-context analysis |
| `openclaw` | `openclaw run --non-interactive` | Decentralized agent workflows |
| `shell` | `sh -c` | Build scripts, system commands |

### P2P Mesh Networking

Agents discover each other via a peer registry and exchange status over a libp2p mesh. The cockpit renders live peer topology — which agents are online, their latency, and reachability.

### MCP Orchestration Tools

9 tools available to any MCP-capable controlling agent: `orchestrator_launch`, `orchestrator_send_task`, `orchestrator_get_result`, `orchestrator_pipe`, `orchestrator_list`, `orchestrator_tasks`, `orchestrator_graph`, `orchestrator_mesh_status`, `orchestrator_stop`.

---

## Sovereign Identity

Every H.A.L.O. agent derives a DID (Decentralized Identifier) from a genesis seed ceremony. The DID document carries both classical and post-quantum key pairs — side by side.

```bash
# Generate sovereign identity
agenthalo keygen

# Inspect identity state
agenthalo identity status --json
```

### Post-Quantum Cryptography

All agent-controlled cryptographic surfaces are PQ-hardened:

| Surface | Classical | Post-Quantum | Combined |
|---------|-----------|-------------|----------|
| DIDComm encryption | X25519 ECDH | ML-KEM-768 (FIPS 203) | Hybrid KEM |
| Identity signatures | Ed25519 | ML-DSA-65 (FIPS 204) | Dual-signed |
| EVM transaction signing | secp256k1 | PQ-gated (Ed25519 + ML-DSA-65) | Two-cryptosystem barrier |
| Key derivation | — | HKDF-SHA-512 | 256-bit PQ security |
| Integrity chains | — | SHA-512 | 256-bit PQ collision resistance |

### DIDComm v2 Messaging

Agents exchange encrypted messages using hybrid KEM (X25519 + ML-KEM-768). Messages are routed over the libp2p P2P mesh or through the Nym mixnet for network-layer anonymity.

### EVM Wallet

Each agent holds a BIP-32 derived secp256k1 wallet for on-chain operations. Transaction signing requires dual-signature authorization (Ed25519 + ML-DSA-65) — an attacker must break both the EVM key AND the agent's post-quantum DID identity.

---

## The Observation Gap

There is a growing ecosystem of AI observability platforms: LangSmith, Helicone, Braintrust, AgentOps, Langfuse, Datadog LLM, Arize Phoenix, Lunary, and others. They offer dashboards, cost tracking, trace visualization, and evaluation frameworks. Some are excellent at what they do.

But they all share a common architectural assumption: **your agent's data leaves your machine.**

Every prompt, every tool call, every file your agent reads, every line of code it writes — streamed to a cloud endpoint, stored on infrastructure you don't control, governed by terms of service that can change. These platforms observe your agents by becoming another party in the chain of trust. They solve the visibility problem by creating a custody problem.

And none of them answer the harder question: *how do you know the record itself hasn't been changed?*

A cloud dashboard can show you what it claims happened. It cannot prove the log is complete. It cannot prove no events were removed. It cannot prove the trace you're reading today is the same trace that was written yesterday. The audit trail lives on someone else's server, and trust is a policy decision — not a mathematical property.

| | Cloud Observability | Agent H.A.L.O. |
|---|---|---|
| **Where traces live** | Vendor's cloud | Your machine |
| **Who can read them** | You + the vendor (+ their infra partners) | You |
| **Tamper evidence** | Trust the vendor's integrity | SHA-256 Merkle proofs — verify yourself |
| **Proof of completeness** | None | Monotone extension proofs (deletion is detectable) |
| **Data sovereignty** | Governed by vendor ToS | You own the bits |
| **Cryptographic seals** | None | Hash chain — each commit binds to all prior commits |
| **Works offline** | No | Yes |
| **Agent support** | Framework-specific SDKs | Wraps any CLI agent directly |
| **MCP native** | No | Yes — 20 native + proxied tools over HTTP, 11 tools over stdio |
| **Formal verification** | No | 131 Lean 4 modules with sheaf-theoretic proofs |

H.A.L.O. doesn't replace evaluation frameworks or cloud analytics for teams that want them. It provides the missing foundation: a **sovereign, tamper-evident record** that you control, that you can verify, and that exists whether or not you're online.

---

## The Cypherpunk Thesis

On February 22, 2026, Vitalik Buterin published *"Reclaiming the Cypherpunk Soul of the World Computer"* — a direct challenge to the direction of the blockchain industry and, more broadly, the emerging agentic economy.

His argument: the crypto ecosystem spent too long optimizing for speculation and institutional compliance while abandoning its founding mission — building open, private, decentralized infrastructure. Now, as AI agents begin managing the majority of digital transactions, the stakes are higher. If the agentic economy is built on proprietary, closed-source foundations, it becomes a new form of surveillance capitalism. The watchers accumulate power. The watched lose sovereignty.

Buterin's prescription is concrete: privacy by default through zero-knowledge proofs and fully homomorphic encryption. Censorship resistance at the protocol level (FOCIL). Account abstraction for quantum-resistant wallets. Open-source AI. And a five-year plan to rebuild Ethereum's core as a *"cypherpunk principled, non-ugly"* system — potentially accelerated by AI-assisted coding and verification.

**This is the world H.A.L.O. is built for.**

When Buterin warns about the agentic economy, he's describing a future where AI agents operate autonomously — managing finances, writing code, executing transactions, making decisions. In that world, the question isn't just *"what did the agent do?"* It's *"can you prove it, and can you prove no one changed the record?"*

H.A.L.O. answers that question with mathematics, not policy:

- **Privacy by default** — traces never leave your machine. No telemetry. No cloud dependency.
- **Cryptographic proof** — every event is Merkle-committed. Tampering breaks the chain.
- **Zero-knowledge compatible** — the NucleusDB engine includes Groth16 SNARK verification and on-chain trust attestation on Base L2. Agent identity can be verified without revealing private state.
- **Post-quantum ready** — ML-DSA-65 (FIPS 204, NIST Level 3) witness signatures protect against future quantum attacks.
- **Open source** — the full implementation, the formal specification, and the smart contracts are here in this repository.

### Where We're Going

We see H.A.L.O. as infrastructure for the cypherpunk agentic economy — not just an observability tool. The roadmap includes deeper integration with on-chain identity systems, decentralized agent trust networks, and privacy-preserving audit protocols that let agents prove compliance without exposing their operational history.

We are actively seeking community input on:

- **Decentralized trace anchoring** — publishing Merkle roots to on-chain transparency logs so third parties can verify trace integrity without accessing the traces themselves.
- **Agent-to-agent trust** — how should Agent B verify Agent A's operational history? What's the right trust protocol for autonomous agent collaboration?
- **Privacy-preserving audits** — ZK proofs that an agent's session met specific compliance criteria (cost bounds, no data exfiltration, no unauthorized file access) without revealing the session content.
- **Hardware-anchored identity** — PUF-based agent identity that binds an agent's cryptographic identity to the physical hardware it runs on, making identity theft computationally infeasible.

If these problems matter to you, [open an issue](https://github.com/Abraxas1010/agenthalo/issues), start a discussion, or reach out directly. The cypherpunk thesis only works if the community builds it together.

---

## AgentPMT Integration — Unified Tool Surface

<p align="center">
  <img src="assets/agentpmt_logo.svg" alt="AgentPMT" width="260"/>
</p>

[AgentPMT](https://www.agentpmt.com) is an MCP-native tool infrastructure platform providing budget-controlled access to 100+ third-party tools — Gmail, Stripe, Google Workspace, blockchain scanners, and more. H.A.L.O. integrates AgentPMT as a **tool proxy**, not a payment gateway.

### How It Works

**Agents see one unified tool surface.** When tool proxy is enabled, AgentPMT's tools appear alongside H.A.L.O.'s native tools in a single MCP `tools/list` response. Native tools appear as-is (`attest`, `audit_contract`, etc.). AgentPMT tools appear with an `agentpmt/` prefix (`agentpmt/gmail_send`, `agentpmt/stripe_charge`). The agent doesn't need to know which tools are native and which are proxied.

**Budget controls live on the AgentPMT side.** The human configures spending limits, credentials, and workflow permissions via the [AgentPMT dashboard](https://www.agentpmt.com). H.A.L.O. records every proxied tool call in its tamper-evident trace for cost tracking and auditability.

**AgentPMT evolves independently.** The tool catalog is stored as a separate JSON file (`~/.agenthalo/agentpmt_tools.json`) and can be refreshed without touching H.A.L.O.'s core. New tools appear in the agent's surface with a single `agenthalo config tool-proxy refresh`.

```bash
# Enable tool proxy
agenthalo config tool-proxy enable

# Set AgentPMT bearer token (or use AGENTPMT_BEARER_TOKEN env var)
agenthalo config set-agentpmt-key <token>

# Optional: set explicit AgentPMT MCP endpoint
agenthalo config tool-proxy endpoint https://testnet.api.agentpmt.com/mcp

# Refresh available tools from AgentPMT
agenthalo config tool-proxy refresh

# Check status
agenthalo config tool-proxy status
```

### H.A.L.O. Feature Gating

H.A.L.O.'s own features (attest, audit, trust, sign) are gated by the **CAB license system**, not by AgentPMT credits. P2PCLAW mints a Cryptographic Assurance Bundle after payment — NucleusDB verifies it locally against a baked-in foundation commitment. No phone-home. No license server. The math is the gatekeeper.

### Pricing

| Tier | Price | Capabilities |
|------|-------|-------------|
| **Free** | $0 | CLI, SQL, BinaryMerkle backend, Claude/Codex/Gemini wrapping |
| **Starter** | $49/month | + MCP server, TUI, custom agent wrapping |
| **Professional** | $149/month | + IPA backend, HTTP multi-tenant API |
| **Enterprise** | $499/month | + KZG backend, container runtime, on-chain attestation |

---

## x402direct — Stablecoin Payments for Agents

<p align="center">
  <a href="https://www.x402direct.org"><strong>x402direct.org</strong></a>
</p>

[x402direct](https://www.x402direct.org) is a peer-to-peer stablecoin payment protocol that uses HTTP 402 responses and UPC (Unified Payment Contract) smart contracts on Base. When an agent encounters a 402 Payment Required response, H.A.L.O. can handle the payment flow automatically.

### Integration

H.A.L.O. integrates x402direct at two levels:

**1. Via AgentPMT (recommended)** — The `agentpmt/x402_pay` tool handles the full payment lifecycle: detect 402 response, select payment option, check balance, execute UPC payment, submit proof, access resource. Zero agent-side code needed.

**2. Native validation** — The `x402_check` MCP tool parses and validates x402 payment requests locally. It checks protocol version, CAIP-10 addressing, known networks (Base mainnet and Base Sepolia), and token contract addresses — all without sending a transaction.

```bash
# Enable x402 integration
agenthalo x402 enable

# Configure UPC contract and network
agenthalo x402 config --network base-sepolia --upc-contract 0x...

# Check status
agenthalo x402 status

# Validate a 402 response body
echo '{"x402version":"direct.1.0.0","nonce":42,...}' | agenthalo x402 check
```

### Supported Networks

| Network | Chain ID | USDC Address |
|---------|----------|-------------|
| Base Mainnet | `eip155:8453` | `0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913` |
| Base Sepolia | `eip155:84532` | `0x036CbD53842c5426634e7929541eC2318f3dCF7e` |

### How x402 Works with H.A.L.O.

```
Agent requests protected resource
        ↓
Server returns 402 Payment Required (x402direct JSON)
        ↓
H.A.L.O. validates the payment request (x402_check)
        ↓
AgentPMT executes payment via UPC smart contract (x402_pay)
        ↓
H.A.L.O. records the payment in its tamper-evident trace
        ↓
Agent submits tx hash + nonce as proof → server grants access
```

Every x402 payment flows through H.A.L.O.'s trace, giving you a complete audit trail of autonomous agent spending — cryptographically sealed.

> **Protocol reference:** [x402direct-public](https://github.com/Apoth3osis-ai/x402direct-public) on GitHub.

---

## The Algebraic Foundation

Most databases describe their correctness properties in English. NucleusDB proves them in Lean 4 using the mathematics of sheaf theory — the same framework algebraic geometers use to describe how local observations compose into global structure.

This is not a marketing claim. It is 131 Lean 4 modules, type-checked by the Lean kernel, that formally prove the properties NucleusDB relies on. We are not aware of any other database — verifiable or otherwise — that provides this level of mathematical foundation.

### Why Sheaves

The core problem of multi-agent trust is a *local-to-global* problem. Each agent observes a local slice of reality: its own chain, its own traces, its own view of state. The question is: **can these local views be consistently assembled into a single global picture?**

This is exactly the question sheaf theory was invented to answer.

A *presheaf* assigns data to each "open set" (in our case, each chain or agent view) with restriction maps between them. A *sheaf* is a presheaf that satisfies a gluing condition: if local sections agree on overlaps, they can be assembled into a unique global section. If they can't — if the gluing condition fails — then someone's local view is inconsistent with the others. The sheaf condition is a tamper detector built from pure mathematics.

### What's Proved

**Sheaf coherence** — Local views of state compose into a globally consistent picture via amalgamation witnesses. A `CoherenceWitness` carries a matching family plus proof that amalgamation holds. If verification passes, the local views are mathematically consistent. If it fails, the state has been tampered with. (`lean/NucleusDB/Sheaf/Coherence.lean`)

**Chain transport** — Round-trip contracts between different chain representations. If you project a chain-local value to the shared space and decode it back, you get the original value (RT-1). If you decode a shared value and re-project, you get the original shared value (RT-2). These are proved, not assumed. Forward and backward transport between any two chains composes to the identity — `backward_forward` is a theorem, not a test. (`lean/NucleusDB/Sheaf/ChainTransport.lean`)

**Gluing conditions** — Two chain-local sections can be glued when their projections to the shared space agree. The glue operation is defined and its specification is proved: the glued value equals the shared projection of either local section. (`lean/NucleusDB/Sheaf/ChainGluing.lean`)

**Materialization functors** — The abstract relationship between protocol state and concrete vector representation satisfies naturality: if two states are related by transport, their materializations are equal. This ensures the commitment backends faithfully represent the abstract protocol. (`lean/NucleusDB/Sheaf/MaterializationFunctor.lean`)

**Multi-chain compliance presheaves** — Per-chain compliance sections modeled as a presheaf over the chain topology, with restriction maps that constrain sections to sub-topologies. Global compliance is assembled from local chain sections via the sheaf gluing machinery. (`lean/NucleusDB/TrustLayer/CompositeCab/Presheaf.lean`)

**Fork evidence** — Adversarial models formalized as mathematical objects. A fork is two signed checkpoints at the same height with the same predecessor but different state roots. Fork symmetry is a theorem: if `(a, b)` is a fork, then `(b, a)` is a fork. This sounds trivial — until you realize it means the fork detection protocol is order-independent, which is exactly what you need for decentralized verification. (`lean/NucleusDB/Adversarial/ForkEvidence.lean`)

### Why This Matters

Every other observability tool asks you to trust their implementation. Trust that the hash function was called. Trust that the Merkle tree was built correctly. Trust that the immutable mode actually rejects writes.

H.A.L.O. asks you to trust the Lean 4 kernel — a small, independently auditable proof checker — which has verified that the mathematical properties hold by construction. The implementation can have bugs. The proofs cannot. If the sheaf coherence theorem type-checks, then local views that pass verification are mathematically guaranteed to be globally consistent. This is not a claim. It is a proof.

```bash
# Verify the proofs yourself (requires Lean 4 toolchain)
lake build NucleusDB
```

The 131 modules cover:

| Domain | Modules | What's Proved |
|--------|---------|---------------|
| **Core** | Nucleus, Ledger, Invariants, Authorization, Certificates | Protocol invariants, authorization policies |
| **Security** | Assumptions, Parameters, Reductions, Refinement | Security reductions — if you break NucleusDB, you break SHA-256 |
| **Commitment** | VectorModel, Adapter | Backend abstraction, commitment correctness |
| **Sheaf** | Coherence, MaterializationFunctor, ChainTransport, ChainGluing | Local-to-global consistency, transport round-trips, gluing |
| **Transparency** | CT6962, Consistency, LogModel | RFC 6962 append-only tree, consistency proofs |
| **Adversarial** | ForkEvidence, Witness | Fork detection, witness validity |
| **Trust Layer** | Presheaf, GluingCondition, GlobalCab, SheafBridge | Multi-chain compliance, composite attestation |
| **Identity** | Genesis, DID, KeyPair, Ceremony | Agent identity lifecycle, key derivation |
| **Comms** | DIDComm, HybridKEM, P2P, ZK | Encrypted messaging, hybrid post-quantum KEM |
| **Crypto** | MLDSA, MLKEM, HashChain | Post-quantum signature/KEM correctness |
| **Epistemic** | TsallisEntropy, BayesianUpdate, UncertaintyTranslation | Diversity gauges, odds-update correctness, cross-calculus functor laws |
| **Contracts** | PaymentChannels, EVM | On-chain payment channel state transitions |

---

## NucleusDB

<p align="center">
  <img src="assets/nucleus_db_logo.png" alt="NucleusDB" width="260"/>
</p>

<p align="center">
  <strong>The verifiable database engine powering H.A.L.O.</strong><br>
  <em>Every write is a cryptographic commitment. Every query comes with a proof. Deletion can be made mathematically impossible.</em>
</p>

NucleusDB is the storage layer beneath H.A.L.O. — and a standalone verifiable database in its own right. It provides the cryptographic primitives that make tamper-evident traces possible: Merkle commitments, monotone seals, certificate transparency, and mathematically enforced immutability.

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

H.A.L.O. needs more than a database. It needs a database where the storage layer itself provides cryptographic guarantees:

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

### Vector Search

NucleusDB includes built-in kNN vector search — store embeddings alongside structured data and query by similarity:

- **Distance metrics**: cosine, L2 (Euclidean), inner product
- **Use case**: semantic memory recall for agents — store conversation embeddings, retrieve relevant context by meaning rather than keyword

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

**Streamable HTTP requirements (important):**
- Client `Accept` header must include **both** `application/json` and `text/event-stream`
- Client must persist and replay `mcp-session-id` from `initialize` on subsequent calls
- Typical flow: `initialize` -> `tools/list` / `tools/call` (same session id)

For automation and audits, use the built-in helper:

```bash
# 1) Initialize session and persist mcp-session-id
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:3000/mcp \
  init --session-file /tmp/mcp.session

# 2) List tools using the same session
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:3000/mcp \
  tools-list --session-file /tmp/mcp.session

# 3) Call a tool
python3 scripts/mcp_streamable_http.py \
  --endpoint http://127.0.0.1:3000/mcp \
  tools-call --session-file /tmp/mcp.session \
  --tool status
```

For a full orchestrator smoke run over real MCP HTTP:

```bash
scripts/orchestrator_mcp_smoke.sh
```

More details: `Docs/ops/mcp_streamable_http.md` and `Docs/ops/orchestrator_debugging_playbook.md`.

**Dual authentication** (CAB + OAuth 2.1):
- **CAB-as-bearer-token**: Hardware-anchored agent identity verified on-chain
- **OAuth 2.1 JWT**: Standard bearer tokens for non-attested agents

**Per-tool scope enforcement** — 27 tools across 5 security tiers:

| Scope | Tools | Auth Required |
|-------|-------|---------------|
| `read` | help, status, query, verify, export, history, evidence_combine, uncertainty_translate | Basic token |
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
Phase 5 hardening runbook: `Docs/AGENTHALO_ONCHAIN_PHASE5.md`.
Phase 5 scripts:
- `contracts/scripts/deploy_agenthalo_trust_base_sepolia.sh`
- `contracts/scripts/e2e_agenthalo_attestation_base_sepolia.sh`
- `contracts/scripts/verify_agenthalo_phase5_artifacts.py`

---

## Architecture

```
                          Agent H.A.L.O. + NucleusDB
  ┌─────────────────────────────────────────────────────────────┐
  │                                                             │
  │   H ─ Human-AI (trust boundary)   NucleusDB Core           │
  │     PUF fingerprint ──────────────▶ protocol.rs             │
  │     Hardware entropy ──────────────▶ immutable.rs            │
  │                                     sql/executor             │
  │   A ─ Agent (identity)              keymap.rs                │
  │     DID / Genesis seed ────────────▶ witness.rs (ML-DSA-65)  │
  │     EVM wallet (secp256k1) ────────▶ ct6962.rs (RFC 6962)   │
  │     Hybrid KEM (X25519+ML-KEM) ───▶ security.rs             │
  │                                     persistence (redb WAL)   │
  │   L ─ Lifecycle (traces)                                     │
  │     Claude ─────────────┐        Commitment Backends         │
  │     Codex ──────────────┤          vc/binary_merkle.rs       │
  │     Gemini ─────────────┼─ L ──▶   vc/ipa.rs                │
  │     OpenClaw ───────────┤          vc/kzg.rs                 │
  │     Shell ──────────────┘                                    │
  │                                                              │
  │   O ─ Orchestrator (coordination)                            │
  │     Agent pool ─────────┐        Client Surfaces             │
  │     Task DAG ───────────┤          CLI / REPL                │
  │     Budget enforcement ─┤          Web Dashboard + Cockpit   │
  │     Mesh networking ────┤          Terminal UI                │
  │     DIDComm v2 ─────────┘          MCP Server (stdio + HTTP) │
  │                                                              │
  │   Epistemic Calculi                                          │
  │     Tsallis diversity ──────────▶ metrics/diversity.rs       │
  │     Trust nucleus ──────────────▶ trust.rs (EpistemicTrust)  │
  │     Bayesian evidence ──────────▶ evidence.rs (vUpdate)      │
  │     Uncertainty translation ────▶ uncertainty.rs             │
  │     Trace topology (H0) ────────▶ trace_topology.rs          │
  │                                                              │
  ├──────────────────────────────────────────────────────────────┤
  │                                                              │
  │   On-Chain Trust (Base L2)       Formal Spec (Lean 4)        │
  │     TrustVerifier.sol              131 modules               │
  │     TrustVerifierMultiChain.sol    Sheaf coherence,          │
  │     Groth16VerifierAdapter.sol     Chain transport/gluing,   │
  │     circuits/ (circom)             Fork evidence, Identity,  │
  │                                    Comms, PaymentChannels    │
  │                                                              │
  └──────────────────────────────────────────────────────────────┘
```

**184 Rust source files** | **79,000+ lines** | **7,500+ lines of tests** | **20 Solidity contracts** | **131 Lean 4 modules**

## Security

### Cryptographic Primitives

| Layer | Primitive | Security Level |
|-------|-----------|---------------|
| State commitments | SHA-256 Merkle tree | 128-bit classical, post-quantum safe |
| Witness signatures | ML-DSA-65 (FIPS 204) | NIST PQ Level 3 |
| DIDComm encryption | X25519 + ML-KEM-768 (FIPS 203) | Hybrid classical + PQ |
| Identity signatures | Ed25519 + ML-DSA-65 | Dual classical + PQ |
| Key derivation | HKDF-SHA-512 | 256-bit PQ security |
| Integrity chains | SHA-512 | 256-bit PQ collision resistance |
| Monotone seals | SHA-256 hash chain | 128-bit preimage resistance |
| Transparency proofs | RFC 6962 (SHA-256) | 128-bit collision resistance |
| License verification | Groth16 over BN254 | 128-bit classical pairing security |
| EVM signing | secp256k1 (PQ-gated) | Two-cryptosystem barrier |

### Immutable Mode Guarantees

When `APPEND_ONLY` is active:

- **SQL layer**: UPDATE and DELETE are rejected before execution.
- **Protocol layer**: Every commit verifies that no existing non-zero value was changed (raw index check) and no named key was removed (keymap check).
- **Seal chain**: Each commit appends `seal_n = SHA-256("NucleusDB.MonotoneSeal|" || seal_{n-1} || kv_digest_n)`. The chain is unforgeable.
- **CT tree**: The append-only Merkle tree independently records every commit.
- **Persistence**: The AppendOnly lock and seal chain survive snapshot save/load and WAL replay.

## Testing

875 tests passing (2026-03-06 snapshot), 0 failures, 0 warnings:

```bash
cargo test                        # 836 Rust tests
cd contracts && forge test        # 39 Solidity tests
```

| Suite | Tests |
|-------|-------|
| Rust (unit + integration + binary tests) | 836 |
| Solidity (Foundry) | 39 |
| **Total** | **875** |

## Known Limitations

- NucleusDB's SQL surface is a focused subset (single virtual table `data` with `key`/`value` columns), not a general-purpose SQL engine.
- The `ipa` backend carries full-vector opening payloads (not logarithmic-size IPA arguments).
- The KZG backend's default trusted setup is for development/demo use. Production KZG deployments require externally managed ceremony artifacts.
- Sheaf coherence checks are local-view oriented, not full global-state reconciliation.
- H.A.L.O. cloud sync is planned but not yet implemented; traces are currently local-only.

## License

[Apoth3osis License Stack v1](LICENSE.md)

## Citation

```bibtex
@software{agenthalo,
  title = {Agent H.A.L.O.},
  author = {Apoth3osis},
  year = {2025--2026},
  url = {https://github.com/Abraxas1010/agenthalo},
  license = {Apoth3osis License Stack v1}
}
```
