<p align="center">
  <img src="../assets/agent_halo_logo.png" alt="Agent H.A.L.O." width="240"/>
</p>

<h1 align="center">Agent H.A.L.O. Reference Guide</h1>

<p align="center">
  <strong>H</strong>osted <strong>A</strong>gent <strong>L</strong>ogic <strong>O</strong>rbit<br>
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
- [Configuration](#configuration)
- [Environment Variables](#environment-variables)
- [Pricing Tables](#pricing-tables)
- [Trace Schema](#trace-schema)
- [Architecture](#architecture)
- [Security](#security)
- [Troubleshooting](#troubleshooting)

---

## Overview

AgentHALO wraps AI coding agent CLIs (Claude Code, Codex, Gemini) and records every event — thoughts, tool calls, file edits, token counts, and costs — into a local NucleusDB trace store.

Every trace event is a content-addressed blob with a SHA-256 Merkle proof. If any event is modified after the fact, the proof chain breaks. This provides a tamper-evident audit log of everything your agents do.

**Key properties:**

- **Zero telemetry.** Nothing leaves your machine. No analytics, no tracking, no phone-home.
- **Zero config.** `agenthalo run claude` auto-injects the right flags for structured output.
- **Tamper-evident.** Content-addressed storage in NucleusDB with Merkle proofs.
- **Agent-native.** Parses each agent's native structured output format.

## Installation

AgentHALO ships as a binary inside the NucleusDB build:

```bash
git clone https://github.com/Abraxas1010/nucleusdb.git
cd nucleusdb
cargo build --release --bin agenthalo
```

The binary is at `target/release/agenthalo`. Copy it to your `PATH`:

```bash
cp target/release/agenthalo ~/.local/bin/
```

Verify:

```bash
agenthalo version
# agenthalo 0.2.0
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

## Observability Commands (v0.2.1)

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

### MCP Observability Tools

The MCP server exposes 4 observability tools (14 native tools total):

| Tool | Description |
|------|-------------|
| `halo_traces` | List sessions or get session detail (with `limit` and `session_id` params) |
| `halo_costs` | Cost buckets by day or month (with `monthly` param) |
| `halo_status` | Auth state, session count, total cost, latest session |
| `halo_export` | Full session export as JSON |

### Model Auto-Detection

AgentHALO now automatically detects the model name from each agent's structured output stream:

- **Claude**: extracted from `message.model` or `event.model` in `stream-json`
- **Codex**: extracted from `model` or `response.model` in JSON output
- **Gemini**: extracted from `model` or `response.model` in `stream-json`

If `--model` is not explicitly provided, the detected model is used for cost calculation and display. The `--model` flag still takes precedence when specified.

## Additional Commands (v0.2.0)

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
```

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
| `AGENTHALO_ONCHAIN_STUB` | `0` | Set to `1` to disable real RPC posting (returns deterministic stub tx hashes) |

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
│              └─────────────────────┘             │
│                                                  │
└──────────────────────────────────────────────────┘
```

### Source Layout

```
src/halo/
  mod.rs               — module root, generic_agents_allowed()
  addons.rs            — add-on toggle mechanism (p2pclaw, agentpmt-workflows)
  agentpmt.rs          — AgentPMT tool proxy config, catalog, and unified surface
  attest.rs            — session attestation (Merkle root, event digest)
  audit.rs             — Solidity static analysis engine
  auth.rs              — OAuth flow, API key, credential storage (0600 perms)
  circuit.rs           — Groth16 proving/verifying (BN254, arkworks)
  circuit_policy.rs    — dev vs production circuit key policy
  config.rs            — path resolution (AGENTHALO_HOME, DB_PATH)
  detect.rs            — agent type detection, flag injection with dedup
  onchain.rs           — Base L2 posting, signer modes, TrustVerifier calls
  pq.rs                — ML-DSA-65 keygen and signing
  pricing.rs           — model pricing table, cost calculation
  public_input_schema.rs — Groth16 public input layout versioning
  runner.rs            — subprocess management, signal forwarding, adapter dispatch, model detection
  schema.rs            — SessionMetadata, TraceEvent, EventType, SessionSummary
  trace.rs             — TraceWriter (NucleusDB writes), read-side queries, blob encoding
  trust.rs             — trust score computation
  util.rs              — SHA-256 digest helpers, hex encode/decode
  viewer.rs            — CLI output formatting (tables, timestamps, costs, JSON, status, export)
  wrap.rs              — shell alias management (.bashrc/.zshrc)
  x402.rs              — x402direct protocol types, CAIP-10 parsing, validation, config
  adapters/
    mod.rs             — StreamAdapter trait
    claude.rs          — Claude Code stream-json parser
    codex.rs           — Codex JSON parser
    gemini.rs          — Gemini CLI parser
    generic.rs         — Raw stdout capture

src/bin/
  agenthalo.rs             — CLI binary (run, attest, audit, sign, trust, onchain, ...)
  agenthalo_mcp_server.rs  — HTTP MCP server (14 native + proxied tools)
  nucleusdb.rs             — NucleusDB CLI binary
  nucleusdb_mcp.rs         — NucleusDB MCP server (stdio + HTTP transport)
  nucleusdb_server.rs      — NucleusDB multi-tenant HTTP server
  nucleusdb_tui.rs         — NucleusDB terminal UI
```

## Security

### Credential Storage

- Credentials are stored in `~/.agenthalo/credentials.json` with Unix mode `0600` (owner read/write only).
- API keys set via `config set-key` (without an argument) prompt interactively — the key never appears in shell history or `ps` output.
- OAuth flows use a CSRF `state` parameter to prevent local process injection attacks.

### Trace Integrity

- Every event's `content_hash` is `SHA-256(serialized_event)`.
- Events are written to NucleusDB as content-addressed blobs.
- The NucleusDB commit for each session can be verified with `VERIFY` queries.
- Traces are local-only — they never leave your machine.

### Signal Handling

- SIGINT and SIGTERM are forwarded to the child process via `libc::kill()`.
- The signal handler runs in a dedicated thread using the `signal-hook` crate.
- AgentHALO waits for the child to exit before writing the session summary.

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
