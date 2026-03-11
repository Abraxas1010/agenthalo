# Formal Verification

## Scope

NucleusDB consumes the canonical formal proof surface from the Heyting repo and mirrors the runtime-critical subset locally under `lean/NucleusDB/`.

The canonical source of truth is the Heyting repo at `origin/master`. NucleusDB does not import Heyting modules directly; it records dual provenance:

- `formal_basis`: canonical theorem path in Heyting
- `formal_basis_local`: self-contained local mirror theorem path in NucleusDB

## Runtime Surfaces

Rust modules exposing formal provenance:

- `src/security.rs`
- `src/transparency/ct6962.rs`
- `src/vc/ipa.rs`
- `src/sheaf/coherence.rs`
- `src/protocol.rs`

These feed:

- `configs/proof_gate.json`
- `/api/formal-proofs`
- the dashboard Formal Proofs page
- integration tests in `tests/formal_integration_tests.rs`

## Local Lean Mirrors

This repo maintains local mirror modules for the runtime-facing formal surface:

- `lean/NucleusDB/Core/NucleusBridge.lean`
- `lean/NucleusDB/Crypto/KEM/HybridKEM.lean`
- `lean/NucleusDB/Crypto/Commit/IPAInstance.lean`
- `lean/NucleusDB/Crypto/EVMGate.lean`
- `lean/NucleusDB/Sheaf/TraceTopology.lean`
- existing mirror surfaces such as `Transparency/CT6962.lean` and `TrustLayer/AttestationCircuit.lean`

The mirrors are self-contained. They do not import Heyting modules.

## Proof Gate

The proof gate is configured in `configs/proof_gate.json`.

Current policy:

- `enabled = false`
- requirements are populated with exact theorem FQNs plus expected statement/commit hashes
- signatures are required for every configured advisory certificate
- enforcement remains disabled until certificate generation is part of operator workflow
- dashboard/API results are simulated against the advisory configuration while enforcement is off

## Certificates

Certificate files use the existing `.lean4export` parser in `src/verifier/checker.rs`.

Important limitation:

- A `.lean4export` file is a signed metadata attestation about a theorem name, a Heyting commit, and a declaration line hash.
- It is **not** Lean kernel proof replay.
- `theorem_statement_sha256` hashes the declaration line only, so it binds the claim text rather than a particular proof term.
- The current assurance comes from exact FQN resolution, statement/commit binding, and Ed25519 signing of the certificate payload.
- Treat the certificates as provenance claims bound to a canonical snapshot, not as standalone proof objects.

Generation and validation commands:

```bash
./scripts/validate_formal_provenance.sh
./scripts/generate_proof_certificates.sh
cargo run --bin nucleusdb -- verify-certificate ~/.nucleusdb/proof_certificates/<file>.lean4export
```

Each generated certificate includes:

- `#THM <fully.qualified.theorem>`
- trusted axiom lines
- `#META commit_hash`
- `#META theorem_statement_sha256`
- `#META generated_at`
- `#META signing_did`
- `#META signing_key_multibase`
- `#META signature_ed25519`

The helper scripts auto-discover the Heyting checkout from `HEYTING_ROOT`, `../heyting`, or `~/Work/heyting`.
`validate_formal_provenance.sh` also fails if the pinned `expected_commit_hash` values in `configs/proof_gate.json` no longer match live `origin/master` in the Heyting repo.

## Operator Notes

- Use the Heyting repo as read-only source of theorem truth.
- Validate provenance before generating certificates.
- Initialize local genesis material before generating certificates, because signing is mandatory for configured theorem requirements.
- Prefer targeted Lean and Rust verification in worktrees; do not mutate the shared dirty checkout in place.
