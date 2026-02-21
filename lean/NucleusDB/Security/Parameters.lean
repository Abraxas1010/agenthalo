import NucleusDB.Security.Assumptions

namespace HeytingLean
namespace NucleusDB
namespace Security

/-- Security-relevant runtime parameter surface mirrored in Lean. -/
structure ParameterSet where
  fieldBits : Nat
  maxVectorLen : Nat
  maxDeltaWrites : Nat
  maxWitnesses : Nat
  minWitnessThreshold : Nat
  maxWitnessThreshold : Nat
  requireKzgTrustedSetup : Bool
  kzgTrustedSetupId : Option String

/-- Parameter validity for a concrete backend and witness configuration. -/
def ParameterSet.ValidFor
    (p : ParameterSet)
    (backend : CommitmentBackend)
    (witnessCount threshold : Nat) : Prop :=
  0 < p.fieldBits ∧
  0 < p.maxVectorLen ∧
  0 < p.maxDeltaWrites ∧
  witnessCount ≤ p.maxWitnesses ∧
  p.minWitnessThreshold ≤ threshold ∧
  threshold ≤ p.maxWitnessThreshold ∧
  threshold ≤ witnessCount ∧
  (backend = CommitmentBackend.kzg →
    p.requireKzgTrustedSetup = true →
    p.kzgTrustedSetupId.isSome = true)

theorem validFor_kzg_requires_setup_id
    {p : ParameterSet}
    {witnessCount threshold : Nat}
    (h : p.ValidFor CommitmentBackend.kzg witnessCount threshold)
    (hreq : p.requireKzgTrustedSetup = true) :
    p.kzgTrustedSetupId.isSome = true :=
  h.2.2.2.2.2.2.2 rfl hreq

theorem validFor_of_bounds
    (p : ParameterSet)
    (backend : CommitmentBackend)
    (witnessCount threshold : Nat)
    (hField : 0 < p.fieldBits)
    (hVec : 0 < p.maxVectorLen)
    (hDelta : 0 < p.maxDeltaWrites)
    (hWitnesses : witnessCount ≤ p.maxWitnesses)
    (hMin : p.minWitnessThreshold ≤ threshold)
    (hMax : threshold ≤ p.maxWitnessThreshold)
    (hCount : threshold ≤ witnessCount)
    (hKzg : backend = CommitmentBackend.kzg →
      p.requireKzgTrustedSetup = true →
      p.kzgTrustedSetupId.isSome = true) :
    p.ValidFor backend witnessCount threshold := by
  exact ⟨hField, hVec, hDelta, hWitnesses, hMin, hMax, hCount, hKzg⟩

end Security
end NucleusDB
end HeytingLean
