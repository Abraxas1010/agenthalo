# Project Ledger — NucleusDB Formal Integration v2

- Scope: src/security.rs, src/transparency/ct6962.rs, src/vc/ipa.rs, src/sheaf/coherence.rs, src/protocol.rs, src/dashboard/, src/verifier/gate.rs, configs/proof_gate.json, scripts/, tests/, lean/NucleusDB/
- Base: origin/master @ 9fc0606
- Acceptance:
  - Rust provenance surfaces and proof-gate config land
  - NucleusDB local Lean mirrors for WP-1/3/5/6/7/8 compile with zero sorry
  - formal integration tests pass
  - validation/certificate scripts exist and are usable
  - docs updated for future agents

## Completion

- Status: completed in isolated worktree
- Rust provenance surfaces: landed across security / CT / IPA / sheaf / protocol
- Local Lean mirrors: landed for WP-1/3, WP-5, WP-6, WP-7, WP-8 surfaces
- Proof gate: populated, advisory-only (`enabled = false`)
- Certificates: 14 generated and submitted into `~/.nucleusdb/proof_certificates`
- Dashboard: `/api/formal-proofs` and Formal Proofs UI section landed
- Verification:
  - `./scripts/validate_formal_provenance.sh`
  - `lake build NucleusDB`
  - `cargo check --bin nucleusdb --bin nucleusdb-server --bin nucleusdb-mcp --bin nucleusdb-tui --bin nucleusdb-discord`
  - `cargo test --test formal_integration_tests`
  - `cargo test`

## Honest Scope Boundary

- Heyting remains the read-only canonical source of theorem truth.
- Local mirrors are self-contained runtime mirrors, not replacements for the canonical Heyting proofs.
- The proof gate remains advisory until operators choose to enable enforcement.
