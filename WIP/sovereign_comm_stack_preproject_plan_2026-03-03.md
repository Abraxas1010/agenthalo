# Sovereign Communication Stack — Pre-Project Plan

**Date:** 2026-03-03
**Owner:** PM Agent
**Repo:** `agenthalo` at `366e14d` (origin/master, clean)
**Prerequisite:** E1 seed-wrap lifecycle fix (corrigendum C2)

## Objective

Establish a complete, formally verified pipeline from quantum entropy acquisition through EVM wallet derivation, decentralized identity anchoring, and sovereign DIDComm communication — such that an agent's communication identity is deterministically derived from its genesis entropy, anchored on both a local ledger and Twine, and every step from entropy to envelope is covered by category-theoretic Lean proofs.

## Current State Inventory

### What EXISTS and works

| Component | Location | Status |
|-----------|----------|--------|
| Entropy harvest (CURBy-Q + NIST + drand + OS) | `src/halo/genesis_entropy.rs` | **PASS** — 4 sources, XOR mix, SHA-256 fingerprint, parallel fetch, fixture mode for tests |
| Genesis seed storage (AES-256-GCM encrypted) | `src/halo/genesis_seed.rs` | **PASS** — store-once semantics, HKDF key derivation from wallet seed |
| EVM wallet derivation (BIP-39 → BIP-32 → secp256k1) | `src/halo/evm_wallet.rs` | **PASS** — deterministic, `m/44'/60'/0'/0/0`, Keccak-256 address |
| AgentAddress identity struct | `src/halo/identity.rs:116` | **PASS** — `AgentAddressIdentity { evm_address, generated_at, source }` |
| AgentAddress generate (external + genesis-derived) | `src/dashboard/api.rs:3898`, `src/bin/agenthalo_mcp_server.rs:1948` | **BLOCKED BY E1** — genesis path calls `derive_wallet_mnemonic()` → broken chain |
| Identity social ledger (append-only JSONL, hash-chained) | `src/halo/identity_ledger.rs` | **PASS** — immutable, PQ-signed entries, `genesis_entropy_sha256` anchor |
| DID identity (Ed25519 + ML-DSA-65 + X25519 + ML-KEM-768) | `src/halo/did.rs` | **PASS** — dual classical+PQ, genesis-derived via HKDF |
| DIDComm messaging (authcrypt + anoncrypt) | `src/halo/didcomm.rs` | **PASS** — AES-256-GCM envelope, dual Ed25519+ML-DSA-65 signatures |
| DIDComm sessions | `src/comms/session.rs` | **PASS** — peer session management, capability caching |
| Mesh networking (3-instance, peer registry) | `src/container/mesh.rs` | **PASS** — verified in acceptance test |
| CURBy-Q Twine hash (metadata only) | `genesis_entropy.rs:200-210` | **PARTIAL** — `twine_hash` captured from CURBy response, logged as provenance, NOT written to Twine |
| Lean: genesis derivation functor | `lean/.../GenesisDerivation.lean` | **PASS** — `derivationFunctor`, determinism theorem |
| Lean: DID document well-formedness | `lean/.../DIDDocumentSpec.lean` | **PASS** — dual classical+PQ, controller self-ref |
| Lean: DIDComm envelope spec | `lean/.../DIDComm/EnvelopeSpec.lean` | **EXISTS** — not audited for this plan |
| Lean: wallet state machine | `lean/.../Identity/Wallet.lean` | **PASS** — delta transitions, authorization policy, materialization functor |

### What's MISSING

| Gap | Description |
|-----|-------------|
| **E1 fix** | `migration.rs:99` erases wrap key but leaves `pq_wallet.json` → all genesis-derived paths fail post-migration |
| **Twine write** | No code writes identity/entropy attestation TO Twine; only reads CURBy-Q twine_hash as metadata |
| **AgentAddress → DID binding** | DIDComm `from` field uses `did:key:z6Mk...` (Ed25519-derived). No link to EVM address. Two identities coexist without formal binding |
| **Lean: entropy mixing proof** | No theorem that XOR + SHA-256 mixing preserves min-entropy from ≥2 independent sources |
| **Lean: AgentAddress derivation functor** | No formal model connecting genesis seed → HKDF → BIP-39 → BIP-32 → EVM address |
| **Lean: identity-to-communication natural transformation** | No theorem that the DID identity derived from genesis seed correctly maps to the DIDComm sender identity |
| **Lean: Twine anchoring spec** | No formal model of the Twine DAG or anchoring operation |

## Architecture: The Sovereign Communication Pipeline

```
CURBy-Q ─┐
NIST    ─┤  XOR mix    HKDF         BIP-39      BIP-32        Keccak-256
drand   ─┼──────────→ [seed 64B] ──────────→ [entropy 32B] ──────────→ [mnemonic 24w] ──→ [secp256k1] ──→ 0xAgentAddress
OsRng   ─┘     │                      │
               │                      │  HKDF branches
               │                      ├──→ Ed25519 keypair ──→ did:key:z6Mk... (DID subject)
               │                      ├──→ ML-DSA-65 keypair (PQ signing)
               ▼                      ├──→ X25519 agreement key
         identity_ledger.jsonl        └──→ ML-KEM-768 agreement key
         (local hash chain)                         │
               │                                    ▼
               │                            DIDDocument {
               │                              verification: [Ed25519, ML-DSA-65]
               │                              agreement: [X25519, ML-KEM-768]
               │                              ← NEW: also_known_as: [0xAgentAddress]
               │                            }
               │                                    │
               ▼                                    ▼
         Twine DAG ←── NEW: publish              DIDComm envelope
         (attestation anchor)                    (from: did:key:..., bound to 0xAgentAddress)
```

## Implementation Phases

### Phase 0 — E1 Fix (BLOCKER)

Fix the seed-wrap lifecycle so genesis-derived paths work post-migration.

**Implementation:** Three changes, defense-in-depth:

1. `migration.rs` after line 107 — delete the poison file:
   ```rust
   let wallet_path = config::pq_wallet_path();
   if wallet_path.exists() {
       let _ = std::fs::remove_file(&wallet_path);
       report.legacy_wallet_deleted = true;
   }
   ```

2. `pq.rs` `load_or_create_wallet_wrap_key()` — fail-closed when crypto header exists:
   ```rust
   if !key_path.exists() {
       if crate::halo::config::crypto_header_path().exists() {
           return Err("v2 migration completed; wrap key erased; use v2 path".into());
       }
       // ... existing create-new-key logic (only for fresh installs)
   }
   ```

3. Route `extract_wallet_seed_bytes()` through v2 decryption when `.v2.enc` exists.

**Acceptance checks:**
1. `cargo test` — all existing tests pass (no regression).
2. New test: create wallet → migrate to v2 → call `derive_wallet_mnemonic()` → succeeds.
3. New test: post-migration, `load_or_create_wallet_wrap_key()` returns `Err` (not silent new key).
4. Re-run acceptance test phases 4, 4.2, 7.4, 7.5, 15 — all unblocked.

**Scope:** ~50 lines Rust, ~80 lines tests.

### Phase 1 — Twine Attestation Write

Publish the agent's identity attestation to Twine as a second decentralized anchor.

**What gets published** (public, non-secret):
- `evm_address`: the agent's 0x address
- `did_subject`: the agent's `did:key:z6Mk...`
- `combined_entropy_sha256`: provenance fingerprint of the entropy harvest
- `curby_pulse_id`: the CURBy-Q pulse used
- `genesis_timestamp`: when the identity was created
- `pq_signature`: ML-DSA-65 signature over the above fields (verifiable by anyone with the DID document)

**What does NOT get published** (secret):
- Private keys, mnemonic, seed bytes, wrap keys

**Implementation:**
- New module `src/halo/twine_anchor.rs`:
  - `publish_identity_attestation(attestation: &IdentityAttestation) -> Result<TwineReceipt, String>`
  - Uses CURBy-Q Twine strand API (HTTP PUT to `https://random.colorado.edu/api/twine/...`)
  - Returns `TwineReceipt { strand_id, pulse_cid, timestamp }`
- Extend genesis ceremony in `api.rs` and `agenthalo.rs`: after successful genesis + AgentAddress generation, call `publish_identity_attestation()`
- Store `TwineReceipt` in identity ledger entry metadata
- New CLI command: `agenthalo identity twine-status` — show anchored attestation

**Acceptance checks:**
1. Genesis ceremony produces a Twine receipt with valid CID.
2. Identity ledger entry contains `twine_receipt` in payload.
3. Attestation is retrievable from Twine by CID.
4. ML-DSA-65 signature on attestation verifies against DID document.
5. No secret material appears in the published attestation.

**Scope:** ~200 lines Rust, ~60 lines tests.

**Risk:** CURBy-Q Twine API availability. Fallback: degrade gracefully (log warning, proceed without anchor, mark identity as `twine_anchor: pending`). Retry on next startup.

### Phase 2 — AgentAddress ↔ DID Binding

Formally bind the EVM address to the DID identity so they are one sovereign credential, not two coexisting identifiers.

**Implementation:**

1. **DID Document extension** — `src/halo/did.rs`:
   - Add `also_known_as: Vec<String>` to `DIDDocument`
   - When AgentAddress is genesis-derived, include `did:pkh:eip155:1:{evm_address}` in `also_known_as`
   - This follows the [DID-PKH](https://github.com/w3c-ccg/did-pkh/blob/main/did-pkh-method-draft.md) specification

2. **DIDComm sender enrichment** — `src/halo/didcomm.rs`:
   - Extend `AuthcryptProtected` with `sender_evm_address: Option<String>`
   - On authcrypt, if AgentAddress exists, include it in the protected header
   - Recipient can verify: the `sender_did` and `sender_evm_address` derive from the same genesis seed

3. **Binding proof** — new endpoint `POST /api/identity/agent-address/binding-proof`:
   - Returns a signed statement: `{ did_subject, evm_address, timestamp, ed25519_sig, mldsa65_sig, secp256k1_sig }`
   - Triple-signed: Ed25519 (classical DID), ML-DSA-65 (PQ DID), secp256k1 (EVM wallet)
   - Any verifier can confirm all three keys derive from the same identity
   - Also published to Twine (Phase 1 infrastructure)

4. **Identity ledger event** — `agent_address_bound` entry type:
   - Records the binding proof hash in the append-only ledger
   - Links to `genesis_entropy_sha256` for full provenance chain

**Acceptance checks:**
1. `DIDDocument.also_known_as` contains the EVM address in `did:pkh` format.
2. DIDComm authcrypt envelope includes `sender_evm_address` when available.
3. Binding proof triple-signature verifies with all three key types.
4. Binding proof is anchored on Twine with valid CID.
5. Identity ledger contains `agent_address_bound` event.

**Scope:** ~250 lines Rust, ~100 lines tests.

### Phase 3 — Sovereign Communication Channel

Wire the bound identity into the communication layer so every DIDComm message is attributable to the sovereign EVM address.

**Implementation:**

1. **Message attribution** — extend `DIDCommMessage`:
   - `sender_binding_proof_cid: Option<String>` — Twine CID of the binding proof
   - Recipients can verify: look up the CID on Twine, verify the triple-signature, confirm the sender's DID and EVM address match

2. **Session binding** — extend `PeerSession`:
   - `peer_evm_address: Option<String>` — populated during handshake if peer provides binding proof
   - `peer_binding_verified: bool` — true if binding proof was verified

3. **Mesh peer identity** — extend mesh registration:
   - Include `evm_address` in peer registry alongside `did_subject`
   - Mesh peers can verify each other's binding proofs before establishing sessions

4. **A2A bridge update** — `src/halo/a2a_bridge.rs`:
   - A2A agent cards include `evm_address` and `binding_proof_cid`
   - Cross-agent communication carries sovereign identity through the full stack

**Acceptance checks:**
1. DIDComm message from Agent A to Agent B includes `sender_binding_proof_cid`.
2. Agent B verifies the binding proof and populates `peer_evm_address` in session state.
3. Mesh peer registry shows `evm_address` for all registered peers.
4. A2A agent card includes EVM address and Twine CID.
5. 3-instance mesh test: all peers verify each other's binding proofs.

**Scope:** ~200 lines Rust, ~80 lines tests.

### Phase 4 — Lean Formal Proofs

Category-theoretic Lean proofs covering the full pipeline from entropy to envelope.

**4A: Entropy Mixing Functor**

New file: `lean/NucleusDB/Comms/Identity/EntropyMixing.lean`

- Model entropy sources as objects in a discrete category `SourceCat` (CURBy, NIST, drand, OS)
- XOR mixing as a fold morphism `Sources → CombinedEntropy`
- SHA-256 fingerprint as a natural transformation from `CombinedEntropy` to `Fingerprint`
- **Theorem:** `entropy_mixing_deterministic` — same inputs → same combined output
- **Theorem:** `entropy_mixing_min_sources` — harvest fails if fewer than `SOURCE_MIN_SUCCESS` remote sources

**4B: AgentAddress Derivation Natural Transformation**

New file: `lean/NucleusDB/Comms/Identity/AgentAddressDerivation.lean`

Extend the existing `derivationFunctor` (GenesisDerivation.lean) with:
- New `DerivationInfo` variant: `walletEntropy`
- New `KeypairType` variant: `secp256k1`
- BIP-39 and BIP-32 as abstract oracles (axioms, matching existing `hkdf_sha256` pattern)
- **Theorem:** `agentaddress_deterministic` — same genesis seed → same EVM address
- **Theorem:** `derivation_functor_extended_correct` — new branch maps correctly

**4C: Identity-to-Communication Natural Transformation**

New file: `lean/NucleusDB/Comms/Identity/SovereignBinding.lean`

The key theorem: the entire pipeline forms a natural transformation from the identity presheaf to the communication presheaf.

- `IdentityPresheaf`: assigns to each agent its (genesis_seed, EVM_address, DID_subject)
- `CommPresheaf`: assigns to each agent its (DIDComm_sender, binding_proof)
- `sovereignBindingNT`: natural transformation witnessing that the communication identity is functorially determined by the genesis identity
- **Theorem:** `sovereign_binding_natural` — the binding diagram commutes: for any agent morphism (key rotation, re-derivation), the communication identity transforms consistently
- **Theorem:** `binding_proof_verifiable` — a well-formed binding proof implies the DID and EVM address share a common genesis

**4D: Twine Anchoring Spec**

New file: `lean/NucleusDB/Comms/Identity/TwineAnchor.lean`

- Model Twine as a content-addressed DAG (abstract: `TwineCID → Attestation`)
- Anchoring as a morphism from `IdentityAttestation` to `TwineCID`
- **Theorem:** `twine_anchor_content_addressed` — same attestation → same CID
- **Theorem:** `twine_anchor_retrievable` — anchored attestation is retrievable

**Acceptance checks:**
1. `lake build` passes with all new Lean files.
2. `guard_no_sorry.sh` reports 0 sorry/admit.
3. All theorems are fully proved (no axioms beyond the existing `hkdf_sha256` + new BIP-39/BIP-32 oracles).
4. New files are imported in the appropriate root module.

**Scope:** ~400 lines Lean across 4 files.

## Dependency Graph

```
Phase 0 (E1 fix)
    │
    ├──→ Phase 1 (Twine write)
    │        │
    │        └──→ Phase 2 (AgentAddress ↔ DID binding) ──→ Phase 3 (Sovereign communication)
    │
    └──→ Phase 4A (Entropy mixing proof)
         Phase 4B (AgentAddress derivation NT) ──→ Phase 4C (Sovereign binding NT)
         Phase 4D (Twine anchor spec)
```

- Phase 0 is the blocker for everything.
- Phases 1, 4A, 4B, 4D can run in parallel after Phase 0.
- Phase 2 depends on Phase 1 (needs Twine infra for binding proof anchoring).
- Phase 3 depends on Phase 2 (needs bound identity).
- Phase 4C depends on 4A + 4B (composes both into the natural transformation).

## Risk + Fallback

| Risk | Impact | Fallback |
|------|--------|----------|
| CURBy-Q Twine API unavailable or changed | Phase 1 blocked | Degrade to local-only attestation + retry loop. Twine CID becomes optional in all downstream consumers. |
| E1 fix causes regression in non-migration paths | Phase 0 | Defense-in-depth: fail-closed guard only activates when crypto header exists (fresh installs unaffected). Full test suite catches regressions. |
| BIP-39/BIP-32 oracles in Lean are too opaque for useful theorems | Phase 4B | Follow existing `hkdf_sha256` axiom pattern — the theorems prove structural properties (determinism, functoriality) conditional on oracle correctness, which is the right abstraction level. |
| DID-PKH `also_known_as` not recognized by external DIDComm peers | Phase 2-3 | The binding proof is self-contained and independently verifiable. External compatibility is a nice-to-have; sovereign verification doesn't depend on it. |

## Done Definition

- E1 fixed: genesis-derived AgentAddress works post-migration (verified by re-running acceptance phases 4, 4.2, 7.4, 7.5, 15).
- Identity attestation published to Twine with verifiable CID and ML-DSA-65 signature.
- AgentAddress and DID are formally bound with triple-signed proof, anchored on both local ledger and Twine.
- DIDComm envelopes carry sovereign identity (EVM address + binding proof CID).
- 3-instance mesh test confirms peers verify each other's binding proofs.
- All Lean theorems build with 0 sorry/admit.
- `cargo test` passes with 0 failures.
- Corrected scorecard items (Phases 4, 4.2, 7.4, 7.5, 14, 15) re-tested and upgraded.

## Estimated Scope

| Phase | Rust lines | Test lines | Lean lines |
|-------|-----------|------------|------------|
| 0 — E1 fix | ~50 | ~80 | — |
| 1 — Twine write | ~200 | ~60 | — |
| 2 — Address ↔ DID binding | ~250 | ~100 | — |
| 3 — Sovereign communication | ~200 | ~80 | — |
| 4 — Lean proofs | — | — | ~400 |
| **Total** | **~700** | **~320** | **~400** |
