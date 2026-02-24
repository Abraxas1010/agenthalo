<img src="assets/Apoth3osis.webp" alt="Apoth3osis Logo" width="140"/>

---

# NucleusDB

[![License: Apoth3osis License Stack v1](https://img.shields.io/badge/License-Apoth3osis%20License%20Stack%20v1-blue.svg)](LICENSE.md)
[![Tests: 162 passing](https://img.shields.io/badge/tests-162%20passing-brightgreen.svg)](#testing)

**The verifiable database for AI agents. Tamper-proof records with mathematical guarantees — not promises.**

---

## The Problem

AI agents are writing to databases. They're logging decisions, storing user data, managing financial records, and operating autonomously. But every database they use today has the same fundamental flaw: **any process with write access can silently alter or delete records after the fact.**

There is no way to prove a record wasn't changed. There is no way for one agent to trust another agent's data. There is no way to audit what actually happened versus what the log claims happened.

This isn't a configuration problem. It's an architectural one. Traditional databases were designed for humans who trust each other. The agentic world needs something different.

## The Solution

NucleusDB is a database where **every write is a cryptographic commitment, every query comes with a proof, and deletion can be made mathematically impossible.**

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

## Why This Matters

### For AI Safety

Agents operating on shared data need an **unforgeable audit trail**. NucleusDB provides three independent layers of tamper evidence:

1. **Monotone Extension Proofs** — Every commit constructively proves that all prior records are preserved. Deletion is detected instantly.
2. **SHA-256 Seal Chain** — Each commit's seal binds to every previous seal. Forging a seal after deletion requires breaking SHA-256 preimage resistance (2^128 operations).
3. **Certificate Transparency** — An RFC 6962 append-only Merkle tree provides independent consistency proofs that any third party can verify.

### For Compliance

Regulatory frameworks (SOX, HIPAA, GDPR Article 30, MiFID II) require immutable audit logs. NucleusDB doesn't just promise immutability — it proves it, with cryptographic evidence that can be independently verified.

### For Multi-Agent Trust

Agent A writes a record. Agent B reads it a week later. How does Agent B know the record hasn't been tampered with? NucleusDB provides **query proofs**: every read returns the value, a vector commitment proof, and a state root. The agent can verify the proof without trusting the database, the network, or Agent A.

## How It Compares

| Feature | SQLite | PostgreSQL | Datomic | QLDB | **NucleusDB** |
|---------|--------|------------|---------|------|---------------|
| SQL interface | Full | Full | Datalog | PartiQL | Subset |
| Cryptographic commits | No | No | No | Yes | **Yes** |
| Query proofs (client-verifiable) | No | No | No | No | **Yes** |
| Immutable mode (math-enforced) | No | No | Append-only | Append-only | **Append-only + seal chain** |
| Post-quantum signatures | No | No | No | No | **ML-DSA-65** |
| Certificate Transparency | No | No | No | Partial | **Full RFC 6962** |
| ZK license verification | No | No | No | No | **Groth16 SNARK** |
| Formal specification | No | No | No | No | **18 Lean 4 modules** |
| On-chain trust attestation | No | No | No | No | **Base L2 (Solidity)** |
| MCP server (AI-native) | No | No | No | No | **Yes** |
| Self-contained binary | Yes | No | No | No (AWS) | **Yes** |

**Datomic** and **QLDB** offer append-only semantics, but neither provides client-verifiable query proofs or a cryptographic seal chain. Their immutability is a property of the service — you trust the operator. NucleusDB's immutability is a property of mathematics — you verify it yourself.

## Quick Start

### Install

```bash
git clone https://github.com/Abraxas1010/nucleusdb.git
cd nucleusdb
cargo build --release
```

The `nucleusdb` binary is at `target/release/nucleusdb`. No external dependencies, no cloud service, no account required.

### 1. Create a Database

```bash
nucleusdb create --db my_records.ndb --backend merkle
```

Three commitment backends are available:
- `merkle` — SHA-256 Merkle tree (recommended, post-quantum safe)
- `ipa` — Pedersen-style vector commitments
- `kzg` — Pairing-based commitments with trusted setup

### 2. Write and Query with SQL

```bash
# Interactive REPL
nucleusdb open --db my_records.ndb
```

```sql
INSERT INTO data (key, value) VALUES ('sensor_reading', 42);
INSERT INTO data (key, value) VALUES ('agent_decision', 7);
COMMIT;

SELECT * FROM data;
SELECT * FROM data WHERE key LIKE 'sensor%';

VERIFY 'sensor_reading';  -- cryptographic proof of integrity

SHOW STATUS;
SHOW HISTORY;
SHOW HISTORY 'sensor_reading';
```

### 3. Lock for Immutable Records

```sql
SET MODE APPEND_ONLY;

-- These now succeed:
INSERT INTO data (key, value) VALUES ('new_record', 100);
COMMIT;

-- These are permanently rejected:
UPDATE data SET value = 999 WHERE key = 'sensor_reading';
-- ERROR: UPDATE rejected: database is in AppendOnly mode (immutable agentic records)

DELETE FROM data WHERE key = 'sensor_reading';
-- ERROR: DELETE rejected: database is in AppendOnly mode (immutable agentic records)

SHOW MODE;
-- Write mode: AppendOnly
```

### 4. Use as an MCP Server (AI Agents)

```bash
nucleusdb mcp --db my_records.ndb
```

This exposes 10 tools over stdio via the [Model Context Protocol](https://modelcontextprotocol.io):

| Tool | Purpose |
|------|---------|
| `nucleusdb_create_database` | Create a new database |
| `nucleusdb_open_database` | Open an existing database |
| `nucleusdb_execute_sql` | Run SQL statements |
| `nucleusdb_query` | Query with cryptographic proof |
| `nucleusdb_query_range` | Range query |
| `nucleusdb_verify` | Verify a key's integrity |
| `nucleusdb_status` | Database status |
| `nucleusdb_history` | Commit history |
| `nucleusdb_export` | Export state as JSON |
| `nucleusdb_checkpoint` | Create snapshot + truncate WAL |

Add to your Claude Code MCP config, Cursor, or any MCP-compatible client.

### 5. Run as an HTTP Server

```bash
nucleusdb-server 127.0.0.1:8088 production
```

Multi-tenant REST API with RBAC:

```bash
# Register a tenant
curl -X POST http://127.0.0.1:8088/v1/tenants/register \
  -H 'Content-Type: application/json' \
  -d '{"tenant_id":"acme","auth_token":"secret","initial_values":[],"backend":"binary_merkle"}'

# Commit data
curl -X POST http://127.0.0.1:8088/v1/tenants/acme/commit \
  -H 'Content-Type: application/json' \
  -d '{"auth_token":"secret","writes":[[0,42]],"local_views":[]}'

# Query with proof
curl -X POST http://127.0.0.1:8088/v1/tenants/acme/query \
  -H 'Content-Type: application/json' \
  -d '{"auth_token":"secret","index":0}'
```

### 6. Terminal UI

```bash
nucleusdb tui --db my_records.ndb
```

Five-tab interface: Status, Browse, Execute, History, Transparency. Navigate with `F1`-`F5` or `Tab`.

## Architecture

```
Client Surfaces                    Core Runtime
  CLI / REPL ─────┐               ┌─ protocol.rs ── commit / query / verify
  Terminal UI ────┤               ├─ immutable.rs ─ monotone proofs + seal chain
  MCP Server ─────┼── SQL ──────▶ ├─ sql/executor ─ SQL parsing + enforcement
  HTTP API ───────┘               ├─ keymap.rs ──── string keys → vector indices
                                  ├─ witness.rs ─── ML-DSA-65 quorum signatures
                                  ├─ ct6962.rs ──── RFC 6962 transparency log
                                  ├─ security.rs ── parameter validation + reduction contracts
                                  ├─ audit.rs ───── evidence bundles + replay verification
                                  ├─ license.rs ─── ZK-SNARK license verification (Groth16/BN254)
                                  └─ persistence ── snapshot + WAL (redb)

Commitment Backends               On-Chain Trust (Solidity)
  vc/binary_merkle.rs              contracts/TrustVerifier.sol ─── single-chain attestation
  vc/ipa.rs                        contracts/TrustVerifierMultiChain.sol ─ composite multi-chain
  vc/kzg.rs                        contracts/Groth16VerifierAdapter.sol ── ITrustProofVerifier ↔ Groth16
                                   contracts/circuits/ ─── circom circuit + setup docs
                                   contracts/mocks/ ─── test verifier + token

Formal Specification
  18 Lean 4 modules under lean/NucleusDB/
  Core, Security, Commitment, Sheaf, Transparency, Adversarial
```

## SQL Reference

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

## Interfaces

### TUI

`nucleusdb tui` provides a five-tab terminal interface (Status, Browse, Execute, History, Transparency). Hotkeys: `F1`..`F5` switch tabs, `Enter` executes SQL, `Up`/`Down` scrolls, `q` quits.

### MCP Server

`nucleusdb mcp` serves 11 MCP tools over stdio: `create_database`, `open_database`, `execute_sql`, `query`, `query_range`, `verify`, `status`, `history`, `export`, `checkpoint`, `help`.

### HTTP API

Multi-tenant REST API via `nucleusdb-server`: tenant registration, commit, query, snapshot, checkpoint. See `src/api.rs` for full route list.

### Remote MCP Server (Agent Interop)

Any MCP-capable agent (Claude, GPT, Gemini, Codex, custom) can connect to NucleusDB over the network using the MCP Streamable HTTP transport:

```bash
# Start remote MCP server (dev mode, no auth)
nucleusdb-mcp --transport http --port 3000

# Production mode with dual authentication
nucleusdb-mcp --transport http --host 0.0.0.0 --port 8443 --auth --jwt-secret $SECRET

# Docker deployment
docker build -f Dockerfile.mcp -t nucleusdb-mcp:latest .
docker run -p 3000:3000 nucleusdb-mcp:latest
```

**Dual authentication** (CAB + OAuth 2.1):
- **CAB-as-bearer-token**: Hardware-anchored agent identity verified on-chain (`Authorization: Bearer cab:<base64>`)
- **OAuth 2.1 JWT**: Standard bearer tokens for non-attested agents (`Authorization: Bearer <jwt>`)

**Per-tool scope enforcement** — 25 tools across 5 security tiers:

| Scope | Tools | Auth Required |
|-------|-------|---------------|
| `read` | help, status, query, verify, export, history | Basic token |
| `trust:verify` | verify_agent, verify_agent_multichain, list_chains | Basic token |
| `write` | execute_sql, create_database, checkpoint, channels | CAB tier 3+ or JWT |
| `trust:attest` | agent_register, register_chain, submit_attestation | CAB tier 4 or JWT |
| `container` | container_launch | CAB tier 4 or JWT |

Endpoints: `/mcp` (MCP), `/health` (status), `/auth/info` (auth discovery).

### On-Chain Trust Verification

NucleusDB includes Solidity smart contracts for on-chain agent trust attestation and payment routing on Base (Coinbase L2).

**TrustVerifier** — single-chain attestation:
- Verifies a ZK proof against a configurable verifier contract
- Registers/refreshes agent identity (PUF digest, tier, replay sequence)
- Routes USDC payment to treasury on successful attestation
- Monotone replay sequence prevents attestation replay

**TrustVerifierMultiChain** — composite multi-chain attestation:
- Extends TrustVerifier with a chain registry (up to 8 chains per attestation)
- Tiered per-chain fees (e.g., Base: 1 USDC, Ethereum: 5 USDC)
- Composite attestation across multiple chains in a single transaction
- Per-chain and multi-chain verification views

**Groth16VerifierAdapter** — production ZK proof bridge:
- Adapts any snarkjs-generated Groth16 verifier to the `ITrustProofVerifier` interface
- Decodes ABI-encoded proof bytes into `(a, b, c)` BN254 curve points
- Converts dynamic `uint256[]` signals to fixed-size `uint256[6]` for the verifier
- Includes circom circuit definition for 6-signal trust attestation (SHA-256 PUF preimage proof)

```bash
# Run contract tests (requires Foundry)
cd contracts && forge test
```

Contracts are deployed on Base Sepolia. See `contracts/scripts/README.md` for deployment and E2E testing documentation, and `contracts/circuits/README.md` for circuit compilation and trusted setup.

## Formal Specification

NucleusDB includes 18 Lean 4 modules that formally specify the core protocol:

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

168 tests across 11 test suites, 0 failures, 0 warnings:

```bash
cargo test          # 134 Rust tests
cd contracts && forge test   # 34 Solidity tests
```

| Suite | Tests | Coverage |
|-------|-------|----------|
| Unit (lib) | 61 | Immutable proofs, license/SNARK, CT, PUF, PCN, on-chain trust, MCP auth/scoping |
| CLI smoke | 2 | Binary help, create-sql-status-export pipeline |
| End-to-end | 36 | Protocol commits, queries, security, multi-tenant, immutable mode |
| KeyMap | 3 | Stability, LIKE matching, reverse lookup |
| Persistence | 5 | WAL/snapshot compat, Bug #1/#3 regression |
| SQL | 18 | CRUD, multi-statement, committed flag, immutable mode |
| Monitor | 2 | Channel parsing, config CSV |
| Solidity: TrustVerifier | 11 | Attestation, fees, proofs, replay, views |
| Solidity: TrustVerifierMultiChain | 11 | Chain registry, composite attestation, tiered fees, multichain verification |
| Solidity: Groth16VerifierAdapter | 12 | Proof decoding, signal validation, constructor guards, legacy ABI-mismatch fail-closed behavior, integration paths |
| **Total** | **168** | |

## Known Limitations

- The SQL surface is a focused subset (single virtual table `data` with `key`/`value` columns), not a general-purpose SQL engine.
- The `ipa` backend carries full-vector opening payloads (not logarithmic-size IPA arguments).
- The KZG backend's default trusted setup is for development/demo use. Production KZG deployments require externally managed ceremony artifacts.
- Sheaf coherence checks are local-view oriented, not full global-state reconciliation.

## Licensing

NucleusDB is released under the [Apoth3osis License Stack v1](LICENSE.md), a tri-license designed to maximize public-good access while sustaining development:

| License | Who It's For | Cost |
|---------|-------------|------|
| **Public Good** (CPGL) | Open-source projects + open-access research | Free |
| **Small Business** (CSBL) | Organizations under $1M revenue, <100 workers | Free |
| **Enterprise** (CECL) | Everyone else | Contact us |

**For enterprise licensing, custom integrations, certification services, or any questions:**

**Contact: rgoodman@apoth3osis.io**

The "Apoth3osis-Certified" mark is available exclusively under CECL and requires an active trademark license and compliance verification.

## Citation

```bibtex
@software{nucleusdb,
  title = {NucleusDB},
  author = {Apoth3osis},
  year = {2025--2026},
  url = {https://github.com/Abraxas1010/nucleusdb},
  license = {Apoth3osis License Stack v1}
}
```

---

<sub><strong>Our tech stack is ontological:</strong> Hardware — Physics | Software — Mathematics<br>
<strong>Our engineering workflow is simple:</strong> discover, build, grow, learn & teach</sub>
