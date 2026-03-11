# DIDComm Composition Policy (AES-GCM Key-Commitment Closure)

Date: 2026-03-03  
Scope: standalone Lean DIDComm policy/refinement specs in this repo; historical
runtime call-path evidence lived in the parent AgentHALO DIDComm implementation

## Decision

NucleusDB does **not** use `anoncrypt(authcrypt(...))` envelope composition.
`authcrypt` and `anoncrypt` are mutually exclusive envelope modes in this runtime profile.

## Context

The IOG formal analysis highlights a key-commitment risk in composed DIDComm mode:

- Source: IOG / IACR ePrint 2024/1361, "A Formal Analysis of DIDComm’s Anonymous Message Broadcasting."
- Finding: combined `anoncrypt(authcrypt(...))` requires key-committing AEAD behavior; AES-GCM is non-key-committing, while AES-CBC-HMAC is key-committing for this attack class.

NucleusDB uses `A256GCM` in both standalone envelope modes (`authcrypt` and `anoncrypt`), so composition must be forbidden to keep the attack precondition unreachable.

## Why Non-Composition Is Safe

The composed mode tries to combine two properties:

1. sender authentication (inner `authcrypt`)
2. sender anonymity (outer `anoncrypt`)

NucleusDB achieves the same composition of properties on different layers:

- Envelope layer: `authcrypt` provides sender authentication via dual signature verification (Ed25519 + ML-DSA-65).
- Transport layer: Nym routing provides sender network anonymity for sensitive DIDComm types.

Because anonymity is provided by transport and not by a second envelope layer, the key-commitment precondition from composed-mode attacks is not exercised.

## Evidence (Standalone Policy Surface)

The deleted parent-repo DIDComm runtime implementation originally carried the
call-path evidence for `pack_anoncrypt`, `pack_authcrypt`, and
`unpack_with_resolver`. The standalone repo intentionally keeps the formal
policy surface rather than that historical runtime module. In this standalone
tree, the evidence is:

1. `lean/NucleusDB/Comms/Protocol/CompositionPolicy.lean` models a closed
   envelope kind sum (`authcrypt | anoncrypt`) with no nested composed mode.
2. `lean/NucleusDB/Security/DIDCommRefinement.lean` captures the accept/reject
   gate shape for single-layer envelope kinds.
3. `lean/NucleusDB/Comms/Privacy/FailClosedSpec.lean` routes sensitive DIDComm
   traffic via maximum privacy rather than a second envelope layer.
4. `lean/NucleusDB/Comms/Privacy/NymLifecycleSpec.lean` constrains Nym
   degradation so the transport-anonymity assumption is explicit.
5. The theorem `authcrypt_plus_nym_achieves_both` in
   `lean/NucleusDB/Comms/Protocol/CompositionPolicy.lean` states the intended
   replacement for nested-envelope composition.

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
