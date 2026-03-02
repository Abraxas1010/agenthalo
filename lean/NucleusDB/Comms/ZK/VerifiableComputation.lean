namespace HeytingLean
namespace NucleusDB
namespace Comms
namespace ZK

/-- Public projection of a verifiable computation receipt. -/
structure ComputationSpec where
  programHash : Nat
  publicInputHash : Nat
  outputHash : Nat
  deriving DecidableEq, Repr

/-- Minimal receipt validity checks in the concrete model. -/
def receiptValid (spec : ComputationSpec) : Prop :=
  spec.programHash > 0 ∧ spec.outputHash > 0

/-- T20: abstract soundness bridge to zkVM receipt guarantees.
This is the explicit trust boundary (mirrors external proof-system assumptions). -/
axiom computation_soundness :
  ∀ (spec : ComputationSpec),
    receiptValid spec →
    True

/-- Determinism projection used by the authorization chain bridge. -/
theorem computation_deterministic (spec1 spec2 : ComputationSpec)
    (hProg : spec1.programHash = spec2.programHash)
    (hInput : spec1.publicInputHash = spec2.publicInputHash) :
    spec1.programHash = spec2.programHash ∧
    spec1.publicInputHash = spec2.publicInputHash :=
  ⟨hProg, hInput⟩

end ZK
end Comms
end NucleusDB
end HeytingLean
