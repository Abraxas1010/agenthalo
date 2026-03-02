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

/-- Builtin guest kinds implemented directly in Rust. -/
inductive BuiltinGuestKind where
  | rangeProof
  | setMembership
  | secureAggregation
  | algorithmCompliance
  deriving DecidableEq, Repr

/-- Builtin acceptance model:
receipt validity plus non-zero program hash (domain separation witness). -/
def builtinAccepts (_kind : BuiltinGuestKind) (spec : ComputationSpec) : Prop :=
  receiptValid spec ∧ spec.programHash ≠ 0

/-- Builtin acceptance always implies receipt validity. -/
theorem builtin_accepts_implies_receipt_valid
    (kind : BuiltinGuestKind) (spec : ComputationSpec)
    (h : builtinAccepts kind spec) :
    receiptValid spec := by
  exact h.1

/-- Determinism witness for the builtin range-proof path. -/
theorem builtin_range_proof_deterministic
    (spec1 spec2 : ComputationSpec)
    (hProg : spec1.programHash = spec2.programHash)
    (_hInput : spec1.publicInputHash = spec2.publicInputHash)
    (hOut : spec1.outputHash = spec2.outputHash) :
    builtinAccepts .rangeProof spec1 → builtinAccepts .rangeProof spec2 := by
  intro hAccept
  rcases hAccept with ⟨hValid, hNonZero⟩
  refine ⟨?_, ?_⟩
  · rcases hValid with ⟨hProgPos, hOutPos⟩
    constructor
    · simpa [hProg] using hProgPos
    · simpa [hOut] using hOutPos
  · intro hZero
    apply hNonZero
    simpa [hProg] using hZero

/- External trust boundary for non-builtin zkVM acceptance.
In Rust this corresponds to successful verifier checks for custom ELF receipts. -/
axiom zkVmAccepts : ComputationSpec → Prop

/-- Abstract correctness predicate attached to a verified custom computation receipt. -/
axiom outputMatchesProgram : ComputationSpec → Prop

/-- T20: abstract soundness bridge to zkVM receipt guarantees. -/
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
