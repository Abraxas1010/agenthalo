# Partner Instructions: PQ Defense-in-Depth (3 Immediate Items)

**Date:** 2026-03-04
**Repo:** `/home/abraxas/Work/nucleusdb/` (GitHub: `Abraxas1010/agenthalo`)
**Base commit:** `b41d5a1` (master, synced with origin)
**Agent instructions:** `CLAUDE.md` (150 lines) + `Docs/ARCHITECTURE.md` (328 lines)
**Prior work:** PQ Hardening closure report at `WIP/pq_hardening_closure_report_2026-03-04.md`

## Context

PQ Hardening (3 commits: b2c4fd0, 9f8d386, b41d5a1) closed all CRITICAL and MEDIUM
quantum vulnerabilities in AgentHALO's own code. Three upstream dependencies remain
quantum-vulnerable: libp2p Noise XX, Nym Sphinx, Ethereum ECDSA. This project adds
defense-in-depth layers that AgentHALO controls, reducing residual risk even before
upstream projects upgrade.

## 3 Work Packages

### WP-A: HKDF-SHA-512 in Hybrid KEM (~15 minutes, ~10 lines)

**What:** Upgrade `Hkdf::<Sha256>` → `Hkdf::<Sha512>` in `src/halo/hybrid_kem.rs`

**Why:** The hybrid KEM key derivation currently uses HKDF-SHA-256. While the 32-byte
output is sufficient for AES-256-GCM, the PRF itself has only 128-bit quantum security
under Grover's algorithm. HKDF-SHA-512 gives 256-bit quantum security on the PRF,
matching ML-KEM-768's NIST Level 3.

**File:** `src/halo/hybrid_kem.rs`

**Changes:**
1. Line 18: Change `use sha2::Sha256;` → `use sha2::Sha512;`
2. Line 4: Update doc comment from `HKDF-SHA256` → `HKDF-SHA512`
3. Line 88: Change `Hkdf::<Sha256>::new(...)` → `Hkdf::<Sha512>::new(...)`
4. Update `HYBRID_KEM_SALT` domain string from `"AgentHALO-HybridKEM-v1"` to
   `"AgentHALO-HybridKEM-v2"` — this is a **breaking change** for in-flight messages.
   Any DIDComm message encrypted with v1 salt cannot be decrypted with v2 salt.
   This is acceptable because:
   - AgentHALO is pre-production (no deployed agents with stored v1 ciphertexts)
   - The version bump makes the upgrade explicit and auditable
   - If backward compat were needed, add a `kem_version` field and try both

**Tests:** All 7 existing `hybrid_kem` unit tests must still pass (they test roundtrip,
not specific hash lengths). Add 1 new test:
- `hkdf_sha512_produces_same_length_key` — verify combined SS is still exactly 32 bytes

**Acceptance:** `cargo test hybrid_kem` — 8/8 pass.

---

### WP-B: PQ-Gated EVM Transaction Signing (~1-2 hours, ~200 lines)

**What:** Add a DIDComm-verified authorization gate before any EVM transaction can
be signed. Creates a two-cryptosystem barrier: attacker must break both secp256k1
AND ML-KEM-768/ML-DSA-65 to forge a transaction.

**Why:** `evm_wallet::sign_with_evm_key()` currently signs any message given the
private key hex. If secp256k1 is quantum-broken, any caller can forge signatures.
The gate requires that every EVM signing request be accompanied by a DIDComm
authorization proof (dual-signed by the agent's Ed25519 + ML-DSA-65 keys).

**New file:** `src/halo/evm_gate.rs` (~120 lines)

```rust
//! PQ-gated EVM transaction signing.
//!
//! Every EVM transaction must be authorized by a DIDComm-verified proof
//! (dual-signed Ed25519 + ML-DSA-65) before the secp256k1 key signs it.
//! This creates a two-cryptosystem barrier: an attacker must break both
//! secp256k1 AND the PQ-safe DID identity to forge a transaction.

/// Authorization request — the agent intends to sign this EVM payload.
pub struct EvmSigningRequest {
    /// Raw message bytes to be signed by secp256k1.
    pub message: Vec<u8>,
    /// The EVM address that will sign (must match the agent's derived wallet).
    pub evm_address: String,
    /// Unix timestamp of the request.
    pub requested_at: u64,
    /// Nonce to prevent replay (monotonically increasing per agent).
    pub nonce: u64,
}

/// Authorization proof — dual-signed by the agent's DID identity.
pub struct EvmSigningAuthorization {
    /// The signing request being authorized.
    pub request: EvmSigningRequest,
    /// Ed25519 signature over canonical(request).
    pub ed25519_signature: Vec<u8>,
    /// ML-DSA-65 signature over canonical(request).
    pub mldsa65_signature: Vec<u8>,
}

/// Verify the authorization proof, then sign with secp256k1.
/// Returns the secp256k1 signature bytes.
pub fn sign_evm_gated(
    authorization: &EvmSigningAuthorization,
    did_document: &DIDDocument,
    evm_private_key_hex: &str,
) -> Result<Vec<u8>, String> { ... }
```

**Design details:**
1. `canonical(request)` = `"agenthalo.evm_gate.v1|addr={evm_address}|nonce={nonce}|ts={requested_at}|msg_sha512={sha512_hex(message)}"`
2. `sign_evm_gated()` first verifies both signatures via `dual_verify(did_document, canonical, ed_sig, pq_sig)`, then calls `evm_wallet::sign_with_evm_key()` only if verification passes.
3. Nonce tracking is caller's responsibility (the gate validates signatures, not nonce ordering). A future enhancement can add a monotonic nonce store.
4. The `evm_address` in the request must match the address derivable from `evm_private_key_hex` — verify this to prevent key confusion attacks.

**Convenience function:**
```rust
/// Create a signed authorization and immediately sign the EVM message.
/// Use this when the authorizing identity and the signing wallet belong
/// to the same agent (the common case).
pub fn authorize_and_sign(
    identity: &DIDIdentity,
    evm_private_key_hex: &str,
    evm_address: &str,
    message: &[u8],
    nonce: u64,
) -> Result<(EvmSigningAuthorization, Vec<u8>), String> { ... }
```

**Modified files:**
- `src/halo/mod.rs` — add `pub mod evm_gate;`
- `src/halo/twine_anchor.rs` lines 156-175 — the `create_sovereign_binding_proof()`
  function calls `evm_wallet::sign_with_evm_key()` directly. Migrate to use
  `evm_gate::authorize_and_sign()`. This is the primary EVM signing callsite.
- `src/bin/agenthalo_mcp_server.rs` — if there's an MCP tool that signs EVM
  transactions, route through `evm_gate`. Search for `sign_with_evm_key` callers.
- `src/dashboard/api.rs` — same: search for `sign_with_evm_key` callers.

**Tests (in `evm_gate.rs`):**
1. `authorize_and_sign_roundtrip` — create authorization, verify, sign, check secp256k1 sig
2. `gated_sign_rejects_bad_ed25519_sig` — tamper Ed25519 sig → rejected
3. `gated_sign_rejects_bad_mldsa65_sig` — tamper ML-DSA-65 sig → rejected
4. `gated_sign_rejects_wrong_evm_address` — authorization for address A, key for address B → rejected
5. `gated_sign_rejects_unsigned_request` — no signatures → rejected
6. `authorize_and_sign_deterministic` — same inputs produce same authorization

**Acceptance:** `cargo test evm_gate` — 6/6 pass. All existing `twine_anchor` tests
still pass (they now go through the gate).

---

### WP-C: Gossipsub Metadata Minimization (~30-45 minutes, ~40 lines)

**What:** Remove listen addresses from gossipsub announcements. Agents discover
each other via DHT address resolution instead of broadcasting network topology.

**Why:** When Noise XX is quantum-broken, an attacker can read gossipsub traffic.
Currently, `AgentAnnouncement.multiaddrs` broadcasts every listen address
(`/ip4/x.x.x.x/tcp/9090`). This reveals the agent's network location to any
passive observer. By moving addresses to DHT-only, the attacker must actively
query for specific DIDs rather than passively harvesting the entire mesh topology.

**This is defense-in-depth, not a full fix** — the DHT also runs over Noise XX.
But it changes the attack model from passive bulk harvesting to active per-agent
queries, which is significantly harder and more detectable.

**File:** `src/halo/p2p_discovery.rs`

**Changes:**
1. Add a `GossipPrivacy` enum:
   ```rust
   #[derive(Clone, Copy, Debug, PartialEq, Eq)]
   pub enum GossipPrivacy {
       /// Include listen addresses in gossipsub (legacy behavior).
       Full,
       /// Omit listen addresses from gossipsub; use DHT for address resolution.
       AddressesViaDhtOnly,
   }
   ```

2. Add `gossip_privacy: GossipPrivacy` field to `AgentDiscovery`.

3. In `AgentDiscovery::announce()` (line 304-315): if `gossip_privacy == AddressesViaDhtOnly`,
   clone the announcement, clear `multiaddrs`, and publish the stripped version.
   The full announcement (with addresses) still goes to DHT via `publish_to_dht()`.

4. Default `GossipPrivacy::AddressesViaDhtOnly` in `AgentDiscovery::new()`.

**File:** `src/halo/p2p_node.rs`

**Changes:**
1. `run_with_discovery()` (line 365-369): the announcement constructed for gossipsub
   already gets addresses from `self.listen_addresses()`. No change needed here —
   the stripping happens in `AgentDiscovery::announce()`.
2. The DHT publish on line 377 already sends the full announcement. No change needed.

**File:** `src/halo/startup.rs`

**Changes:**
1. Line 187: `agent_discovery.announce()` already sends the announcement — it will
   now auto-strip addresses per `GossipPrivacy` default. No change needed unless
   we want to make this configurable via env var (optional).

**Tests:**
1. `gossip_announce_strips_multiaddrs_in_dht_only_mode` — create announcement with
   addresses, announce via gossipsub with `AddressesViaDhtOnly`, verify published
   payload has empty `multiaddrs`. (This requires capturing the gossipsub publish
   payload — use a mock or verify via serialization.)
2. `gossip_announce_preserves_multiaddrs_in_full_mode` — same but with `Full`,
   verify addresses present.
3. `dht_publish_always_includes_multiaddrs` — regardless of gossip privacy mode,
   DHT records include full addresses.

**Note:** These tests don't need a live gossipsub network. Test by:
- Serializing the announcement that `announce()` WOULD publish
- Verifying the multiaddrs field is empty/present
- This requires a small refactor: extract the "prepare gossip payload" logic into
  a testable helper, e.g. `fn prepare_gossip_announcement(&self, announcement: &AgentAnnouncement) -> AgentAnnouncement`

**Acceptance:** `cargo test p2p_discovery` — all existing + 3 new tests pass.

---

## Implementation Order

**WP-A → WP-B → WP-C** (ordered by risk/complexity)

- WP-A is a 3-line change with immediate security benefit and zero blast radius.
- WP-B is the highest-impact item but requires more code and callsite migration.
- WP-C is the least urgent (metadata exposure, not content exposure) and can be
  done independently.

## Commit Strategy

3 separate commits, each pushed immediately:

1. `[PQ] HKDF-SHA-512 for hybrid KEM key derivation (WP-A)`
2. `[PQ] PQ-gated EVM transaction signing (WP-B)`
3. `[PQ] Gossipsub metadata minimization — DHT-only addresses (WP-C)`

## Regression

After each commit: `cargo test` — 731+ tests pass, 0 failures, 0 warnings.

## Risk Assessment

| WP | Risk | Mitigation |
|----|------|-----------|
| A | Breaking change for in-flight DIDComm | Pre-production; no stored v1 ciphertexts. Salt version bump is explicit. |
| B | Callers bypass gate by calling `sign_with_evm_key` directly | Audit all callers (grep for `sign_with_evm_key`); consider `pub(crate)` on `sign_with_evm_key` |
| C | Address stripping breaks peer discovery | DHT still has full addresses; only gossipsub is stripped. Fallback: `GossipPrivacy::Full`. |
