# NucleusDB Architecture

## Binaries

- `nucleusdb` — CLI for creation, SQL, export, MCP launch, and dashboard launch
- `nucleusdb-server` — multi-tenant HTTP API
- `nucleusdb-mcp` — MCP stdio/HTTP server
- `nucleusdb-tui` — terminal UI
- `nucleusdb-discord` — Discord recorder and slash-command bot

## Core Data Flow

```text
client / bot / MCP tool
        │
        ▼
  NucleusDb protocol layer
        │
        ├─ keymap / state / typed values
        ├─ blob store / vector index
        ├─ SQL executor
        ├─ witness signatures
        ├─ transparency roots
        └─ immutable monotone seals
```

## Modules

### Database core

- `src/protocol.rs` — commits, proofs, typed-value helpers
- `src/state.rs` — in-memory state and deltas
- `src/keymap.rs` — deterministic key-to-index mapping
- `src/persistence.rs` — snapshot plus WAL persistence
- `src/immutable.rs` — append-only mode and monotone seals
- `src/security.rs` / `src/security_utils.rs` — parameter validation and reduction-policy checks
- `src/audit.rs` / `src/witness.rs` — evidence bundles and witness-signature quorum

### Data services

- `src/blob_store.rs` — content-addressed blobs
- `src/vector_index.rs` — vector search
- `src/typed_value.rs` / `src/type_map.rs` — typed storage layer
- `src/sql/` — parser and executor
- `src/multitenant.rs` / `src/api.rs` — HTTP-facing tenant manager

### Formal verification

- `src/verifier/checker.rs` — `.lean4export` certificate parser, trust-tier computation, Ed25519 signature verification
- `src/verifier/gate.rs` — proof gate evaluation against `configs/proof_gate.json`
- `src/transparency/ct6962.rs` — RFC 6962 transparency provenance
- `src/vc/ipa.rs` — IPA/Pedersen commitment provenance
- `src/sheaf/coherence.rs` — sheaf coherence and trace topology provenance
- `scripts/formal_provenance_resolver.py` — namespace-aware Lean FQN resolution and commit-staleness detection

### Identity and local security

- `src/genesis.rs` — entropy harvest and genesis-seed persistence
- `src/did.rs` — DID derivation from genesis seed
- `src/identity.rs` / `src/identity_ledger.rs` — identity state and anchor integration
- `src/password.rs` / `src/encrypted_file.rs` / `src/crypto_scope.rs` — password-derived file encryption
- `src/vault.rs` — encrypted provider-key storage

### Product surfaces

- `src/discord/` — Discord recorder, slash commands, backfill, status sidecar
- `src/mcp/` — 16-tool MCP surface over NucleusDB and Discord records
- `src/dashboard/` — stripped standalone dashboard with Overview, Genesis, Identity, Security, NucleusDB, Discord
- `src/tui/` — terminal UI over the same database

## Discord Recording Model

Keys:

- `msg:<channel_id>:<message_id>`
- `edit:<channel_id>:<message_id>:<timestamp>`
- `del:<channel_id>:<message_id>:<timestamp>`

The bot keeps the database in append-only mode. A delete event does not remove the original message; it adds a new immutable fact that the delete occurred.

## Deployment Surfaces

- `deploy/nucleusdb-discord.service`
- `deploy/nucleusdb-mcp.service`
- `deploy/nucleusdb-dashboard.service`
- `Dockerfile`
- `docker-compose.yml`
- `deploy/entrypoint.sh`

The intended production shape is one shared database file with multiple cooperating processes:

- Discord bot
- MCP server
- REST API
- dashboard

## Formal Layer

`lean/NucleusDB/` contains 74 local Lean 4 mirror modules. Runtime-critical theorems are mirrored locally and linked back to the canonical [Heyting](https://github.com/Abraxas1010/heyting) proofs through dual provenance strings exposed from Rust.

### Provenance Surfaces

Five Rust modules export `formal_provenance()` with 22 unique canonical theorem FQNs and 19 local mirror paths:

- `src/security.rs` — 7 entries (certificate refinement, authorization, dual auth)
- `src/transparency/ct6962.rs` — 4 entries (RFC 6962 consistency, inclusion, append-only)
- `src/vc/ipa.rs` — 5 entries (Pedersen/IPA commitment correctness, soundness, hiding)
- `src/sheaf/coherence.rs` — 4 entries (sheaf coherence, trace topology, component counting)
- `src/protocol.rs` — 2 entries (core nucleus steps, commit certificate verification)

These surfaces feed the advisory proof gate (`configs/proof_gate.json`), the verifier pipeline under `src/verifier/`, the dashboard endpoint `/api/formal-proofs`, and integration tests in `tests/formal_integration_tests.rs`.

### Verifier Pipeline

- `src/verifier/checker.rs` — `.lean4export` certificate parser with Ed25519 signature verification and trust-tier computation (Untrusted → Legacy → Standard → CryptoExtended)
- `src/verifier/gate.rs` — proof gate evaluation: checks theorem FQN, declaration-line SHA-256, Heyting commit hash, and signature for each of 14 requirements across 6 tool surfaces
- `scripts/formal_provenance_resolver.py` — namespace-aware Lean FQN resolution with commit-staleness detection (replaces short-name grep)

### Proof Gate

`configs/proof_gate.json` defines 14 theorem requirements across 6 tool surfaces:

| Tool surface | Requirements |
|---|---|
| `nucleusdb_execute_sql` | 3 (commit certificate, sheaf coherence, IPA opening) |
| `nucleusdb_container_launch` | 2 (core nucleus steps, certificate refinement) |
| `nucleusdb_commit` | 3 (consistency/inclusion proofs, commitment soundness) |
| `nucleusdb_evm_sign` | 2 (dual authorization, authorization composability) |
| `nucleusdb_kem_encapsulate` | 1 (hybrid KEM security) |
| `nucleusdb_trace_analysis` | 3 (connectivity preservation, component lifting, component monotonicity) |

Each requirement binds: exact canonical FQN, expected declaration-line SHA-256, expected Heyting commit hash, and `require_signature: true`.

Current status: `enabled: false`, all `enforced: false` (advisory mode).

### Certificate Flow

1. Validate theorem references with `scripts/validate_formal_provenance.sh` (namespace-aware resolution + commit-staleness check).
2. Generate signed `.lean4export` provenance attestations with `scripts/generate_proof_certificates.sh`.
3. Submit certificates through the CLI / verifier gate; submission re-checks statement hash, commit hash, and signature requirements.
4. Keep `enabled: false` in the proof gate until operators are ready to enforce theorem requirements in production.

Certificates are signed metadata attestations binding theorem claims to a specific Heyting commit and declaration line hash. They are not Lean kernel proof replay artifacts. See [FORMAL_VERIFICATION.md](FORMAL_VERIFICATION.md) for full details.
