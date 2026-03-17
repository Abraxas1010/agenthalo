# Agent H.A.L.O. Architecture

## Binaries

| Binary | Source | Purpose |
|--------|--------|---------|
| `agenthalo` | `src/bin/agenthalo.rs` | Main CLI — agent wrapping, identity, attestation, trust, governance, mesh, ZK, dashboard |
| `agenthalo-mcp-server` | `src/bin/agenthalo_mcp_server.rs` | MCP server for the full AgentHALO tool surface |
| `nucleusdb` | `src/bin/nucleusdb.rs` | NucleusDB CLI — database creation, SQL, export, MCP, dashboard |
| `nucleusdb-server` | `src/bin/nucleusdb_server.rs` | Multi-tenant HTTP API for NucleusDB |
| `nucleusdb-mcp` | `src/bin/nucleusdb_mcp.rs` | Standalone NucleusDB MCP server (stdio + HTTP) |
| `nucleusdb-tui` | `src/bin/nucleusdb_tui.rs` | Terminal UI for NucleusDB |
| `nucleusdb-discord` | `src/bin/nucleusdb_discord.rs` | Discord recorder and slash-command bot |

## Platform Data Flow

```text
                        ┌──────────────┐
                        │  AI Agents   │
                        │ Claude/Codex │
                        │   /Gemini    │
                        └──────┬───────┘
                               │  agenthalo run <agent>
                        ┌──────▼───────┐
                        │  AgentHALO   │  CLI wrapper + orchestrator
                        │   runner     │
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
```

## Modules

### HALO platform layer (`src/halo/`)

#### Identity and cryptography

- `src/halo/genesis_seed.rs` / `src/halo/genesis_entropy.rs` — genesis seed ceremony and entropy harvest
- `src/halo/did.rs` — DID derivation from genesis seed
- `src/halo/identity.rs` / `src/halo/identity_ledger.rs` — identity state and append-only ledger
- `src/halo/pq.rs` — post-quantum keygen (ML-KEM-768, ML-DSA-65) and wallet key identity
- `src/halo/hybrid_kem.rs` — hybrid X25519 + ML-KEM-768 KEM
- `src/halo/evm_wallet.rs` / `src/halo/evm_gate.rs` — BIP-32 EVM wallet and PQ-gated signing
- `src/halo/password.rs` / `src/halo/encrypted_file.rs` / `src/halo/crypto_scope.rs` — password-derived encryption
- `src/halo/vault.rs` — encrypted provider-key storage (AES-GCM + Argon2)
- `src/halo/hash.rs` — content hashing

#### Agent wrapping and observability

- `src/halo/runner.rs` — agent CLI wrapper (Claude, Codex, Gemini)
- `src/halo/detect.rs` — agent type detection
- `src/halo/trace.rs` — trace writer (content-addressed, SHA-512 Merkle)
- `src/halo/schema.rs` — session metadata and status types
- `src/halo/wrap.rs` / `src/halo/viewer.rs` — session wrapping and trace viewing
- `src/halo/metrics/` — observability metrics

#### Attestation and trust

- `src/halo/attest.rs` — content-addressed attestation with optional ZK proof
- `src/halo/audit.rs` — smart contract auditing
- `src/halo/trust.rs` / `src/halo/trust_score.rs` — trust score queries
- `src/halo/evidence.rs` — evidence collection
- `src/halo/circuit.rs` / `src/halo/circuit_policy.rs` — attestation ZK circuits (Groth16)
- `src/halo/topo_signature.rs` / `src/halo/trace_topology.rs` — topological trace signatures

#### Communication

- `src/halo/didcomm.rs` / `src/halo/didcomm_handler.rs` — DIDComm v2 encrypted messaging (hybrid KEM)
- `src/halo/p2p_node.rs` / `src/halo/p2p_discovery.rs` — libp2p mesh node and peer discovery
- `src/halo/nym.rs` / `src/halo/nym_native.rs` — Nym mixnet integration
- `src/halo/privacy_controller.rs` — privacy controller settings
- `src/halo/eclipse_detector.rs` — eclipse attack detection
- `src/halo/capability_beacon.rs` — capability advertisement over mesh

#### Governance and economics

- `src/halo/governor.rs` / `src/halo/governor_registry.rs` / `src/halo/governor_telemetry.rs` — governor policy, registry, telemetry
- `src/halo/funding.rs` / `src/halo/x402.rs` — funding and x402 payment protocol
- `src/halo/onchain.rs` — on-chain contract deployment and interaction
- `src/halo/pricing.rs` — pricing models
- `src/halo/auth.rs` / `src/halo/agent_auth.rs` / `src/halo/api_keys.rs` — authentication and API keys
- `src/halo/admission.rs` — agent admission control

#### Integrations

- `src/halo/agentpmt.rs` — AgentPMT tool proxy (100+ third-party tools via MCP)
- `src/halo/p2pclaw.rs` / `src/halo/p2pclaw_verify.rs` — P2PCLAW publishing and verification client
- `src/halo/addons.rs` — addon management (AgentPMT, P2PCLAW)
- `src/halo/local_models.rs` — local model management
- `src/halo/pinata.rs` — IPFS pinning via Pinata
- `src/halo/wdk_proxy.rs` — WDK sidecar proxy
- `src/halo/a2a_bridge.rs` — A2A protocol bridge

#### Capabilities and credentials

- `src/halo/capability_spec.rs` / `src/halo/capability_task.rs` / `src/halo/capability_verification.rs` — capability specification and verification
- `src/halo/capability_erc8004.rs` — ERC-8004 capability tokens
- `src/halo/zk_credential.rs` — ZK credential proofs
- `src/halo/zk_compute.rs` — ZK compute dispatch
- `src/halo/public_input_schema.rs` — public input schemas for ZK circuits
- `src/halo/discovery_candidates.rs` — discovery candidate management

#### ZK guests (`src/halo/zk_guests/`)

- `range_proof.rs` — ZK range proofs (RISC Zero)
- `set_membership.rs` — ZK set membership
- `secure_aggregation.rs` — ZK secure aggregation
- `algorithm_compliance.rs` — ZK algorithm compliance checks

#### Other HALO modules

- `src/halo/config.rs` — HALO configuration
- `src/halo/startup.rs` — startup sequence
- `src/halo/session_manager.rs` — session management
- `src/halo/migration.rs` — data migration
- `src/halo/profile.rs` — agent profile
- `src/halo/proxy.rs` — reverse proxy
- `src/halo/uncertainty.rs` — uncertainty quantification
- `src/halo/chebyshev_evictor.rs` — Chebyshev-based cache eviction
- `src/halo/twine_anchor.rs` — Twine anchor integration
- `src/halo/policy_registry.rs` — policy registry
- `src/halo/http_client.rs` — HTTP client utilities
- `src/halo/util.rs` — general utilities
- `src/halo/adapters/` — agent-specific adapters

### Orchestration (`src/orchestrator/`)

- `src/orchestrator/dispatch.rs` / `src/orchestrator/container_dispatch.rs` — task dispatch and container-based dispatch
- `src/orchestrator/agent_pool.rs` — agent pool management
- `src/orchestrator/task.rs` / `src/orchestrator/task_graph.rs` — task definition and DAG execution
- `src/orchestrator/subsidiary_registry.rs` — subsidiary agent registry
- `src/orchestrator/a2a.rs` — A2A protocol handler
- `src/orchestrator/trace_bridge.rs` — trace bridge to observability

### Cockpit (`src/cockpit/`)

- `src/cockpit/pty_manager.rs` — PTY session management
- `src/cockpit/ws_bridge.rs` — WebSocket bridge for browser terminals
- `src/cockpit/session.rs` — cockpit session state
- `src/cockpit/deploy.rs` — deploy management

### Container (`src/container/`)

- `src/container/launcher.rs` — container lifecycle management
- `src/container/agent_hookup.rs` — agent-to-container hookup
- `src/container/mesh.rs` / `src/container/mesh_init.rs` — container mesh networking
- `src/container/coordination.rs` — multi-container coordination
- `src/container/agent_lock.rs` — agent locking

### Swarm (`src/swarm/`)

- `src/swarm/chunk_engine.rs` / `src/swarm/chunk_store.rs` — content-addressed chunk storage
- `src/swarm/bitswap.rs` — bitswap protocol for chunk exchange
- `src/swarm/manifest.rs` — swarm manifest management
- `src/swarm/config.rs` / `src/swarm/types.rs` — configuration and types

### Pod (`src/pod/`)

- `src/pod/access_policy.rs` / `src/pod/acl.rs` — access control policies and ACLs
- `src/pod/capability.rs` — pod capabilities
- `src/pod/did_acl_bridge.rs` — DID-to-ACL bridge
- `src/pod/discovery.rs` — pod discovery
- `src/pod/envelope.rs` / `src/pod/identity_share.rs` — encrypted envelopes and identity sharing

### Communication (`src/comms/`)

- `src/comms/didcomm.rs` — DIDComm message handling
- `src/comms/envelope.rs` — encrypted envelope format
- `src/comms/session.rs` — communication sessions

### Trust (`src/trust/`)

- `src/trust/composite_cab.rs` — composite CAB (Capability-Attestation-Binding) trust
- `src/trust/onchain.rs` — on-chain trust verification

### Other platform modules

- `src/commitment/` — commitment scheme core
- `src/pcn/` — payment channel network adapter
- `src/puf/` — physical unclonable function server
- `src/materialize.rs` — materialized view support
- `src/embeddings.rs` — embedding storage
- `src/memory.rs` — memory/recall subsystem
- `src/license.rs` — license enforcement (CAB)
- `src/config.rs` — global configuration

### NucleusDB core

- `src/protocol.rs` — commits, proofs, typed-value helpers, witness signatures, seal chaining
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

### Product surfaces

- `src/discord/` — Discord recorder, slash commands, backfill, status sidecar
- `src/mcp/` — MCP tool surfaces (NucleusDB + AgentHALO)
- `src/dashboard/` — web dashboard (Overview, Genesis, Identity, Security, NucleusDB, Discord, Sessions, Cockpit)
- `src/tui/` — terminal UI
- `src/cli/` — CLI command implementations

### Smart contracts (`contracts/`)

- `TrustVerifier.sol` — on-chain attestation verification
- `TrustVerifierMultiChain.sol` — cross-chain attestation queries
- `Groth16VerifierAdapter.sol` — ZK proof verification adapter
- `CrossChainAttestationQuery.sol` — cross-chain attestation query surface
- `circuits/trust_attestation.circom` — Circom circuit for trust attestation proofs
- `mocks/` — mock contracts for testing
- `test/` — Foundry test suites

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
- `scripts/agenthalo-instances.sh`
- `deploy/entrypoint.sh`

The intended production shape is one shared database file with multiple cooperating processes:

- Discord bot
- MCP server
- REST API
- Dashboard

## Formal Layer

`lean/NucleusDB/` contains 148 local Lean 4 mirror modules. Runtime-critical theorems are mirrored locally and linked back to the canonical [Heyting](https://github.com/Abraxas1010/heyting) proofs through dual provenance strings exposed from Rust.

### Provenance Surfaces

Five Rust modules export `formal_provenance()` with 22 unique canonical theorem FQNs and 19 local mirror paths:

- `src/security.rs` — 7 entries (certificate refinement, authorization, dual auth)
- `src/transparency/ct6962.rs` — 4 entries (RFC 6962 consistency, inclusion, append-only)
- `src/vc/ipa.rs` — 5 entries (Pedersen/IPA commitment correctness, soundness, hiding)
- `src/sheaf/coherence.rs` — 4 entries (sheaf coherence, trace topology, component counting)
- `src/protocol.rs` — 2 entries (core nucleus steps, commit certificate verification)

These surfaces feed the proof gate (`configs/proof_gate.json`), the verifier pipeline under `src/verifier/`, the dashboard endpoint `/api/formal-proofs`, and integration tests in `tests/formal_integration_tests.rs`.

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

Current status: `enabled: true`, all requirements enforced by default.

### Certificate Flow

1. Validate theorem references with `scripts/validate_formal_provenance.sh` (namespace-aware resolution + commit-staleness check).
2. Generate signed `.lean4export` provenance attestations with `scripts/generate_proof_certificates.sh`.
3. Submit certificates through the CLI / verifier gate; submission re-checks statement hash, commit hash, and signature requirements.
4. Use `AGENTHALO_PROOF_GATE_SKIP=1` only for explicit development escape-hatch sessions; production runs stay enforced.

Certificates are signed metadata attestations binding theorem claims to a specific Heyting commit and declaration line hash. They are not Lean kernel proof replay artifacts. See [FORMAL_VERIFICATION.md](FORMAL_VERIFICATION.md) for full details.
