# DIDComm Composition Policy (AES-GCM Key-Commitment Closure)

Date: 2026-03-03  
Scope: `src/halo/didcomm.rs`, Lean comms protocol/refinement specs

## Decision

AgentHALO does **not** use `anoncrypt(authcrypt(...))` envelope composition.
`authcrypt` and `anoncrypt` are mutually exclusive envelope modes in this runtime profile.

## Context

The IOG formal analysis highlights a key-commitment risk in composed DIDComm mode:

- Source: IOG / IACR ePrint 2024/1361, "A Formal Analysis of DIDComm’s Anonymous Message Broadcasting."
- Finding: combined `anoncrypt(authcrypt(...))` requires key-committing AEAD behavior; AES-GCM is non-key-committing, while AES-CBC-HMAC is key-committing for this attack class.

AgentHALO uses `A256GCM` in both standalone envelope modes (`authcrypt` and `anoncrypt`), so composition must be forbidden to keep the attack precondition unreachable.

## Why Non-Composition Is Safe

The composed mode tries to combine two properties:

1. sender authentication (inner `authcrypt`)
2. sender anonymity (outer `anoncrypt`)

AgentHALO achieves the same composition of properties on different layers:

- Envelope layer: `authcrypt` provides sender authentication via dual signature verification (Ed25519 + ML-DSA-65).
- Transport layer: Nym routing provides sender network anonymity for sensitive DIDComm types.

Because anonymity is provided by transport and not by a second envelope layer, the key-commitment precondition from composed-mode attacks is not exercised.

## Evidence (Code + Runtime Shape)

1. `pack_anoncrypt` has zero production callsites (outside tests) in `src/`.
2. All outbound handler traffic uses `pack_authcrypt`/`pack_authcrypt_enriched`.
3. `unpack_with_resolver` rejects nested `kind` forms containing parentheses.
4. `pack_anoncrypt` is crate-scoped and rejects nested envelope bodies by policy check.
5. Privacy policy routes sensitive DIDComm message types via maximum privacy (Nym transport path).

## Formal Backing

The decision is formalized in:

- `lean/NucleusDB/Comms/Protocol/CompositionPolicy.lean`
  - closed envelope kind sum (`authcrypt | anoncrypt`)
  - kind exhaustiveness theorem
  - single-layer non-ambiguity axiom boundary
  - authcrypt + maximum privacy theorem
- `lean/NucleusDB/Comms/Privacy/FailClosedSpec.lean`
  - sensitive DIDComm routing at maximum privacy
- `lean/NucleusDB/Comms/Privacy/NymLifecycleSpec.lean`
  - non-silent Nym degradation constraints
- `lean/NucleusDB/Security/DIDCommRefinement.lean`
  - runtime accept/reject behavior refinement for kind gating

## Consequence

No cipher migration is required for current runtime policy.
`A256GCM` remains acceptable for standalone `authcrypt` and standalone `anoncrypt` as long as envelope composition remains forbidden.
