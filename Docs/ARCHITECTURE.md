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

`lean/NucleusDB/` contains the formal NucleusDB proof surface kept with the standalone repo.
