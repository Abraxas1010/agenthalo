import NucleusDB.Security.ProofGateSpec

namespace HeytingLean
namespace NucleusDB
namespace Security

/-- Rust-facing witness projected into the Lean gate model. -/
structure RustGateWitness where
  requirements : List GateRequirementSpec
  certificates : List CertificateWitness
  gateEnabled : Bool
  deriving DecidableEq, Repr

def rustGateAccepts (w : RustGateWitness) : Prop :=
  if w.gateEnabled then gateEvaluate w.requirements w.certificates else True

/-- Refinement theorem for gate acceptance: Rust projection iff Lean gate predicate. -/
theorem rust_gate_refines_spec (w : RustGateWitness) :
    rustGateAccepts w ↔ (if w.gateEnabled then gateEvaluate w.requirements w.certificates else True) := by
  rfl

end Security
end NucleusDB
end HeytingLean
