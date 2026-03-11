import Mathlib

namespace HeytingLean
namespace NucleusDB
namespace Core

/-!
Runtime-local arithmetic mirror of evidence combination.

The canonical Bayesian/enriched-category theorem lives in
`HeytingLean.EpistemicCalculus.Updating.vUpdate_chain_comm`. This module keeps a
minimal local witness for the concrete false-over-true odds arithmetic used by
`src/halo/evidence.rs`.
-/

/-- Runtime-shaped evidence item in false-over-true odds orientation. -/
structure EvidenceLikelihood where
  likelihoodGivenTrue : Rat
  likelihoodGivenFalse : Rat
  deriving Repr

def factor (e : EvidenceLikelihood) : Rat :=
  e.likelihoodGivenFalse / e.likelihoodGivenTrue

def combinedFactor : List EvidenceLikelihood → Rat
  | [] => 1
  | e :: evidence => factor e * combinedFactor evidence

def combineEvidence (priorOddsFalseOverTrue : Rat)
    (evidence : List EvidenceLikelihood) : Rat :=
  priorOddsFalseOverTrue * combinedFactor evidence

theorem combineEvidence_telescopes
    (priorOddsFalseOverTrue : Rat) (evidence : List EvidenceLikelihood) :
    combineEvidence priorOddsFalseOverTrue evidence =
      priorOddsFalseOverTrue * combinedFactor evidence := by
  rfl

theorem combineEvidence_comm
    (priorOddsFalseOverTrue : Rat)
    {left right : List EvidenceLikelihood}
    (hperm : List.Perm left right) :
    combineEvidence priorOddsFalseOverTrue left =
      combineEvidence priorOddsFalseOverTrue right := by
  have hFactors : combinedFactor left = combinedFactor right := by
    induction hperm with
    | nil =>
        simp [combinedFactor]
    | @cons x xs l₂ _ ih =>
        simp [combinedFactor, ih]
    | @swap x y xs =>
        simp [combinedFactor, mul_assoc, mul_left_comm, mul_comm]
    | @trans xs ys zs _ _ ih₁ ih₂ =>
        exact Eq.trans ih₁ ih₂
  simp [combineEvidence, hFactors]

end Core
end NucleusDB
end HeytingLean
