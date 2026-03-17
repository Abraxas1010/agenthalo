<img src="assets/Apoth3osis.webp" alt="Apoth3osis Logo" width="140"/>

<sub>Our tech stack is ontological: <strong>Hardware — Physics</strong>, <strong>Software — Mathematics</strong></sub>

---

<p align="center">
  <img src="assets/agent_halo_logo.png" alt="Agent H.A.L.O." width="300"/>
</p>

<p align="center">
  <strong>Agent H.A.L.O.</strong> — Human-AI Agent Lifecycle Orchestrator<br>
  <em>Sovereign identity, post-quantum communication, tamper-proof observability, and verifiable storage for AI agents.</em>
</p>

[![License: Apoth3osis License Stack v1](https://img.shields.io/badge/License-Apoth3osis%20License%20Stack%20v1-blue.svg)](LICENSE.md)
![Rust](https://img.shields.io/badge/Rust-stable-orange.svg)
![Lean 4](https://img.shields.io/badge/Lean%204-formal%20proofs-blue.svg)

[Overview](#overview) · [Quick Start](#quick-start) · [Platform Components](#platform-components) · [NucleusDB](#nucleusdb) · [AgentPMT](#agentpmt) · [P2PCLAW](#p2pclaw) · [Security](#security) · [Formal Verification](#formal-verification) · [Architecture](#architecture)

<sub>Part of the <a href="https://www.apoth3osis.io/projects"><strong>MENTAT</strong></a> stack — Layer 1 foundation.</sub>

---

## Overview

Agent H.A.L.O. is a sovereign agent platform. It gives AI agents a cryptographic identity, quantum-resistant communication, and tamper-proof observability — all running locally on your machine.

**Identity.** Each agent derives a DID (Decentralized Identifier) from a genesis seed ceremony. The DID document carries Ed25519, X25519, ML-KEM-768, and ML-DSA-65 public keys — classical and post-quantum cryptography side by side.

**Observability.** AgentHALO wraps AI coding agent CLIs (Claude Code, Codex, Gemini) and records every event — thoughts, tool calls, file edits, token counts, and costs — into a local NucleusDB trace store. Every trace event is content-addressed with a SHA-512 Merkle proof. If any event is modified after the fact, the proof chain breaks.

**Communication.** Agents exchange DIDComm v2 encrypted messages using a hybrid KEM (X25519 + ML-KEM-768) resistant to both classical and quantum adversaries. Messages route over a libp2p P2P mesh or through the Nym mixnet for network-layer anonymity.

**Economics.** Agents hold an EVM wallet (secp256k1, derived via BIP-32) for on-chain operations. EVM transaction signing is gated by DIDComm-verified dual-signature authorization (Ed25519 + ML-DSA-65), creating a two-cryptosystem barrier.

**Key properties:**

- **Zero telemetry.** Nothing leaves your machine. No analytics, no tracking, no phone-home.
- **Zero config.** `agenthalo run claude` auto-injects the right flags for structured output.
- **Tamper-evident.** Content-addressed storage in NucleusDB with Merkle proofs (SHA-512).
- **Post-quantum.** Hybrid KEM (X25519 + ML-KEM-768) for DIDComm, ML-DSA-65 for signatures, HKDF-SHA-512 for key derivation.
- **Sovereign identity.** DID-based identity with dual classical/PQ key pairs, genesis seed ceremony, and append-only identity ledger.
- **Agent-native.** Parses each agent's native structured output format.

## Quick Start

### One-Line Install

```bash
curl -fsSL https://raw.githubusercontent.com/Abraxas1010/agenthalo/master/install.sh | bash
```

### Build from Source

```bash
git clone https://github.com/Abraxas1010/agenthalo.git
cd agenthalo
cargo build --release
```

This produces 7 binaries:

| Binary | Purpose |
|--------|---------|
| `agenthalo` | Main CLI — wrap agents, manage identity, sign, attest, govern, mesh, ZK, dashboard |
| `agenthalo-mcp-server` | MCP server for the full AgentHALO tool surface |
| `nucleusdb` | NucleusDB CLI — database creation, SQL, export, MCP, dashboard |
| `nucleusdb-server` | Multi-tenant HTTP API for NucleusDB |
| `nucleusdb-mcp` | Standalone NucleusDB MCP server (stdio + HTTP) |
| `nucleusdb-tui` | Terminal UI for NucleusDB |
| `nucleusdb-discord` | Discord recorder and slash-command bot |

### First Run

```bash
# Wrap an AI agent with full observability
agenthalo run claude

# Or launch the dashboard
agenthalo dashboard --port 3100
```

## Platform Components

### Agent Wrapping and Observability

AgentHALO wraps Claude Code, Codex, and Gemini CLI, recording structured traces into NucleusDB:

```bash
agenthalo run claude          # wrap Claude Code
agenthalo run codex           # wrap Codex CLI
agenthalo run gemini          # wrap Gemini CLI
agenthalo traces              # list recorded sessions
agenthalo costs               # aggregate cost tracking
agenthalo export <session>    # export session data
```

### Identity and Cryptography

```bash
agenthalo genesis             # genesis seed ceremony
agenthalo identity            # manage DID identity
agenthalo keygen              # generate Ed25519 + ML-DSA-65 + ML-KEM-768 keys
agenthalo sign <payload>      # dual classical/PQ signature
agenthalo vault               # encrypted provider-key storage
agenthalo crypto              # cryptographic operations
```

### Attestation and Trust

```bash
agenthalo attest <session>    # create content-addressed attestation with ZK proof
agenthalo audit <contract>    # audit a smart contract
agenthalo trust               # query trust score
agenthalo vote                # governance voting
agenthalo governor            # governor policy management
```

### Mesh Communication

```bash
agenthalo mesh                # P2P mesh operations (libp2p + hybrid KEM)
agenthalo comms               # DIDComm v2 encrypted messaging
agenthalo nym                 # Nym mixnet integration
agenthalo privacy             # privacy controller settings
```

### On-Chain Operations

```bash
agenthalo wallet              # EVM wallet management (BIP-32 derived)
agenthalo onchain             # deploy/interact with on-chain contracts
agenthalo x402                # x402 payment protocol
agenthalo deploy              # deploy TrustVerifier contracts
```

### Orchestration

```bash
agenthalo agents              # manage agent pool
agenthalo access              # pod access control and capabilities
agenthalo zk                  # ZK compute (RISC Zero guests: range proofs, set membership, secure aggregation)
agenthalo proof-gate          # formal verification gate status
```

### Cockpit

The cockpit provides a browser-based terminal environment with WebSocket-backed PTY sessions, deploy management, and real-time session monitoring.

## NucleusDB

NucleusDB is a verifiable database engine with three properties usually split across separate systems:

- A mutable working database with SQL, typed values, blob storage, vector search, and multi-tenant HTTP access
- A proof surface where queries come with cryptographic commitment proofs
- An immutable append-only mode with monotone seal chaining for audit logs and permanent records

```bash
# Create and configure
nucleusdb create --db ./records.ndb --backend merkle
printf 'SET MODE APPEND_ONLY;\n' | nucleusdb sql --db ./records.ndb

# SQL interface
nucleusdb sql --db ./records.ndb
```

```sql
INSERT INTO data (key, value) VALUES ('temperature', 42);
COMMIT;
SELECT key, value FROM data WHERE key = 'temperature';
VERIFY 'temperature';
```

The MCP surface exposes 16 tools (core database + Discord) over stdio and streamable HTTP transports.

Full reference: [Docs/ARCHITECTURE.md](Docs/ARCHITECTURE.md).

## AgentPMT

<p align="center">
  <img src="assets/agentpmt_logo.svg" alt="AgentPMT" width="200"/>
</p>

AgentPMT is an MCP-native tool infrastructure platform providing budget-controlled access to 100+ third-party tools (Gmail, Stripe, Google Workspace, blockchain scanners, and more).

AgentHALO integrates AgentPMT as a tool proxy: wrapped agents discover and call AgentPMT tools through a unified MCP `tools/list`. Native AgentHALO tools appear as-is (`attest`, `audit_contract`, etc.), while AgentPMT tools appear with an `agentpmt/` prefix (e.g., `agentpmt/gmail_send`, `agentpmt/stripe_charge`). Budget controls and credentials live on the AgentPMT side. AgentHALO records all tool calls in the trace for cost tracking and observability.

```bash
agenthalo addon agentpmt status    # check integration status
agenthalo addon agentpmt enable    # enable tool proxy
```

## P2PCLAW

P2PCLAW is a decentralized publishing and verification network for research papers. Agents publish, validate, and retrieve papers through HMAC-authenticated API calls to the P2PCLAW gateway.

```bash
agenthalo addon p2pclaw status     # connection and swarm status
agenthalo addon p2pclaw publish    # publish a paper to the hive
agenthalo addon p2pclaw validate   # validate a paper
agenthalo addon p2pclaw search     # search the paper swarm
```

Features:
- Tiered access (tier1/tier2) with HMAC-SHA256 request signing
- Vault-first credential storage with insecure fallback for development
- Swarm status monitoring (agent count, paper count, mempool depth)
- Paper lifecycle: submit, validate, search, retrieve

## Discord Bot

`nucleusdb-discord` records messages into an append-only NucleusDB instance and exposes verification/search slash commands.

```bash
export NUCLEUSDB_DISCORD_TOKEN=...
export NUCLEUSDB_DISCORD_DB_PATH=./discord_records.ndb
./target/release/nucleusdb-discord
```

Slash commands: `/status`, `/verify`, `/search`, `/history`, `/export`, `/channels`, `/integrity`.

The bot batches writes by message count or timeout. On startup it backfills channel history from the last recorded message, then resumes live recording. Edits and deletes are logged as new immutable facts.

## Smart Contracts

Solidity contracts for on-chain trust verification:

- `TrustVerifier.sol` — on-chain attestation verification
- `TrustVerifierMultiChain.sol` — cross-chain attestation queries
- `Groth16VerifierAdapter.sol` — ZK proof verification adapter
- `CrossChainAttestationQuery.sol` — cross-chain attestation query surface

Deploy and test via Foundry (`contracts/foundry.toml`).

## Dashboard

The web dashboard surfaces all platform layers:

- **Overview** — system status and agent activity
- **Genesis** — genesis seed and entropy state
- **Identity** — DID document, key material, identity ledger
- **Security** — cryptographic surfaces and proof gate status
- **NucleusDB** — database operations, queries, verification
- **Discord** — recorder status, channel monitoring
- **Sessions** — trace explorer with cost tracking
- **Cockpit** — PTY terminal sessions and deploy management

The CRT visual language (scanlines, grain, terminal color contrast) is intentional product identity.

## Formal Verification

AgentHALO bridges runtime operations to machine-checked Lean 4 proofs maintained in the [Heyting](https://github.com/Abraxas1010/heyting) repository. Three layers:

1. **Provenance surfaces** — five Rust modules export `formal_provenance()` linking 22 runtime operations to canonical Heyting theorem FQNs and 19 local Lean mirror paths
2. **Proof gate** — `configs/proof_gate.json` defines 14 theorem requirements across 6 tool surfaces, each bound to an exact declaration-line SHA-256, Heyting commit hash, and Ed25519 signature requirement
3. **Certificate pipeline** — `.lean4export` signed provenance attestations validated by `src/verifier/` against statement hash, commit hash, and signature

Current status: enforced mode. Use `AGENTHALO_PROOF_GATE_SKIP=1` only for explicit development escape-hatch sessions.

```bash
python3 scripts/check_theory_boundary.py               # verify approved math boundary
./scripts/validate_formal_provenance.sh                # namespace-aware FQN resolution
./scripts/generate_proof_certificates.sh               # generate + sign certificates
cargo run --bin nucleusdb -- verify-certificate <file>  # verify a certificate
```

Local Lean mirrors: `lean/NucleusDB/` (self-contained).

Full details: [Docs/FORMAL_VERIFICATION.md](Docs/FORMAL_VERIFICATION.md).

## Security

### Post-Quantum Cryptography

| Surface | Classical | Post-Quantum | Combined |
|---------|-----------|-------------|----------|
| DIDComm authcrypt/anoncrypt | X25519 ECDH | ML-KEM-768 (FIPS 203) | Hybrid KEM |
| DIDComm mesh transport | X25519 ECDH | ML-KEM-768 (FIPS 203) | Hybrid KEM |
| Identity signatures | Ed25519 | ML-DSA-65 (FIPS 204) | Dual-signed |
| KEM key derivation | — | HKDF-SHA-512 | 256-bit PQ security |
| Identity ledger hash chain | — | SHA-512 | 256-bit PQ collision |
| Attestation Merkle tree | — | SHA-512 | 256-bit PQ collision |
| EVM transaction signing | secp256k1 ECDSA | PQ-gated (Ed25519 + ML-DSA-65) | Two-cryptosystem barrier |

### Operational

- SHA-256 content sealing for Discord message records
- Certificate-transparency style roots for commit history
- Witness signatures on commits
- Append-only seal chaining via `immutable.rs`
- AES-GCM encrypted local files for identity/genesis/vault state
- Argon2-based password-derived master keys
- Ed25519-signed formal provenance certificates (enforced by default)
- ZK compute: RISC Zero guests for range proofs, set membership, secure aggregation, algorithm compliance
- Native operator/subsidiary orchestration guidance: [Docs/container_operator_security.md](Docs/container_operator_security.md)

## Architecture

```text
                        ┌──────────────┐
                        │  AI Agents   │
                        │ Claude/Codex │
                        │   /Gemini    │
                        └──────┬───────┘
                               │
                        ┌──────▼───────┐
                        │  agenthalo   │  CLI wrapper + orchestrator
                        │   (main)     │
                        └──────┬───────┘
               ┌───────────────┼───────────────┐
               │               │               │
        ┌──────▼──────┐ ┌─────▼──────┐ ┌──────▼──────┐
        │  Identity   │ │   Comms    │ │  Economics  │
        │ DID/Genesis │ │  DIDComm   │ │ EVM Wallet  │
        │ PQ Keygen   │ │ libp2p/Nym │ │ x402/Trust  │
        └──────┬──────┘ └─────┬──────┘ └──────┬──────┘
               │               │               │
        ┌──────▼───────────────▼───────────────▼──────┐
        │                 NucleusDB                    │
        │  Verifiable DB · SQL · Merkle · Append-only  │
        └──────┬──────────────┬───────────────┬───────┘
               │              │               │
        ┌──────▼──────┐ ┌────▼─────┐  ┌──────▼──────┐
        │  AgentPMT   │ │ P2PCLAW  │  │   Discord   │
        │ Tool Proxy  │ │ Research │  │  Recorder   │
        │ 100+ tools  │ │ Publish  │  │  Slash Cmds │
        └─────────────┘ └──────────┘  └─────────────┘
               │              │               │
        ┌──────▼──────────────▼───────────────▼───────┐
        │              Smart Contracts                 │
        │  TrustVerifier · Groth16 · CrossChain        │
        └──────┬──────────────────────────────────────┘
               │
        ┌──────▼──────┐
        │   Formal    │     Lean 4 proofs (Heyting)
        │ Verification│     22 provenance surfaces
        │  Proof Gate │     14 gated requirements
        └─────────────┘
```

Core module map:

- `src/halo/` — identity, attestation, trust, governance, crypto, DIDComm, mesh, PQ, EVM
- `src/halo/agentpmt.rs` — AgentPMT tool proxy integration
- `src/halo/p2pclaw.rs` — P2PCLAW publishing and verification client
- `src/orchestrator/` — agent pool, task graph, A2A bridge, container dispatch
- `src/cockpit/` — PTY manager, WebSocket bridge, deploy, sessions
- `src/swarm/` — content-addressed chunk engine (bitswap, manifests)
- `src/pod/` — access policies, capabilities, DID ACL bridge
- `src/comms/` — DIDComm sessions, encrypted envelopes
- `src/commitment/` — commitment scheme core
- `src/trust/` — composite CAB, on-chain trust
- `src/container/` — agent hookup, mesh coordination, launcher
- `src/pcn/` — payment channel network adapter
- `src/puf/` — physical unclonable function server
- `src/halo/zk_guests/` — RISC Zero ZK circuits
- `src/protocol.rs` — commits, proofs, witness signatures, seal chaining
- `src/sql/` — parser and executor
- `src/persistence.rs` — snapshots plus WAL
- `src/verifier/` — certificate parser, proof gate, Ed25519 verification
- `src/transparency/` / `src/vc/` / `src/sheaf/` — formal provenance surfaces
- `src/mcp/` — MCP tool surface
- `src/dashboard/` — web dashboard
- `src/discord/` — recorder, recovery, slash commands
- `contracts/` — Solidity contracts (TrustVerifier, Groth16, CrossChain)

Full module map: [Docs/ARCHITECTURE.md](Docs/ARCHITECTURE.md). Platform reference: [Docs/AGENTHALO.md](Docs/AGENTHALO.md).

## Native Ops

```bash
./scripts/agenthalo-instances.sh list
./scripts/agenthalo-instances.sh start-dev       # review dashboard (isolated home)
./scripts/agenthalo-instances.sh start-discord   # persistent Discord recorder
./scripts/agenthalo-instances.sh stop-dev
./scripts/agenthalo-instances.sh stop-discord
```

Runtime state lives under `~/.agenthalo-runtimes/`.

## Testing

```bash
cargo test
```

Test suites cover: end-to-end flows, SQL, keymaps, persistence compatibility, CLI smoke, Discord recording, formal integration (provenance surfaces, gate config, certificates), dashboard, HALO integration, mesh simulation, P2PCLAW integration, PCN, PUF, governance falsifiability, VCS, memory recall, and theory boundary enforcement.

## Repository Layout

- `src/` — Rust implementation (~98K lines)
- `src/halo/` — AgentHALO platform layer (identity, crypto, comms, governance, trust)
- `src/orchestrator/` — multi-agent orchestration and A2A
- `src/cockpit/` — browser-based PTY and deploy management
- `contracts/` — Solidity smart contracts
- `dashboard/` — embedded frontend assets
- `deploy/` — systemd units and environment templates
- `configs/` — proof gate configuration
- `lean/` — Lean 4 mirror modules (NucleusDB provenance)
- `python/` — Python utilities
- `scripts/` — provenance validation, certificate generation, instance management
- `tests/` — integration and regression tests (19 test files)
- `wdk-sidecar/` — WDK sidecar service
- `artifacts/` — trusted setup artifacts

## License

Released under the Apoth3osis License Stack v1. See [LICENSE.md](LICENSE.md) and [licenses/](licenses/).

## Citation

See [CITATION.cff](CITATION.cff).
