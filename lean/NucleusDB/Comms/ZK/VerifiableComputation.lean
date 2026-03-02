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
  0 < spec.programHash ∧ 0 < spec.outputHash

/- External trust boundary for zkVM acceptance. In Rust this corresponds to
successful receipt verification by the zkVM verifier. -/
axiom zkVmAccepts : ComputationSpec → Prop

/-- Abstract correctness predicate attached to a verified computation receipt. -/
axiom outputMatchesProgram : ComputationSpec → Prop

/-- T20: abstract soundness bridge to zkVM receipt guarantees.
This is the explicit trust boundary (mirrors external proof-system assumptions). -/
axiom computation_soundness :
  ∀ (spec : ComputationSpec),
    receiptValid spec →
    zkVmAccepts spec →
    outputMatchesProgram spec

/-- Determinism projection used by the authorization chain bridge. -/
theorem computation_deterministic (spec1 spec2 : ComputationSpec)
    (hProg : spec1.programHash = spec2.programHash)
    (_hInput : spec1.publicInputHash = spec2.publicInputHash)
    (hOutput : spec1.outputHash = spec2.outputHash) :
    receiptValid spec1 → receiptValid spec2 := by
  intro hValid
  rcases hValid with ⟨hProgPos, hOutPos⟩
  constructor
  · simpa [hProg] using hProgPos
  · simpa [hOutput] using hOutPos

end ZK
end Comms
end NucleusDB
end HeytingLean
