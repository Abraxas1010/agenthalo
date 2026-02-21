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

# NucleusDB

[![License: Apoth3osis License Stack v1](https://img.shields.io/badge/License-Apoth3osis%20License%20Stack%20v1-blue.svg)](LICENSE.md)

**Verifiable database with vector commitments, post-quantum signatures, and Certificate Transparency.**

## What Is NucleusDB

NucleusDB is an immutable, evidence-oriented database runtime built around verifiable state transitions. Each commit updates a cryptographic state root, appends a transparency leaf, and emits replayable metadata so an independent process can check integrity without trusting the writer.

The project supports multiple commitment backends (`ipa`, `kzg`, `binary_merkle`), RFC6962-style transparency proofs, witness-signature verification (ML-DSA-65 default), and multi-tenant RBAC with WAL-backed recovery and checkpointing.

## Features

- Verifiable commits and query proofs over append-only state history.
- RFC6962-style transparency log (`SHA-256`, inclusion/consistency proof semantics).
- Witness signature verification with algorithm-tagged evidence metadata.
- Multi-tenant RBAC (`Reader`, `Writer`, `Admin`) with token auth.
- WAL replay and snapshot persistence with compatibility checks.
- SQL subset over string keys mapped to internal vector indices.
- CLI, REPL, HTTP API server, MCP server, and terminal TUI.
- Included Lean formal surface modules for security/refinement/coherence modeling.

## Installation

```bash
git clone https://github.com/Abraxas1010/nucleusdb.git
cd nucleusdb
cargo build
```

## Quick Start

1. Create a database.

```bash
cargo run --bin nucleusdb -- create --db /tmp/nucleusdb.ndb --backend merkle
```

2. Execute SQL (file or stdin).

```bash
echo "INSERT INTO data (key, value) VALUES ('temperature', 42); COMMIT;" \
  | cargo run --bin nucleusdb -- sql --db /tmp/nucleusdb.ndb
```

3. Query status.

```bash
cargo run --bin nucleusdb -- status --db /tmp/nucleusdb.ndb
```

4. Export state.

```bash
cargo run --bin nucleusdb -- export --db /tmp/nucleusdb.ndb
```

5. Open interactive REPL.

```bash
cargo run --bin nucleusdb -- open --db /tmp/nucleusdb.ndb
```

## SQL Reference

Supported SQL and command surface in `SqlExecutor`:

- `INSERT INTO data (key, value) VALUES ('k', 1);`
- `SELECT key, value FROM data;`
- `SELECT * FROM data WHERE key = 'k';`
- `SELECT * FROM data WHERE key LIKE 'prefix%';`
- `SELECT * FROM data WHERE key ILIKE 'prefix%';`
- `UPDATE data SET value = 9 WHERE key = 'k';`
- `DELETE FROM data WHERE key = 'k';` (tombstone via value `0`)
- `CREATE TABLE data (...)` (virtual-table validation only)
- `COMMIT;`
- `SHOW STATUS;`
- `SHOW HISTORY;`
- `SHOW HISTORY 'k';`
- `VERIFY 'k';`
- `EXPORT;`
- `CHECKPOINT;` (reported as CLI-path requirement)

## CLI Usage

Primary binary (`nucleusdb`) commands:

- `create`
- `open`
- `server`
- `tui`
- `mcp`
- `sql`
- `status`
- `export`

Examples:

```bash
cargo run --bin nucleusdb -- --help
cargo run --bin nucleusdb -- tui --db /tmp/nucleusdb.ndb
cargo run --bin nucleusdb -- mcp --db /tmp/nucleusdb.ndb
cargo run --bin nucleusdb-server -- 127.0.0.1:8088 production
```

## TUI

`nucleusdb-tui` (and `nucleusdb tui`) provides a five-tab terminal interface:

- `Status`
- `Browse`
- `Execute`
- `History`
- `Transparency`

Hotkeys:

- `F1`..`F5` switch tabs
- `Tab` / `Shift-Tab` cycle tabs
- `Up` / `Down` scroll lists
- `Enter` executes SQL in Execute tab
- `Esc` clears SQL input
- `q` quits outside Execute tab
- `Ctrl-C` quits from any tab

## MCP Server

`nucleusdb-mcp` (and `nucleusdb mcp`) serves MCP tools over stdio via `rmcp`.

Implemented tools:

1. `nucleusdb_create_database`
2. `nucleusdb_open_database`
3. `nucleusdb_execute_sql`
4. `nucleusdb_query`
5. `nucleusdb_query_range`
6. `nucleusdb_verify`
7. `nucleusdb_status`
8. `nucleusdb_history`
9. `nucleusdb_export`
10. `nucleusdb_checkpoint`

Example MCP server command:

```bash
cargo run --bin nucleusdb-mcp -- /tmp/nucleusdb.ndb
```

## HTTP API

Server routes (`src/api.rs`):

- `GET /v1/health`
- `GET /v1/tenants`
- `POST /v1/tenants/register`
- `POST /v1/tenants/register_from_wal`
- `POST /v1/tenants/{tenant_id}/principals/register`
- `POST /v1/tenants/{tenant_id}/commit`
- `POST /v1/tenants/{tenant_id}/query`
- `POST /v1/tenants/{tenant_id}/snapshot`
- `POST /v1/tenants/{tenant_id}/checkpoint`

Run server:

```bash
cargo run --bin nucleusdb-server -- 127.0.0.1:8088 production
```

Sample requests:

```bash
curl -s http://127.0.0.1:8088/v1/health

curl -s -X POST http://127.0.0.1:8088/v1/tenants/register \
  -H 'Content-Type: application/json' \
  -d '{
    "tenant_id": "acme",
    "auth_token": "acme-admin-token",
    "initial_values": [],
    "backend": "binary_merkle"
  }'

curl -s -X POST http://127.0.0.1:8088/v1/tenants/acme/commit \
  -H 'Content-Type: application/json' \
  -d '{
    "auth_token": "acme-admin-token",
    "writes": [[0, 42]],
    "local_views": []
  }'

curl -s -X POST http://127.0.0.1:8088/v1/tenants/acme/query \
  -H 'Content-Type: application/json' \
  -d '{
    "auth_token": "acme-admin-token",
    "index": 0
  }'
```

## Architecture

```text
Client Surfaces
  ├─ CLI / REPL
  ├─ TUI
  ├─ MCP (stdio)
  └─ HTTP API (axum)

Core Runtime
  ├─ protocol.rs (commit/query/verify)
  ├─ state.rs + materialize.rs
  ├─ keymap.rs + sql/executor.rs
  ├─ transparency/ct6962.rs
  ├─ witness.rs
  ├─ security.rs
  ├─ multitenant.rs
  ├─ persistence.rs
  └─ audit.rs

Commitment Backends
  ├─ vc/ipa.rs
  ├─ vc/kzg.rs
  └─ vc/binary_merkle.rs
```

## Vector Commitment Backends

- `binary_merkle`: hash-based commitment (SHA-256) with Merkle proofs.
- `ipa`: Pedersen-style vector commitment path (current opening payload is non-succinct).
- `kzg`: pairing-based commitment path with trusted setup policy checks.

## Post-Quantum Security

- Default witness algorithm: `ML-DSA-65` (`ml_dsa65` metadata tag).
- Recommended commitment profile for PQ posture: `binary_merkle`.
- Transparency path is hash-only and uses RFC6962-style SHA-256 structures.

## Certificate Transparency

NucleusDB includes CT-style structures and verification behavior:

- Domain-separated leaf/node hashing.
- Signed tree head representation in commit metadata.
- Inclusion and consistency proof replay checks.
- Chain growth validation via evidence replay and strict verification scripts.

## Formal Specifications

The repo includes 18 Lean modules under `lean/NucleusDB/`:

- `Adversarial/ForkEvidence.lean`
- `Adversarial/Witness.lean`
- `Commitment/Adapter.lean`
- `Commitment/VectorModel.lean`
- `Core/Authorization.lean`
- `Core/Certificates.lean`
- `Core/Invariants.lean`
- `Core/Ledger.lean`
- `Core/Nucleus.lean`
- `Security/Assumptions.lean`
- `Security/Parameters.lean`
- `Security/Reductions.lean`
- `Security/Refinement.lean`
- `Sheaf/Coherence.lean`
- `Sheaf/MaterializationFunctor.lean`
- `Transparency/CT6962.lean`
- `Transparency/Consistency.lean`
- `Transparency/LogModel.lean`

A minimal standalone Lean package scaffold is also included:

- `lakefile.lean`
- `lean-toolchain`
- `lean/NucleusDB.lean`

Build (optional):

```bash
lake build NucleusDB
```

## Known Limitations

- `ipa` opening proof path currently carries full-vector payload (not logarithmic-size IPA argument).
- Sheaf coherence runtime check is local-view coherence oriented and not yet a full global-state reconciliation proof.
- Default KZG setup path is intended for controlled/demo use unless external trusted setup artifacts are managed under strict policy.

## License

[Apoth3osis License Stack v1](LICENSE.md)
