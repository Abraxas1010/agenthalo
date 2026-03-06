<p align="center">
  <img src="../assets/agent_halo_logo.png" alt="Agent H.A.L.O." width="240"/>
</p>

<h1 align="center">Agent H.A.L.O. Reference Guide</h1>

<p align="center">
  <strong>H</strong>uman-AI <strong>A</strong>gent <strong>L</strong>ifecycle <strong>O</strong>rchestrator<br>
  <em>Local-first observability for AI coding agents. Tamper-proof session recording backed by NucleusDB.</em>
</p>

---

## Table of Contents

- [Overview](#overview)
- [Installation](#installation)
- [Authentication](#authentication)
- [Recording Sessions](#recording-sessions)
- [Inspecting Traces](#inspecting-traces)
- [Cost Tracking](#cost-tracking)
- [Shell Wrapping](#shell-wrapping)
- [Supported Agents](#supported-agents)
- [Web Dashboard](#web-dashboard)
- [Doctor Command](#doctor-command)
- [Configuration](#configuration)
- [Environment Variables](#environment-variables)
- [Pricing Tables](#pricing-tables)
- [Trace Schema](#trace-schema)
- [Architecture](#architecture)
- [Security](#security)
- [Troubleshooting](#troubleshooting)

---

## Overview

AgentHALO is a sovereign agent platform: it gives AI agents a cryptographic identity, quantum-resistant communication, and tamper-proof observability — all running locally on your machine.

At the observability layer, AgentHALO wraps AI coding agent CLIs (Claude Code, Codex, Gemini) and records every event — thoughts, tool calls, file edits, token counts, and costs — into a local NucleusDB trace store. Every trace event is a content-addressed blob with a SHA-512 Merkle proof (SHA-256 for legacy entries). If any event is modified after the fact, the proof chain breaks.

At the identity layer, each agent derives a DID (Decentralized Identifier) from a genesis seed ceremony. The DID document carries Ed25519, X25519, ML-KEM-768, and ML-DSA-65 public keys — classical and post-quantum cryptography side by side.

At the communication layer, agents exchange DIDComm v2 encrypted messages using a hybrid KEM (X25519 + ML-KEM-768) that is resistant to both classical and quantum adversaries. Messages are routed over a libp2p P2P mesh or through the Nym mixnet for network-layer anonymity.

At the economic layer, agents hold an EVM wallet (secp256k1, derived via BIP-32) for on-chain operations. EVM transaction signing is gated by a DIDComm-verified dual-signature authorization (Ed25519 + ML-DSA-65), creating a two-cryptosystem barrier: an attacker must break both secp256k1 AND the agent's post-quantum DID identity to forge a transaction.

**Key properties:**

- **Zero telemetry.** Nothing leaves your machine. No analytics, no tracking, no phone-home.
- **Zero config.** `agenthalo run claude` auto-injects the right flags for structured output.
- **Tamper-evident.** Content-addressed storage in NucleusDB with Merkle proofs (SHA-512).
- **Post-quantum.** Hybrid KEM (X25519 + ML-KEM-768) for DIDComm, ML-DSA-65 for signatures, HKDF-SHA-512 for key derivation. No CRITICAL or MEDIUM quantum vulnerabilities remain in AgentHALO-controlled code.
- **Sovereign identity.** DID-based identity with dual classical/PQ key pairs, genesis seed ceremony, and append-only identity ledger.
- **Agent-native.** Parses each agent's native structured output format.

### Post-Quantum Cryptography Summary

AgentHALO's PQ hardening protects all agent-controlled cryptographic surfaces:

| Surface | Classical Crypto | PQ Crypto | Combined |
|---------|-----------------|-----------|----------|
| DIDComm authcrypt/anoncrypt | X25519 ECDH | ML-KEM-768 (FIPS 203) | Hybrid KEM |
| DIDComm mesh transport | X25519 ECDH | ML-KEM-768 (FIPS 203) | Hybrid KEM |
| Identity signatures | Ed25519 | ML-DSA-65 (FIPS 204) | Dual-signed |
| KEM key derivation | — | HKDF-SHA-512 | 256-bit PQ security |
| Identity ledger hash chain | — | SHA-512 | 256-bit PQ collision |
| Attestation Merkle tree | — | SHA-512 | 256-bit PQ collision |
| EVM transaction signing | secp256k1 ECDSA | PQ-gated (Ed25519 + ML-DSA-65 authorization) | Two-cryptosystem barrier |
| Gossipsub discovery | Ed25519 + ML-DSA-65 signed | Addresses stripped (DHT-only) | Metadata minimized |

Three upstream dependencies remain quantum-vulnerable and cannot be fixed unilaterally:

| Dependency | Vulnerability | Impact | Mitigation |
|------------|--------------|--------|------------|
| libp2p Noise XX (X25519) | Transport decryption | Metadata only (no DIDComm content) | Awaiting PQ Noise variants |
| Nym Sphinx (X25519) | Traffic deanonymization | Communication patterns (not content) | DIDComm hybrid KEM protects payload |
| Ethereum ECDSA (secp256k1) | Key recovery | Ecosystem-wide | PQ-gated signing reduces unilateral risk |

See `Docs/ops/pq_mesh_hardening.md`, `Docs/ops/pq_nym_assessment.md`, and `Docs/ops/pq_evm_assessment.md` for detailed assessments.

## Installation

### One-Line Install (Recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/Abraxas1010/agenthalo/master/install.sh | bash
```

Detects your OS and architecture, downloads the binary, and adds it to `~/.local/bin`.

### Build from Source

```bash
git clone https://github.com/Abraxas1010/agenthalo.git
cd agenthalo
cargo install --path . --bin agenthalo
```

### First Run

```bash
# Interactive wizard — guides you to dashboard, CLI, or MCP workflow
agenthalo setup

# Check everything is working
agenthalo doctor

# Launch the web dashboard
agenthalo dashboard
```

Verify:

```bash
agenthalo version
# agenthalo 0.3.0
```

## Authentication

AgentHALO requires authentication before recording. Three options:

### GitHub OAuth (recommended)

```bash
agenthalo login github
```

Opens a browser for GitHub OAuth. Credentials are saved to `~/.agenthalo/credentials.json` with owner-only permissions (0600).

### Google OAuth

```bash
agenthalo login google
```

### API Key

```bash
# Interactive (key not exposed in shell history)
agenthalo config set-key

# Scripted (key visible in process list — use with caution)
agenthalo config set-key sk-your-key-here
```

### Environment Variable

```bash
export AGENTHALO_API_KEY=sk-your-key-here
```

When `AGENTHALO_API_KEY` is set, it takes precedence over saved credentials. Useful for CI/CD.

### Verify Authentication

```bash
agenthalo config show
# AGENTHALO_HOME=/home/user/.agenthalo
# DB_PATH=/home/user/.agenthalo/traces.ndb
# CREDENTIALS=/home/user/.agenthalo/credentials.json
# PRICING=/home/user/.agenthalo/pricing.json
# AUTHENTICATED=true
```

## Recording Sessions

### Basic Usage

```bash
# Run Claude Code with recording
agenthalo run claude -p "explain this function" --allowedTools ""

# Run Codex
agenthalo run codex exec "write tests for auth.rs"

# Run Gemini CLI
agenthalo run gemini -p "find performance issues"
```

AgentHALO automatically:
1. Detects the agent type from the command name
2. Injects flags for structured output (unless you already passed them)
3. Spawns the agent as a subprocess
4. Tees stdout/stderr (you see everything in real time)
5. Parses the structured output stream into trace events
6. Records events into `~/.agenthalo/traces.ndb`
7. Forwards SIGINT/SIGTERM to the child process

### Auto-Injected Flags

| Agent | Flags Injected | Purpose |
|-------|---------------|---------|
| Claude | `--output-format stream-json --verbose` | Enables NDJSON event stream |
| Codex | `--json` | Enables JSON output mode |
| Gemini | `--output-format stream-json` | Enables NDJSON event stream |

If you already pass any of these flags, AgentHALO won't duplicate them.

### Exit Behavior

AgentHALO preserves the agent's exit code. If the agent exits with code 1, `agenthalo run` also exits with code 1 — after recording the session summary.

```bash
agenthalo run claude -p "fix the bug"
echo $?  # same as claude's exit code
```

On completion, a summary line is printed:

```
Recorded session sess-1740000000-12345 events=47 cost=$3.2100
```

## Inspecting Traces

### List All Sessions

```bash
agenthalo traces
```

```
 Session ID              | Agent  | Model           | Tokens   | Cost    | Duration | Status
-------------------------+--------+-----------------+----------+---------+----------+-----------
 sess-1740000000-12345   | claude | claude-opus-4-6 | 142,800  | $14.82  | 8m 32s   | completed
 sess-1740000100-12346   | codex  | o4-mini         | 23,400   | $0.12   | 1m 5s    | completed
 sess-1740000200-12347   | claude | claude-opus-4-6 | 0        | $0.00   | 0s       | failed
```

### Session Detail

```bash
agenthalo traces sess-1740000000-12345
```

```
Session: sess-1740000000-12345
Agent: claude
Model: claude-opus-4-6
Status: Completed
Started: 2026-02-24 04:00:00 UTC
Ended: 2026-02-24 04:08:32 UTC
Tokens in/out: 98200/44600
Cost: $14.8200
Duration: 512s

Event timeline:
      1  BashCommand       {"command":"claude","args":["--output-format","stream-json",...]}
      2  AssistantMessage   {"text":"I'll start by reading the authentication module..."}
      3  McpToolCall        {"tool":"Read","input":{"file_path":"/src/auth.rs"}}
      4  McpToolResult      {"result":"...content..."}
      ...
```

## Cost Tracking

### Session Costs

Costs are computed per-event using token counts from the agent's structured output and model-specific pricing tables.

```bash
agenthalo costs
```

```
 Bucket      | Sessions | Tokens  | Cost
-------------+----------+---------+---------
 2026-02-24  | 5        | 284,200 | $31.42
 2026-02-23  | 12       | 891,000 | $104.55
```

### Monthly Rollup

```bash
agenthalo costs --month
```

```
 Bucket      | Sessions | Tokens    | Cost
-------------+----------+-----------+----------
 2026-02     | 47       | 2,184,000 | $248.30
 2026-01     | 31       | 1,442,000 | $168.90
TOTAL: sessions=78 tokens=3,626,000 cost=$417.2000
```

## Shell Wrapping

Shell wrapping adds aliases to your shell RC file so that running `claude` transparently invokes `agenthalo run claude`.

### Wrap All Agents

```bash
agenthalo wrap --all
# Wrapped claude/codex/gemini in /home/user/.bashrc
```

This adds lines like:

```bash
# agenthalo: claude
alias claude='agenthalo run claude'
```

### Wrap a Single Agent

```bash
agenthalo wrap claude
```

### Remove Wrapping

```bash
agenthalo unwrap --all
# or
agenthalo unwrap claude
```

Removal cleanly strips only the AgentHALO-managed alias lines. Your RC file is otherwise untouched.

## Supported Agents

| Agent | Command | Structured Output | Adapter |
|-------|---------|-------------------|---------|
| Claude Code | `claude` | `stream-json` (NDJSON) | `ClaudeAdapter` |
| Codex | `codex` | `--json` (JSON) | `CodexAdapter` |
| Gemini CLI | `gemini` | `stream-json` (NDJSON) | `GeminiAdapter` |
| Custom | any | raw stdout lines | `GenericAdapter` |

### Custom/Generic Agents

Custom agent wrapping is gated behind the paid tier:

```bash
# Enable custom agents
export AGENTHALO_ALLOW_GENERIC=1

# Now any command works
agenthalo run my-custom-agent --flag value
```

Without this flag, unrecognized agent commands are rejected.

The `GenericAdapter` captures every stdout line as a `RawOutput` event. No structured parsing is performed. Token counting and cost tracking require the agent to emit parseable output.

## Configuration

### File Locations

| File | Path | Purpose |
|------|------|---------|
| Home directory | `~/.agenthalo/` | All state |
| Trace database | `~/.agenthalo/traces.ndb` | Session + event storage (NucleusDB) |
| Credentials | `~/.agenthalo/credentials.json` | OAuth tokens / API key (mode 0600) |
| Pricing table | `~/.agenthalo/pricing.json` | Model cost table (auto-generated) |
| AgentPMT config | `~/.agenthalo/agentpmt.json` | Tool proxy enabled/disabled, budget tag |
| AgentPMT catalog | `~/.agenthalo/agentpmt_tools.json` | Cached tool catalog from AgentPMT |
| x402 config | `~/.agenthalo/x402.json` | x402direct integration settings (UPC contract, network, auto-approve limit) |
| Add-ons config | `~/.agenthalo/addons.json` | p2pclaw, agentpmt-workflows toggles |
| On-chain config | `~/.agenthalo/onchain.json` | RPC URL, contract address, signer mode |
| PQ wallet | `~/.agenthalo/pq_wallet.json` | ML-DSA-65 keypair (mode 0600) |
| Identity config | `~/.agenthalo/identity.json` | Identity category state (device/network/social/super-secure) |
| Identity social ledger | `~/.agenthalo/identity_social_ledger.jsonl` | Append-only hash-chained social + super-secure events |
| Attestations | `~/.agenthalo/attestations/` | Saved attestation results |
| Audits | `~/.agenthalo/audits/` | Saved audit results |
| Signatures | `~/.agenthalo/signatures/` | Saved PQ signature envelopes |

### Custom Pricing

On first run, `pricing.json` is written with default rates. Edit it to add or update model pricing:

```json
{
  "claude-opus-4-6": {
    "input_per_mtok": 15.0,
    "output_per_mtok": 75.0,
    "cache_read_per_mtok": 1.5
  },
  "my-custom-model": {
    "input_per_mtok": 2.0,
    "output_per_mtok": 8.0,
    "cache_read_per_mtok": null
  }
}
```

Pricing is per million tokens. Cache-read pricing is optional (`null` if the model doesn't support prompt caching).

## Observability Commands

### Status Overview

```bash
# Text summary
agenthalo status

# JSON output
agenthalo status --json
```

Shows session count, total tokens, total cost, database path, and latest session info.

### JSON Output

The `traces` and `costs` commands accept a `--json` flag for machine-readable output:

```bash
# Session list as JSON
agenthalo traces --json

# Session detail as JSON
agenthalo traces --json sess-17...

# Cost buckets as JSON
agenthalo costs --json
agenthalo costs --month --json
```

### Session Export

```bash
# Export to file
agenthalo export sess-17... --output session_export.json

# Export to stdout
agenthalo export sess-17...
```

Produces a complete `agenthalo-export-v1` JSON document with session metadata, summary, and full event timeline.

### Identity Commands

```bash
# Full identity snapshot (profile + config + social ledger projection)
agenthalo identity status --json

# Social providers
agenthalo identity social status
agenthalo identity social connect google <token> --expires-days 30
agenthalo identity social revoke google --reason rotate_token

# Super-secure flags
agenthalo identity super-secure status
agenthalo identity super-secure set passkey true
agenthalo identity super-secure set totp true --label "My Authenticator"
```

All social connect/revoke and super-secure updates are appended to
`identity_social_ledger.jsonl` with SHA-256 hash chaining for immutable auditability.

### MCP Observability Tools

The MCP server exposes 22 native tools (all with `inputSchema` for parameter discovery):

| Tool | Description |
|------|-------------|
| `attest` | Tamper-evident session attestation (Merkle local or Groth16 on-chain) |
| `sign_pq` | Post-quantum detached signing (ML-DSA / Dilithium) |
| `audit_contract` | Solidity static analysis (small/medium/large tiers) |
| `trust_query` | Computed trust score for a session |
| `vote` | Record governance vote intent locally |
| `sync` | Record cloud sync intent locally |
| `privacy_pool_create` | Record privacy pool creation intent (agentpmt-workflows add-on) |
| `privacy_pool_withdraw` | Record privacy pool withdrawal intent (agentpmt-workflows add-on) |
| `pq_bridge_transfer` | Record PQ bridge cross-chain transfer intent (p2pclaw add-on) |
| `x402_check` | Parse and validate an x402direct payment request |
| `x402_pay` | Execute an x402direct USDC payment on Base (with idempotency protection) |
| `x402_balance` | Check USDC wallet balance |
| `x402_summary` | Unified x402 spending dashboard: budget, spent, remaining |
| `halo_traces` | List sessions with filters (agent, model) or get session detail |
| `halo_costs` | Cost buckets by day/month, optionally including paid operations |
| `halo_status` | Auth state, session count, total cost, latest session |
| `halo_export` | Full session export as JSON |
| `halo_capabilities` | Discover enabled features, add-ons, and configuration status |
| `identity_status` | Return profile identity, social projection, and super-secure state |
| `identity_social_connect` | Connect social token, persist secret, append immutable ledger entry |
| `identity_social_revoke` | Revoke social token and append immutable revoke event |
| `identity_super_secure_set` | Set passkey/security-key/TOTP flags with immutable ledger update |

### Model Auto-Detection

AgentHALO now automatically detects the model name from each agent's structured output stream:

- **Claude**: extracted from `message.model` or `event.model` in `stream-json`
- **Codex**: extracted from `model` or `response.model` in JSON output
- **Gemini**: extracted from `model` or `response.model` in `stream-json`

If `--model` is not explicitly provided, the detected model is used for cost calculation and display. The `--model` flag still takes precedence when specified.

## Web Dashboard

```bash
agenthalo dashboard                  # opens http://localhost:3100
agenthalo dashboard --port 8080      # custom port
agenthalo dashboard --no-open        # don't auto-open browser
```

The dashboard is a 6-page SPA embedded at compile time (rust-embed) — no npm, no CDN at runtime, no external dependencies. All assets are served from the single `agenthalo` binary.

### Pages

| Page | What It Shows |
|------|---------------|
| **Overview** | Live KPIs (sessions, tokens, cost, active agents), recent sessions table |
| **Sessions** | Filterable session list, drill-down to full event timeline |
| **Costs** | Daily cost line chart, agent doughnut chart, model bar chart, paid operations |
| **Configuration** | Toggle agent wrapping and x402 payments from the browser |
| **Trust** | Attestation list, one-click verify, create new attestations |
| **NucleusDB** | Browse the verifiable store, execute SQL, view history |

### Features

- **Dark/light theme** — toggles via button, persisted to localStorage
- **SSE live updates** — session count and status refresh in real time via `/events`
- **Chart.js analytics** — cost trends, agent distribution, model comparison
- **Session export** — download full session JSON from the browser
- **Responsive** — sidebar collapses at 768px

### API Endpoints

The dashboard is backed by a JSON API at `/api/`:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/status` | GET | Auth state, session count, total cost |
| `/api/sessions` | GET | Session list (filterable by `?agent=` and `?model=`) |
| `/api/sessions/:id` | GET | Session detail with summary |
| `/api/sessions/:id/events` | GET | Full event timeline |
| `/api/sessions/:id/export` | GET | Export session as JSON |
| `/api/sessions/:id/attest` | POST | Create attestation |
| `/api/costs` | GET | Cost buckets (daily or `?monthly=true`) |
| `/api/costs/daily` | GET | Daily cost time series |
| `/api/costs/by-agent` | GET | Cost grouped by agent |
| `/api/costs/by-model` | GET | Cost grouped by model |
| `/api/costs/paid` | GET | Paid operation breakdown |
| `/api/config` | GET | Current configuration |
| `/api/config/wrap` | POST | Toggle agent wrapping |
| `/api/config/x402` | POST | Update x402 configuration |
| `/api/trust` | GET | Trust score summary |
| `/api/attestations` | GET | List attestations |
| `/api/attestations/verify` | POST | Verify attestation by digest |
| `/api/nucleusdb/status` | GET | NucleusDB store info |
| `/api/nucleusdb/browse` | GET | Browse key-value entries |
| `/api/nucleusdb/sql` | POST | Execute SQL query |
| `/api/nucleusdb/history` | GET | Commit history |
| `/api/capabilities` | GET | Feature and add-on discovery |
| `/api/x402/summary` | GET | x402 spending summary |
| `/api/x402/balance` | GET | Wallet balance |
| `/events` | SSE | Real-time session count updates |

## Doctor Command

```bash
agenthalo doctor
```

Comprehensive diagnostic that checks all subsystems in one view:

```
  Agent H.A.L.O. v0.3.0

  Authentication:     OK  (GitHub: user@example.com)
  Trace store:        OK  (47 sessions, 2,184,000 tokens, $248.30 total)
  Agent wrapping:
    claude            WRAPPED
    codex             WRAPPED
    gemini            NOT WRAPPED
  x402 payments:      ENABLED  (Base Sepolia, 5 USDC auto-approve)
  AgentPMT proxy:     ENABLED  (42 tools, budget tag: default)
  PQ wallet:          OK  (ML-DSA-65)
  On-chain:           CONFIGURED  (Base Sepolia, contract 0x1234...)
  License:            Community (free)
  Dashboard:          Run `agenthalo dashboard` to start

  All checks completed.
```

## Additional Commands

### Attestation

```bash
# Local Merkle attestation
agenthalo attest --session sess-17...

# On-chain Groth16 attestation (posts to Base Sepolia)
agenthalo attest --session sess-17... --onchain

# Anonymous attestation (attester identity masked)
agenthalo attest --session sess-17... --anonymous
```

### Contract Audit

```bash
agenthalo audit contracts/MyContract.sol --size small
```

Static analysis with findings, risk score, and attestation digest.

### Post-Quantum Signing

```bash
# Generate ML-DSA-65 keypair
agenthalo keygen --pq

# Sign a message
agenthalo sign --pq --message "critical decision recorded"

# Sign a file
agenthalo sign --pq --file artifacts/report.json
```

### Trust Query

```bash
agenthalo trust query --session sess-17...
```

### On-Chain Configuration

```bash
# Show on-chain config
agenthalo onchain status

# Configure Base Sepolia
agenthalo onchain config --rpc-url https://sepolia.base.org \
  --contract 0x... --signer-mode private_key_env

# Deploy TrustVerifier
agenthalo onchain deploy

# Verify attestation on-chain
agenthalo onchain verify <attestation-digest>
```

### AgentPMT Tool Proxy

```bash
# Enable/disable tool proxy
agenthalo config tool-proxy enable [budget-tag]
agenthalo config tool-proxy disable

# Configure AgentPMT auth + endpoint
agenthalo config set-agentpmt-key <token>
agenthalo config tool-proxy endpoint https://testnet.api.agentpmt.com/mcp

# Refresh tool catalog from AgentPMT
agenthalo config tool-proxy refresh

# Check status
agenthalo config tool-proxy status
```

When enabled, AgentPMT tools appear alongside native tools in the MCP `tools/list` response with an `agentpmt/` prefix. Budget controls and credentials are managed on the AgentPMT side.

### x402direct Payments

```bash
# Enable x402 integration
agenthalo x402 enable

# Configure UPC contract and network
agenthalo x402 config --network base-sepolia --upc-contract 0x...

# Set max auto-approve (in base units, default 5 USDC = 5000000)
agenthalo x402 config --max-auto-approve 10000000

# Check status
agenthalo x402 status

# Validate a 402 response body
echo '{"x402version":"direct.1.0.0",...}' | agenthalo x402 check
# or: agenthalo x402 check --body '{"x402version":"direct.1.0.0",...}'

# Check wallet balance
agenthalo x402 balance

# Execute payment (reads 402 body from stdin or --body flag)
agenthalo x402 pay --body '{"x402version":"direct.1.0.0",...}'
# or: echo '<402-json>' | agenthalo x402 pay
# Optionally select a specific payment option: --option po_base_usdc
```

#### Payment Execution Flow

When an agent encounters an HTTP 402 response:

1. **Validate**: Parse and validate the x402direct payment request
2. **Select option**: Auto-select a known network/token option, or use `--option` to pick one
3. **Budget check**: Enforce `max_auto_approve` limit (default 5 USDC)
4. **Balance check**: Verify sufficient USDC on the target network
5. **Execute**: ERC-20 `transfer(address,uint256)` via `cast send` with nonce retry
6. **Receipt**: Wait for on-chain receipt (block number, gas used)
7. **Proof**: Return `X402PaymentProof` for submission back to the vendor

The wallet private key is read from the `AGENTHALO_X402_PRIVATE_KEY` environment variable. This is separate from `AGENTHALO_ONCHAIN_PRIVATE_KEY` used for attestation posting, allowing independent key management.

#### MCP Payment Tools

| Tool | Description |
|------|-------------|
| `x402_check` | Parse and validate a 402 response body (no on-chain interaction) |
| `x402_pay` | Execute payment: validate, check balance, transfer USDC, return proof |
| `x402_balance` | Check wallet USDC balance on the configured network |

Supported networks: Base mainnet (`eip155:8453`) and Base Sepolia (`eip155:84532`). Protocol reference: [x402direct.org](https://www.x402direct.org).

### Add-ons

```bash
agenthalo addon list
agenthalo addon enable tool-proxy
agenthalo addon enable p2pclaw
agenthalo addon enable agentpmt-workflows
```

### License

```bash
agenthalo license status
agenthalo license verify path/to/certificate.json
```

CAB certificate verification is fully offline — no phone-home.

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `AGENTHALO_HOME` | `~/.agenthalo` | Override home directory for all state |
| `AGENTHALO_DB_PATH` | `$AGENTHALO_HOME/traces.ndb` | Override trace database path |
| `AGENTHALO_API_KEY` | (none) | API key (takes precedence over saved credentials) |
| `AGENTHALO_ALLOW_GENERIC` | `0` | Set to `1`, `true`, or `yes` to enable custom agent wrapping |
| `AGENTHALO_NO_TELEMETRY` | `1` | Always 1. Documented for transparency. |
| `AGENTHALO_X402_PRIVATE_KEY` | (none) | Private key for x402 USDC payments (separate from attestation key) |
| `AGENTHALO_ONCHAIN_SIMULATION` | `0` | Set to `1` to disable real RPC posting (returns deterministic simulated tx hashes) |

## Pricing Tables

Default pricing (as of February 2026):

| Model | Input ($/MTok) | Output ($/MTok) | Cache Read ($/MTok) |
|-------|---------------|-----------------|---------------------|
| `claude-opus-4-6` | $15.00 | $75.00 | $1.50 |
| `claude-sonnet-4-6` | $3.00 | $15.00 | $0.30 |
| `claude-haiku-4-5` | $0.80 | $4.00 | $0.08 |
| `o3` | $10.00 | $40.00 | -- |
| `o4-mini` | $1.10 | $4.40 | -- |
| `gpt-4.1` | $2.00 | $8.00 | -- |
| `gemini-2.5-pro` | $1.25 | $10.00 | -- |
| `gemini-2.5-flash` | $0.15 | $0.60 | -- |

Edit `~/.agenthalo/pricing.json` to customize or add models.

## Trace Schema

### Session Metadata

Each recording session creates a `SessionMetadata` record:

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | `sess-{unix_timestamp}-{pid}` |
| `agent` | string | Detected agent name (`claude`, `codex`, `gemini`, or custom) |
| `model` | string? | Model name if `--model`/`-m` flag detected |
| `started_at` | u64 | Unix timestamp |
| `ended_at` | u64? | Unix timestamp (null while running) |
| `prompt` | string? | Compact textual preview of the prompt |
| `status` | enum | `Running`, `Completed`, or `Failed` |
| `user_id` | string? | From OAuth credentials |
| `machine_id` | string? | `$HOSTNAME` |

### Event Types

| Type | When Emitted |
|------|-------------|
| `AssistantMessage` | Agent produces text output |
| `UserMessage` | Input/prompt to the agent |
| `McpToolCall` | Agent invokes a tool |
| `McpToolResult` | Tool returns a result |
| `FileChange` | File created, modified, or read |
| `BashCommand` | Shell command executed |
| `Error` | Stderr line or failure |
| `RawOutput` | Generic agent stdout line (GenericAdapter) |
| `SystemInfo` | Environment or system metadata |

### Event Fields

| Field | Type | Description |
|-------|------|-------------|
| `seq` | u32 | Sequence number within session |
| `timestamp` | u64 | Unix timestamp |
| `event_type` | EventType | See above |
| `content` | JSON | Event payload |
| `input_tokens` | u64? | Tokens consumed |
| `output_tokens` | u64? | Tokens produced |
| `cache_read_tokens` | u64? | Cached tokens |
| `tool_name` | string? | For tool call/result events |
| `tool_input` | JSON? | Tool input parameters |
| `tool_output` | JSON? | Tool output data |
| `file_path` | string? | For file change events |
| `content_hash` | string | SHA-256 of serialized event |

### Session Summary

Computed at session end:

| Field | Type |
|-------|------|
| `event_count` | u32 |
| `total_input_tokens` | u64 |
| `total_output_tokens` | u64 |
| `total_cache_read_tokens` | u64 |
| `estimated_cost_usd` | f64 |
| `files_created` | u32 |
| `files_modified` | u32 |
| `files_read` | u32 |
| `tool_calls` | u32 |
| `duration_secs` | u64 |

## Architecture

```
                    AgentHALO
┌──────────────────────────────────────────────────┐
│                                                  │
│   agenthalo run claude -p "fix the bug"          │
│       │                                          │
│       ▼                                          │
│   ┌─────────┐    ┌──────────────┐                │
│   │ detect  │───▶│ AgentRunner  │                │
│   │ agent   │    │  spawn child │                │
│   └─────────┘    │  tee stdout  │                │
│                  │  tee stderr  │                │
│                  └──────┬───────┘                │
│                         │                        │
│              ┌──────────┼──────────┐             │
│              ▼          ▼          ▼             │
│         ┌────────┐ ┌────────┐ ┌────────┐        │
│         │ Claude │ │ Codex  │ │ Gemini │        │
│         │Adapter │ │Adapter │ │Adapter │        │
│         └───┬────┘ └───┬────┘ └───┬────┘        │
│             └───────────┼─────────┘              │
│                         ▼                        │
│              ┌──────────────────┐                │
│              │   TraceWriter    │                │
│              │ (NucleusDB WAL) │                │
│              └──────────────────┘                │
│                         │                        │
│              ┌──────────▼──────────┐             │
│              │  ~/.agenthalo/      │             │
│              │    traces.ndb       │             │
│              │    credentials.json │             │
│              │    pricing.json     │             │
│              └──────────┬──────────┘             │
│                         │                        │
│              ┌──────────▼──────────┐             │
│              │  Web Dashboard      │             │
│              │  :3100 (embedded)   │             │
│              │  6 pages, SSE, API  │             │
│              └─────────────────────┘             │
│                                                  │
└──────────────────────────────────────────────────┘
```

### Source Layout

```
src/halo/
  mod.rs               — module root, generic_agents_allowed()

  # Identity & Cryptography
  did.rs               — DID derivation, Ed25519 + ML-DSA-65 dual sign/verify, DIDDocument
  genesis_seed.rs      — genesis seed ceremony, BIP-39 mnemonic derivation, entropy mixing
  genesis_entropy.rs   — entropy source management for genesis ceremonies
  identity.rs          — identity category state (device/network/social/super-secure)
  identity_ledger.rs   — append-only hash-chained identity ledger (SHA-512)
  pq.rs                — ML-DSA-65 PQ wallet management (keygen, signing, envelopes)
  hash.rs              — HashAlgorithm dispatch (SHA-256 legacy / SHA-512 current)

  # Hybrid KEM & DIDComm
  hybrid_kem.rs        — X25519 + ML-KEM-768 hybrid KEM (IETF Composite, HKDF-SHA-512)
  didcomm.rs           — DIDComm v2 authcrypt/anoncrypt pack/unpack (hybrid KEM paths)
  didcomm_handler.rs   — inbound DIDComm message handling (hybrid KEM detection)

  # EVM & On-Chain
  evm_wallet.rs        — BIP-32 secp256k1 wallet derivation, signing (crate-private)
  evm_gate.rs          — PQ-gated EVM signing (dual Ed25519 + ML-DSA-65 authorization)
  twine_anchor.rs      — CURBy-Q Twine identity attestation, triple-signed binding proofs
  onchain.rs           — Base L2 posting, signer modes, TrustVerifier calls
  funding.rs           — wallet funding utilities
  x402.rs              — x402direct protocol types, CAIP-10 parsing, payment execution

  # P2P Mesh
  p2p_node.rs          — libp2p swarm (Noise XX, gossipsub, Kademlia, relay, AutoNAT)
  p2p_discovery.rs     — agent discovery, GossipPrivacy metadata minimization, DHT publish
  a2a_bridge.rs        — HTTP bridge for agent-to-agent DIDComm (hybrid KEM)
  startup.rs           — full stack orchestration (P2P + Nym + DIDComm bootstrap)

  # Nym Mixnet
  nym.rs               — Nym SOCKS5 proxy integration
  nym_native.rs        — native Sphinx packet construction, SURB replies, cover traffic

  # Observability
  schema.rs            — SessionMetadata, TraceEvent, EventType, SessionSummary
  trace.rs             — TraceWriter (NucleusDB writes), read-side queries, blob encoding
  detect.rs            — agent type detection, flag injection with dedup
  runner.rs            — subprocess management, signal forwarding, adapter dispatch
  viewer.rs            — CLI output formatting (tables, timestamps, costs, JSON, export)

  # Trust & Attestation
  attest.rs            — session attestation (Merkle root SHA-512, anonymous membership proofs)
  trust.rs             — trust score computation (SHA-512 digest)
  circuit.rs           — Groth16 proving/verifying (BN254, arkworks)
  circuit_policy.rs    — dev vs production circuit key policy
  public_input_schema.rs — Groth16 public input layout versioning
  audit.rs             — Solidity static analysis engine
  zk_compute.rs        — ZK compute receipts
  zk_credential.rs     — ZK credential proofs and anonymous membership

  # Auth & Config
  auth.rs              — OAuth flow, API key, credential storage (0600 perms)
  config.rs            — path resolution (AGENTHALO_HOME, DB_PATH)
  vault.rs             — AES-256-GCM encrypted API key vault
  crypto_scope.rs      — scoped cryptographic key management
  session_manager.rs   — session lifecycle management
  password.rs          — password hashing and verification
  encrypted_file.rs    — encrypted file I/O
  api_keys.rs          — API key management
  agent_auth.rs        — agent authentication middleware
  privacy_controller.rs — privacy policy enforcement

  # Integrations
  addons.rs            — add-on toggle mechanism (p2pclaw, agentpmt-workflows)
  agentpmt.rs          — AgentPMT tool proxy config, catalog, and unified surface
  pricing.rs           — model pricing table, cost calculation
  profile.rs           — agent profile management
  proxy.rs             — OpenAI-compatible multi-provider API proxy
  http_client.rs       — shared HTTP client utilities
  pinata.rs            — IPFS pinning via Pinata
  wdk_proxy.rs         — WDK proxy integration
  migration.rs         — data migration utilities
  wrap.rs              — shell alias management (.bashrc/.zshrc)
  util.rs              — hex encode/decode, SHA digest helpers, timestamps

  adapters/
    mod.rs             — StreamAdapter trait
    claude.rs          — Claude Code stream-json parser
    codex.rs           — Codex JSON parser
    gemini.rs          — Gemini CLI parser
    generic.rs         — Raw stdout capture

src/comms/
  mod.rs               — communications module root
  didcomm.rs           — DIDComm v2 mesh envelope (hybrid KEM encrypt/decrypt)
  envelope.rs          — envelope serialization
  session.rs           — communication session state

src/dashboard/
  mod.rs               — axum server, browser launch, DashboardState
  api.rs               — 25+ JSON API endpoints, SSE live updates
  assets.rs            — rust-embed static file serving

dashboard/
  index.html           — SPA shell (6 pages)
  app.js               — SPA router, Chart.js analytics, SSE
  style.css            — dark/light theme, responsive layout

src/bin/
  agenthalo.rs             — CLI binary (run, setup, dashboard, doctor, attest, ...)
  agenthalo_mcp_server.rs  — HTTP MCP server (22 native + proxied tools)
  nucleusdb.rs             — NucleusDB CLI binary
  nucleusdb_mcp.rs         — NucleusDB MCP server (stdio + HTTP transport)
  nucleusdb_server.rs      — NucleusDB multi-tenant HTTP server
  nucleusdb_tui.rs         — NucleusDB terminal UI
```

## Security

### Post-Quantum Cryptographic Architecture

AgentHALO implements defense-in-depth post-quantum cryptography across all agent-controlled surfaces:

**Hybrid Key Encapsulation (DIDComm):**
- All DIDComm authcrypt and anoncrypt use a hybrid KEM: X25519 ECDH + ML-KEM-768 (FIPS 203).
- Key derivation: HKDF-SHA-512 with domain-separated salt (`AgentHALO-HybridKEM-v2`), producing 32-byte AES-256-GCM keys.
- IKM includes ciphertext binding (`ecdh_ss || mlkem_ss || mlkem_ct`) per IETF Composite ML-KEM.
- Classical fallback: if the recipient has no ML-KEM key, falls back to X25519-only (no PQ protection).

**Dual Signatures (Identity):**
- Every identity operation is dual-signed: Ed25519 (classical) + ML-DSA-65 (FIPS 204, post-quantum).
- Discovery announcements, identity attestations, and binding proofs all carry both signatures.
- Verification requires BOTH signatures to pass (`ed_ok && pq_ok`).

**Hash Upgrade (Integrity Surfaces):**
- New entries use SHA-512 (256-bit PQ collision resistance under Grover's algorithm).
- Legacy entries remain SHA-256 with automatic detection via `hash_algorithm` field or hash length.
- Upgraded surfaces: identity ledger, attestation Merkle tree, PQ signature payloads, trust score digests, DIDComm/P2P binding proofs.
- Groth16 circuit: SHA-512 digests compressed to 32 bytes via domain-separated SHA-256 for BN254 compatibility.

**PQ-Gated EVM Signing:**
- `sign_with_evm_key()` is `pub(crate)` — external callers cannot bypass the gate.
- Every EVM signature requires a dual-signed DIDComm authorization (Ed25519 + ML-DSA-65) over a canonical request including address, nonce, timestamp, and SHA-512 message digest.
- Address binding: the authorization's EVM address must match the signing key's derived address.

**Gossipsub Metadata Minimization:**
- Gossipsub announcements strip listen addresses by default (`GossipPrivacy::AddressesViaDhtOnly`).
- Full addresses are published to Kademlia DHT only, changing the attack model from passive bulk harvesting to active per-DID queries.

### Credential Storage

- Credentials are stored in `~/.agenthalo/credentials.json` with Unix mode `0600` (owner read/write only).
- API keys set via `config set-key` (without an argument) prompt interactively — the key never appears in shell history or `ps` output.
- OAuth flows use a CSRF `state` parameter to prevent local process injection attacks.
- The encrypted vault (`vault.enc`) uses AES-256-GCM with a master key derived via HKDF-SHA-256 from the PQ wallet's secret seed.

### Trace Integrity

- New events: `content_hash` is `SHA-512(serialized_event)`. Legacy events use SHA-256.
- Events are written to NucleusDB as content-addressed blobs.
- The NucleusDB commit for each session can be verified with `VERIFY` queries.
- Attestation Merkle trees use SHA-512 for leaf and node hashing.
- Traces are local-only — they never leave your machine.

### Signal Handling

- SIGINT and SIGTERM are forwarded to the child process via `libc::kill()`.
- The signal handler runs in a dedicated thread using the `signal-hook` crate.
- AgentHALO waits for the child to exit before writing the session summary.

### Upstream Quantum Vulnerabilities (Not Fixable Unilaterally)

These are documented for transparency. AgentHALO's application-layer protections mitigate content exposure even if these transports are compromised:

- **libp2p Noise XX (X25519):** Transport-layer encryption is classical-only. If broken, attacker sees signed discovery metadata (semi-public by design). DIDComm payload content is NOT exposed.
- **Nym Sphinx (X25519):** Mixnet anonymity layer is classical-only. If broken, attacker learns communication patterns but NOT message content (protected by DIDComm hybrid KEM).
- **Ethereum ECDSA (secp256k1):** Ecosystem-wide vulnerability. AgentHALO's PQ-gated EVM signing creates a two-cryptosystem barrier but cannot prevent key recovery if Shor's algorithm is realized at scale.

## Troubleshooting

### "not authenticated"

```
not authenticated. Run `agenthalo login` or set AGENTHALO_API_KEY.
```

Run `agenthalo login` or set the environment variable:

```bash
export AGENTHALO_API_KEY=your-key
```

### "custom agent commands are disabled"

```
custom agent commands are disabled in free tier. Set AGENTHALO_ALLOW_GENERIC=1...
```

The command you're wrapping isn't `claude`, `codex`, or `gemini`. Enable custom agents:

```bash
export AGENTHALO_ALLOW_GENERIC=1
```

### "spawn 'agent ...': No such file"

The agent binary isn't in your `PATH`. Verify with `which claude` (or the agent you're trying to run).

### Wrong cost calculations

Edit `~/.agenthalo/pricing.json` to match current model pricing. The file is auto-generated on first run but may become stale as providers update their rates.

### Traces database missing

If `~/.agenthalo/traces.ndb` doesn't exist, it's created automatically on the first `agenthalo run`. If you need a fresh start:

```bash
rm ~/.agenthalo/traces.ndb
```

---

<p align="center">
  <sub>AgentHALO is part of <a href="../README.md">NucleusDB</a> by <strong>Apoth3osis</strong></sub>
</p>
