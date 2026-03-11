## AgentHALO Formal Provenance Note

This note resolves the dual-provenance ambiguity surfaced in the 2026-03-10 hostile audit.

### Canonical vs local theorem paths

- `formal_basis` in Rust points to the canonical Heyting theorem that carries the strongest mathematical statement used for external assurance.
- `formal_basis_local` in Rust points to the nucleusdb-local Lean theorem that mirrors the concrete runtime arithmetic or state-machine behavior used in this repository.

The two paths are intentionally related but not theorem-identical in every case.

### Surface-by-surface mapping

1. `src/halo/trust.rs`
- canonical: `HeytingLean.EpistemicCalculus.NucleusBridge.nucleus_combine_floor_bound`
- local: `HeytingLean.NucleusDB.Core.EpistemicTrust.combine_floor_respected`
- relationship: local theorem proves the concrete floor-preservation property of the runtime diode; canonical theorem proves the same behavior in the richer Heyting-algebra setting.

2. `src/halo/evidence.rs`
- canonical: `HeytingLean.EpistemicCalculus.Updating.vUpdate_chain_comm`
- local: `HeytingLean.NucleusDB.Core.EvidenceFusion.combineEvidence_comm`
- relationship: local theorem proves order-independence of the concrete false-over-true odds fold; canonical theorem proves the abstract Bayesian updating commutativity statement.

3. `src/halo/circuit.rs`
- canonical: `HeytingLean.NucleusDB.Circuit.AttestationR1CS.attestation_circuit_satisfiable`
- local: `HeytingLean.NucleusDB.TrustLayer.AttestationCircuit.attestation_circuit_satisfiable`
- relationship: local theorem proves satisfiability of the five equality gates in the runtime-shaped mirror; canonical theorem proves satisfiability of the actual R1CS encoding used for the stronger assurance story.

4. `src/halo/evm_gate.rs`
- canonical: `HeytingLean.NucleusDB.Crypto.EVMGate.evm_sign_requires_dual_auth`
- local: `HeytingLean.NucleusDB.Comms.Identity.EVMGate.evm_sign_requires_dual_auth`
- relationship: local theorem mirrors the nucleusdb state machine; canonical theorem is the Heyting-side theorem carried for external formal assurance.

### Audit conclusion

The provenance chain is now explicit instead of implicit:

- external assurance should cite `formal_basis`
- local runtime mirroring should cite `formal_basis_local`

This split is deliberate and documented, not accidental divergence.
